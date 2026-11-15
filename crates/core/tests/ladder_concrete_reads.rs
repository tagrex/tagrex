//! Concrete tag reads of a file the read ladder took on a later rung (#770).

use std::collections::BTreeMap;

use tagrex_core::model::{AudioFormat, TagEngine, TagField, TrackFile};

/// #770, found by the fuzzer: a damaged file named `.mpc`, with an ID3v2 tag
/// buried after junk. It reads by its extension, as Musepack, and a write
/// appends an APE tag. With that tag in place the read ladder takes the file
/// as MPEG on its best-attempt rung, but the follow-up read of the APE tag
/// tried only the ordinary rules, failed, and failed the whole read: the file
/// the write had just succeeded on no longer opened. Those follow-up reads now
/// fall back the way the ladder does, so it reads again, with the new values,
/// and nothing of the original bytes is lost.
#[test]
fn a_file_the_ladder_reads_late_still_reads_after_a_write() {
    let fixture: &[u8] = include_bytes!("fixtures/junk-before-id3v2.mpc");
    let path =
        std::env::temp_dir().join(format!("tagrex-ladder-concrete-{}.mpc", std::process::id()));
    std::fs::write(&path, fixture).unwrap();

    let before = TagEngine::read(&path);
    // The fuzz target's write, value for value: the APE tag it appends is what
    // makes the best-attempt MPEG rung take the file afterwards.
    let mut tags = BTreeMap::new();
    tags.insert(TagField::Artist, "Fuzzed Artist".to_string());
    tags.insert(TagField::Album, "Fuzzed Album".to_string());
    tags.insert(TagField::TrackNumber, "7".to_string());
    tags.insert(TagField::Custom("FUZZ".into()), "value".to_string());
    let written = TagEngine::write(&TrackFile {
        path: path.clone(),
        format: AudioFormat::Musepack,
        tags,
    });
    let after = TagEngine::read(&path);
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).ok();

    assert_eq!(before.unwrap().format, AudioFormat::Musepack);
    written.unwrap();
    let after = after.unwrap_or_else(|err| panic!("the written file no longer reads: {err}"));
    assert_eq!(
        after.tags.get(&TagField::Artist).map(String::as_str),
        Some("Fuzzed Artist")
    );
    assert_eq!(
        &bytes[..fixture.len()],
        fixture,
        "the original bytes changed"
    );
}
