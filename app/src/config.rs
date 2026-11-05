//! User configuration (TOML) and per-OS locations.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// Written on first launch; also the source of the defaults.
const DEFAULT_CONFIG: &str = r#"# Configuration de SpeakWrite.
# Après modification : menu de l'icône > « Recharger la configuration ».

[hotkeys]
# Syntaxe : "F9", "Ctrl+Shift+L", "Alt+F2"...
read = "F9"         # lire la sélection
pause = "F10"       # mettre en pause / reprendre la lecture
stop = "Shift+F10"  # arrêter la lecture
save = "Shift+F9"   # lire la sélection et l'enregistrer en MP3

[selection]
# Remettre le presse-papier tel qu'il était après avoir copié la sélection.
restore_clipboard = true
# Si rien n'est sélectionné, lire le contenu actuel du presse-papier.
fallback_to_clipboard = true
# Délai maximal d'attente de la copie, en millisecondes.
copy_timeout_ms = 500
# Linux : lire directement la sélection primaire (texte surligné), sans simuler de copie.
linux_primary_selection = true

[playback]
# Vitesse de lecture : 1.0 = normale, 1.25 = 25 % plus rapide, 0.8 = plus lente.
# La hauteur de la voix ne change pas. Entre 0.5 et 2.0.
speed = 1.0

[recording]
# Enregistrer aussi chaque lecture en MP3, telle qu'entendue (voix et vitesse).
# Maj+F9 lit et enregistre une fois, même quand cette option est désactivée.
enabled = false
# Dossier des enregistrements. Vide : Musique/SpeakWrite.
folder = ""
# Débit MP3 en kbit/s, de 32 à 160 : 64 suffit pour la voix.
bitrate_kbps = 64

[dictation]
enabled = true
# Maintenir cette touche pour dicter : le texte s'écrit dans le champ actif.
# Une touche sans modificateur (F8, Pause...) : avec Ctrl ou Alt enfoncé, la frappe simulée
# déclencherait des raccourcis.
hotkey = "F8"
# "type" : écrit au fil de la parole. "paste" : colle tout au relâchement (plus rapide,
# et compatible avec les applications qui ignorent la frappe simulée).
insert = "type"
# Ajoute une espace après chaque dictée, pour enchaîner les phrases.
trailing_space = true
# La capture continue un peu après le relâchement, pour ne pas couper la dernière syllabe.
release_delay_ms = 300
# Calcul : "auto" (carte graphique si disponible), "cuda", "metal" ou "cpu".
device = "auto"
# Modèle Kyutai sur Hugging Face (téléchargé une fois, environ 2,4 Go).
model = "kyutai/stt-1b-en_fr-candle"

[tts]
# Moteur de voix. Disponible : "pocket" (Kyutai Pocket TTS, local, sur le processeur).
engine = "pocket"

[pocket]
# Langue de lecture : "auto" reconnaît la langue du texte, ligne par ligne, parmi
# auto_languages. Ou une langue fixe : "french", "english", "german", "spanish",
# "italian", "portuguese", "dutch".
language = "auto"
# Langues reconnues en mode "auto" ; la première sert quand le texte est trop court pour
# trancher. Chaque langue fait tourner son propre modèle (0,5 à 0,8 Go de mémoire).
auto_languages = ["french", "english"]
# Voix : se choisit dans le menu de l'icône (Voix > ...), qui met à jour cette ligne.
# - une voix fournie : "estelle", "cosette", "marius"...
# - un enregistrement à cloner (.wav, .mp3, .m4a...) : "voices/ma_voix.wav", chemin relatif
#   au dossier de ce fichier ou absolu. Le clonage demande d'accepter les conditions sur
#   https://huggingface.co/kyutai/pocket-tts puis `uvx --from huggingface_hub hf auth login`.
# La même voix lit toutes les langues. Vide : la voix fournie de chaque langue
# (« estelle » en français, « alba » en anglais).
voice = ""
# Exécutable pocket-tts. Vide : téléchargé automatiquement (version figée, empreinte vérifiée).
binary = ""

