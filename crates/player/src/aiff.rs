//! An AIFF as the decoder can read it (#525).
//!
//! An AIFF's `SSND` chunk may start its sample data some way in — the chunk's
//! `offset` field skips that many bytes of padding first, and `blockSize` hints
//! at an alignment. The format allows both; Symphonia 0.5 refuses any file that
//! uses either ("No support for AIFF block-aligned data"), so such a file could
//! neither play nor draw its waveform. It is not rare: whole rips come out of
//! some tools as AIFF-C `sowt` with 2280 bytes of padding on every track.
//!
//! [`AudioFile`] hands the decoder a view of the file in which the padding is
//! not there: the header up to the `SSND` chunk as it is, but with `offset` and
//! `blockSize` at zero and the chunk and `FORM` sizes shrunk by the padding, then
//! the file from the first sample frame on. Positions map straight through, so
//! seeking works as on any file. Everything else — every other format, and an
//! AIFF without padding — is read directly. The file on disk is never touched.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

/// A file opened for decoding: read as it is, or with an AIFF's sample-data
/// padding left out (see the module docs).
pub struct AudioFile {
    file: File,
    /// The view's length — the file's, less any padding left out.
    len: u64,
    padding: Option<Padding>,
    /// Where the view is read from next, when it is a remapped one.
    pos: u64,
}

/// Where an AIFF's `SSND` padding sits, and the header bytes that change when
/// it is left out.
#[derive(Debug, Clone, PartialEq)]
struct Padding {
    /// The first byte after the `SSND` chunk's offset and blockSize fields — in
    /// the view, the first sample frame; in the file, the padding.
    data_start: u64,
    /// How many bytes of padding the file has there.
    bytes: u64,
    /// Four-byte fields rewritten in the view: the `FORM` size, and the `SSND`
    /// size, offset and blockSize. Positions are the same in file and view,
    /// since all of them come before `data_start`.
    patches: Vec<(u64, [u8; 4])>,
}

impl AudioFile {
    pub fn open(path: &Path) -> io::Result<Self> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        let padding = ssnd_padding(&mut file, len).unwrap_or(None);
        file.seek(SeekFrom::Start(0))?;
        Ok(Self {
            file,
            len: len - padding.as_ref().map_or(0, |p| p.bytes),
            padding,
            pos: 0,
        })
    }

    /// The length of what the decoder sees.
    pub fn len(&self) -> u64 {
        self.len
    }
}

/// The padding of an AIFF/AIFC's `SSND` chunk, when it has any to leave out.
/// `None` for anything that isn't an AIFF, has no padding, or doesn't parse —
/// those are read as they are, and the decoder says what it thinks of them.
fn ssnd_padding(file: &mut File, len: u64) -> io::Result<Option<Padding>> {
    let mut head = [0u8; 12];
    if file.read_exact(&mut head).is_err()
        || &head[0..4] != b"FORM"
        || !matches!(&head[8..12], b"AIFF" | b"AIFC")
    {
        return Ok(None);
    }
    let form_size = u32::from_be_bytes([head[4], head[5], head[6], head[7]]);
    let mut at = 12u64;
    while at + 8 <= len {
        file.seek(SeekFrom::Start(at))?;
        let mut chunk = [0u8; 16];
        file.read_exact(&mut chunk[..8])?;
        let size = u32::from_be_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
        if &chunk[0..4] != b"SSND" {
            at += 8 + u64::from(size) + u64::from(size & 1);
            continue;
        }
        file.read_exact(&mut chunk[8..16])?;
        let offset = u32::from_be_bytes([chunk[8], chunk[9], chunk[10], chunk[11]]);
        let block = u32::from_be_bytes([chunk[12], chunk[13], chunk[14], chunk[15]]);
        if (offset == 0 && block == 0) || offset > size.saturating_sub(8) {
            return Ok(None);
        }
        return Ok(Some(Padding {
            data_start: at + 16,
            bytes: u64::from(offset),
            patches: vec![
                (4, form_size.saturating_sub(offset).to_be_bytes()),
                (at + 4, (size - offset).to_be_bytes()),
                (at + 8, [0; 4]),
                (at + 12, [0; 4]),
            ],
        }));
    }
    Ok(None)
}

