//! Prepares a voice recording for cloning.
//!
//! pocket-tts conditions on the whole file given to `serve --default-voice`: a 4-minute
//! recording made it try to allocate 73 GB and crash. Recordings are therefore reduced to the
//! best 25 seconds of speech, in mono, with long pauses shortened, low rumble removed and a
//! consistent level. The result is cached, keyed by the source file and its modification time.

use std::fs::File;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as DecodeError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Bump when the processing changes, to invalidate cached voices.
const PREP_VERSION: u32 = 1;
/// pocket-tts truncates uploaded prompts to 30 s; 25 s of speech clones well and loads fast.
const TARGET_SECONDS: f32 = 25.0;
const MAX_SECONDS: f32 = 30.0;
const MIN_SPEECH_SECONDS: f32 = 3.0;
/// Longer inputs are not decoded further: the best 25 s are found well before.
const MAX_DECODED_SECONDS: usize = 15 * 60;
const FRAME_SECONDS: f32 = 0.02;

/// Returns the path to give pocket-tts: exported voices as is, recordings prepared and cached.
pub fn prepared_voice(source: &Path, cache_dir: &Path) -> Result<PathBuf> {
    if source.extension().is_some_and(|e| e.eq_ignore_ascii_case("safetensors")) {
        return Ok(source.to_path_buf());
    }
    let metadata = std::fs::metadata(source)
        .with_context(|| format!("voice file not found: {}", source.display()))?;
    let key = format!(
        "{PREP_VERSION}|{}|{}|{:?}",
        source.display(),
        metadata.len(),
        metadata.modified().ok()
    );
    let digest: String = Sha256::digest(key.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect();
    let prepared = cache_dir.join(format!("{digest}.wav"));
    if prepared.is_file() {
        return Ok(prepared);
    }

    let started = std::time::Instant::now();
    let (samples, sample_rate) = decode_mono(source)?;
    let voice = prepare(&samples, sample_rate);
    if (voice.len() as f32) < MIN_SPEECH_SECONDS * sample_rate as f32 {
        bail!(
            "not enough speech in {} (at least {MIN_SPEECH_SECONDS} s needed)",
            source.display()
        );
    }
    std::fs::create_dir_all(cache_dir)?;
    let staging = prepared.with_extension("wav.tmp");
    write_wav(&staging, sample_rate, &voice)?;
    std::fs::rename(&staging, &prepared)?;
    tracing::info!(
        source = %source.display(),
        input_s = samples.len() as f32 / sample_rate as f32,
        output_s = voice.len() as f32 / sample_rate as f32,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "voice prepared"
    );
    Ok(prepared)
}

/// Decodes any supported audio file to mono f32 samples.
pub fn decode_mono(path: &Path) -> Result<(Vec<f32>, u32)> {
    let file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(extension);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, stream, &FormatOptions::default(), &MetadataOptions::default())
        .with_context(|| format!("unsupported audio file: {}", path.display()))?;
    let mut format = probed.format;
    let track = format.default_track().context("no audio track")?;
    let track_id = track.id;
    let mut sample_rate = track.codec_params.sample_rate;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .context("unsupported audio codec")?;

    let mut mono = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(DecodeError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(DecodeError::ResetRequired) => break,
            Err(e) => return Err(e).context("cannot read the audio file"),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(DecodeError::DecodeError(_)) => continue, // corrupt packet: skip it
            Err(e) => return Err(e).context("cannot decode the audio file"),
        };
        let spec = *decoded.spec();
        let rate = *sample_rate.get_or_insert(spec.rate);
        let channels = spec.channels.count().max(1);
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        buffer.copy_interleaved_ref(decoded);
        mono.extend(buffer.samples().chunks(channels).map(|f| f.iter().sum::<f32>() / channels as f32));
        if mono.len() > MAX_DECODED_SECONDS * rate as usize {
            break;
        }
    }
    let sample_rate = sample_rate.context("unknown sample rate")?;
    Ok((mono, sample_rate))
}

