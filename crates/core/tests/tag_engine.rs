//! End-to-end read/write smoke test against a real (synthetic) FLAC file.
//!
//! There's no test-fixture audio in the repo yet, so this builds the
//! smallest file FLAC parsers accept by hand: the `fLaC` magic plus a single
//! STREAMINFO metadata block, no audio frames. Enough for lofty to identify
//! the format and read/write a Vorbis Comments block.

use std::collections::BTreeMap;
use std::path::PathBuf;

use tagrex_core::model::{AudioFormat, TagEngine, TagField, TrackFile};

/// `fLaC` + a non-last STREAMINFO block (34 bytes, all zeroed out except a
/// plausible sample rate/channels/bit depth) + a trailing PADDING block.
///
/// STREAMINFO-only files (no padding) make lofty's FLAC writer index past
/// the metadata block when it tries to splice one in, since it assumes
/// there's always at least a padding block to reuse or resize — true of
/// every file a real encoder produces, but not of a hand-built minimal one.
/// Shaping the fixture the way real encoders do (metadata always followed by
/// padding) avoids that and exercises the same code path real files take.
const MINIMAL_FLAC: [u8; 62] = [
    0x66, 0x4c, 0x61, 0x43, 0x00, 0x00, 0x00,
    0x22, // "fLaC" + STREAMINFO header (not last), length 34
    0x10, 0x00, 0x10, 0x00, 0x00, 0x00, 0x00,
    0x00, // min/max blocksize = 4096, min/max frame size = 0
    0x00, 0x00, 0x0a, 0xc4, 0x42, 0xf0, 0x00, 0x00, // 44100 Hz, 2ch, 16 bps, 0 samples
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // MD5 signature (zeroed)
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x81, 0x00, 0x00, 0x10, 0x00,
    0x00, // PADDING header (last), length 16
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // padding content
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

fn temp_flac_path(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "tagrex-tag-engine-test-{name}-{}.flac",
        std::process::id()
    ));
    path
}

#[test]
fn write_then_read_round_trips_known_and_custom_fields() {
    let path = temp_flac_path("round-trip");
    std::fs::write(&path, MINIMAL_FLAC).expect("write fixture");

    let mut tags = BTreeMap::new();
    tags.insert(TagField::Artist, "Test Artist".to_string());
    tags.insert(TagField::Title, "Test Title".to_string());
    // Deliberately not a key lofty recognizes for Vorbis Comments (unlike,
    // say, "MOOD" or "COMPOSER") — a recognized key would round-trip back as
    // its matching `ItemKey` variant, not as this literal `Custom` string.
    tags.insert(
        TagField::Custom("TAGREX_CUSTOM_TEST".to_string()),
        "Energetic".to_string(),
    );

    let file = TrackFile {
        path: path.clone(),
        format: AudioFormat::Flac,
        tags,
    };

    TagEngine::write(&file).expect("write tags");

    let read_back = TagEngine::read(&path).expect("read tags");
    std::fs::remove_file(&path).ok();

    assert_eq!(read_back.format, AudioFormat::Flac);
    assert_eq!(
        read_back.tags.get(&TagField::Artist).map(String::as_str),
        Some("Test Artist")
    );
    assert_eq!(
        read_back.tags.get(&TagField::Title).map(String::as_str),
        Some("Test Title")
    );
    assert_eq!(
        read_back
            .tags
            .get(&TagField::Custom("TAGREX_CUSTOM_TEST".to_string()))
            .map(String::as_str),
        Some("Energetic")
    );
}
