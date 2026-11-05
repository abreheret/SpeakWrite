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
}
