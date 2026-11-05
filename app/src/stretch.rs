//! Playback speed without pitch change: streaming WSOLA time-stretch.
//!
//! WSOLA (waveform similarity overlap-add): the output is built from 30 ms windowed frames,
//! overlapped by half. Frames are read from the input every `hop * speed` samples, each one
//! shifted by up to ±7.5 ms to the position that best continues the previous frame, which
//! avoids the phasing artefacts of a plain overlap-add. Well suited to speech at 0.5x-2x.

use crate::tts::AudioChunk;

pub const MIN_SPEED: f32 = 0.5;
pub const MAX_SPEED: f32 = 2.0;

pub struct TimeStretch {
    speed: f64,
    frame: usize,
    hop: usize,
    tolerance: usize,
    window: Vec<f32>,
    /// Buffered input; `input[0]` is the absolute sample `base`.
    input: Vec<f32>,
    base: usize,
    received: usize,
    /// Ideal absolute input position of the next frame.
    position: f64,
    /// Absolute input start of the previous frame.
    previous: Option<usize>,
    /// Second half of the previous windowed frame, waiting for its overlap.
    tail: Vec<f32>,
}

impl TimeStretch {
    pub fn new(speed: f32, sample_rate: u32) -> Self {
        let frame = (((sample_rate as f64 * 0.030).round() as usize).max(64)) & !1;
        let hop = frame / 2;
        // Periodic Hann window: two copies offset by half a frame sum to exactly 1.
        let window = (0..frame)
            .map(|n| (0.5 - 0.5 * (std::f64::consts::TAU * n as f64 / frame as f64).cos()) as f32)
            .collect();
        Self {
            speed: speed.clamp(MIN_SPEED, MAX_SPEED) as f64,
            frame,
            hop,
            tolerance: hop / 2,
            window,
            input: Vec::new(),
            base: 0,
            received: 0,
            position: 0.0,
            previous: None,
            tail: vec![0.0; hop],
        }
    }

    fn is_passthrough(&self) -> bool {
        (self.speed - 1.0).abs() < 1e-3
    }

    /// Feeds input samples (mono), returns the stretched samples available so far.
    pub fn process(&mut self, samples: &[f32]) -> Vec<f32> {
        if self.is_passthrough() {
            return samples.to_vec();
        }
        self.input.extend_from_slice(samples);
        self.received += samples.len();
        let mut out = Vec::with_capacity((samples.len() as f64 / self.speed) as usize + self.hop);
        self.run(&mut out, None);
        out
    }

    /// Flushes the end of the stream.
    pub fn finish(&mut self) -> Vec<f32> {
        if self.is_passthrough() {
            return Vec::new();
        }
        let end = self.received;
        // Silence after the end, so that the last frames can be read whole.
        let padding = self.frame + 2 * self.tolerance + self.hop;
        self.input.resize(self.input.len() + padding, 0.0);
        let mut out = Vec::new();
        self.run(&mut out, Some(end));
        out.extend_from_slice(&self.tail);
        self.tail.fill(0.0);
        out
    }

    fn run(&mut self, out: &mut Vec<f32>, end: Option<usize>) {
        let available = self.base + self.input.len();
        loop {
            let target = self.position.round() as usize;
            if end.is_some_and(|end| target >= end) {
                break;
            }
            let natural = self.previous.map(|p| p + self.hop);
            if target + self.tolerance + self.frame > available
                || natural.is_some_and(|n| n + self.hop > available)
            {
                break; // wait for more input
            }
            let start = match natural {
                None => target,
                Some(natural) => self.best_start(target, natural),
            };
            let segment = &self.input[start - self.base..start - self.base + self.frame];
            for i in 0..self.hop {
                out.push(self.tail[i] + segment[i] * self.window[i]);
                self.tail[i] = segment[self.hop + i] * self.window[self.hop + i];
            }
            self.previous = Some(start);
            self.position += self.hop as f64 * self.speed;
        }
        // Drop the input that no future frame can reach.
        let next_low = (self.position.round() as usize).saturating_sub(self.tolerance);
        let keep_from = self.previous.map_or(next_low, |p| next_low.min(p + self.hop));
        if keep_from > self.base {
            let drop = (keep_from - self.base).min(self.input.len());
            self.input.drain(..drop);
            self.base += drop;
        }
    }