impl Read for AudioFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let Some(padding) = &self.padding else {
            return self.file.read(buf);
        };
        if self.pos >= self.len || buf.is_empty() {
            return Ok(0);
        }
        // One side of `data_start` per read: the header as it is (patched), or
        // the samples from past the padding.
        let (from, max) = if self.pos < padding.data_start {
            (self.pos, padding.data_start - self.pos)
        } else {
            (self.pos + padding.bytes, self.len - self.pos)
        };
        let want = buf.len().min(usize::try_from(max).unwrap_or(usize::MAX));
        self.file.seek(SeekFrom::Start(from))?;
        let got = self.file.read(&mut buf[..want])?;
        if self.pos < padding.data_start {
            for (at, bytes) in &padding.patches {
                for (i, byte) in bytes.iter().enumerate() {
                    let at = at + i as u64;
                    if at >= self.pos && at < self.pos + got as u64 {
                        buf[(at - self.pos) as usize] = *byte;
                    }
                }
            }
        }
        self.pos += got as u64;
        Ok(got)
    }
}

impl Seek for AudioFile {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        if self.padding.is_none() {
            return self.file.seek(to);
        }
        let target = match to {
            SeekFrom::Start(at) => Some(at),
            SeekFrom::End(delta) => self.len.checked_add_signed(delta),
            SeekFrom::Current(delta) => self.pos.checked_add_signed(delta),
        };
        let Some(target) = target else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek before the start of the file",
            ));
        };
        self.pos = target;
        Ok(target)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A minimal AIFF of 16-bit stereo PCM at 44.1 kHz, `frames` frames of a
    /// ramp, with `padding` bytes before the samples in its `SSND` chunk and a
    /// chunk after it, so the remapping has something past the data to carry.
    pub(crate) fn aiff(frames: u32, padding: u32) -> Vec<u8> {
        let mut comm = Vec::new();
        comm.extend_from_slice(&2u16.to_be_bytes());
        comm.extend_from_slice(&frames.to_be_bytes());
        comm.extend_from_slice(&16u16.to_be_bytes());
        comm.extend_from_slice(&[0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0]);
        let mut ssnd = Vec::new();
        ssnd.extend_from_slice(&padding.to_be_bytes());
        ssnd.extend_from_slice(&0u32.to_be_bytes());
        ssnd.extend(std::iter::repeat_n(0xAA, padding as usize));
        for frame in 0..frames {
            let sample = (frame as i16).wrapping_mul(37);
            ssnd.extend_from_slice(&sample.to_be_bytes());
            ssnd.extend_from_slice(&sample.wrapping_neg().to_be_bytes());
        }
        let name = b"NAME\x00\x00\x00\x04demo";
        let mut body = b"AIFF".to_vec();
        for (id, data) in [(b"COMM", &comm), (b"SSND", &ssnd)] {
            body.extend_from_slice(id);
            body.extend_from_slice(&(data.len() as u32).to_be_bytes());
            body.extend_from_slice(data);
            if data.len() % 2 == 1 {
                body.push(0);
            }
        }
        body.extend_from_slice(name);
        let mut file = b"FORM".to_vec();
        file.extend_from_slice(&(body.len() as u32).to_be_bytes());
        file.extend_from_slice(&body);
        file
    }

    fn write(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("tagrex-aiff-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // The view of a padded file is byte for byte the same file written without
    // the padding — headers, samples and the chunk after them.
    #[test]
    fn a_padded_aiff_reads_as_the_same_file_without_padding() {
        let dir = temp_dir("view");
        let padded = write(&dir, "padded.aiff", &aiff(500, 2280));
        let plain = aiff(500, 0);
        let mut view = AudioFile::open(&padded).unwrap();
        assert_eq!(view.len(), plain.len() as u64);
        let mut read = Vec::new();
        view.read_to_end(&mut read).unwrap();
        assert_eq!(read, plain);

        // Seeking lands where it would in the plain file, on both sides of the
        // first sample frame.
        for at in [0u64, 30, 50, 51, 900, plain.len() as u64 - 3] {
            view.seek(SeekFrom::Start(at)).unwrap();
            let mut byte = [0u8; 3];
            let got = view.read(&mut byte).unwrap();
            assert_eq!(
                &byte[..got],
                &plain[at as usize..at as usize + got],
                "at {at}"
            );
        }
        assert_eq!(
            view.seek(SeekFrom::End(-4)).unwrap(),
            plain.len() as u64 - 4
        );
    }

    // Anything else is read as it is: an AIFF without padding, and a file that
    // isn't an AIFF at all.
    #[test]
    fn other_files_are_read_as_they_are() {
        let dir = temp_dir("plain");
        for (name, bytes) in [
            ("plain.aiff", aiff(100, 0)),
            ("other.wav", b"RIFF\x04\x00\x00\x00WAVE".to_vec()),
        ] {
            let path = write(&dir, name, &bytes);
            let mut view = AudioFile::open(&path).unwrap();
            assert!(view.padding.is_none(), "{name}");
            let mut read = Vec::new();
            view.read_to_end(&mut read).unwrap();
            assert_eq!(read, bytes, "{name}");
        }
    }
}
