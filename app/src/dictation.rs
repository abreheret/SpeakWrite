//! Push-to-talk dictation: hold the hotkey, speak, the text is written into the active field.
//!
//! A worker thread owns the STT model (loaded once, kept in GPU memory), the microphone while
//! a dictation runs, and the text injector.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossbeam_channel::{Receiver, Sender, select};

use crate::config::{DictationConfig, InsertMode};
use crate::inject::Injector;
use crate::mic::Microphone;
use crate::resample::Resampler;
use crate::stt::{self, SttModel};

#[derive(Debug, Clone)]
pub enum DictationStatus {
    Loading,
    Ready,
    Listening,
    Disabled,
    Error(String),
}

/// Settings changed from the menu without reloading the model; read at each dictation start.
#[derive(Debug, Clone, Copy)]
pub struct DictationOptions {
    pub insert: InsertMode,
    pub trailing_space: bool,
}

pub type SharedOptions = Arc<Mutex<DictationOptions>>;

enum Command {
    Start,
    Stop,
    Shutdown(Sender<()>),
}

pub struct Dictation {
    commands: Sender<Command>,
}

impl Dictation {
    pub fn spawn(
        config: DictationConfig,
        options: SharedOptions,
        on_status: impl Fn(DictationStatus) + Send + 'static,
    ) -> Result<Self> {
        let (commands, rx) = crossbeam_channel::unbounded();
        std::thread::Builder::new()
            .name("dictation".into())
            .spawn(move || Worker { config, options }.run(rx, on_status))?;
        Ok(Self { commands })
    }

    /// Hotkey pressed. Repeated presses (key auto-repeat) are ignored while listening.
    pub fn start(&self) {
        let _ = self.commands.send(Command::Start);
    }

    /// Hotkey released.
    pub fn stop(&self) {
        let _ = self.commands.send(Command::Stop);
    }

    pub fn shutdown(self) {
        let (ack, done) = crossbeam_channel::bounded(1);
        if self.commands.send(Command::Shutdown(ack)).is_ok() {
            let _ = done.recv_timeout(Duration::from_secs(3));
        }
    }
}

struct Worker {
    config: DictationConfig,
    options: SharedOptions,
}

/// How a dictation session ended.
enum End {
    Done,
    Shutdown(Sender<()>),
}

impl Worker {
    fn run(self, commands: Receiver<Command>, on_status: impl Fn(DictationStatus)) {
        on_status(DictationStatus::Loading);
        let started = Instant::now();
        let mut model = match SttModel::load(&self.config.model, &self.config.device) {
            Ok(model) => model,
            Err(e) => {
                tracing::error!("STT model failed to load: {e:#}");
                on_status(DictationStatus::Error(format!("{e:#}")));
                return drain_until_shutdown(commands);
            }
        };
        tracing::info!(
            device = model.device_name(),
            seconds = started.elapsed().as_secs_f32(),
            "STT model ready"
        );
        if model.device_name() == "cpu" {
            tracing::warn!("dictation runs on the CPU: it may lag behind speech");
        }
        let mut injector = match Injector::new() {
            Ok(injector) => injector,
            Err(e) => {
                on_status(DictationStatus::Error(format!("{e:#}")));
                return drain_until_shutdown(commands);
            }
        };
        on_status(DictationStatus::Ready);

        while let Ok(command) = commands.recv() {
            match command {
                Command::Start => {
                    on_status(DictationStatus::Listening);
                    let end = self.session(&mut model, &mut injector, &commands);
                    on_status(DictationStatus::Ready);
                    if let End::Shutdown(ack) = end {
                        let _ = ack.send(());
                        return;
                    }
                }
                Command::Stop => {}
                Command::Shutdown(ack) => {
                    let _ = ack.send(());
                    return;
                }
            }
        }
    }

