//! Minimal reader for a streamed WAV: header first, data size unknown or bogus.

use std::io::Read;

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleFormat {
    Pcm16,
    Float32,
}

#[derive(Debug, Clone, Copy)]
pub struct WavFormat {
    pub channels: u16,
    pub sample_rate: u32,
    pub sample_format: SampleFormat,
}

impl WavFormat {
    fn bytes_per_sample(&self) -> usize {
        match self.sample_format {
            SampleFormat::Pcm16 => 2,
            SampleFormat::Float32 => 4,
        }
    }
}

/// Reads chunks up to the start of the `data` chunk.
pub fn read_header(r: &mut impl Read) -> Result<WavFormat> {
    let mut riff = [0u8; 12];
    r.read_exact(&mut riff).context("truncated WAV header")?;
    if &riff[0..4] != b"RIFF" || &riff[8..12] != b"WAVE" {
        bail!("not a WAV stream");
    }
    let mut format = None;
    loop {
        let mut header = [0u8; 8];
        r.read_exact(&mut header).context("truncated WAV header")?;
        let size = u32::from_le_bytes(header[4..8].try_into().expect("4 bytes")) as usize;
        match &header[0..4] {
            b"fmt " => {
                let mut fmt = vec![0u8; size];
                r.read_exact(&mut fmt).context("truncated fmt chunk")?;
                if fmt.len() < 16 {
                    bail!("fmt chunk too short");
                }
                let u16_at = |i: usize| u16::from_le_bytes([fmt[i], fmt[i + 1]]);
                let audio_format = u16_at(0);
                let channels = u16_at(2);
                let sample_rate = u32::from_le_bytes(fmt[4..8].try_into().expect("4 bytes"));
                let bits = u16_at(14);
                let sample_format = match (audio_format, bits) {
                    (1, 16) => SampleFormat::Pcm16,
                    (3, 32) => SampleFormat::Float32,
                    _ => bail!("unsupported WAV encoding (format {audio_format}, {bits} bits)"),
                };
                if channels == 0 || sample_rate == 0 {
                    bail!("invalid WAV format: {channels} channels at {sample_rate} Hz");
                }
                format = Some(WavFormat { channels, sample_rate, sample_format });
            }
            b"data" => return format.context("WAV data chunk before fmt chunk"),
            _ => {
                // Chunks are padded to an even size.
                let skip = (size + size % 2) as u64;
                std::io::copy(&mut r.take(skip), &mut std::io::sink())?;
            }
        }
    }
}

/// Decodes whole samples from `bytes` into `out`. Returns the number of bytes consumed, so that
/// a sample split across two network reads is decoded on the next call.
pub fn decode(format: &WavFormat, bytes: &[u8], out: &mut Vec<f32>) -> usize {
    let size = format.bytes_per_sample();
    let usable = bytes.len() - bytes.len() % size;
    out.reserve(usable / size);
    for sample in bytes[..usable].chunks_exact(size) {
        out.push(match format.sample_format {
            SampleFormat::Pcm16 => i16::from_le_bytes([sample[0], sample[1]]) as f32 / 32768.0,
            SampleFormat::Float32 => f32::from_le_bytes(sample.try_into().expect("4 bytes")),
        });
    }
    usable
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm16_header(sample_rate: u32) -> Vec<u8> {
        let mut h = Vec::new();
        h.extend_from_slice(b"RIFF");
        h.extend_from_slice(&u32::MAX.to_le_bytes());
        h.extend_from_slice(b"WAVE");
        h.extend_from_slice(b"LIST");
        h.extend_from_slice(&3u32.to_le_bytes());
        h.extend_from_slice(&[0, 0, 0, 0]); // 3 bytes + 1 padding byte
        h.extend_from_slice(b"fmt ");
        h.extend_from_slice(&16u32.to_le_bytes());
        h.extend_from_slice(&1u16.to_le_bytes());
        h.extend_from_slice(&1u16.to_le_bytes());
        h.extend_from_slice(&sample_rate.to_le_bytes());
        h.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        h.extend_from_slice(&2u16.to_le_bytes());
        h.extend_from_slice(&16u16.to_le_bytes());
        h.extend_from_slice(b"data");
        h.extend_from_slice(&u32::MAX.to_le_bytes());
        h
    }

    #[test]
    fn parses_streamed_header_and_split_samples() {
        let mut stream = pcm16_header(24_000);
        for v in [0i16, 16384, -32768] {
            stream.extend_from_slice(&v.to_le_bytes());
        }
        let mut reader = stream.as_slice();
        let format = read_header(&mut reader).unwrap();
        assert_eq!(format.sample_rate, 24_000);
        assert_eq!(format.channels, 1);
        assert_eq!(format.sample_format, SampleFormat::Pcm16);

        // First read ends in the middle of a sample.
        let mut out = Vec::new();
        assert_eq!(decode(&format, &reader[..3], &mut out), 2);
        assert_eq!(decode(&format, &reader[2..], &mut out), 4);
        assert_eq!(out, vec![0.0, 0.5, -1.0]);
    }
}
