//! Speech synthesis engines.

mod install;
mod pocket;
mod wav;

use std::collections::HashMap;

use anyhow::{Result, bail};

use crate::config::{Config, Paths};

/// A block of interleaved samples in [-1, 1].
pub struct AudioChunk {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

pub trait TtsEngine: Send {
    /// Blocks until the engine can synthesize (model loaded). Called before every use.
    fn wait_ready(&mut self) -> Result<()>;

    fn is_ready(&self) -> bool;

    /// Streams the audio of `text` to `on_audio` as it is generated. Returns early, without
    /// error, as soon as `is_cancelled` returns true.
    fn synthesize(
        &mut self,
        text: &str,
        is_cancelled: &dyn Fn() -> bool,
        on_audio: &mut dyn FnMut(AudioChunk),
    ) -> Result<()>;
}

/// One engine per language, started on demand: a Pocket TTS model reads a single language.
pub struct Engines {
    config: Config,
    paths: Paths,
    running: HashMap<String, Box<dyn TtsEngine>>,
}

impl Engines {
    pub fn new(config: Config, paths: Paths) -> Self {
        Self { config, paths, running: HashMap::new() }
    }

    /// Starts the engine of `language` in the background, if it is not running yet.
    pub fn launch(&mut self, language: &str) -> Result<()> {
        if !self.running.contains_key(language) {
            let engine: Box<dyn TtsEngine> = match self.config.tts.engine.as_str() {
                "pocket" => Box::new(pocket::PocketTts::spawn(&self.config.pocket, language, &self.paths)?),
                other => bail!("unknown TTS engine '{other}' (available: pocket)"),
            };
            self.running.insert(language.to_string(), engine);
        }
        Ok(())
    }

    /// Starts the engine of `language` if needed and waits until it is ready. May take
    /// minutes on first use (downloads). A failed engine is dropped, to be retried next time.
    pub fn start(&mut self, language: &str) -> Result<()> {
        self.launch(language)?;
        let engine = self.running.get_mut(language).expect("launched above");
        if let Err(e) = engine.wait_ready() {
            self.running.remove(language);
            return Err(e);
        }
        Ok(())
    }

    pub fn is_ready(&self, language: &str) -> bool {
        self.running.get(language).is_some_and(|engine| engine.is_ready())
    }

    /// The engine of `language`, once [`Engines::start`] succeeded.
    pub fn get(&mut self, language: &str) -> Option<&mut dyn TtsEngine> {
        Some(self.running.get_mut(language)?.as_mut())
    }
}
