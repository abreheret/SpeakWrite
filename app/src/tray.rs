//! System tray icon and its menu: reading, voices, speed, settings.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use tray_icon::menu::{CheckMenuItem, IsMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::config::{DictationConfig, HotkeysConfig, InsertMode, RecordingConfig, UiLanguage};
use crate::dictation::DictationStatus;
use crate::language;
use crate::reader::Status;
use crate::voices::{self, VoiceEntry};

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Read,
    TogglePause,
    Stop,
    SelectVoice(String),
    AddVoice,
    /// Config value of the user voice to rename ("voices/ma_voix.wav").
    RenameVoice(String),
    OpenVoicesFolder,
    SetSpeed(f32),
    /// `language::AUTO` or a model name.
    SetLanguage(String),
    Preview,
    SetInsertMode(InsertMode),
    ToggleTrailingSpace,
    ToggleRecording,
    ReadAndSave,
    OpenLastRecording,
    OpenRecordingsFolder,
    ChooseRecordingsFolder,
    /// Switches the menu between French and English.
    ToggleUiLanguage,
    OpenConfig,
    Reload,
    OpenLogs,
    Quit,
}

/// What the menu shows; it is rebuilt whenever one of these changes.
pub struct MenuState<'a> {
    /// Language of the menu.
    pub ui: UiLanguage,
    pub hotkeys: &'a HotkeysConfig,
    /// Config value of the current voice ("" for the default).
    pub voice: &'a str,
    pub user_voices: &'a [VoiceEntry],
    /// Reading language setting: `language::AUTO` or a model name.
    pub language: &'a str,
    pub auto_languages: &'a [String],
    pub speed: f32,
    pub dictation: &'a DictationConfig,
    pub recording: &'a RecordingConfig,
    pub last_recording: Option<&'a Path>,
    /// The current reading is paused.
    pub paused: bool,
}

pub const SPEEDS: [f32; 11] = [0.8, 0.9, 1.0, 1.1, 1.2, 1.25, 1.3, 1.4, 1.5, 1.75, 2.0];

/// "1,25×" in French (decimal comma), "1.25×" in English.
pub fn speed_label(speed: f32, ui: UiLanguage) -> String {
    if (speed - 1.0).abs() < 1e-3 {
        return ui.pick("1× (normale)", "1× (normal)").to_string();
    }
    let digits = format!("{speed:.2}");
    let digits = digits.trim_end_matches('0').trim_end_matches('.');
    match ui {
        UiLanguage::French => format!("{}×", digits.replace('.', ",")),
        UiLanguage::English => format!("{digits}×"),
    }
}

pub struct Tray {
    icon: TrayIcon,
    actions: HashMap<MenuId, Action>,
}

impl Tray {
    pub fn new(state: &MenuState) -> Result<Self> {
        let (menu, actions) = build_menu(state)?;
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("SpeakWrite")
            .with_icon(make_icon(COLOR_STARTING))
            .build()?;
        Ok(Self { icon, actions })
    }

    /// Rebuilds the menu, so that check marks and labels follow the settings.
    pub fn refresh(&mut self, state: &MenuState) {
        match build_menu(state) {
            Ok((menu, actions)) => {
                self.icon.set_menu(Some(Box::new(menu)));
                self.actions = actions;
            }
            Err(e) => tracing::error!("cannot rebuild the tray menu: {e:#}"),
        }
    }

    pub fn action(&self, id: &MenuId) -> Option<Action> {
        self.actions.get(id).cloned()
    }

