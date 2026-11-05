//! Streaming windowed-sinc resampler (microphone rate -> 24 kHz for the STT model).

use std::f64::consts::PI;

/// Zero crossings of the sinc kept on each side: plenty for speech recognition.
const HALF_TAPS: usize = 16;

pub struct Resampler {
    /// Input samples per output sample.
    step: f64,
    /// Low-pass cutoff relative to the input Nyquist (below 1 when downsampling).
    cutoff: f64,
    passthrough: bool,
    history: Vec<f32>,
    /// Position of the next output sample, in input samples relative to `history[0]`.
    position: f64,
}

impl Resampler {
    pub fn new(input_rate: u32, output_rate: u32) -> Self {
        let step = input_rate as f64 / output_rate as f64;
        Self {
            step,
            cutoff: (1.0 / step).min(1.0) * 0.95,
            passthrough: input_rate == output_rate,
            // Leading zeros stand in for the samples before the stream starts.
            history: vec![0.0; HALF_TAPS],
            position: HALF_TAPS as f64,
        }
    }

    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        if self.passthrough {
            return input.to_vec();
        }
        self.history.extend_from_slice(input);
        let reach = (HALF_TAPS as f64 / self.cutoff).ceil() as usize;
        let mut out = Vec::with_capacity((input.len() as f64 / self.step) as usize + 1);
        while (self.position as usize) + reach < self.history.len() {
            out.push(self.sample_at(self.position, reach));
            self.position += self.step;
        }
        // Keep only what the next outputs can reach.
        let keep_from = (self.position as usize).saturating_sub(reach);
        self.history.drain(..keep_from);
        self.position -= keep_from as f64;
        out
    }

    fn sample_at(&self, position: f64, reach: usize) -> f32 {
        let center = position.floor() as usize;
        let frac = position - center as f64;
        let mut acc = 0.0f64;
        let mut weight_sum = 0.0f64;
        for i in center.saturating_sub(reach)..=(center + reach).min(self.history.len() - 1) {
            let x = (i as f64 - center as f64 - frac) * self.cutoff;
            if x.abs() >= HALF_TAPS as f64 {
                continue;
            }
            let sinc = if x == 0.0 { 1.0 } else { (PI * x).sin() / (PI * x) };
            // Blackman window over [-HALF_TAPS, HALF_TAPS].
            let t = (x / HALF_TAPS as f64 + 1.0) / 2.0;
            let window = 0.42 - 0.5 * (2.0 * PI * t).cos() + 0.08 * (4.0 * PI * t).cos();
            let w = sinc * window;
            acc += self.history[i] as f64 * w;
            weight_sum += w;
        }
        // Normalised: unit gain at DC whatever the fractional position.
        (acc / weight_sum.max(1e-9)) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, rate: u32, seconds: f64) -> Vec<f32> {
        (0..(rate as f64 * seconds) as usize)
            .map(|n| (2.0 * PI * freq * n as f64 / rate as f64).sin() as f32)
            .collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    #[test]
    fn keeps_speech_band_and_length() {
        for input_rate in [48_000u32, 44_100, 16_000] {
            let mut resampler = Resampler::new(input_rate, 24_000);
            let input = sine(1000.0, input_rate, 1.0);
            let mut out = Vec::new();
            for chunk in input.chunks(480) {
                out.extend(resampler.process(chunk));
            }
            let expected = 24_000.0;
            assert!((out.len() as f64 - expected).abs() < 100.0, "{input_rate}: {} samples", out.len());
            let core = &out[2000..out.len() - 2000];
            let level = rms(core);
            assert!((level - 0.707).abs() < 0.03, "{input_rate}: rms {level}");
        }
    }

    #[test]
    fn removes_frequencies_above_the_new_nyquist() {
        let mut resampler = Resampler::new(48_000, 24_000);
        let out = resampler.process(&sine(15_000.0, 48_000, 1.0));
        let level = rms(&out[2000..out.len() - 2000]);
        assert!(level < 0.05, "aliasing rms {level}");
    }
}
