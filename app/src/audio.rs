//! Audio output: plays streamed chunks gaplessly; stopping is instantaneous.

use std::num::NonZero;
use std::sync::Mutex;

use anyhow::{Context, Result};
use rodio::buffer::SamplesBuffer;
use rodio::mixer::Mixer;
use rodio::Player;

use crate::tts::AudioChunk;

pub struct AudioOut {
    mixer: Mixer,
    state: Mutex<State>,
}

struct State {
    /// Only chunks of this reading are played: late chunks of a cancelled one are dropped.
    generation: u64,
    player: Option<Player>,
}

impl AudioOut {
    pub fn new() -> Result<Self> {
        // The OS stream is not `Send` on every platform: it lives on its own thread for the
        // whole life of the app, and only its mixer handle is shared.
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("audio-output".into())
            .spawn(move || match rodio::DeviceSinkBuilder::open_default_sink() {
                Ok(mut sink) => {
                    sink.log_on_drop(false);
                    let _ = tx.send(Ok(sink.mixer().clone()));
                    loop {
                        std::thread::park();
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(anyhow::anyhow!("cannot open the audio output: {e}")));
                }
            })?;
        let mixer = rx.recv().context("audio output thread died")??;
        Ok(Self { mixer, state: Mutex::new(State { generation: 0, player: None }) })
    }

    /// Stops what is playing; from now on only chunks of `generation` are accepted.
    pub fn stop(&self, generation: u64) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.generation = generation;
        if let Some(player) = state.player.take() {
            player.stop();
        }
    }

    pub fn append(&self, generation: u64, chunk: AudioChunk) {
        let (Some(channels), Some(rate)) = (NonZero::new(chunk.channels), NonZero::new(chunk.sample_rate))
        else {
            return;
        };
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.generation != generation {
            return;
        }
        let mixer = &self.mixer;
        let player = state.player.get_or_insert_with(|| Player::connect_new(mixer));
        player.append(SamplesBuffer::new(channels, rate, chunk.samples));
    }

    /// Pauses or resumes the current reading. Returns the new state (`true` = paused), or
    /// `None` when nothing is playing. Chunks still arriving are queued while paused.
    pub fn toggle_pause(&self) -> Option<bool> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let player = state.player.as_ref().filter(|p| !p.empty())?;
        if player.is_paused() {
            player.play();
            Some(false)
        } else {
            player.pause();
            Some(true)
        }
    }

    /// True while chunks of the current reading are queued or playing (paused included).
    pub fn is_playing(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.player.as_ref().is_some_and(|p| !p.empty())
    }
}