    pub fn set_status(&self, reading: &Status, dictation: &DictationStatus, state: &MenuState) {
        let color = match (reading, dictation) {
            (_, DictationStatus::Listening) => COLOR_LISTENING,
            (Status::Reading, _) if state.paused => COLOR_PAUSED,
            (Status::Error(_), _) | (_, DictationStatus::Error(_)) => COLOR_ERROR,
            (Status::Reading, _) => COLOR_READING,
            (Status::Starting, _) => COLOR_STARTING,
            (Status::Ready | Status::Saved(_) | Status::SaveFailed(_), _) => COLOR_READY,
        };
        let t = |french: &'static str, english: &'static str| state.ui.pick(french, english);
        let (hotkeys, hold) = (state.hotkeys, &state.dictation.hotkey);
        let reading_line = match reading {
            Status::Starting => t("Lecture : démarrage…", "Reading: starting…").to_string(),
            Status::Ready | Status::Saved(_) | Status::SaveFailed(_) => match state.ui {
                UiLanguage::French => format!("Lecture : {} (pause {})", hotkeys.read, hotkeys.pause),
                UiLanguage::English => format!("Reading: {} (pause {})", hotkeys.read, hotkeys.pause),
            },
            Status::Reading if state.paused => match state.ui {
                UiLanguage::French => format!("Lecture : en pause ({} reprendre)", hotkeys.pause),
                UiLanguage::English => format!("Reading: paused ({} to resume)", hotkeys.pause),
            },
            Status::Reading => t("Lecture : en cours", "Reading: in progress").to_string(),
            Status::Error(_) => t("Lecture : erreur", "Reading: error").to_string(),
        };
        let dictation_line = match dictation {
            DictationStatus::Loading => t("Dictée : chargement…", "Dictation: loading…").to_string(),
            DictationStatus::Ready => format!("{} {hold}", t("Dictée : maintenir", "Dictation: hold")),
            DictationStatus::Listening => t("Dictée : écoute…", "Dictation: listening…").to_string(),
            DictationStatus::Disabled => t("Dictée : désactivée", "Dictation: off").to_string(),
            DictationStatus::Error(_) => t("Dictée : erreur", "Dictation: error").to_string(),
        };
        // Windows truncates tooltips at 127 characters.
        let tooltip: String = format!("SpeakWrite\n{reading_line}\n{dictation_line}").chars().take(120).collect();
        let _ = self.icon.set_tooltip(Some(tooltip));
        let _ = self.icon.set_icon(Some(make_icon(color)));
    }
}

struct Builder {
    actions: HashMap<MenuId, Action>,
}

impl Builder {
    fn item(&mut self, text: impl AsRef<str>, action: Action) -> MenuItem {
        let item = MenuItem::new(text, true, None);
        self.actions.insert(item.id().clone(), action);
        item
    }

    fn check(&mut self, text: impl AsRef<str>, checked: bool, action: Action) -> CheckMenuItem {
        let item = CheckMenuItem::new(text, true, checked, None);
        self.actions.insert(item.id().clone(), action);
        item
    }
}

