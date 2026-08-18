//! End-to-end transaction pipeline tests: real tag writes to real files in a
//! temp directory that doubles as the allowed root. Nothing is written
//! outside the per-test temp dir.

use std::path::{Path, PathBuf};

use tagrex_core::journal::{UndoJournal, VecJournal};
use tagrex_core::model::{TagEngine, TagField};
use tagrex_core::plan::{ChangePlan, Executor, FieldChange, FileChange, PlanError};

/// `fLaC` magic + STREAMINFO + PADDING — the same minimal, writable shape
/// used by the tag-engine tests. Enough for lofty to identify the format and
/// read/write a Vorbis Comments block.
const MINIMAL_FLAC: [u8; 62] = [
    0x66, 0x4c, 0x61, 0x43, 0x00, 0x00, 0x00, 0x22, 0x10, 0x00, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x0a, 0xc4, 0x42, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x81, 0x00, 0x00, 0x10, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// A unique temp directory for one test, created fresh.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "tagrex-executor-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    /// Write a fresh minimal FLAC at `name` and return its path.
    fn flac(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, MINIMAL_FLAC).unwrap();
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// The allowed-root slice the executor takes (#153). Every test here works
/// inside one directory; the multi-root cases build their own.
fn roots(dir: &Path) -> Vec<PathBuf> {
    vec![dir.to_path_buf()]
}

fn set_artist(path: &Path, old: Option<&str>, new: Option<&str>) -> ChangePlan {
    ChangePlan {
        description: "set artist".to_string(),
        changes: vec![FileChange {
            path: path.to_path_buf(),
            tag_changes: vec![FieldChange {
                field: TagField::Artist,
                old: old.map(str::to_string),
                new: new.map(str::to_string),
            }],
            cover_change: None,
            rename_to: None,
            sidecar_renames: Vec::new(),
            ..FileChange::default()
        }],
        ..ChangePlan::default()
    }
}

#[test]
fn apply_writes_tags_and_records_the_batch() {
    let dir = TempDir::new("apply");
    let track = dir.flac("track.flac");
    let mut journal = VecJournal::new();

    let plan = set_artist(&track, None, Some("Boards of Canada"));
    let batch = Executor::apply(&plan, &mut journal, &roots(dir.path())).unwrap();

    assert_eq!(
        TagEngine::read(&track)
            .unwrap()
            .tags
            .get(&TagField::Artist)
            .map(String::as_str),
        Some("Boards of Canada")
    );
    assert_eq!(batch.description, "set artist");
    assert_eq!(journal.batches().unwrap().len(), 1);
}

#[test]
fn undo_restores_the_previous_value() {
    let dir = TempDir::new("undo");
    let track = dir.flac("track.flac");
    let mut journal = VecJournal::new();

    // Field starts absent, so undo should remove it again.
    let plan = set_artist(&track, None, Some("Temporary"));
    let batch = Executor::apply(&plan, &mut journal, &roots(dir.path())).unwrap();
    assert!(TagEngine::read(&track)
        .unwrap()
        .tags
        .contains_key(&TagField::Artist));

    Executor::undo(&mut journal, batch.id, &roots(dir.path())).unwrap();

    assert!(!TagEngine::read(&track)
        .unwrap()
        .tags
        .contains_key(&TagField::Artist));
    assert!(journal.batches().unwrap().is_empty());
}

#[test]
fn rejects_an_invalid_year_without_poisoning_the_file() {
    let dir = TempDir::new("badyear");
    let track = dir.flac("track.flac");
    let mut journal = VecJournal::new();

    // A 3-digit year: lofty writes it but then rejects it on read, which would
    // make the file unreadable. Apply must fail instead of corrupting it.
    let bad = ChangePlan {
        description: "set year".to_string(),
        changes: vec![FileChange {
            path: track.clone(),
            tag_changes: vec![FieldChange {
                field: TagField::Year,
                old: None,
                new: Some("222".to_string()),
            }],
            cover_change: None,
            rename_to: None,
            sidecar_renames: Vec::new(),
            ..FileChange::default()
        }],
        ..ChangePlan::default()
    };
    assert!(Executor::apply(&bad, &mut journal, &roots(dir.path())).is_err());
    // The file is still readable and carries no year — untouched.
    let tags = TagEngine::read(&track).unwrap().tags;
    assert!(!tags.contains_key(&TagField::Year));

    // A proper 4-digit year still writes fine (the guard isn't over-eager).
    let good = ChangePlan {
        description: "set year".to_string(),
        changes: vec![FileChange {
            path: track.clone(),
            tag_changes: vec![FieldChange {
                field: TagField::Year,
                old: None,
                new: Some("1996".to_string()),
            }],
            cover_change: None,
            rename_to: None,
            sidecar_renames: Vec::new(),
            ..FileChange::default()
        }],
        ..ChangePlan::default()
    };
    Executor::apply(&good, &mut journal, &roots(dir.path())).unwrap();
    assert_eq!(
        TagEngine::read(&track)
            .unwrap()
            .tags
            .get(&TagField::Year)
            .map(String::as_str),
        Some("1996")
    );
}

#[test]
fn rejects_a_path_outside_the_allowed_root() {
    let root = TempDir::new("root");
    let outside = TempDir::new("outside");
    let track = outside.flac("track.flac");
    let mut journal = VecJournal::new();

    let plan = set_artist(&track, None, Some("Nope"));
    let err = Executor::apply(&plan, &mut journal, &roots(root.path())).unwrap_err();

    assert!(matches!(err, PlanError::OutsideRoot(_)));
    // Nothing recorded, nothing written.
    assert!(journal.batches().unwrap().is_empty());
    assert!(!TagEngine::read(&track)
        .unwrap()
        .tags
        .contains_key(&TagField::Artist));
}

#[test]
fn rejects_a_stale_plan_without_writing() {
    let dir = TempDir::new("stale");
    let track = dir.flac("track.flac");
    let mut journal = VecJournal::new();

    // The file has no artist, but the plan claims the current value is
    // "Something Else" -- so the plan was built against a stale snapshot.
    let plan = set_artist(&track, Some("Something Else"), Some("New"));
    let err = Executor::apply(&plan, &mut journal, &roots(dir.path())).unwrap_err();

    assert!(matches!(err, PlanError::Stale(_)));
    assert!(journal.batches().unwrap().is_empty());
    assert!(!TagEngine::read(&track)
        .unwrap()
        .tags
        .contains_key(&TagField::Artist));
}

#[test]
fn applies_tags_then_rename_and_undo_reverses_both() {
    let dir = TempDir::new("rename");
    let track = dir.flac("track.flac");
    let renamed = dir.path().join("renamed.flac");
    let mut journal = VecJournal::new();

    let mut plan = set_artist(&track, None, Some("New Artist"));
    plan.changes[0].rename_to = Some(renamed.clone());
    let batch = Executor::apply(&plan, &mut journal, &roots(dir.path())).unwrap();

    // File moved, tags written at the new location.
    assert!(!track.exists());
    assert!(renamed.exists());
    assert_eq!(
        TagEngine::read(&renamed)
            .unwrap()
            .tags
            .get(&TagField::Artist)
            .map(String::as_str),
        Some("New Artist")
    );

    Executor::undo(&mut journal, batch.id, &roots(dir.path())).unwrap();

    // Moved back, tags restored (Artist was absent originally).
    assert!(track.exists());
    assert!(!renamed.exists());
    assert!(!TagEngine::read(&track)
        .unwrap()
        .tags
        .contains_key(&TagField::Artist));
}

#[test]
fn rejects_a_rename_target_that_already_exists() {
    let dir = TempDir::new("rename-collision");
    let track = dir.flac("track.flac");
    let occupied = dir.flac("occupied.flac");
    let mut journal = VecJournal::new();

    let mut plan = set_artist(&track, None, Some("New"));
    plan.changes[0].rename_to = Some(occupied.clone());
    let err = Executor::apply(&plan, &mut journal, &roots(dir.path())).unwrap_err();

    assert!(matches!(err, PlanError::RenameCollision(_)));
    // Nothing applied: source untouched, no tags written.
    assert!(track.exists());
    assert!(!TagEngine::read(&track)
        .unwrap()
        .tags
        .contains_key(&TagField::Artist));
    assert!(journal.batches().unwrap().is_empty());
}

#[test]
fn rejects_two_files_renamed_onto_the_same_target() {
    let dir = TempDir::new("rename-dup");
    let a = dir.flac("a.flac");
    let b = dir.flac("b.flac");
    let target = dir.path().join("merged.flac");
    let mut journal = VecJournal::new();

    let plan = ChangePlan {
        description: "collide".to_string(),
        changes: vec![
            FileChange {
                path: a,
                tag_changes: vec![],
                cover_change: None,
                rename_to: Some(target.clone()),
                sidecar_renames: Vec::new(),
                ..FileChange::default()
            },
            FileChange {
                path: b,
                tag_changes: vec![],
                cover_change: None,
                rename_to: Some(target),
                sidecar_renames: Vec::new(),
                ..FileChange::default()
            },
        ],
        ..ChangePlan::default()
    };
    let err = Executor::apply(&plan, &mut journal, &roots(dir.path())).unwrap_err();

    assert!(matches!(err, PlanError::RenameCollision(_)));
    assert!(journal.batches().unwrap().is_empty());
}

#[test]
fn rejects_a_rename_target_outside_the_root() {
    let root = TempDir::new("rename-root");
    let outside = TempDir::new("rename-outside");
    let track = root.flac("track.flac");
    let mut journal = VecJournal::new();

    let mut plan = set_artist(&track, None, Some("New"));
    plan.changes[0].rename_to = Some(outside.path().join("escaped.flac"));
    let err = Executor::apply(&plan, &mut journal, &roots(root.path())).unwrap_err();

    assert!(matches!(err, PlanError::OutsideRoot(_)));
    assert!(track.exists());
    assert!(journal.batches().unwrap().is_empty());
}

#[test]
fn embeds_cover_and_undo_removes_it() {
    use tagrex_core::model::CoverArt;
    use tagrex_core::plan::CoverChange;

    let dir = TempDir::new("cover");
    let track = dir.flac("track.flac");
    let mut journal = VecJournal::new();

    let cover = CoverArt {
        mime: "image/png".to_string(),
        data: vec![0x89, 0x50, 0x4e, 0x47, 9, 8, 7],
        ..CoverArt::default()
    };
    let plan = ChangePlan {
        description: "embed cover".to_string(),
        changes: vec![FileChange {
            path: track.clone(),
            tag_changes: vec![],
            cover_change: Some(CoverChange {
                old: Vec::new(),
                new: vec![cover.clone()],
            }),
            rename_to: None,
            sidecar_renames: Vec::new(),
            ..FileChange::default()
        }],
        ..ChangePlan::default()
    };

    let batch = Executor::apply(&plan, &mut journal, &roots(dir.path())).unwrap();
    assert_eq!(
        TagEngine::read_cover(&track).unwrap().map(|c| c.data),
        Some(cover.data.clone())
    );

    Executor::undo(&mut journal, batch.id, &roots(dir.path())).unwrap();
    assert_eq!(TagEngine::read_cover(&track).unwrap(), None);
}

/// #47: stripping a tag block goes through the executor like any other change —
/// the other blocks survive it, and undo puts the stripped one back with what it
/// held. ID3v1 beside ID3v2 is the case that matters: it is the pair most files
/// in a real library carry, and the one whose rebuild is exact.
#[test]
fn strips_a_tag_block_and_undo_restores_it() {
    use lofty::config::WriteOptions;
    use lofty::id3::v1::Id3v1Tag;
    use lofty::prelude::{Accessor, TagExt};
    use tagrex_core::model::TagBlockKind;
    use tagrex_core::plan::BlockChange;

    let dir = TempDir::new("block");
    let track = dir.path().join("track.mp3");
    // A minimal MP3: five silent frames, enough for the backend to identify the
    // format and write both blocks.
    let mut frame = vec![0xFF, 0xFB, 0x90, 0x00];
    frame.resize(417, 0);
    std::fs::write(&track, frame.repeat(5)).unwrap();

    let mut modern = lofty::id3::v2::Id3v2Tag::new();
    modern.set_artist("Current".to_string());
    modern
        .save_to_path(&track, WriteOptions::default())
        .unwrap();
    let legacy = Id3v1Tag {
        artist: Some("Stale Artist".to_string()),
        title: Some("Stale Title".to_string()),
        ..Default::default()
    };
    legacy
        .save_to_path(&track, WriteOptions::default())
        .unwrap();

    let removed = TagEngine::read_block(&track, TagBlockKind::Id3v1)
        .unwrap()
        .expect("the file carries an ID3v1 block");
    let plan = ChangePlan {
        description: "Remove ID3v1 tag".to_string(),
        changes: vec![FileChange {
            path: track.clone(),
            block_changes: vec![BlockChange::removal(TagBlockKind::Id3v1, removed.clone())],
            ..FileChange::default()
        }],
        ..ChangePlan::default()
    };

    let mut journal = VecJournal::new();
    let batch = Executor::apply(&plan, &mut journal, &roots(dir.path())).unwrap();

    let kinds: Vec<TagBlockKind> = TagEngine::read_with_props(&track)
        .unwrap()
        .blocks
        .iter()
        .map(|block| block.kind)
        .collect();
    assert_eq!(kinds, vec![TagBlockKind::Id3v2], "the wrong block went");
    assert_eq!(
        TagEngine::read(&track)
            .unwrap()
            .tags
            .get(&TagField::Artist)
            .map(String::as_str),
        Some("Current"),
        "the block that stayed was rewritten"
    );

    Executor::undo(&mut journal, batch.id, &roots(dir.path())).unwrap();
    assert_eq!(
        TagEngine::read_block(&track, TagBlockKind::Id3v1).unwrap(),
        Some(removed),
        "undo did not put the block back as it was"
    );
}

/// #205: converting a block writes the target and drops the source in one
/// change, and undo puts the file back the way it was — both halves of it.
#[test]
fn converts_one_block_into_another_and_undo_reverses_both_halves() {
    use tagrex_core::model::{TagBlockContent, TagBlockKind};
    use tagrex_core::plan::BlockChange;

    let dir = TempDir::new("block-convert");
    let track = dir.path().join("track.mp3");
    let mut frame = vec![0xFF, 0xFB, 0x90, 0x00];
    frame.resize(417, 0);
    std::fs::write(&track, frame.repeat(5)).unwrap();

    // Start from an APE block only, so the conversion has a source and the
    // target genuinely does not exist yet.
    let mut source = TagBlockContent::default();
    source
        .tags
        .insert(TagField::Artist, "Convert Me".to_string());
    source.tags.insert(TagField::Title, "A Title".to_string());
    TagEngine::write_block(&track, TagBlockKind::Ape, None, &source).unwrap();

    let plan = ChangePlan {
        description: "Convert APE to ID3v2".to_string(),
        changes: vec![FileChange {
            path: track.clone(),
            block_changes: vec![
                BlockChange {
                    kind: TagBlockKind::Id3v2,
                    revision: None,
                    old_revision: None,
                    old_bytes: None,
                    old: None,
                    new: Some(source.clone()),
                },
                BlockChange::removal(TagBlockKind::Ape, source.clone()),
            ],
            ..FileChange::default()
        }],
        ..ChangePlan::default()
    };

    let mut journal = VecJournal::new();
    let batch = Executor::apply(&plan, &mut journal, &roots(dir.path())).unwrap();

    let kinds: Vec<TagBlockKind> = TagEngine::read_with_props(&track)
        .unwrap()
        .blocks
        .iter()
        .map(|block| block.kind)
        .collect();
    assert_eq!(
        kinds,
        vec![TagBlockKind::Id3v2],
        "the target should be the only block left"
    );
    assert_eq!(
        TagEngine::read(&track)
            .unwrap()
            .tags
            .get(&TagField::Artist)
            .map(String::as_str),
        Some("Convert Me"),
        "the values did not come across"
    );

    Executor::undo(&mut journal, batch.id, &roots(dir.path())).unwrap();
    assert_eq!(
        TagEngine::read_block(&track, TagBlockKind::Ape).unwrap(),
        Some(source),
        "the source block did not come back"
    );
    assert!(
        TagEngine::read_block(&track, TagBlockKind::Id3v2)
            .unwrap()
            .is_none(),
        "the block the conversion created should be gone again"
    );
}

/// #205: switching an ID3v2 block between 2.3 and 2.4 restamps the header and
/// keeps the frames — including the binary ones the model cannot express, which
/// is the whole reason this case does not go through the rebuild. Undo puts the
/// original revision back rather than the app-wide default.
#[test]
fn a_revision_switch_keeps_every_frame_and_undo_restores_the_revision() {
    use lofty::config::{ParseOptions, WriteOptions};
    use lofty::file::AudioFile;
    use lofty::id3::v2::{Frame, PrivateFrame};
    use lofty::mpeg::MpegFile;
    use lofty::prelude::{Accessor, TagExt};
    use tagrex_core::model::{Id3v2Revision, TagBlockKind};
    use tagrex_core::plan::BlockChange;

    let dir = TempDir::new("block-revision");
    let track = dir.path().join("track.mp3");
    let mut frame = vec![0xFF, 0xFB, 0x90, 0x00];
    frame.resize(417, 0);
    std::fs::write(&track, frame.repeat(5)).unwrap();

    // A DJ cue point stands in for everything the model cannot express.
    let mut tag = lofty::id3::v2::Id3v2Tag::new();
    tag.set_artist("Kept".to_string());
    tag.insert(Frame::Private(PrivateFrame::new(
        "SeratoMarkers".to_string(),
        vec![7, 7, 7, 7],
    )));
    tag.save_to_path(&track, WriteOptions::default().use_id3v23(true))
        .unwrap();
    assert_eq!(
        TagEngine::id3v2_revision(&track).unwrap(),
        Some(Id3v2Revision::V3)
    );

    let content = TagEngine::read_block(&track, TagBlockKind::Id3v2)
        .unwrap()
        .unwrap();
    let plan = ChangePlan {
        description: "Convert to ID3v2.4".to_string(),
        changes: vec![FileChange {
            path: track.clone(),
            block_changes: vec![BlockChange {
                kind: TagBlockKind::Id3v2,
                revision: Some(Id3v2Revision::V4),
                old_revision: Some(Id3v2Revision::V3),
                old_bytes: None,
                old: Some(content.clone()),
                new: Some(content),
            }],
            ..FileChange::default()
        }],
        ..ChangePlan::default()
    };

    let mut journal = VecJournal::new();
    let batch = Executor::apply(&plan, &mut journal, &roots(dir.path())).unwrap();

    assert_eq!(
        TagEngine::id3v2_revision(&track).unwrap(),
        Some(Id3v2Revision::V4)
    );
    let private_frames = |path: &Path| -> usize {
        let mut file = std::fs::File::open(path).unwrap();
        MpegFile::read_from(&mut file, ParseOptions::new())
            .unwrap()
            .id3v2()
            .map(|tag| {
                tag.iter()
                    .filter(|frame| matches!(frame, Frame::Private(_)))
                    .count()
            })
            .unwrap_or(0)
    };
    assert_eq!(
        private_frames(&track),
        1,
        "the cue-point frame did not survive the revision switch"
    );

    Executor::undo(&mut journal, batch.id, &roots(dir.path())).unwrap();
    assert_eq!(
        TagEngine::id3v2_revision(&track).unwrap(),
        Some(Id3v2Revision::V3),
        "undo left the file in the revision it was converted to"
    );
    assert_eq!(private_frames(&track), 1);
}

/// #206: undoing a change that destroyed an ID3v2 block must give back the
/// frames the model cannot express — a DJ cue point is the case that matters,
/// and a rebuild from text and pictures would silently drop it.
#[test]
fn undoing_a_destroyed_id3v2_block_brings_its_binary_frames_back() {
    use lofty::config::{ParseOptions, WriteOptions};
    use lofty::file::AudioFile;
    use lofty::id3::v2::{Frame, PrivateFrame};
    use lofty::mpeg::MpegFile;
    use lofty::prelude::{Accessor, TagExt};
    use tagrex_core::model::{TagBlockContent, TagBlockKind};
    use tagrex_core::plan::BlockChange;

    let dir = TempDir::new("block-bytes");
    let track = dir.path().join("track.mp3");
    let mut frame = vec![0xFF, 0xFB, 0x90, 0x00];
    frame.resize(417, 0);
    std::fs::write(&track, frame.repeat(5)).unwrap();

    let mut tag = lofty::id3::v2::Id3v2Tag::new();
    tag.set_artist("Convert Me".to_string());
    tag.insert(Frame::Private(PrivateFrame::new(
        "SeratoMarkers".to_string(),
        vec![9, 8, 7, 6],
    )));
    tag.save_to_path(&track, WriteOptions::default()).unwrap();

    let cue_points = |path: &Path| -> Vec<Vec<u8>> {
        let mut file = std::fs::File::open(path).unwrap();
        MpegFile::read_from(&mut file, ParseOptions::new())
            .unwrap()
            .id3v2()
            .map(|tag| {
                tag.iter()
                    .filter_map(|frame| match frame {
                        Frame::Private(private) => Some(private.private_data.to_vec()),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    assert_eq!(cue_points(&track), vec![vec![9, 8, 7, 6]], "seeding failed");

    // Convert ID3v2 to ID3v1: the ID3v2 block is destroyed, and only the bytes
    // kept with the plan can bring its cue point back.
    let content = TagEngine::read_block(&track, TagBlockKind::Id3v2)
        .unwrap()
        .unwrap();
    let bytes = TagEngine::dump_id3v2(&track).unwrap().expect("bytes");
    let plan = ChangePlan {
        description: "Convert ID3v2 to ID3v1".to_string(),
        changes: vec![FileChange {
            path: track.clone(),
            block_changes: vec![
                BlockChange {
                    kind: TagBlockKind::Id3v1,
                    revision: None,
                    old_revision: None,
                    old_bytes: None,
                    old: None,
                    new: Some(content.clone()),
                },
                BlockChange {
                    kind: TagBlockKind::Id3v2,
                    revision: None,
                    old_revision: None,
                    old_bytes: Some(bytes),
                    old: Some(content),
                    new: None,
                },
            ],
            ..FileChange::default()
        }],
        ..ChangePlan::default()
    };

    let mut journal = VecJournal::new();
    let batch = Executor::apply(&plan, &mut journal, &roots(dir.path())).unwrap();
    assert!(
        cue_points(&track).is_empty(),
        "the ID3v2 block should be gone after the conversion"
    );

    Executor::undo(&mut journal, batch.id, &roots(dir.path())).unwrap();
    assert_eq!(
        cue_points(&track),
        vec![vec![9, 8, 7, 6]],
        "undo rebuilt the block instead of restoring its bytes"
    );
    assert_eq!(
        TagEngine::read(&track)
            .unwrap()
            .tags
            .get(&TagField::Artist)
            .map(String::as_str),
        Some("Convert Me")
    );
    assert!(
        !TagBlockContent::exact(TagBlockKind::Id3v2),
        "a rebuild is still not exact — the bytes are what made this one so"
    );
}

/// A plan built against a block that has since changed on disk must not be
/// applied: the snapshot it carries is the only copy undo would have (#47).
#[test]
fn a_stale_block_snapshot_is_refused() {
    use lofty::config::WriteOptions;
    use lofty::id3::v1::Id3v1Tag;
    use lofty::prelude::TagExt;
    use tagrex_core::model::{TagBlockContent, TagBlockKind};
    use tagrex_core::plan::BlockChange;

    let dir = TempDir::new("block-stale");
    let track = dir.path().join("track.mp3");
    let mut frame = vec![0xFF, 0xFB, 0x90, 0x00];
    frame.resize(417, 0);
    std::fs::write(&track, frame.repeat(5)).unwrap();
    let legacy = Id3v1Tag {
        artist: Some("On Disk".to_string()),
        ..Default::default()
    };
    legacy
        .save_to_path(&track, WriteOptions::default())
        .unwrap();

    // The plan claims the block held nothing, which no longer matches the file.
    let plan = ChangePlan {
        description: "Remove ID3v1 tag".to_string(),
        changes: vec![FileChange {
            path: track.clone(),
            block_changes: vec![BlockChange::removal(
                TagBlockKind::Id3v1,
                TagBlockContent::default(),
            )],
            ..FileChange::default()
        }],
        ..ChangePlan::default()
    };

    let mut journal = VecJournal::new();
    let result = Executor::apply(&plan, &mut journal, &roots(dir.path()));

    assert!(matches!(result, Err(PlanError::Stale(_))));
    assert!(
        TagEngine::read_block(&track, TagBlockKind::Id3v1)
            .unwrap()
            .is_some(),
        "a refused plan must not have touched the file"
    );
}

/// Moving a file into folders that don't exist yet: the executor creates them,
/// and rollback removes exactly the ones it created — never a directory that
/// was already there, even if undo leaves it empty.
#[test]
fn rename_into_new_folders_creates_them_and_undo_removes_them() {
    let dir = TempDir::new("reorganize");
    let track = dir.flac("loose.flac");

    // This one already exists and must survive the rollback untouched.
    let preexisting = dir.path().join("Existing");
    std::fs::create_dir(&preexisting).unwrap();
    let inside_existing = dir.flac("second.flac");

    let target = dir
        .path()
        .join("Various/1996 - La Bush/01 - Desert Rain.flac");
    let target_in_existing = preexisting.join("moved.flac");

    let plan = ChangePlan {
        description: "Reorganize".to_string(),
        changes: vec![
            FileChange {
                path: track.clone(),
                rename_to: Some(target.clone()),
                ..FileChange::default()
            },
            FileChange {
                path: inside_existing.clone(),
                rename_to: Some(target_in_existing.clone()),
                ..FileChange::default()
            },
        ],
        ..ChangePlan::default()
    };

    let mut journal = VecJournal::default();
    let batch = Executor::apply(&plan, &mut journal, &roots(dir.path())).unwrap();

    assert!(target.exists(), "file moved into the new folder tree");
    assert!(!track.exists());
    assert!(target_in_existing.exists());
    // Two levels were created for the first target; `Existing` was not.
    assert_eq!(batch.created_dirs.len(), 2);
    assert!(batch.created_dirs.iter().all(|d| d.starts_with(dir.path())));

    Executor::undo(&mut journal, batch.id, &roots(dir.path())).unwrap();

    assert!(track.exists(), "file restored to its original path");
    assert!(!target.exists());
    assert!(
        !dir.path().join("Various").exists(),
        "created folders removed on undo"
    );
    assert!(
        preexisting.exists(),
        "a pre-existing folder must survive rollback even when left empty"
    );

    std::fs::remove_dir_all(dir.path()).ok();
}

/// A move whose target would escape the library is refused, even though the
/// intermediate folders don't exist yet — the containment check resolves
/// against the nearest existing ancestor.
#[test]
fn rename_into_new_folders_still_cannot_escape_the_root() {
    let dir = TempDir::new("reorganize-escape");
    let track = dir.flac("track.flac");

    let plan = ChangePlan {
        description: "Escape".to_string(),
        changes: vec![FileChange {
            path: track.clone(),
            rename_to: Some(dir.path().join("../outside/new/track.flac")),
            ..FileChange::default()
        }],
        ..ChangePlan::default()
    };

    let mut journal = VecJournal::default();
    let result = Executor::apply(&plan, &mut journal, &roots(dir.path()));
    assert!(matches!(result, Err(PlanError::OutsideRoot(_))));
    assert!(track.exists(), "nothing moved");
    assert!(
        !dir.path().join("../outside").exists(),
        "no folders created"
    );

    std::fs::remove_dir_all(dir.path()).ok();
}

// #58: a sidecar recorded on a change moves with the track on apply and is
// restored with it on undo — round-tripping through the journal.
#[test]
fn sidecar_files_move_and_restore_with_the_track() {
    let dir = TempDir::new("sidecar");
    let track = dir.flac("track.flac");
    let lrc = dir.path().join("track.lrc");
    std::fs::write(&lrc, b"lyrics").unwrap();

    let target = dir.path().join("renamed.flac");
    let lrc_target = dir.path().join("renamed.lrc");

    let plan = ChangePlan {
        description: "rename with sidecar".to_string(),
        changes: vec![FileChange {
            path: track.clone(),
            rename_to: Some(target.clone()),
            sidecar_renames: vec![(lrc.clone(), lrc_target.clone())],
            ..FileChange::default()
        }],
        ..ChangePlan::default()
    };

    let mut journal = VecJournal::default();
    let batch = Executor::apply(&plan, &mut journal, &roots(dir.path())).unwrap();
    assert!(
        target.exists() && lrc_target.exists(),
        "both moved to target"
    );
    assert!(!track.exists() && !lrc.exists(), "originals gone");

    Executor::undo(&mut journal, batch.id, &roots(dir.path())).unwrap();
    assert!(track.exists() && lrc.exists(), "both restored");
    assert!(
        !target.exists() && !lrc_target.exists(),
        "targets cleaned up"
    );

    std::fs::remove_dir_all(dir.path()).ok();
}

// #58: a sidecar must never clobber a file already at its destination — the
// whole plan is rejected before anything moves.
#[test]
fn sidecar_never_overwrites_an_existing_target() {
    let dir = TempDir::new("sidecar-collide");
    let track = dir.flac("track.flac");
    let lrc = dir.path().join("track.lrc");
    std::fs::write(&lrc, b"lyrics").unwrap();
    let occupied = dir.path().join("renamed.lrc");
    std::fs::write(&occupied, b"existing").unwrap();

    let plan = ChangePlan {
        description: "collide sidecar".to_string(),
        changes: vec![FileChange {
            path: track.clone(),
            rename_to: Some(dir.path().join("renamed.flac")),
            sidecar_renames: vec![(lrc.clone(), occupied.clone())],
            ..FileChange::default()
        }],
        ..ChangePlan::default()
    };

    let mut journal = VecJournal::default();
    let result = Executor::apply(&plan, &mut journal, &roots(dir.path()));
    assert!(matches!(result, Err(PlanError::RenameCollision(_))));
    assert!(track.exists() && lrc.exists(), "nothing moved");
    assert_eq!(
        std::fs::read(&occupied).unwrap(),
        b"existing",
        "occupied target untouched"
    );

    std::fs::remove_dir_all(dir.path()).ok();
}
