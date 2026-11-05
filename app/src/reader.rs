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
use crate::language::{LanguageDetector, Segment};
use crate::recorder::{self, Mp3Writer};
use crate::selection::SelectionReader;
use crate::stretch::SpeedControl;
use crate::textnorm::Normalizer;
use crate::tts::{self, Engines};

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

/// Reading language shared with the UI: `language::AUTO` or a model name, read at the start
/// of each reading. Engines start on demand, so a change needs no restart.
pub type SharedLanguage = Arc<Mutex<String>>;

enum Request {
    /// `save`: record this reading even if recording is off.
    ReadSelection { save: bool },
    Speak(String),
    /// Starts these languages' engines in the background, ahead of their first reading.
    Launch(Vec<String>),
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
        language: SharedLanguage,
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
            language,
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

    /// Starts the engines of `languages` in the background; the current reading goes on.
    pub fn launch(&self, languages: Vec<String>) {
        let generation = self.generation.load(Ordering::SeqCst);
        let _ = self.requests.send((generation, Request::Launch(languages)));
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
    language: SharedLanguage,
}

impl Worker {
    fn run(self, requests: Receiver<(u64, Request)>, on_status: impl Fn(Status)) {
        on_status(Status::Starting);
        let languages = self.config.pocket.startup_languages();
        let detector = LanguageDetector::new(&self.config.pocket.auto_languages);
        let mut engines = Engines::new(self.config.clone(), self.paths.clone());
        // The first language is ready before the others start: they share downloaded files.
        if let Err(e) = engines.start(&languages[0]) {
            tracing::error!("TTS engine failed to start: {e:#}");
            on_status(Status::Error(format!("{e:#}")));
            return drain_until_shutdown(requests);
        }
        launch_all(&mut engines, &languages[1..]);
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
            let request = match request {
                Request::Shutdown(ack) => {
                    drop(engines);
                    let _ = ack.send(());
                    return;
                }
                Request::Launch(languages) => {
                    launch_all(&mut engines, &languages);
                    continue;
                }
                request => request,
            };
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
                Request::Launch(_) | Request::Shutdown(_) => unreachable!("handled above"),
            };
            // Previews are never recorded; selections when asked or when recording is on.
            let recording = self.recording.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let record = (force_save || recording.enabled).then_some(&recording);
            let setting = self.language.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let segments = detector.split(&setting, &text);
            on_status(Status::Reading);
            let reading = Reading { generation, text: &text, segments, record, fallback: &languages[0] };
            self.read(&mut engines, &normalizer, reading, &on_status);
            on_status(Status::Ready);
        }
    }

    fn read(&self, engines: &mut Engines, normalizer: &Normalizer, reading: Reading, on_status: &dyn Fn(Status)) {
        let Reading { generation, text, segments, record, fallback } = reading;
        let speed = f32::from_bits(self.speed.load(Ordering::Relaxed));
        tracing::info!(chars = text.len(), speed, record = record.is_some(), segments = segments.len(), "reading");
        let is_cancelled = || self.generation.load(Ordering::SeqCst) != generation;
        let mut stretch = SpeedControl::new(speed);
        let mut recording = Recording::new(record, text);
        let mut result = Ok(());
        for (i, segment) in segments.iter().enumerate() {
            if is_cancelled() {
                break;
            }
            // First use of a language takes a few seconds. Only shown before any audio: a
            // status change would also clear the pause indicator.
            let starting = i == 0 && !engines.is_ready(&segment.language);
            if starting {
                on_status(Status::Starting);
            }
            let language = match engines.start(&segment.language) {
                Ok(()) => segment.language.as_str(),
                Err(e) => {
                    tracing::error!(language = %segment.language, "TTS engine failed to start, reading in {fallback}: {e:#}");
                    fallback
                }
            };
            if starting {
                on_status(Status::Reading);
            }
            let Some(engine) = engines.get(language) else { continue };
            let spoken = normalizer.normalize(&segment.text, language);
            tracing::info!(language, chars = segment.text.len(), "segment");
            tracing::debug!(%spoken, "normalized text");
            result = engine.synthesize(&spoken, &is_cancelled, &mut |chunk| {
                if let Some(chunk) = stretch.process(chunk) {
                    recording.write(&chunk);
                    self.audio.append(generation, chunk);
                }
            });
            if let Err(e) = &result {
                tracing::error!("synthesis failed: {e:#}");
                break;
            }
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

struct Reading<'a> {
    generation: u64,
    /// As selected, for the MP3 file name.
    text: &'a str,
    segments: Vec<Segment>,
    record: Option<&'a RecordingConfig>,
    /// Reads the segments whose engine cannot start.
    fallback: &'a str,
}

/// Starts engines without waiting for them; a failure only costs that language.
fn launch_all(engines: &mut Engines, languages: &[String]) {
    for language in languages {
        if let Err(e) = engines.launch(language) {
            tracing::error!(%language, "cannot start the TTS engine: {e:#}");
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