fn build_menu(state: &MenuState) -> Result<(Menu, HashMap<MenuId, Action>)> {
    let mut b = Builder { actions: HashMap::new() };
    let separator = PredefinedMenuItem::separator;
    let ui = state.ui;
    let t = |french: &'static str, english: &'static str| ui.pick(french, english);

    // Voices: the user's recordings, then the built-in ones.
    let current = match state.voice.trim() {
        "" => voices::DEFAULT_FRENCH,
        voice => voice,
    };
    let voice_menu = Submenu::new(format!("{} {}", t("Voix :", "Voice:"), voices::label_of(state.voice)), true);
    if state.user_voices.is_empty() {
        voice_menu.append(&MenuItem::new(t("Aucune voix personnelle", "No personal voice"), false, None))?;
    }
    for voice in state.user_voices {
        voice_menu.append(&b.check(&voice.label, voice.value == current, Action::SelectVoice(voice.value.clone())))?;
    }
    voice_menu.append(&separator())?;
    let builtin_menu = Submenu::new(t("Voix fournies par Kyutai", "Voices provided by Kyutai"), true);
    for voice in voices::builtin_voices() {
        let label = if voice.value == voices::DEFAULT_FRENCH {
            format!("{} {}", voice.label, t("(française)", "(French)"))
        } else {
            voice.label.clone()
        };
        builtin_menu.append(&b.check(label, voice.value == current, Action::SelectVoice(voice.value)))?;
    }
    voice_menu.append(&builtin_menu)?;
    voice_menu.append(&separator())?;
    voice_menu.append(&b.item(t("Ajouter une voix…", "Add a voice…"), Action::AddVoice))?;
    if !state.user_voices.is_empty() {
        let rename_menu = Submenu::new(t("Renommer une voix", "Rename a voice"), true);
        for voice in state.user_voices {
            rename_menu.append(&b.item(format!("{}…", voice.label), Action::RenameVoice(voice.value.clone())))?;
        }
        voice_menu.append(&rename_menu)?;
    }
    voice_menu.append(&b.item(t("Ouvrir le dossier des voix", "Open the voices folder"), Action::OpenVoicesFolder))?;

    // Reading language: detected among the configured ones, or fixed.
    // French writes language names in lower case inside a sentence, English does not.
    let in_sentence = |model: &str| match ui {
        UiLanguage::French => language::label(model, ui).to_lowercase(),
        UiLanguage::English => language::label(model, ui),
    };
    let auto = state.language == language::AUTO;
    let language_menu = Submenu::new(
        if auto {
            t("Langue : automatique", "Language: automatic").to_string()
        } else {
            format!("{} {}", t("Langue :", "Language:"), language::label(state.language, ui))
        },
        true,
    );
    let auto_labels: Vec<String> = state.auto_languages.iter().map(|l| in_sentence(l)).collect();
    language_menu.append(&b.check(
        format!("{} ({})", t("Automatique", "Automatic"), auto_labels.join(", ")),
        auto,
        Action::SetLanguage(language::AUTO.to_string()),
    ))?;
    language_menu.append(&separator())?;
    let mut fixed: Vec<&str> = state.auto_languages.iter().map(String::as_str).collect();
    if !auto && !fixed.contains(&state.language) {
        fixed.push(state.language);
    }
    for name in fixed {
        language_menu.append(&b.check(
            format!("{} {}", t("Toujours en", "Always in"), in_sentence(name)),
            name == state.language,
            Action::SetLanguage(name.to_string()),
        ))?;
    }

    // Speeds: presets, plus the current value if it was set by hand to something else.
    let speed_menu = Submenu::new(format!("{} {}", t("Vitesse :", "Speed:"), speed_label(state.speed, ui)), true);
    let mut speeds = SPEEDS.to_vec();
    if !speeds.iter().any(|s| (s - state.speed).abs() < 1e-3) {
        speeds.push(state.speed);
        speeds.sort_by(f32::total_cmp);
    }
    for speed in speeds {
        let checked = (speed - state.speed).abs() < 1e-3;
        speed_menu.append(&b.check(speed_label(speed, ui), checked, Action::SetSpeed(speed)))?;
    }

    // Dictation: how the text is inserted.
    let dictation = state.dictation;
    let dictation_menu = Submenu::new(
        if dictation.enabled {
            format!("{} {}", t("Dictée : maintenir", "Dictation: hold"), dictation.hotkey)
        } else {
            t("Dictée : désactivée", "Dictation: off").to_string()
        },
        true,
    );
    dictation_menu.append(&b.check(
        t("Écrire au fil de la parole", "Type while speaking"),
        dictation.insert == InsertMode::Type,
        Action::SetInsertMode(InsertMode::Type),
    ))?;
    dictation_menu.append(&b.check(
        t("Coller tout au relâchement", "Paste everything on release"),
        dictation.insert == InsertMode::Paste,
        Action::SetInsertMode(InsertMode::Paste),
    ))?;
    dictation_menu.append(&separator())?;
    dictation_menu.append(&b.check(
        t("Espace après chaque dictée", "Space after each dictation"),
        dictation.trailing_space,
        Action::ToggleTrailingSpace,
    ))?;

    // MP3 recording of readings.
    let recording_menu = Submenu::new(
        if state.recording.enabled {
            t("Enregistrement MP3 : activé", "MP3 recording: on")
        } else {
            t("Enregistrement MP3", "MP3 recording")
        },
        true,
    );
    recording_menu.append(&b.check(
        t("Enregistrer chaque lecture en MP3", "Save every reading as MP3"),
        state.recording.enabled,
        Action::ToggleRecording,
    ))?;
    recording_menu.append(&b.item(
        format!("{} ({})", t("Lire et enregistrer la sélection", "Read and save the selection"), state.hotkeys.save),
        Action::ReadAndSave,
    ))?;
    recording_menu.append(&separator())?;
    let last_label = match state.last_recording.and_then(Path::file_name) {
        Some(name) => format!("{} {}", t("Ouvrir :", "Open:"), name.to_string_lossy()),
        None => t("Ouvrir le dernier enregistrement", "Open the last recording").to_string(),
    };
    let last = b.item(last_label, Action::OpenLastRecording);
    last.set_enabled(state.last_recording.is_some());
    recording_menu.append(&last)?;
    recording_menu.append(&b.item(
        t("Ouvrir le dossier des enregistrements", "Open the recordings folder"),
        Action::OpenRecordingsFolder,
    ))?;
    recording_menu.append(&b.item(t("Choisir le dossier…", "Choose the folder…"), Action::ChooseRecordingsFolder))?;

    let pause_label = if state.paused { t("Reprendre la lecture", "Resume reading") } else { "Pause" };
    let items: Vec<Box<dyn IsMenuItem>> = vec![
        Box::new(b.item(
            format!("{} ({})", t("Lire la sélection", "Read the selection"), state.hotkeys.read),
            Action::Read,
        )),
        Box::new(b.item(format!("{pause_label} ({})", state.hotkeys.pause), Action::TogglePause)),
        Box::new(b.item(
            format!("{} ({})", t("Arrêter la lecture", "Stop reading"), state.hotkeys.stop),
            Action::Stop,
        )),
        Box::new(separator()),
        Box::new(voice_menu),
        Box::new(language_menu),
        Box::new(speed_menu),
        Box::new(b.item(t("Écouter un exemple", "Play a sample"), Action::Preview)),
        Box::new(recording_menu),
        Box::new(separator()),
        Box::new(dictation_menu),
        Box::new(separator()),
        // Written in the other language, so that someone who cannot read this one finds it.
        Box::new(b.item(t("Menu in English", "Menu en français"), Action::ToggleUiLanguage)),
        Box::new(b.item(t("Réglages avancés (fichier)…", "Advanced settings (file)…"), Action::OpenConfig)),
        Box::new(b.item(t("Recharger la configuration", "Reload the configuration"), Action::Reload)),
        Box::new(b.item(t("Ouvrir les journaux", "Open the logs"), Action::OpenLogs)),
        Box::new(separator()),
        Box::new(b.item(t("Quitter", "Quit"), Action::Quit)),
    ];
    let menu = Menu::new();
    for item in &items {
        menu.append(item.as_ref())?;
    }
    Ok((menu, b.actions))
}