    /// Start in `target ± tolerance` whose beginning best matches the natural continuation of
    /// the previous frame (normalised cross-correlation over the overlap).
    fn best_start(&self, target: usize, natural: usize) -> usize {
        let low = target.saturating_sub(self.tolerance).max(self.base);
        let high = target + self.tolerance;
        let reference = &self.input[natural - self.base..natural - self.base + self.hop];
        let mut best = (f32::MIN, target.clamp(low, high));
        for start in low..=high {
            let candidate = &self.input[start - self.base..start - self.base + self.hop];
            let (mut dot, mut energy) = (0.0f32, 1e-9f32);
            for (a, b) in candidate.iter().zip(reference) {
                dot += a * b;
                energy += a * a;
            }
            let score = dot / energy.sqrt();
            if score > best.0 {
                best = (score, start);
            }
        }
        best.1
    }
}

/// Applies the configured speed to a stream of engine chunks.
pub struct SpeedControl {
    speed: f32,
    stretch: Option<(TimeStretch, u32, u16)>,
}

impl SpeedControl {
    pub fn new(speed: f32) -> Self {
        Self { speed, stretch: None }
    }

    pub fn process(&mut self, chunk: AudioChunk) -> Option<AudioChunk> {
        // Engines produce mono speech; anything else is played as is.
        if chunk.channels != 1 {
            return Some(chunk);
        }
        let (stretch, ..) = self
            .stretch
            .get_or_insert_with(|| (TimeStretch::new(self.speed, chunk.sample_rate), chunk.sample_rate, 1));
        let samples = stretch.process(&chunk.samples);
        (!samples.is_empty()).then_some(AudioChunk { samples, ..chunk })
    }

    pub fn finish(&mut self) -> Option<AudioChunk> {
        let (stretch, sample_rate, channels) = self.stretch.as_mut()?;
        let samples = stretch.finish();
        (!samples.is_empty()).then_some(AudioChunk {
            sample_rate: *sample_rate,
            channels: *channels,
            samples,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 24_000;

    fn sine(freq: f32, seconds: f32) -> Vec<f32> {
        (0..(RATE as f32 * seconds) as usize)
            .map(|n| 0.5 * (std::f32::consts::TAU * freq * n as f32 / RATE as f32).sin())
            .collect()
    }

    fn stretch_all(speed: f32, input: &[f32], chunk: usize) -> Vec<f32> {
        let mut stretch = TimeStretch::new(speed, RATE);
        let mut out = Vec::new();
        for part in input.chunks(chunk) {
            out.extend(stretch.process(part));
        }
        out.extend(stretch.finish());
        out
    }

    /// Frequency from the rising zero crossings, away from the edges.
    fn frequency(signal: &[f32]) -> f32 {
        let core = &signal[signal.len() / 5..signal.len() * 4 / 5];
        let crossings: Vec<usize> =
            (1..core.len()).filter(|&i| core[i - 1] < 0.0 && core[i] >= 0.0).collect();
        let span = (crossings[crossings.len() - 1] - crossings[0]) as f32;
        (crossings.len() - 1) as f32 * RATE as f32 / span
    }

    #[test]
    fn normal_speed_is_untouched() {
        let input = sine(220.0, 0.5);
        assert_eq!(stretch_all(1.0, &input, 1000), input);
    }

    #[test]
    fn faster_keeps_the_pitch() {
        let input = sine(200.0, 2.0);
        for speed in [1.25f32, 1.5, 0.8] {
            let out = stretch_all(speed, &input, 4096);
            let expected = input.len() as f32 / speed;
            let ratio = out.len() as f32 / expected;
            assert!((0.97..1.03).contains(&ratio), "speed {speed}: length ratio {ratio}");
            let f = frequency(&out);
            assert!((f - 200.0).abs() < 4.0, "speed {speed}: frequency {f}");
        }
    }

    #[test]
    fn streaming_matches_one_shot() {
        let input: Vec<f32> = sine(180.0, 1.0).iter().zip(sine(330.0, 1.0)).map(|(a, b)| a + b).collect();
        let one_shot = stretch_all(1.25, &input, input.len());
        let streamed = stretch_all(1.25, &input, 777);
        assert_eq!(one_shot, streamed);
    }
}
