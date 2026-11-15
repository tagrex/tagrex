//! APE tags without the header the tag library's write assumes (#769).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tagrex_core::model::{AudioFormat, TagEngine, TagField, TagIoError, TrackFile};

/// 24 WavPack blocks of a 3 s sine, no tag, as an encoder writes them.
const WAVPACK: &[u8] = include_bytes!("fixtures/multi-block.wv");

/// Twenty silent MPEG-1 Layer III frames, no tag.
fn mp3_audio() -> Vec<u8> {
    let mut out = Vec::new();
    for _ in 0..20 {
        let mut frame = vec![0u8; 417];
        frame[..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
        out.extend_from_slice(&frame);
    }
    out
}

/// An APEv2 tag holding an artist and a title, with or without its header.
fn ape_tag(with_header: bool) -> Vec<u8> {
    let mut items = Vec::new();
    for (key, value) in [("Artist", "Ape Artist"), ("Title", "Ape Title")] {
        items.extend_from_slice(&(value.len() as u32).to_le_bytes());
        items.extend_from_slice(&0u32.to_le_bytes());
        items.extend_from_slice(key.as_bytes());
        items.push(0);
        items.extend_from_slice(value.as_bytes());
    }
    let size = items.len() as u32 + 32;
    let header_or_footer = |flags: u32| {
        let mut block = b"APETAGEX".to_vec();
        for word in [2000, size, 2, flags] {
            block.extend_from_slice(&u32::to_le_bytes(word));
        }
        block.extend_from_slice(&[0; 8]);
        block
    };
    let has_header = if with_header { 1 << 31 } else { 0 };
    let mut tag = Vec::new();
    if with_header {
        tag.extend(header_or_footer(has_header | 1 << 29));
    }
    tag.extend(items);
    tag.extend(header_or_footer(has_header));
    tag
}

fn scratch(name: &str, ext: &str, bytes: &[u8]) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "tagrex-ape-header-{name}-{}.{ext}",
        std::process::id()
    ));
    std::fs::write(&path, bytes).unwrap();
    path
}

fn write(path: &Path, format: AudioFormat) -> Result<(), TagIoError> {
    let mut tags = BTreeMap::new();
    tags.insert(TagField::Artist, "New Artist".to_string());
    tags.insert(TagField::Title, "New Title".to_string());
    TagEngine::write(&TrackFile {
        path: path.to_path_buf(),
        format,
        tags,
    })
}

/// #769: for a version 2000 APE tag the tag library's write always replaces 32
/// bytes more than the footer's size, taking them for the header. A tag the
/// format allows without one therefore cost the last 32 bytes of audio on every
/// write (an MP3's last frame lost its sync and no longer decoded). Such a
/// write is refused now, before anything is touched.
#[test]
fn a_write_over_an_ape_tag_without_its_header_is_refused() {
    let mp3: Vec<u8> = [mp3_audio(), ape_tag(false)].concat();
    let wavpack: Vec<u8> = [WAVPACK.to_vec(), ape_tag(false)].concat();
    for (name, ext, format, bytes) in [
        ("mp3", "mp3", AudioFormat::Mp3, mp3),
        ("wavpack", "wv", AudioFormat::WavPack, wavpack),
    ] {
        let path = scratch(name, ext, &bytes);
        let read = TagEngine::read(&path);
        let result = write(&path, format);
        let after = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).ok();

        let read = read.unwrap_or_else(|err| panic!("{name}: the file should read: {err}"));
        assert_eq!(
            read.tags.get(&TagField::Artist).map(String::as_str),
            Some("Ape Artist"),
            "{name}"
        );
        assert!(
            matches!(&result, Err(TagIoError::Malformed(why)) if why.contains("without a header")),
            "{name}: expected a refusal, got {result:?}"
        );
        assert_eq!(after, bytes, "{name}: a refused write changed the file");
    }
}

/// The header an APE tag normally has passes the check: the MP3 is written,
/// its tag replaced, and every audio frame is still there.
#[test]
fn a_write_over_an_ape_tag_with_its_header_keeps_the_audio() {
    let audio = mp3_audio();
    let path = scratch("headed", "mp3", &[audio.clone(), ape_tag(true)].concat());
    write(&path, AudioFormat::Mp3).unwrap();
    let read = TagEngine::read(&path);
    let after = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).ok();

    assert_eq!(
        read.unwrap()
            .tags
            .get(&TagField::Artist)
            .map(String::as_str),
        Some("New Artist")
    );
    assert!(
        after
            .windows(audio.len())
            .any(|window| window == audio.as_slice()),
        "the audio frames did not survive the write"
    );
}
