//! Microphone capture. The stream is opened only while dictating, so that the OS "microphone
//! in use" indicator reflects reality.

use anyhow::{Context, Result};
use crossbeam_channel::Receiver;
use rodio::cpal;
use rodio::cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub struct Microphone {
    // Not `Send` on every platform: created, used and dropped on the dictation thread.
    _stream: cpal::Stream,
    pub samples: Receiver<Vec<f32>>,
    pub sample_rate: u32,
}

impl Microphone {
    /// Opens the default input device; samples arrive mixed down to mono.
    pub fn open() -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .context("no microphone found (on macOS: grant microphone access)")?;
        let supported = device.default_input_config().context("cannot query the microphone")?;
        let sample_rate = supported.sample_rate();
        let channels = supported.channels().max(1) as usize;
        let format = supported.sample_format();
        let config = supported.config();
        let (tx, samples) = crossbeam_channel::unbounded();
        let on_error = |e| tracing::error!("microphone error: {e}");

        macro_rules! build {
            ($t:ty) => {
                device.build_input_stream::<$t, _, _>(
                    config,
                    move |data: &[$t], _| {
                        let mono = data
                            .chunks(channels)
                            .map(|frame| {
                                frame.iter().map(|&s| <f32 as cpal::FromSample<$t>>::from_sample_(s)).sum::<f32>()
                                    / channels as f32
                            })
                            .collect();
                        let _ = tx.send(mono);
                    },
                    on_error,
                    None,
                )
            };
        }
        let stream = match format {
            cpal::SampleFormat::F32 => build!(f32),
            cpal::SampleFormat::I16 => build!(i16),
            cpal::SampleFormat::U16 => build!(u16),
            cpal::SampleFormat::I32 => build!(i32),
            other => anyhow::bail!("unsupported microphone sample format: {other}"),
        }
        .context("cannot open the microphone")?;
        stream.play().context("cannot start the microphone")?;
        tracing::info!(sample_rate, channels, %format, "microphone open");
        Ok(Self { _stream: stream, samples, sample_rate })
    }
}
