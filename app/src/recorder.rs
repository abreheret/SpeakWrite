//! Saves readings as MP3 files (LAME, through `mp3lame-encoder`).
//!
//! Encoding runs as the audio is synthesised, into a temporary file renamed at the end: a
//! cancelled reading never leaves a truncated MP3 behind.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use mp3lame_encoder::{Bitrate, Builder, Encoder, FlushNoGap, MonoPcm, Quality};

pub struct Mp3Writer {
    encoder: Encoder,
    file: BufWriter<File>,
    staging: PathBuf,
    path: PathBuf,
    buffer: Vec<u8>,
}

impl Mp3Writer {
    pub fn create(path: PathBuf, sample_rate: u32, bitrate_kbps: u32) -> Result<Self> {
        let mut builder = Builder::new().context("cannot create the MP3 encoder")?;
        builder.set_num_channels(1).map_err(|e| anyhow!("MP3 encoder: {e:?}"))?;
        builder.set_sample_rate(sample_rate).map_err(|e| anyhow!("MP3 encoder: {e:?}"))?;
        builder.set_brate(bitrate(bitrate_kbps)).map_err(|e| anyhow!("MP3 encoder: {e:?}"))?;
        builder.set_quality(Quality::Good).map_err(|e| anyhow!("MP3 encoder: {e:?}"))?;
        let encoder = builder.build().map_err(|e| anyhow!("MP3 encoder: {e:?}"))?;

        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
        }
        let staging = path.with_extension("mp3.part");
        let file = File::create(&staging).with_context(|| format!("cannot create {}", staging.display()))?;
        Ok(Self { encoder, file: BufWriter::new(file), staging, path, buffer: Vec::new() })
    }

    /// Mono samples in [-1, 1], at the sample rate given to `create`.
    pub fn write(&mut self, samples: &[f32]) -> Result<()> {
        self.buffer.clear();
        self.buffer.reserve(mp3lame_encoder::max_required_buffer_size(samples.len()));
        self.encoder
            .encode_to_vec(MonoPcm(samples), &mut self.buffer)
            .map_err(|e| anyhow!("MP3 encoding failed: {e:?}"))?;
        self.file.write_all(&self.buffer)?;
        Ok(())
    }

    /// Completes the file and moves it to its final name.
    pub fn finish(mut self) -> Result<PathBuf> {
        self.buffer.clear();
        self.buffer.reserve(7200); // LAME's documented maximum for a flush
        self.encoder
            .flush_to_vec::<FlushNoGap>(&mut self.buffer)
            .map_err(|e| anyhow!("MP3 encoding failed: {e:?}"))?;
        self.file.write_all(&self.buffer)?;
        self.file.flush()?;
        let path = unique_path(&self.path);
        std::fs::rename(&self.staging, &path)
            .with_context(|| format!("cannot write {}", path.display()))?;
        Ok(path)
    }
}

impl Drop for Mp3Writer {
    fn drop(&mut self) {
        // Only still there when the recording was abandoned (`finish` renamed it).
        let _ = std::fs::remove_file(&self.staging);
    }
}

/// LAME takes discrete bitrates; MPEG-2 (24 kHz) tops out at 160 kbit/s.
fn bitrate(kbps: u32) -> Bitrate {
    match kbps {
        0..=40 => Bitrate::Kbps32,
        41..=56 => Bitrate::Kbps48,
        57..=72 => Bitrate::Kbps64,
        73..=88 => Bitrate::Kbps80,
        89..=104 => Bitrate::Kbps96,
        105..=120 => Bitrate::Kbps112,
        121..=144 => Bitrate::Kbps128,
        _ => Bitrate::Kbps160,
    }
}

/// "2026-11-05 14-32-10 Bonjour voici le texte.mp3": date, then the first words.
pub fn file_name(text: &str, now: chrono::DateTime<chrono::Local>) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '\'' || c == '-' { c } else { ' ' })
        .collect();
    let mut title = String::new();
    for word in cleaned.split_whitespace().take(8) {
        if title.chars().count() + word.chars().count() > 60 {
            break;
        }
        if !title.is_empty() {
            title.push(' ');
        }
        title.push_str(word);
    }
    let stamp = now.format("%Y-%m-%d %H-%M-%S");
    if title.is_empty() { format!("{stamp}.mp3") } else { format!("{stamp} {title}.mp3") }
}

fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    (2..)
        .map(|n| path.with_file_name(format!("{stem} ({n}).mp3")))
        .find(|candidate| !candidate.exists())
        .expect("an unused name exists")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn names_from_text() {
        let now = chrono::Local.with_ymd_and_hms(2026, 11, 5, 14, 32, 10).unwrap();
        assert_eq!(
            file_name("Bonjour ! Voici: le texte <à> lire/écouter.", now),
            "2026-11-05 14-32-10 Bonjour Voici le texte à lire écouter.mp3"
        );
        assert_eq!(file_name("   ", now), "2026-11-05 14-32-10.mp3");
    }

    #[test]
    fn encodes_a_playable_file() {
        let dir = std::env::temp_dir().join(format!("speakwrite-test-{}", std::process::id()));
        let path = dir.join("test.mp3");
        let mut writer = Mp3Writer::create(path.clone(), 24_000, 64).unwrap();
        let tone: Vec<f32> =
            (0..24_000).map(|n| 0.3 * (std::f32::consts::TAU * 440.0 * n as f32 / 24_000.0).sin()).collect();
        for chunk in tone.chunks(4096) {
            writer.write(chunk).unwrap();
        }
        let written = writer.finish().unwrap();
        let bytes = std::fs::read(&written).unwrap();
        // 1 s at 64 kbit/s is about 8 kB; MP3 frames start with an 11-bit sync word.
        assert!((5_000..12_000).contains(&bytes.len()), "{} bytes", bytes.len());
        assert!(bytes.windows(2).any(|w| w[0] == 0xFF && w[1] & 0xE0 == 0xE0));
        assert!(!dir.join("test.mp3.part").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn abandoned_recording_leaves_nothing() {
        let dir = std::env::temp_dir().join(format!("speakwrite-test-drop-{}", std::process::id()));
        let writer = Mp3Writer::create(dir.join("x.mp3"), 24_000, 64).unwrap();
        drop(writer);
        assert!(std::fs::read_dir(&dir).unwrap().next().is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