/// The processing itself, on mono samples.
fn prepare(input: &[f32], sample_rate: u32) -> Vec<f32> {
    let mut signal = highpass(input, sample_rate, 70.0);
    let frame = ((sample_rate as f32 * FRAME_SECONDS) as usize).max(1);
    let voiced = voiced_frames(&signal, frame);
    if !voiced.iter().any(|&v| v) {
        return Vec::new();
    }

    // Shorten pauses: at most 0.25 s of each silent run, 0.1 s before the first word.
    let max_pause = (0.25 / FRAME_SECONDS) as usize;
    let mut kept: Vec<(usize, bool)> = Vec::new(); // (frame index, voiced)
    let mut silent_run = 0;
    let first_voiced = voiced.iter().position(|&v| v).unwrap_or(0);
    for (i, &is_voiced) in voiced.iter().enumerate().skip(first_voiced.saturating_sub(5)) {
        silent_run = if is_voiced { 0 } else { silent_run + 1 };
        if silent_run <= max_pause {
            kept.push((i, is_voiced));
        }
    }
    while kept.len() > 1 && !kept[kept.len() - 1].1 && !kept[kept.len() - 2].1 {
        kept.pop(); // trailing silence, keeping one frame
    }

    // Best window: the most voiced `TARGET_SECONDS`, its edges moved to nearby pauses so that
    // no word is cut.
    let window = (TARGET_SECONDS / FRAME_SECONDS) as usize;
    let max_window = (MAX_SECONDS / FRAME_SECONDS) as usize;
    let (mut start, mut end) = (0, kept.len());
    if kept.len() > window {
        let mut count = kept[..window].iter().filter(|k| k.1).count();
        let (mut best, mut best_start) = (count, 0);
        for s in 1..=kept.len() - window {
            count = count + kept[s + window - 1].1 as usize - kept[s - 1].1 as usize;
            if count > best {
                (best, best_start) = (count, s);
            }
        }
        start = best_start;
        end = best_start + window;
        let slack = (1.0 / FRAME_SECONDS) as usize;
        if let Some(pause) = (start.saturating_sub(slack)..=start).rev().find(|&i| !kept[i].1) {
            start = pause;
        }
        let limit = (start + max_window).min(kept.len());
        if let Some(pause) = (end..limit.min(end + slack)).find(|&i| !kept[i].1) {
            end = pause + 1;
        } else if let Some(pause) = (end.saturating_sub(slack)..end).rev().find(|&i| !kept[i].1) {
            end = pause + 1;
        }
        end = end.min(limit);
    }

    let mut output: Vec<f32> = kept[start..end]
        .iter()
        .flat_map(|&(i, _)| signal[i * frame..((i + 1) * frame).min(signal.len())].iter().copied())
        .collect();
    signal.clear();

    // Level: speech RMS at -20 dBFS, peaks below -1 dBFS, at most +20 dB of gain.
    let voiced_rms = {
        let (mut sum, mut n) = (0.0f64, 0usize);
        for (chunk, &(_, is_voiced)) in output.chunks(frame).zip(&kept[start..end]) {
            if is_voiced {
                sum += chunk.iter().map(|&s| (s as f64).powi(2)).sum::<f64>();
                n += chunk.len();
            }
        }
        (sum / n.max(1) as f64).sqrt() as f32
    };
    let mut gain = (0.1 / voiced_rms.max(1e-6)).min(10.0);
    let peak = output.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
    if peak * gain > 0.89 {
        gain = 0.89 / peak.max(1e-6);
    }
    let fade = (sample_rate as usize / 100).min(output.len() / 2);
    let len = output.len();
    for (i, sample) in output.iter_mut().enumerate() {
        let edge = i.min(len - 1 - i);
        let ramp = if edge < fade { edge as f32 / fade as f32 } else { 1.0 };
        *sample *= gain * ramp;
    }
    output
}

/// Frames louder than both -55 dBFS and 35 dB below the loudest frame.
fn voiced_frames(signal: &[f32], frame: usize) -> Vec<bool> {
    let levels: Vec<f32> = signal
        .chunks(frame)
        .map(|c| {
            let rms = (c.iter().map(|&s| s * s).sum::<f32>() / c.len() as f32).sqrt();
            20.0 * rms.max(1e-9).log10()
        })
        .collect();
    let loudest = levels.iter().copied().fold(f32::MIN, f32::max);
    let threshold = (loudest - 35.0).max(-55.0);
    levels.iter().map(|&db| db > threshold).collect()
}

/// Second-order Butterworth high-pass (RBJ cookbook), removes rumble and DC offset.
fn highpass(input: &[f32], sample_rate: u32, cutoff: f32) -> Vec<f32> {
    let w0 = std::f32::consts::TAU * cutoff / sample_rate as f32;
    let alpha = w0.sin() / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
    let cos = w0.cos();
    let a0 = 1.0 + alpha;
    let (b0, b1, b2) = ((1.0 + cos) / 2.0 / a0, -(1.0 + cos) / a0, (1.0 + cos) / 2.0 / a0);
    let (a1, a2) = (-2.0 * cos / a0, (1.0 - alpha) / a0);
    let (mut x1, mut x2, mut y1, mut y2) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    input
        .iter()
        .map(|&x| {
            let y = b0 * x + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
            (x2, x1, y2, y1) = (x1, x, y1, y);
            y
        })
        .collect()
}

fn write_wav(path: &Path, sample_rate: u32, samples: &[f32]) -> Result<()> {
    let data_len = (samples.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        bytes.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, bytes).with_context(|| format!("cannot write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 24_000;

    /// Speech-like bursts (1.5 s of a modulated tone) separated by `pause` seconds of noise floor.
    fn bursts(count: usize, pause: f32, amplitude: f32) -> Vec<f32> {
        let mut out = Vec::new();
        for _ in 0..count {
            out.extend((0..(1.5 * RATE as f32) as usize).map(|n| {
                let t = n as f32 / RATE as f32;
                amplitude * (std::f32::consts::TAU * 180.0 * t).sin() * (0.6 + 0.4 * (6.0 * t).sin())
            }));
            out.extend((0..(pause * RATE as f32) as usize).map(|n| 0.0005 * ((n * 7919 % 13) as f32 - 6.0) / 6.0));
        }
        out
    }

    fn seconds(samples: &[f32]) -> f32 {
        samples.len() as f32 / RATE as f32
    }

    #[test]
    fn long_recording_is_cut_to_about_25_seconds() {
        let input = bursts(100, 1.0, 0.3); // 250 s
        let voice = prepare(&input, RATE);
        let s = seconds(&voice);
        assert!((20.0..=MAX_SECONDS).contains(&s), "{s} s");
    }

    #[test]
    fn long_pauses_are_shortened() {
        let input = bursts(6, 3.0, 0.3); // 27 s, 18 s of it silent
        let voice = prepare(&input, RATE);
        let s = seconds(&voice);
        assert!((9.0..12.0).contains(&s), "{s} s");
    }

    #[test]
    fn quiet_speech_is_levelled() {
        let voice = prepare(&bursts(8, 0.5, 0.02), RATE);
        let peak = voice.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        assert!((0.1..=0.9).contains(&peak), "peak {peak}");
    }

    #[test]
    fn silence_gives_nothing() {
        assert!(prepare(&vec![0.0; RATE as usize * 5], RATE).is_empty());
    }
}
