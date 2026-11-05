//! Reading aloud: a worker thread owns the TTS engine and the selection reader.
//!
//! Every request gets a generation number. Starting a new reading or stopping bumps the
//! current generation, which cancels the synthesis in progress and silences its audio.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};

use crate::audio::AudioOut;
use crate::config::{Config, Paths, RecordingConfig};
use crate::recorder::{self, Mp3Writer};
use crate::selection::SelectionReader;
use crate::stretch::SpeedControl;
use crate::textnorm::Normalizer;
use crate::tts::{self, TtsEngine};

#[derive(Debug, Clone)]
pub enum Status {
    Starting,
    Ready,
    Reading,
    Error(String),
    /// A reading was saved as MP3 (not a state: the reader stays ready).
    Saved(PathBuf),
    SaveFailed(String),
}

/// Playback speed shared with the UI, as `f32` bits: changes apply from the next reading,
/// without restarting the engine.
pub type SharedSpeed = Arc<AtomicU32>;

/// Recording settings shared with the UI, read at the start of each reading.
pub type SharedRecording = Arc<Mutex<RecordingConfig>>;

enum Request {
    /// `save`: record this reading even if recording is off.
    ReadSelection { save: bool },
    Speak(String),
    Shutdown(Sender<()>),
}

pub struct Reader {
    requests: Sender<(u64, Request)>,
    generation: Arc<AtomicU64>,
    audio: Arc<AudioOut>,
}

impl Reader {
    pub fn spawn(
        config: Config,
        paths: Paths,
        audio: Arc<AudioOut>,
        speed: SharedSpeed,
        recording: SharedRecording,
        on_status: impl Fn(Status) + Send + 'static,
    ) -> Result<Self> {
        let (requests, rx) = crossbeam_channel::unbounded();
        let generation = Arc::new(AtomicU64::new(0));
        let worker = Worker {
            config,
            paths,
            audio: audio.clone(),
            generation: generation.clone(),
            speed,
            recording,
        };
        std::thread::Builder::new()
            .name("reader".into())
            .spawn(move || worker.run(rx, on_status))?;
        Ok(Self { requests, generation, audio })
    }

    /// Stops the current reading, then reads the selection; `save` records it as MP3 even
    /// when recording is off.
    pub fn read_selection(&self, save: bool) {
        let generation = self.cancel();
        let _ = self.requests.send((generation, Request::ReadSelection { save }));
    }

    /// Stops the current reading, then speaks `text` (voice and speed previews). Queued until
    /// the engine is ready.
    pub fn speak(&self, text: String) {
        let generation = self.cancel();
        let _ = self.requests.send((generation, Request::Speak(text)));
    }

    pub fn stop(&self) {
        self.cancel();
    }

    /// Pauses or resumes the reading; `None` when nothing is being read.
    pub fn toggle_pause(&self) -> Option<bool> {
        self.audio.toggle_pause()
    }

    /// Stops the worker and its engine (kills the TTS server).
    pub fn shutdown(self) {
        let generation = self.cancel();
        let (ack, done) = crossbeam_channel::bounded(1);
        if self.requests.send((generation, Request::Shutdown(ack))).is_ok() {
            // The worker may be stuck in a long engine start-up: do not hang the app.
            let _ = done.recv_timeout(Duration::from_secs(3));
        }
    }

    fn cancel(&self) -> u64 {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.audio.stop(generation);
        generation
    }
}

struct Worker {
    config: Config,
    paths: Paths,
    audio: Arc<AudioOut>,
    generation: Arc<AtomicU64>,
    speed: SharedSpeed,
    recording: SharedRecording,
}

