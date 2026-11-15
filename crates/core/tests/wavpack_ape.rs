//! WavPack files whose APE tag claims more bytes than it has (#768).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tagrex_core::model::{AudioFormat, TagEngine, TagField, TagIoError, TrackFile};

/// 24 WavPack blocks of a 3 s sine, no tag, as an encoder writes them.
const SOUND: &[u8] = include_bytes!("fixtures/multi-block.wv");

fn scratch(name: &str, bytes: &[u8]) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "tagrex-wavpack-ape-{name}-{}.wv",
        std::process::id()
    ));
    std::fs::write(&path, bytes).unwrap();
    path
}

fn write(path: &Path, artist: &str) -> Result<(), TagIoError> {
    let mut tags = BTreeMap::new();
    tags.insert(TagField::Artist, artist.to_string());
    tags.insert(TagField::Title, "Title".to_string());
    TagEngine::write(&TrackFile {
        path: path.to_path_buf(),
        format: AudioFormat::WavPack,
        tags,
    })
}

/// #768: the tag library's write replaces as many bytes before the APE footer
/// as the footer's size names, without checking they hold a tag. A size that
/// reaches back into the last audio block made a write cut that block short and
/// leave the file unreadable (found by the fuzzer). Such a write is refused now,
/// before anything is touched: the fuzzer's file, and a sound file whose tag
/// size was bumped by hand to reach 100 bytes into its audio.
#[test]
fn a_wavpack_whose_ape_tag_reaches_into_the_audio_is_not_written() {
    let fuzzed: &[u8] = include_bytes!("fixtures/ape-size-into-audio.wv");

    let tagged = scratch("tagged", SOUND);
    write(&tagged, "Artist").unwrap();
    let mut bumped = std::fs::read(&tagged).unwrap();
    std::fs::remove_file(&tagged).ok();
    let footer = bumped.len() - 32;
    assert_eq!(&bumped[footer..footer + 8], b"APETAGEX");
    let size = u32::from_le_bytes(bumped[footer + 12..footer + 16].try_into().unwrap());
    bumped[footer + 12..footer + 16].copy_from_slice(&(size + 100).to_le_bytes());

    for (name, bytes) in [("fuzzed", fuzzed), ("bumped", bumped.as_slice())] {
        let path = scratch(name, bytes);
        let result = write(&path, "Fuzzed Artist");
        let after = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).ok();

        assert!(
            matches!(&result, Err(TagIoError::Malformed(why)) if why.contains("reaches into the audio")),
            "{name}: expected a refusal, got {result:?}"
        );
        assert_eq!(after, bytes, "{name}: a refused write changed the file");
    }
}

/// The guard walks the blocks of every WavPack it writes, so a sound file has
/// to pass it untouched: tagged for the first time, then rewritten over its
/// own APE tag, with every audio byte kept and the new value read back.
#[test]
fn a_sound_wavpack_is_still_tagged_and_retagged() {
    let path = scratch("sound", SOUND);
    write(&path, "First").unwrap();
    write(&path, "A longer second artist").unwrap();
    let read = TagEngine::read(&path);
    let after = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).ok();

    let read = read.unwrap();
    assert_eq!(
        read.tags.get(&TagField::Artist).map(String::as_str),
        Some("A longer second artist")
    );
    assert_eq!(&after[..SOUND.len()], SOUND, "the audio blocks changed");
    assert_eq!(&after[after.len() - 32..after.len() - 24], b"APETAGEX");
}