    fn session(&self, model: &mut SttModel, injector: &mut Injector, commands: &Receiver<Command>) -> End {
        let mic = match Microphone::open() {
            Ok(mic) => mic,
            Err(e) => {
                tracing::error!("{e:#}");
                return self.wait_for_release(commands);
            }
        };
        if let Err(e) = model.begin() {
            tracing::error!("STT reset failed: {e:#}");
            return self.wait_for_release(commands);
        }
        let options = *self.options.lock().unwrap_or_else(|e| e.into_inner());
        let mut resampler = Resampler::new(mic.sample_rate, stt::SAMPLE_RATE);
        let mut writer = TextWriter::new(options.insert);
        let started = Instant::now();

        // Capture goes on for `release_delay_ms` after the key is released, so that the last
        // syllable is not cut.
        let mut stop_at: Option<Instant> = None;
        let mut end = End::Done;
        loop {
            let timeout = stop_at.map_or(Duration::from_secs(3600), |t| t.saturating_duration_since(Instant::now()));
            select! {
                recv(mic.samples) -> samples => {
                    let Ok(samples) = samples else { break };
                    let pcm = resampler.process(&samples);
                    if let Err(e) = model.feed(&pcm, &mut |word| writer.word(injector, word)) {
                        tracing::error!("STT failed: {e:#}");
                        break;
                    }
                }
                recv(commands) -> command => match command {
                    Ok(Command::Stop) => {
                        stop_at.get_or_insert(Instant::now() + Duration::from_millis(self.config.release_delay_ms));
                    }
                    Ok(Command::Start) => {} // key auto-repeat
                    Ok(Command::Shutdown(ack)) => {
                        end = End::Shutdown(ack);
                        break;
                    }
                    Err(_) => break,
                },
                default(timeout) => break,
            }
            if stop_at.is_some_and(|t| Instant::now() >= t) {
                break;
            }
        }
        drop(mic);
        if let Err(e) = model.finish(&mut |word| writer.word(injector, word)) {
            tracing::error!("STT flush failed: {e:#}");
        }
        writer.finish(injector, options.trailing_space);
        tracing::info!(
            seconds = started.elapsed().as_secs_f32(),
            words = writer.count,
            "dictation done"
        );
        end
    }

    /// The microphone could not be used: wait for the key release, so that the next press
    /// starts a new attempt.
    fn wait_for_release(&self, commands: &Receiver<Command>) -> End {
        loop {
            match commands.recv() {
                Ok(Command::Stop) | Err(_) => return End::Done,
                Ok(Command::Shutdown(ack)) => return End::Shutdown(ack),
                Ok(Command::Start) => {}
            }
        }
    }
}

/// Turns recognised words into text: typed as they come, or pasted at the end.
struct TextWriter {
    mode: InsertMode,
    text: String,
    count: usize,
}

impl TextWriter {
    fn new(mode: InsertMode) -> Self {
        Self { mode, text: String::new(), count: 0 }
    }

    fn word(&mut self, injector: &mut Injector, word: String) {
        let piece = if self.count == 0 { word } else { format!(" {word}") };
        self.count += 1;
        match self.mode {
            InsertMode::Type => {
                if let Err(e) = injector.type_text(&piece) {
                    tracing::error!("{e:#}");
                }
            }
            InsertMode::Paste => self.text.push_str(&piece),
        }
    }

    fn finish(&mut self, injector: &mut Injector, trailing_space: bool) {
        if self.count == 0 {
            return;
        }
        let tail = if trailing_space { " " } else { "" };
        let result = match self.mode {
            InsertMode::Type if tail.is_empty() => Ok(()),
            InsertMode::Type => injector.type_text(tail),
            InsertMode::Paste => injector.paste(&format!("{}{tail}", self.text)),
        };
        if let Err(e) = result {
            tracing::error!("{e:#}");
        }
    }
}

fn drain_until_shutdown(commands: Receiver<Command>) {
    for command in commands {
        if let Command::Shutdown(ack) = command {
            let _ = ack.send(());
            return;
        }
    }
}