const COLOR_STARTING: [u8; 3] = [120, 120, 130];
const COLOR_READY: [u8; 3] = [79, 70, 229];
const COLOR_READING: [u8; 3] = [22, 163, 74];
const COLOR_ERROR: [u8; 3] = [220, 38, 38];
const COLOR_LISTENING: [u8; 3] = [234, 88, 12];
const COLOR_PAUSED: [u8; 3] = [202, 138, 4];

/// A coloured disc with white "sound wave" bars, drawn at runtime (no image asset).
fn make_icon(color: [u8; 3]) -> Icon {
    const SIZE: usize = 32;
    let center = (SIZE as f32 - 1.0) / 2.0;
    let radius = SIZE as f32 / 2.0 - 0.5;
    // (x start, height) of the bars, 3 px wide, centred vertically.
    let bars = [(8usize, 8usize), (13, 16), (18, 12), (23, 6)];
    let mut rgba = vec![0u8; SIZE * SIZE * 4];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dist = ((x as f32 - center).powi(2) + (y as f32 - center).powi(2)).sqrt();
            let coverage = (radius + 0.5 - dist).clamp(0.0, 1.0);
            if coverage == 0.0 {
                continue;
            }
            let in_bar = bars.iter().any(|&(bx, h)| {
                let top = (SIZE - h) / 2;
                (bx..bx + 3).contains(&x) && (top..top + h).contains(&y)
            });
            let [r, g, b] = if in_bar { [255, 255, 255] } else { color };
            let i = (y * SIZE + x) * 4;
            rgba[i..i + 4].copy_from_slice(&[r, g, b, (coverage * 255.0) as u8]);
        }
    }
    Icon::from_rgba(rgba, SIZE as u32, SIZE as u32).expect("icon buffer has the right size")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_labels() {
        assert_eq!(speed_label(1.0, UiLanguage::French), "1× (normale)");
        assert_eq!(speed_label(1.25, UiLanguage::French), "1,25×");
        assert_eq!(speed_label(0.8, UiLanguage::French), "0,8×");
        assert_eq!(speed_label(2.0, UiLanguage::French), "2×");
        assert_eq!(speed_label(1.0, UiLanguage::English), "1× (normal)");
        assert_eq!(speed_label(1.25, UiLanguage::English), "1.25×");
    }
}
