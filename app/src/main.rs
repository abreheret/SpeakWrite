// No console window for release builds on Windows; logs go to a file.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod audio;
mod config;
mod dictation;
mod inject;
mod mic;
mod process;
mod reader;
mod recorder;
mod resample;
mod selection;
mod stretch;
mod stt;
mod textnorm;
mod tray;
mod tts;
mod voiceprep;
mod voices;

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tray_icon::menu::MenuEvent;

use crate::audio::AudioOut;
use crate::config::{Config, InsertMode, Paths};
use crate::dictation::{Dictation, DictationOptions, DictationStatus, SharedOptions};
use crate::reader::{Reader, SharedRecording, SharedSpeed, Status};
use crate::tray::{Action, MenuState, Tray};
use crate::voices::VoiceEntry;

enum UserEvent {
    HotKey(GlobalHotKeyEvent),
    Menu(MenuEvent),
    Status(Status),
    Dictation(DictationStatus),
}

fn main() {
    let paths = match Paths::new() {
        Ok(paths) => paths,
        Err(e) => {
            eprintln!("SpeakWrite: {e:#}");
            std::process::exit(1);
        }
    };
    let log_guard = init_logging(&paths);
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        config = %paths.config_file.display(),
        "SpeakWrite starting"
    );
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        // Diagnostics without tray nor hotkeys: `speakwrite --say "Bonjour"`.
        Some("--say") => say(&paths, &args[1..].join(" ")),
        // Same path as dictation, from a file: `speakwrite --transcribe audio.wav`.
        Some("--transcribe") => transcribe(&paths, Path::new(&args[1..].join(" "))),
        Some("--normalize") => {
            let config = load_config(&paths);
            println!("{}", textnorm::Normalizer::new(config.pronunciation).normalize(&args[1..].join(" ")));
            Ok(())
        }
        Some(other) => Err(anyhow::anyhow!(
            "unknown argument {other} (use --say, --transcribe or --normalize)"
        )),
        None => run(paths, log_guard),
    };
    if let Err(e) = result {
        tracing::error!("{e:#}");
        eprintln!("SpeakWrite: {e:#}");
        std::process::exit(1);
    }
}

