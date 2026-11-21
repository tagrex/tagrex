//! The decoded audio of a file as a digest, for the duplicate scan (#821).
//!
//! Two files whose digests match hold the same samples at the same sample rate
//! and channel count, whatever their tags, covers, padding or container: the
//! same track saved twice with different tags, or the same lossless audio as
//! FLAC and as WAV. The samples come through the same decoder as playback.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use rodio::Source;
use sha2::{Digest, Sha256};

/// Samples hashed between two looks at the cancel flag.
const CHUNK_SAMPLES: usize = 16 * 1024;

/// Why a digest has no result.
#[derive(Debug, PartialEq, Eq)]
pub enum DigestError {
    /// The file would not decode, or held no audio at all.
    Failed(String),
    /// The caller asked to stop.
    Cancelled,
}

/// The digest of the decoded audio of `path`: its sample rate, channel count,
/// every sample and the number of samples. With `seconds`, only the first that
/// many seconds are decoded, which is enough to tell most unrelated tracks
/// apart for a fraction of the work; a file shorter than that gets the same
/// digest as a full one.
///
/// Blocking, and as slow as decoding the file. Keep it off the main thread.
pub fn audio_digest(
    path: &Path,
    seconds: Option<u32>,
    cancel: &AtomicBool,
) -> Result<[u8; 32], DigestError> {
    let decoder = crate::open_decoder(path).map_err(DigestError::Failed)?;
    let channels = decoder.channels().get();
    let rate = decoder.sample_rate().get();
    let limit = seconds.map(|s| u64::from(s) * u64::from(rate) * u64::from(channels));
    digest_samples(decoder, rate, channels, limit, cancel)
}

/// [`audio_digest`] over interleaved samples.
fn digest_samples(
    samples: impl Iterator<Item = f32>,
    rate: u32,
    channels: u16,
    limit: Option<u64>,
    cancel: &AtomicBool,
) -> Result<[u8; 32], DigestError> {
    let mut hash = Sha256::new();
    hash.update(rate.to_le_bytes());
    hash.update(channels.to_le_bytes());
    let limit = limit.map_or(usize::MAX, |n| usize::try_from(n).unwrap_or(usize::MAX));
    let mut bytes = Vec::with_capacity(CHUNK_SAMPLES * 4);
    let mut count = 0u64;
    for sample in samples.take(limit) {
        bytes.extend_from_slice(&sample.to_le_bytes());
        count += 1;
        if bytes.len() == CHUNK_SAMPLES * 4 {
            if cancel.load(Ordering::Relaxed) {
                return Err(DigestError::Cancelled);
            }
            hash.update(&bytes);
            bytes.clear();
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(DigestError::Cancelled);
    }
    if count == 0 {
        return Err(DigestError::Failed("nothing decoded".to_string()));
    }
    hash.update(&bytes);
    hash.update(count.to_le_bytes());
    Ok(hash.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 16-bit PCM WAV of `samples` (mono), with `extra` chunks after the data.
    fn wav(rate: u32, samples: &[i16], extra: &[u8]) -> Vec<u8> {
        let data_len = (samples.len() * 2) as u32;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(4 + 24 + 8 + data_len + extra.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&1u16.to_le_bytes()); // mono
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for sample in samples {
            out.extend_from_slice(&sample.to_le_bytes());
        }
        out.extend_from_slice(extra);
        out
    }

    /// A LIST/INFO chunk naming a title, as a tagger writes one.
    fn info_chunk(title: &str) -> Vec<u8> {
        let mut text = title.as_bytes().to_vec();
        text.push(0);
        if text.len() % 2 == 1 {
            text.push(0);
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"LIST");
        out.extend_from_slice(&(4 + 8 + text.len() as u32).to_le_bytes());
        out.extend_from_slice(b"INFOINAM");
        out.extend_from_slice(&(text.len() as u32).to_le_bytes());
        out.extend_from_slice(&text);
        out
    }

    fn tone(len: usize, step: i32) -> Vec<i16> {
        (0..len)
            .map(|i| ((i as i32 * step) % 20_000 - 10_000) as i16)
            .collect()
    }

    // The same samples with different tags digest the same; other samples, or
    // the same ones at another rate, do not. The quick digest of the start
    // matches for two files that only part later, and the full one tells them
    // apart.
    #[test]
    fn the_digest_follows_the_samples_not_the_tags() {
        let dir = std::env::temp_dir().join(format!("tagrex-digest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let samples = tone(44_100 * 3, 37);
        let mut later = samples.clone();
        let tail = later.len() - 100;
        later[tail] = later[tail].wrapping_add(1);

        let files = [
            ("plain.wav", wav(44_100, &samples, &[])),
            (
                "tagged.wav",
                wav(44_100, &samples, &info_chunk("Another title")),
            ),
            ("other.wav", wav(44_100, &tone(44_100 * 3, 41), &[])),
            ("rate.wav", wav(48_000, &samples, &[])),
            ("later.wav", wav(44_100, &later, &[])),
        ];
        for (name, bytes) in &files {
            std::fs::write(dir.join(name), bytes).unwrap();
        }
        let never = AtomicBool::new(false);
        let full = |name: &str| audio_digest(&dir.join(name), None, &never).unwrap();
        let quick = |name: &str| audio_digest(&dir.join(name), Some(1), &never).unwrap();

        assert_eq!(full("plain.wav"), full("tagged.wav"));
        assert_ne!(full("plain.wav"), full("other.wav"));
        assert_ne!(full("plain.wav"), full("rate.wav"));
        assert_ne!(full("plain.wav"), full("later.wav"));
        assert_eq!(quick("plain.wav"), quick("later.wav"));
        assert_ne!(quick("plain.wav"), full("plain.wav"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_digest_stops_when_asked() {
        let stop = AtomicBool::new(true);
        let samples = std::iter::repeat_n(0.25f32, CHUNK_SAMPLES * 3);
        assert_eq!(
            digest_samples(samples, 44_100, 2, None, &stop),
            Err(DigestError::Cancelled)
        );
    }

    #[test]
    fn a_file_with_no_samples_has_no_digest() {
        let never = AtomicBool::new(false);
        assert!(matches!(
            digest_samples(std::iter::empty(), 44_100, 2, None, &never),
            Err(DigestError::Failed(_))
        ));
    }
}
