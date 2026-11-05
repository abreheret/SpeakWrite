//! Voice catalogue: voices bundled with Pocket TTS, and the user's own recordings.
//!
//! The config stores a voice as either a built-in name ("estelle") or a path to a file,
//! relative to the config directory ("voices/ma_voix.wav").

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::config::Paths;

/// Voices predefined by pocket-tts; each one has French embeddings.
pub const BUILTIN: [&str; 27] = [
    "estelle", "alba", "anna", "azelma", "bill_boerst", "caro_davy", "charles", "cosette", "daan",
    "eponine", "eve", "fantine", "george", "giovanni", "jane", "javert", "jean", "juergen", "lola",
    "marius", "mary", "michael", "paul", "peter_yearsley", "rafael", "stuart_bell", "vera",
];

/// Used by the French model when the config names no voice.
pub const DEFAULT_FRENCH: &str = "estelle";

/// Audio formats the pocket-tts decoder reads, plus exported voices.
pub const FILE_EXTENSIONS: [&str; 6] = ["wav", "mp3", "m4a", "flac", "ogg", "safetensors"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceEntry {
    /// Shown in the menu.
    pub label: String,
    /// Stored in the config.
    pub value: String,
}

pub fn is_builtin(value: &str) -> bool {
    BUILTIN.contains(&value)
}

/// "bill_boerst" -> "Bill Boerst".
fn builtin_label(name: &str) -> String {
    name.split('_')
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn builtin_voices() -> Vec<VoiceEntry> {
    BUILTIN
        .iter()
        .map(|name| VoiceEntry { label: builtin_label(name), value: name.to_string() })
        .collect()
}

/// Recordings found in the voices folder, sorted by name.
pub fn user_voices(paths: &Paths) -> Vec<VoiceEntry> {
    let Ok(entries) = std::fs::read_dir(&paths.voices_dir) else {
        return Vec::new();
    };
    let mut voices: Vec<VoiceEntry> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && has_voice_extension(path))
        .filter_map(|path| {
            let file_name = path.file_name()?.to_string_lossy().into_owned();
            let label = path.file_stem()?.to_string_lossy().replace('_', " ");
            Some(VoiceEntry { label, value: format!("voices/{file_name}") })
        })
        .collect();
    voices.sort_by_key(|voice| voice.label.to_lowercase());
    voices
}

/// The menu label of a configured voice.
pub fn label_of(value: &str) -> String {
    let value = if value.trim().is_empty() { DEFAULT_FRENCH } else { value.trim() };
    if is_builtin(value) {
        return builtin_label(value);
    }
    Path::new(value)
        .file_stem()
        .map_or_else(|| value.to_string(), |stem| stem.to_string_lossy().replace('_', " "))
}

fn has_voice_extension(path: &Path) -> bool {
    path.extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .is_some_and(|ext| FILE_EXTENSIONS.contains(&ext.as_str()))
}

/// Copies a recording into the voices folder (never overwriting) and returns its entry.
pub fn import(paths: &Paths, source: &Path) -> Result<VoiceEntry> {
    if !has_voice_extension(source) {
        bail!("unsupported file type: {}", source.display());
    }
    let stem = source.file_stem().context("file without a name")?.to_string_lossy().into_owned();
    let extension = source.extension().context("file without extension")?.to_string_lossy().to_lowercase();
    let destination = unique_path(&paths.voices_dir, &stem, &extension);
    std::fs::copy(source, &destination).with_context(|| {
        format!("cannot copy {} to {}", source.display(), destination.display())
    })?;
    let file_name = destination.file_name().expect("has a name").to_string_lossy().into_owned();
    Ok(VoiceEntry { label: label_of(&file_name), value: format!("voices/{file_name}") })
}

/// Renames a recording of the voices folder, keeping its extension, and returns its new entry.
/// `new_name` is what the user typed; an extension they added is ignored.
pub fn rename(paths: &Paths, value: &str, new_name: &str) -> Result<VoiceEntry> {
    let file_name = Path::new(value).file_name().context("not a voice file")?;
    let source = paths.voices_dir.join(file_name);
    let extension = source.extension().context("file without extension")?.to_string_lossy().into_owned();
    let stem = file_stem_for(new_name, &extension)?;
    let destination = paths.voices_dir.join(format!("{stem}.{extension}"));
    // A change of case only is the same file on case-insensitive file systems (macOS, Windows).
    let same_file = destination.to_string_lossy().to_lowercase() == source.to_string_lossy().to_lowercase();
    if destination.exists() && !same_file {
        bail!("a voice named \"{stem}\" already exists");
    }
    std::fs::rename(&source, &destination).with_context(|| {
        format!("cannot rename {} to {}", source.display(), destination.display())
    })?;
    let file_name = destination.file_name().expect("has a name").to_string_lossy().into_owned();
    Ok(VoiceEntry { label: label_of(&file_name), value: format!("voices/{file_name}") })
}

/// The file stem for a typed voice name: trimmed, without the voice's own extension.
fn file_stem_for(name: &str, extension: &str) -> Result<String> {
    let name = name.trim();
    let suffix = format!(".{}", extension.to_lowercase());
    let name = if name.to_lowercase().ends_with(&suffix) { &name[..name.len() - suffix.len()] } else { name };
    let name = name.trim();
    if name.is_empty() || name.starts_with('.') {
        bail!("the name is empty");
    }
    if let Some(c) = name.chars().find(|c| matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()) {
        bail!("the name cannot contain \"{c}\"");
    }
    Ok(name.to_string())
}

fn unique_path(dir: &Path, stem: &str, extension: &str) -> PathBuf {
    let mut candidate = dir.join(format!("{stem}.{extension}"));
    let mut n = 2;
    while candidate.exists() {
        candidate = dir.join(format!("{stem} ({n}).{extension}"));
        n += 1;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert_eq!(label_of(""), "Estelle");
        assert_eq!(label_of("bill_boerst"), "Bill Boerst");
        assert_eq!(label_of("voices/ma_voix.wav"), "ma voix");
        assert!(is_builtin("estelle"));
        assert!(!is_builtin("voices/estelle.wav"));
    }

    #[test]
    fn rename_names() {
        assert_eq!(file_stem_for("  Ma voix  ", "wav").unwrap(), "Ma voix");
        assert_eq!(file_stem_for("Ma voix.WAV", "wav").unwrap(), "Ma voix");
        assert_eq!(file_stem_for("v1.2", "mp3").unwrap(), "v1.2");
        assert!(file_stem_for("   ", "wav").is_err());
        assert!(file_stem_for(".wav", "wav").is_err());
        assert!(file_stem_for("../voix", "wav").is_err());
        assert!(file_stem_for("a:b", "wav").is_err());
    }

    #[test]
    fn rename_file() {
        let dir = std::env::temp_dir().join(format!("speakwrite-rename-{}", std::process::id()));
        let paths = Paths {
            config_dir: dir.clone(),
            config_file: dir.join("config.toml"),
            voices_dir: dir.join("voices"),
            voice_cache_dir: dir.join("voice-cache"),
            log_dir: dir.join("logs"),
            bin_dir: dir.join("bin"),
        };
        std::fs::create_dir_all(&paths.voices_dir).unwrap();
        std::fs::write(paths.voices_dir.join("ma_voix.wav"), b"x").unwrap();
        std::fs::write(paths.voices_dir.join("autre.wav"), b"y").unwrap();

        let entry = rename(&paths, "voices/ma_voix.wav", "Amaury").unwrap();
        assert_eq!(entry, VoiceEntry { label: "Amaury".into(), value: "voices/Amaury.wav".into() });
        assert!(paths.voices_dir.join("Amaury.wav").is_file());
        assert!(rename(&paths, "voices/Amaury.wav", "autre").is_err());
        assert!(rename(&paths, "voices/Amaury.wav", "amaury").is_ok());

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