[pronunciation]
# Remplacements mot à mot avant la synthèse des textes en français (sensibles à la casse).
# Kyutai = "kiou-taï"
# OTAN = "otane"
"#;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub hotkeys: HotkeysConfig,
    pub selection: SelectionConfig,
    pub playback: PlaybackConfig,
    pub recording: RecordingConfig,
    pub dictation: DictationConfig,
    pub tts: TtsConfig,
    pub pocket: PocketConfig,
    #[serde(default)]
    pub pronunciation: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HotkeysConfig {
    pub read: String,
    pub pause: String,
    pub stop: String,
    pub save: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecordingConfig {
    pub enabled: bool,
    pub folder: String,
    pub bitrate_kbps: u32,
}

impl RecordingConfig {
    /// The configured folder, or `Music/SpeakWrite` (Documents or home as fallbacks).
    pub fn folder(&self) -> PathBuf {
        let folder = self.folder.trim();
        if !folder.is_empty() {
            return PathBuf::from(folder);
        }
        let dirs = directories::UserDirs::new();
        let base = dirs
            .as_ref()
            .and_then(|d| d.audio_dir().or(d.document_dir()).map(Path::to_path_buf))
            .or_else(|| dirs.as_ref().map(|d| d.home_dir().to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("SpeakWrite")
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SelectionConfig {
    pub restore_clipboard: bool,
    pub fallback_to_clipboard: bool,
    pub copy_timeout_ms: u64,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub linux_primary_selection: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlaybackConfig {
    pub speed: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DictationConfig {
    pub enabled: bool,
    pub hotkey: String,
    pub insert: InsertMode,
    pub trailing_space: bool,
    pub release_delay_ms: u64,
    pub device: String,
    pub model: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InsertMode {
    Type,
    Paste,
}

impl InsertMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Type => "type",
            Self::Paste => "paste",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TtsConfig {
    pub engine: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PocketConfig {
    /// `language::AUTO` or a model name.
    pub language: String,
    pub auto_languages: Vec<String>,
    pub voice: String,
    pub binary: String,
}

impl Default for Config {
    fn default() -> Self {
        toml::from_str(DEFAULT_CONFIG).expect("built-in default config is valid")
    }
}

impl Config {
    /// Reads the config file, creating it with the defaults on first launch.
    pub fn load_or_create(paths: &Paths) -> Result<Self> {
        let file = &paths.config_file;
        if !file.exists() {
            std::fs::write(file, DEFAULT_CONFIG)
                .with_context(|| format!("cannot write {}", file.display()))?;
        }
        let mut text = std::fs::read_to_string(file)
            .with_context(|| format!("cannot read {}", file.display()))?;
        let mut user: toml::Table =
            toml::from_str(&text).with_context(|| format!("invalid TOML in {}", file.display()))?;
        if needs_pause_migration(&user) {
            // Files from before the pause key: F10 stopped, it now pauses (Shift+F10 stops).
            tracing::info!("config: F10 becomes pause, Shift+F10 stop");
            save(paths, |doc| {
                doc["hotkeys"]["stop"] = toml_edit::value("Shift+F10");
                doc["hotkeys"]["pause"] = toml_edit::value("F10");
            })?;
            text = std::fs::read_to_string(file)?;
            user = toml::from_str(&text)?;
        }
        if needs_language_migration(&user) {
            // Files from before language detection read everything in French.
            tracing::info!("config: reading language becomes automatic (French, English)");
            save(paths, |doc| {
                doc["pocket"]["language"] = toml_edit::value(crate::language::AUTO);
                doc["pocket"]["auto_languages"] =
                    toml_edit::value(toml_edit::Array::from_iter(["french", "english"]));
                if let Some(mut key) = doc["pocket"].as_table_mut().and_then(|t| t.key_mut("auto_languages")) {
                    key.leaf_decor_mut().set_prefix(
                        "# Langues reconnues quand language = \"auto\" ; la première sert quand le texte\n\
                         # est trop court pour trancher.\n",
                    );
                }
            })?;
            text = std::fs::read_to_string(file)?;
            user = toml::from_str(&text)?;
        }
        // Missing sections or keys fall back to the defaults.
        let mut merged: toml::Table = toml::from_str(DEFAULT_CONFIG)?;
        merge_tables(&mut merged, user);
        let mut config: Config = toml::Value::Table(merged)
            .try_into()
            .with_context(|| format!("invalid settings in {}", file.display()))?;
        let speed = config.playback.speed;
        let (min, max) = (crate::stretch::MIN_SPEED, crate::stretch::MAX_SPEED);
        if !(min..=max).contains(&speed) {
            tracing::warn!(speed, "playback speed out of range, clamped to {min}..{max}");
            config.playback.speed = speed.clamp(min, max);
        }
        config.pocket.check_languages();
        Ok(config)
    }
}

impl PocketConfig {
    /// Drops unknown language names, so that a typo does not stop the engine.
    fn check_languages(&mut self) {
        use crate::language;
        self.auto_languages.retain(|name| {
            let known = language::is_known(name);
            if !known {
                tracing::warn!(%name, "unknown language in auto_languages, ignored");
            }
            known
        });
        if self.auto_languages.is_empty() {
            self.auto_languages.push("french".to_string());
        }
        if self.language != language::AUTO && !language::is_known(&self.language) {
            tracing::warn!(language = %self.language, "unknown reading language, using \"auto\"");
            self.language = language::AUTO.to_string();
        }
    }

    /// The languages whose engines run from the start: the first one is ready before the
    /// reader reports itself ready.
    pub fn startup_languages(&self) -> Vec<String> {
        if self.language == crate::language::AUTO {
            self.auto_languages.clone()
        } else {
            vec![self.language.clone()]
        }
    }

    /// The `--default-voice` argument: a built-in voice name, or a file resolved against the
    /// config directory. `None` for the model's own default.
    pub fn voice_argument(&self, paths: &Paths) -> Result<Option<OsString>> {
        let voice = self.voice.trim();
        if voice.is_empty() {
            return Ok(None);
        }
        if crate::voices::is_builtin(voice) {
            return Ok(Some(voice.into()));
        }
        let path = Path::new(voice);
        let path = if path.is_absolute() { path.to_path_buf() } else { paths.config_dir.join(path) };
        if !path.is_file() {
            bail!("voice file not found: {}", path.display());
        }
        // Long or noisy recordings are reduced to the best 25 s of speech (cached).
        let prepared = crate::voiceprep::prepared_voice(&path, &paths.voice_cache_dir)?;
        Ok(Some(prepared.into_os_string()))
    }
}

/// Changes settings in the config file, keeping the user's comments and layout.
pub fn save(paths: &Paths, edit: impl FnOnce(&mut toml_edit::DocumentMut)) -> Result<()> {
    let file = &paths.config_file;
    let text = std::fs::read_to_string(file).unwrap_or_else(|_| DEFAULT_CONFIG.to_string());
    let mut document: toml_edit::DocumentMut =
        text.parse().with_context(|| format!("invalid TOML in {}", file.display()))?;
    edit(&mut document);
    // Write then rename: the file is never left half-written.
    let staging = file.with_extension("toml.tmp");
    std::fs::write(&staging, document.to_string())
        .with_context(|| format!("cannot write {}", staging.display()))?;
    std::fs::rename(&staging, file).with_context(|| format!("cannot update {}", file.display()))
}

/// Speeds are stored with two decimals: 1.1, not 1.100000023841858.
pub fn speed_value(speed: f32) -> toml_edit::Item {
    toml_edit::value((speed as f64 * 100.0).round() / 100.0)
}

/// The old default bound F10 to stop and had no pause key.
fn needs_pause_migration(user: &toml::Table) -> bool {
    let Some(hotkeys) = user.get("hotkeys").and_then(toml::Value::as_table) else {
        return false;
    };
    hotkeys.get("pause").is_none() && hotkeys.get("stop").and_then(toml::Value::as_str) == Some("F10")
}

/// Before language detection, `language` was the only key and defaulted to French.
fn needs_language_migration(user: &toml::Table) -> bool {
    let Some(pocket) = user.get("pocket").and_then(toml::Value::as_table) else {
        return false;
    };
    pocket.get("auto_languages").is_none()
        && pocket.get("language").and_then(toml::Value::as_str) == Some("french")
}

fn merge_tables(base: &mut toml::Table, overlay: toml::Table) {
    for (key, value) in overlay {
        match (base.get_mut(&key), value) {
            (Some(toml::Value::Table(base_sub)), toml::Value::Table(sub)) => {
                merge_tables(base_sub, sub)
            }
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

/// Standard per-OS locations:
/// - Windows: `%APPDATA%\SpeakWrite\config`, `%LOCALAPPDATA%\SpeakWrite\data`
/// - macOS: `~/Library/Application Support/SpeakWrite`
/// - Linux: `~/.config/speakwrite`, `~/.local/share/speakwrite`
#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub voices_dir: PathBuf,
    /// Voices prepared for cloning (derived from `voices_dir`, safe to delete).
    pub voice_cache_dir: PathBuf,
    pub log_dir: PathBuf,
    pub bin_dir: PathBuf,
}

impl Paths {
    pub fn new() -> Result<Self> {
        let dirs = directories::ProjectDirs::from("", "", "SpeakWrite")
            .context("cannot determine the user's home directory")?;
        let config_dir = dirs.config_dir().to_path_buf();
        let data_dir = dirs.data_local_dir().to_path_buf();
        let paths = Self {
            config_file: config_dir.join("config.toml"),
            voices_dir: config_dir.join("voices"),
            voice_cache_dir: data_dir.join("voice-cache"),
            config_dir,
            log_dir: data_dir.join("logs"),
            bin_dir: data_dir.join("bin"),
        };
        for dir in [&paths.config_dir, &paths.voices_dir, &paths.voice_cache_dir, &paths.log_dir, &paths.bin_dir] {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("cannot create {}", dir.display()))?;
        }
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_parse() {
        let config = Config::default();
        assert_eq!(config.hotkeys.read, "F9");
        assert_eq!(config.tts.engine, "pocket");
        assert!(config.pronunciation.is_empty());
    }

    #[test]
    fn pause_migration() {
        let old: toml::Table = toml::from_str("[hotkeys]\nread = \"F9\"\nstop = \"F10\"\n").unwrap();
        assert!(needs_pause_migration(&old));
        let custom: toml::Table = toml::from_str("[hotkeys]\nstop = \"F12\"\n").unwrap();
        assert!(!needs_pause_migration(&custom));
        let current: toml::Table = toml::from_str(DEFAULT_CONFIG).unwrap();
        assert!(!needs_pause_migration(&current));
    }

    #[test]
    fn language_migration() {
        let old: toml::Table = toml::from_str("[pocket]\nlanguage = \"french\"\nvoice = \"\"\n").unwrap();
        assert!(needs_language_migration(&old));
        let chosen: toml::Table =
            toml::from_str("[pocket]\nlanguage = \"french\"\nauto_languages = [\"french\"]\n").unwrap();
        assert!(!needs_language_migration(&chosen));
        let current: toml::Table = toml::from_str(DEFAULT_CONFIG).unwrap();
        assert!(!needs_language_migration(&current));
        let config = Config::default();
        assert_eq!(config.pocket.language, crate::language::AUTO);
        assert_eq!(config.pocket.startup_languages(), ["french", "english"]);
    }

    #[test]
    fn edits_keep_comments() {
        let mut document: toml_edit::DocumentMut = DEFAULT_CONFIG.parse().unwrap();
        document["playback"]["speed"] = speed_value(1.1);
        document["pocket"]["voice"] = toml_edit::value("voices/ma voix.wav");
        let text = document.to_string();
        assert!(text.contains("speed = 1.1\n"));
        assert!(text.contains(r#"voice = "voices/ma voix.wav""#));
        assert!(text.contains("# Vitesse de lecture"));
        let config: Config = toml::from_str(&text).unwrap();
        assert_eq!(config.playback.speed, 1.1);
    }

    #[test]
    fn partial_user_file_keeps_defaults() {
        let mut merged: toml::Table = toml::from_str(DEFAULT_CONFIG).unwrap();
        let user: toml::Table =
            toml::from_str("[hotkeys]\nread = \"F8\"\n[pronunciation]\nOTAN = \"otane\"\n").unwrap();
        merge_tables(&mut merged, user);
        let config: Config = toml::Value::Table(merged).try_into().unwrap();
        assert_eq!(config.hotkeys.read, "F8");
        assert_eq!(config.hotkeys.stop, "Shift+F10");
        assert_eq!(config.hotkeys.pause, "F10");
        assert_eq!(config.pronunciation["OTAN"], "otane");
    }
}