fn init_logging(paths: &Paths) -> WorkerGuard {
    let file = tracing_appender::rolling::daily(&paths.log_dir, "speakwrite.log");
    let (writer, guard) = tracing_appender::non_blocking(file);
    let filter = tracing_subscriber::EnvFilter::try_from_env("SPEAKWRITE_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(writer).with_ansi(false))
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();
    guard
}

/// Speaks `text` with the configured engine and reports the latency.
fn say(paths: &Paths, text: &str) -> Result<()> {
    let config = load_config(paths);
    let audio = AudioOut::new()?;
    let started = std::time::Instant::now();
    let mut engine = tts::start_engine(&config, paths)?;
    println!("engine ready in {:.1} s", started.elapsed().as_secs_f32());

    let spoken = textnorm::Normalizer::new(config.pronunciation).normalize(text);
    println!("text: {spoken}");
    let started = std::time::Instant::now();
    let mut first_audio = None;
    let mut seconds = 0.0;
    // SPEAKWRITE_SAY_WAV=out.wav also saves what was received (for TTS -> STT checks).
    let mut recorded: Vec<f32> = Vec::new();
    let mut format = (0u32, 0u16);
    audio.stop(1);
    let mut speed = stretch::SpeedControl::new(config.playback.speed);
    let mut play = |chunk: tts::AudioChunk| {
        first_audio.get_or_insert_with(|| started.elapsed());
        seconds += chunk.samples.len() as f32 / (chunk.sample_rate as f32 * chunk.channels as f32);
        format = (chunk.sample_rate, chunk.channels);
        recorded.extend_from_slice(&chunk.samples);
        audio.append(1, chunk);
    };
    engine.synthesize(&spoken, &|| false, &mut |chunk| {
        if let Some(chunk) = speed.process(chunk) {
            play(chunk);
        }
    })?;
    if let Some(chunk) = speed.finish() {
        play(chunk);
    }
    println!("speed: {}", config.playback.speed);
    if let Some(path) = std::env::var_os("SPEAKWRITE_SAY_WAV") {
        write_wav(Path::new(&path), format.0, format.1, &recorded)?;
    }
    if let Some(path) = std::env::var_os("SPEAKWRITE_SAY_MP3") {
        let mut mp3 = recorder::Mp3Writer::create(PathBuf::from(path), format.0, config.recording.bitrate_kbps)?;
        mp3.write(&recorded)?;
        println!("saved {}", mp3.finish()?.display());
    }
    println!(
        "first audio after {:.0} ms, {seconds:.1} s of audio generated in {:.1} s",
        first_audio.unwrap_or_default().as_secs_f64() * 1000.0,
        started.elapsed().as_secs_f32()
    );
    while audio.is_playing() {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Ok(())
}

/// Streams an audio file through the dictation pipeline (resampler + STT) in 100 ms blocks,
/// as the microphone would, and prints the words with their latency.
fn transcribe(paths: &Paths, file: &Path) -> Result<()> {
    let config = load_config(paths);
    let started = std::time::Instant::now();
    let mut model = stt::SttModel::load(&config.dictation.model, &config.dictation.device)?;
    println!("model ready on {} in {:.1} s", model.device_name(), started.elapsed().as_secs_f32());
    let (samples, rate) = voiceprep::decode_mono(file)?;
    let mut resampler = resample::Resampler::new(rate, stt::SAMPLE_RATE);
    let mut text = String::new();
    let mut on_word = |word: String| {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&word);
    };
    let started = std::time::Instant::now();
    model.begin()?;
    for block in samples.chunks(rate as usize / 10) {
        model.feed(&resampler.process(block), &mut on_word)?;
    }
    let streaming = started.elapsed();
    model.finish(&mut on_word)?;
    let audio_seconds = samples.len() as f32 / rate as f32;
    println!("{text}");
    println!(
        "{audio_seconds:.1} s of audio processed in {:.2} s ({:.0}x real time), flush {:.0} ms",
        streaming.as_secs_f32(),
        audio_seconds / streaming.as_secs_f32().max(1e-3),
        (started.elapsed() - streaming).as_secs_f64() * 1000.0
    );
    Ok(())
}

fn write_wav(path: &Path, sample_rate: u32, channels: u16, samples: &[f32]) -> Result<()> {
    let data_len = (samples.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * channels as u32 * 2).to_le_bytes());
    bytes.extend_from_slice(&(channels * 2).to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        bytes.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, bytes).with_context(|| format!("cannot write {}", path.display()))
}

fn run(paths: Paths, log_guard: WorkerGuard) -> Result<()> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    GlobalHotKeyEvent::set_event_handler(Some(move |e| {
        let _ = proxy.send_event(UserEvent::HotKey(e));
    }));
    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |e| {
        let _ = proxy.send_event(UserEvent::Menu(e));
    }));

    let mut app = App::new(paths, event_loop.create_proxy(), log_guard)?;
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            // macOS requires the tray icon to be created once the event loop runs.
            Event::NewEvents(StartCause::Init) => app.create_tray(),
            Event::UserEvent(UserEvent::HotKey(e)) => app.on_hotkey(e),
            Event::UserEvent(UserEvent::Menu(e)) => {
                if app.on_menu(&e) == Flow::Quit {
                    app.quit();
                    *control_flow = ControlFlow::Exit;
                }
            }
            Event::UserEvent(UserEvent::Status(status)) => app.on_status(status),
            Event::UserEvent(UserEvent::Dictation(status)) => app.on_dictation_status(status),
            _ => {}
        }
    })
}

#[derive(PartialEq, Eq)]
enum Flow {
    Continue,
    Quit,
}

struct App {
    paths: Paths,
    config: Config,
    user_voices: Vec<VoiceEntry>,
    speed: SharedSpeed,
    proxy: EventLoopProxy<UserEvent>,
    hotkey_manager: GlobalHotKeyManager,
    registered: Vec<HotKey>,
    read_id: Option<u32>,
    pause_id: Option<u32>,
    stop_id: Option<u32>,
    save_id: Option<u32>,
    dictate_id: Option<u32>,
    audio: Arc<AudioOut>,
    reader: Option<Reader>,
    dictation: Option<Dictation>,
    dictation_options: SharedOptions,
    recording: SharedRecording,
    last_recording: Option<PathBuf>,
    paused: bool,
    /// The dictation key is held down (its auto-repeat sends more presses).
    dictating: bool,
    tray: Option<Tray>,
    last_status: Status,
    dictation_status: DictationStatus,
    log_guard: Option<WorkerGuard>,
}