impl Worker {
    fn run(self, requests: Receiver<(u64, Request)>, on_status: impl Fn(Status)) {
        on_status(Status::Starting);
        let mut engine = match tts::start_engine(&self.config, &self.paths) {
            Ok(engine) => engine,
            Err(e) => {
                tracing::error!("TTS engine failed to start: {e:#}");
                on_status(Status::Error(format!("{e:#}")));
                return drain_until_shutdown(requests);
            }
        };
        let mut selection = match SelectionReader::new(self.config.selection.clone()) {
            Ok(selection) => selection,
            Err(e) => {
                tracing::error!("{e:#}");
                on_status(Status::Error(format!("{e:#}")));
                return drain_until_shutdown(requests);
            }
        };
        let normalizer = Normalizer::new(self.config.pronunciation.clone());
        on_status(Status::Ready);

        for (generation, request) in requests {
            if let Request::Shutdown(ack) = request {
                drop(engine);
                let _ = ack.send(());
                return;
            }
            if generation != self.generation.load(Ordering::SeqCst) {
                continue; // superseded while queued
            }
            let (text, force_save) = match request {
                Request::Speak(text) => (text, false),
                Request::ReadSelection { save } => match selection.capture() {
                    Ok(Some(text)) => (text, save),
                    Ok(None) => {
                        tracing::info!("nothing to read");
                        continue;
                    }
                    Err(e) => {
                        tracing::error!("cannot get the selection: {e:#}");
                        continue;
                    }
                },
                Request::Shutdown(_) => unreachable!("handled above"),
            };
            // Previews are never recorded; selections when asked or when recording is on.
            let recording = self.recording.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let record = (force_save || recording.enabled).then_some(&recording);
            on_status(Status::Reading);
            self.read(engine.as_mut(), &normalizer, generation, &text, record, &on_status);
            on_status(Status::Ready);
        }
    }

    fn read(
        &self,
        engine: &mut dyn TtsEngine,
        normalizer: &Normalizer,
        generation: u64,
        text: &str,
        record: Option<&RecordingConfig>,
        on_status: &dyn Fn(Status),
    ) {
        let spoken = normalizer.normalize(text);
        let speed = f32::from_bits(self.speed.load(Ordering::Relaxed));
        tracing::info!(chars = text.len(), speed, record = record.is_some(), "reading");
        tracing::debug!(%spoken, "normalized text");
        let is_cancelled = || self.generation.load(Ordering::SeqCst) != generation;
        let mut stretch = SpeedControl::new(speed);
        let mut recording = Recording::new(record, text);
        let result = engine.synthesize(&spoken, &is_cancelled, &mut |chunk| {
            if let Some(chunk) = stretch.process(chunk) {
                recording.write(&chunk);
                self.audio.append(generation, chunk);
            }
        });
        if let Err(e) = &result {
            tracing::error!("synthesis failed: {e:#}");
        }
        if let Some(chunk) = stretch.finish() {
            recording.write(&chunk);
            self.audio.append(generation, chunk);
        }
        // An interrupted reading is not saved: the file would end mid-sentence.
        if result.is_ok() && !is_cancelled() {
            match recording.finish() {
                Ok(Some(path)) => {
                    tracing::info!(path = %path.display(), "reading saved");
                    on_status(Status::Saved(path));
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::error!("cannot save the reading: {e:#}");
                    on_status(Status::SaveFailed(format!("{e:#}")));
                }
            }
        }
        // Synthesis runs ahead of playback: wait for the end of the audio.
        while !is_cancelled() && self.audio.is_playing() {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// The MP3 file of one reading, created with the first audio chunk.
struct Recording {
    target: Option<(PathBuf, u32)>,
    writer: Option<Mp3Writer>,
    error: Option<anyhow::Error>,
}

impl Recording {
    fn new(config: Option<&RecordingConfig>, text: &str) -> Self {
        let target = config.map(|c| {
            (c.folder().join(recorder::file_name(text, chrono::Local::now())), c.bitrate_kbps)
        });
        Self { target, writer: None, error: None }
    }

    fn write(&mut self, chunk: &tts::AudioChunk) {
        let Some((path, bitrate)) = &self.target else { return };
        if self.error.is_some() {
            return;
        }
        if chunk.channels != 1 {
            self.error = Some(anyhow::anyhow!("only mono audio can be recorded"));
            return;
        }
        if self.writer.is_none() {
            match Mp3Writer::create(path.clone(), chunk.sample_rate, *bitrate) {
                Ok(writer) => self.writer = Some(writer),
                Err(e) => {
                    self.error = Some(e);
                    return;
                }
            }
        }
        if let Some(writer) = &mut self.writer
            && let Err(e) = writer.write(&chunk.samples)
        {
            self.error = Some(e);
        }
    }

    fn finish(self) -> Result<Option<PathBuf>> {
        if let Some(e) = self.error {
            return Err(e);
        }
        self.writer.map(Mp3Writer::finish).transpose()
    }
}

fn drain_until_shutdown(requests: Receiver<(u64, Request)>) {
    for (_, request) in requests {
        if let Request::Shutdown(ack) = request {
            let _ = ack.send(());
            return;
        }
    }
}
