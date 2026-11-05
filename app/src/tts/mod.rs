//! Speech synthesis engines.

mod install;
mod pocket;
mod wav;

use anyhow::{Result, bail};

use crate::config::{Config, Paths};

/// A block of interleaved samples in [-1, 1].
pub struct AudioChunk {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

pub trait TtsEngine: Send {
    /// Streams the audio of `text` to `on_audio` as it is generated. Returns early, without
    /// error, as soon as `is_cancelled` returns true.
    fn synthesize(
        &mut self,
        text: &str,
        is_cancelled: &dyn Fn() -> bool,
        on_audio: &mut dyn FnMut(AudioChunk),
    ) -> Result<()>;
}

/// Starts the engine named in the config. May take minutes on first launch (downloads).
pub fn start_engine(config: &Config, paths: &Paths) -> Result<Box<dyn TtsEngine>> {
    match config.tts.engine.as_str() {
        "pocket" => Ok(Box::new(pocket::PocketTts::start(&config.pocket, paths)?)),
        other => bail!("unknown TTS engine '{other}' (available: pocket)"),
    }
}