impl App {
    fn new(paths: Paths, proxy: EventLoopProxy<UserEvent>, log_guard: WorkerGuard) -> Result<Self> {
        let config = load_config(&paths);
        let hotkey_manager =
            GlobalHotKeyManager::new().context("cannot initialise global hotkeys")?;
        let audio = Arc::new(AudioOut::new()?);
        let mut app = Self {
            user_voices: voices::user_voices(&paths),
            speed: Arc::new(AtomicU32::new(config.playback.speed.to_bits())),
            dictation_options: Arc::new(Mutex::new(dictation_options(&config))),
            recording: Arc::new(Mutex::new(config.recording.clone())),
            last_recording: None,
            paused: false,
            paths,
            config,
            proxy,
            hotkey_manager,
            registered: Vec::new(),
            read_id: None,
            pause_id: None,
            stop_id: None,
            save_id: None,
            dictate_id: None,
            audio,
            reader: None,
            dictation: None,
            dictating: false,
            tray: None,
            last_status: Status::Starting,
            dictation_status: DictationStatus::Loading,
            log_guard: Some(log_guard),
        };
        app.register_hotkeys();
        app.start_reader()?;
        app.start_dictation();
        Ok(app)
    }

    fn menu_state(&self) -> MenuState<'_> {
        MenuState {
            hotkeys: &self.config.hotkeys,
            voice: &self.config.pocket.voice,
            user_voices: &self.user_voices,
            speed: self.config.playback.speed,
            dictation: &self.config.dictation,
            recording: &self.config.recording,
            last_recording: self.last_recording.as_deref(),
            paused: self.paused,
        }
    }

    fn create_tray(&mut self) {
        match Tray::new(&self.menu_state()) {
            Ok(tray) => {
                tray.set_status(&self.last_status, &self.dictation_status, &self.menu_state());
                self.tray = Some(tray);
            }
            Err(e) => tracing::error!("cannot create the tray icon: {e:#}"),
        }
    }

    fn refresh_tray(&mut self) {
        let Some(mut tray) = self.tray.take() else { return };
        tray.refresh(&self.menu_state());
        tray.set_status(&self.last_status, &self.dictation_status, &self.menu_state());
        self.tray = Some(tray);
    }

    fn update_tray_status(&self) {
        if let Some(tray) = &self.tray {
            tray.set_status(&self.last_status, &self.dictation_status, &self.menu_state());
        }
    }

    fn register_hotkeys(&mut self) {
        if let Err(e) = self.hotkey_manager.unregister_all(&self.registered) {
            tracing::warn!("cannot unregister hotkeys: {e}");
        }
        self.registered.clear();
        self.read_id = self.register_hotkey("read", &self.config.hotkeys.read.clone());
        self.pause_id = self.register_hotkey("pause", &self.config.hotkeys.pause.clone());
        self.stop_id = self.register_hotkey("stop", &self.config.hotkeys.stop.clone());
        self.save_id = self.register_hotkey("save", &self.config.hotkeys.save.clone());
        self.dictate_id = if self.config.dictation.enabled {
            self.register_hotkey("dictate", &self.config.dictation.hotkey.clone())
        } else {
            None
        };
    }

    fn start_dictation(&mut self) {
        if !self.config.dictation.enabled {
            self.dictation_status = DictationStatus::Disabled;
            return;
        }
        let proxy = self.proxy.clone();
        match Dictation::spawn(self.config.dictation.clone(), self.dictation_options.clone(), move |status| {
            let _ = proxy.send_event(UserEvent::Dictation(status));
        }) {
            Ok(dictation) => self.dictation = Some(dictation),
            Err(e) => self.dictation_status = DictationStatus::Error(format!("{e:#}")),
        }
    }

    fn on_dictation_status(&mut self, status: DictationStatus) {
        if let DictationStatus::Error(e) = &status {
            tracing::error!("{e}");
            show_error(&format!(
                "La dictée n'a pas pu démarrer.\n\n{}\n\nDétails : menu de l'icône > Ouvrir les journaux.",
                e.chars().take(600).collect::<String>()
            ));
        }
        self.dictation_status = status;
        self.update_tray_status();
    }

    fn set_dictation_options(&mut self, insert: InsertMode, trailing_space: bool) {
        self.config.dictation.insert = insert;
        self.config.dictation.trailing_space = trailing_space;
        *self.dictation_options.lock().unwrap_or_else(|e| e.into_inner()) = dictation_options(&self.config);
        if let Err(e) = config::save(&self.paths, |doc| {
            doc["dictation"]["insert"] = toml_edit::value(insert.as_str());
            doc["dictation"]["trailing_space"] = toml_edit::value(trailing_space);
        }) {
            tracing::error!("{e:#}");
        }
        self.refresh_tray();
    }

    fn register_hotkey(&mut self, action: &str, spec: &str) -> Option<u32> {
        let hotkey = match HotKey::from_str(spec) {
            Ok(hotkey) => hotkey,
            Err(e) => {
                tracing::error!(action, spec, "invalid hotkey: {e}");
                return None;
            }
        };
        match self.hotkey_manager.register(hotkey) {
            Ok(()) => {
                tracing::info!(action, spec, "hotkey registered");
                self.registered.push(hotkey);
                Some(hotkey.id())
            }
            Err(e) => {
                tracing::error!(action, spec, "cannot register hotkey (used by another app?): {e}");
                None
            }
        }
    }

    fn start_reader(&mut self) -> Result<()> {
        let proxy = self.proxy.clone();
        self.reader = Some(Reader::spawn(
            self.config.clone(),
            self.paths.clone(),
            self.audio.clone(),
            self.speed.clone(),
            self.recording.clone(),
            move |status| {
                let _ = proxy.send_event(UserEvent::Status(status));
            },
        )?);
        Ok(())
    }

    /// Restarts the engine, e.g. with another voice.
    fn restart_reader(&mut self) {
        if let Some(reader) = self.reader.take() {
            reader.shutdown();
        }
        if let Err(e) = self.start_reader() {
            self.on_status(Status::Error(format!("{e:#}")));
        }
    }

    fn on_hotkey(&mut self, event: GlobalHotKeyEvent) {
        if Some(event.id) == self.dictate_id {
            let Some(dictation) = &self.dictation else { return };
            match event.state {
                HotKeyState::Pressed if !self.dictating => {
                    self.dictating = true;
                    // The microphone would otherwise hear the reading.
                    if let Some(reader) = &self.reader {
                        reader.stop();
                    }
                    dictation.start();
                }
                HotKeyState::Pressed => {} // auto-repeat
                HotKeyState::Released => {
                    self.dictating = false;
                    dictation.stop();
                }
            }
            return;
        }
        let Some(reader) = &self.reader else { return };
        // Read on release: the hotkey's own modifiers would otherwise alter the simulated copy.
        if Some(event.id) == self.read_id && event.state == HotKeyState::Released {
            reader.read_selection(false);
        } else if Some(event.id) == self.save_id && event.state == HotKeyState::Released {
            reader.read_selection(true);
        } else if Some(event.id) == self.pause_id && event.state == HotKeyState::Pressed {
            self.toggle_pause();
        } else if Some(event.id) == self.stop_id && event.state == HotKeyState::Pressed {
            reader.stop();
        }
    }

    fn toggle_pause(&mut self) {
        let Some(paused) = self.reader.as_ref().and_then(Reader::toggle_pause) else { return };
        tracing::info!(paused, "reading paused/resumed");
        self.paused = paused;
        // The menu label switches between "Pause" and "Reprendre la lecture".
        self.refresh_tray();
    }

    fn on_menu(&mut self, event: &MenuEvent) -> Flow {
        let Some(action) = self.tray.as_ref().and_then(|tray| tray.action(&event.id)) else {
            return Flow::Continue;
        };
        match action {
            Action::Read => {
                if let Some(reader) = &self.reader {
                    reader.read_selection(false);
                }
            }
            Action::ReadAndSave => {
                if let Some(reader) = &self.reader {
                    reader.read_selection(true);
                }
            }
            Action::ToggleRecording => {
                let enabled = !self.config.recording.enabled;
                self.config.recording.enabled = enabled;
                self.save_recording_settings(|doc| doc["recording"]["enabled"] = toml_edit::value(enabled));
            }
            Action::OpenLastRecording => {
                if let Some(path) = &self.last_recording {
                    open_file(path);
                }
            }
            Action::OpenRecordingsFolder => {
                let folder = self.config.recording.folder();
                if let Err(e) = std::fs::create_dir_all(&folder) {
                    tracing::error!(folder = %folder.display(), "cannot create: {e}");
                }
                open_file(&folder);
            }
            Action::ChooseRecordingsFolder => self.choose_recordings_folder(),
            Action::TogglePause => self.toggle_pause(),
            Action::Stop => {
                if let Some(reader) = &self.reader {
                    reader.stop();
                }
            }
            Action::SelectVoice(voice) => self.select_voice(voice),
            Action::AddVoice => self.add_voice(),
            Action::OpenVoicesFolder => open_file(&self.paths.voices_dir),
            Action::SetSpeed(speed) => self.set_speed(speed),
            Action::Preview => self.preview(),
            Action::SetInsertMode(mode) => {
                let trailing_space = self.config.dictation.trailing_space;
                self.set_dictation_options(mode, trailing_space);
            }
            Action::ToggleTrailingSpace => {
                let insert = self.config.dictation.insert;
                let trailing_space = !self.config.dictation.trailing_space;
                self.set_dictation_options(insert, trailing_space);
            }
            Action::OpenConfig => open_file(&self.paths.config_file),
            Action::Reload => self.reload(),
            Action::OpenLogs => open_file(&self.paths.log_dir),
            Action::Quit => return Flow::Quit,
        }
        Flow::Continue
    }

    fn select_voice(&mut self, voice: String) {
        if voice != self.config.pocket.voice {
            tracing::info!(%voice, "voice selected");
            self.config.pocket.voice = voice.clone();
            if let Err(e) = config::save(&self.paths, |doc| doc["pocket"]["voice"] = toml_edit::value(voice.as_str())) {
                tracing::error!("{e:#}");
            }
            // The server loads its voice at start-up: about 3 s, the preview waits for it.
            self.restart_reader();
        }
        self.refresh_tray();
        self.preview();
    }

    fn set_speed(&mut self, speed: f32) {
        tracing::info!(speed, "speed selected");
        self.config.playback.speed = speed;
        self.speed.store(speed.to_bits(), Ordering::Relaxed);
        if let Err(e) = config::save(&self.paths, |doc| doc["playback"]["speed"] = config::speed_value(speed)) {
            tracing::error!("{e:#}");
        }
        self.refresh_tray();
        self.preview();
    }

    fn preview(&self) {
        let Some(reader) = &self.reader else { return };
        let speed = self.config.playback.speed;
        let pace = if (speed - 1.0).abs() < 1e-3 {
            "à vitesse normale".to_string()
        } else {
            format!("à la vitesse {} fois", tray::speed_label(speed).trim_end_matches('×'))
        };
        let voice = voices::label_of(&self.config.pocket.voice);
        reader.speak(format!("Bonjour ! Voici la voix {voice}, {pace}."));
    }

    fn add_voice(&mut self) {
        let consent = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Info)
            .set_title("Ajouter une voix")
            .set_description(
                "SpeakWrite va imiter la voix de l'enregistrement choisi.\n\n\
                 N'utilisez que votre propre voix, ou celle d'une personne qui vous a donné son \
                 accord explicite.\n\n\
                 Pour un bon résultat : 10 à 30 secondes de parole naturelle, dans une pièce \
                 calme, sans musique, micro à 15-20 cm.",
            )
            .set_buttons(rfd::MessageButtons::OkCancel)
            .show();
        if !matches!(consent, rfd::MessageDialogResult::Ok) {
            return;
        }
        let Some(file) = rfd::FileDialog::new()
            .set_title("Choisir un enregistrement de voix")
            .add_filter("Enregistrements audio", &["wav", "mp3", "m4a", "flac", "ogg"])
            .add_filter("Voix exportée (pocket-tts)", &["safetensors"])
            .pick_file()
        else {
            return;
        };
        match voices::import(&self.paths, &file) {
            Ok(entry) => {
                tracing::info!(voice = %entry.value, "voice added");
                self.user_voices = voices::user_voices(&self.paths);
                self.select_voice(entry.value);
            }
            Err(e) => show_error(&format!("Impossible d'ajouter cette voix :\n{e:#}")),
        }
    }

    fn on_status(&mut self, status: Status) {
        // Recording outcomes are events, not reader states.
        match status {
            Status::Saved(path) => {
                self.last_recording = Some(path);
                self.refresh_tray();
                return;
            }
            Status::SaveFailed(e) => {
                show_error(&format!("La lecture n'a pas pu être enregistrée en MP3.\n\n{e}"));
                return;
            }
            _ => {}
        }
        let error = match &status {
            Status::Error(e) => Some(e.clone()),
            _ => None,
        };
        // A new reading, or the end of one, is never paused.
        let pause_changed = std::mem::take(&mut self.paused);
        self.last_status = status;
        if pause_changed {
            self.refresh_tray();
        }
        self.update_tray_status();
        if let Some(e) = error {
            tracing::error!("{e}");
            show_error(&format!(
                "Le moteur de voix n'a pas pu démarrer.\n\n{}\n\nDétails : menu de l'icône > Ouvrir les journaux.",
                e.chars().take(600).collect::<String>()
            ));
        }
    }

    fn reload(&mut self) {
        tracing::info!("reloading the configuration");
        self.config = load_config(&self.paths);
        self.speed.store(self.config.playback.speed.to_bits(), Ordering::Relaxed);
        *self.dictation_options.lock().unwrap_or_else(|e| e.into_inner()) = dictation_options(&self.config);
        *self.recording.lock().unwrap_or_else(|e| e.into_inner()) = self.config.recording.clone();
        self.user_voices = voices::user_voices(&self.paths);
        self.register_hotkeys();
        self.restart_reader();
        if let Some(dictation) = self.dictation.take() {
            dictation.shutdown();
        }
        self.dictating = false;
        self.dictation_status = DictationStatus::Loading;
        self.start_dictation();
        self.refresh_tray();
    }

    /// Applies `self.config.recording` live and stores the change in the config file.
    fn save_recording_settings(&mut self, edit: impl FnOnce(&mut toml_edit::DocumentMut)) {
        *self.recording.lock().unwrap_or_else(|e| e.into_inner()) = self.config.recording.clone();
        if let Err(e) = config::save(&self.paths, edit) {
            tracing::error!("{e:#}");
        }
        self.refresh_tray();
    }

    fn choose_recordings_folder(&mut self) {
        let Some(folder) = rfd::FileDialog::new()
            .set_title("Dossier des enregistrements MP3")
            .set_directory(self.config.recording.folder())
            .pick_folder()
        else {
            return;
        };
        let value = folder.to_string_lossy().into_owned();
        tracing::info!(folder = %value, "recordings folder selected");
        self.config.recording.folder = value.clone();
        self.save_recording_settings(|doc| doc["recording"]["folder"] = toml_edit::value(value));
    }

    fn quit(&mut self) {
        tracing::info!("quitting");
        if let Some(reader) = self.reader.take() {
            reader.shutdown();
        }
        if let Some(dictation) = self.dictation.take() {
            dictation.shutdown();
        }
        let _ = self.hotkey_manager.unregister_all(&self.registered);
        self.tray = None;
        // Flush the log file: the event loop exits the process without running destructors.
        self.log_guard = None;
    }
}

fn dictation_options(config: &Config) -> DictationOptions {
    DictationOptions { insert: config.dictation.insert, trailing_space: config.dictation.trailing_space }
}

fn load_config(paths: &Paths) -> Config {
    match Config::load_or_create(paths) {
        Ok(config) => config,
        Err(e) => {
            tracing::error!("{e:#}; using the default configuration");
            Config::default()
        }
    }
}

fn show_error(message: &str) {
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("SpeakWrite")
        .set_description(message)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

/// Opens a file or folder with the desktop's default application.
fn open_file(path: &Path) {
    #[cfg(windows)]
    let result = if path.extension().is_some_and(|e| e == "toml") {
        // .toml files often have no associated application on Windows.
        std::process::Command::new("notepad").arg(path).spawn()
    } else {
        std::process::Command::new("explorer").arg(path).spawn()
    };
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = std::process::Command::new("xdg-open").arg(path).spawn();
    if let Err(e) = result {
        tracing::error!(path = %path.display(), "cannot open: {e}");
    }
}
