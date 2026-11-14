//! End-to-end transaction pipeline tests: real tag writes to real files in a
//! temp directory that doubles as the allowed root. Nothing is written
//! outside the per-test temp dir.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use tagrex_core::journal::{UndoJournal, VecJournal};
use tagrex_core::model::{TagEngine, TagField};
use tagrex_core::plan::{
    ChangePlan, Executor, FieldChange, FileChange, KeptField, PlanError, UndoMode,
};

/// `fLaC` magic + STREAMINFO + PADDING + one audio frame — the same minimal, writable shape
/// used by the tag-engine tests. Enough for lofty to identify the format and
/// read/write a Vorbis Comments block.
const MINIMAL_FLAC: [u8; 76] = [
    0x66, 0x4c, 0x61, 0x43, 0x00, 0x00, 0x00, 0x22, 0x10, 0x00, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x0a, 0xc4, 0x42, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x81, 0x00, 0x00, 0x10, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // One audio frame (#416): 4096 samples of stereo silence, two CONSTANT
    // subframes, with their CRCs. A file is tags followed by audio, and a
    // tag write is only ever tested against that shape.
    0xff, 0xf8, 0xc9, 0x18, 0x00, 0xc2, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xb8, 0xee,
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
fn cloned_bulk_plans_share_artwork_and_copy_on_write_is_explicit() {
    use tagrex_core::model::CoverArt;
    use tagrex_core::plan::CoverChange;
    let art = CoverArt {
        data: vec![7; 128 * 1024].into(),
        ..CoverArt::default()
    };
    let plan = ChangePlan {
        changes: (0..64)
            .map(|index| FileChange {
                path: PathBuf::from(format!("{index}.flac")),
                cover_change: Some(CoverChange {
                    old: Vec::new(),
                    new: vec![art.clone()],
                }),
                ..FileChange::default()
            })
            .collect(),
        ..ChangePlan::default()
    };
    let mut cloned = plan.clone();
    for change in &cloned.changes {
        assert_eq!(
            art.data.as_ptr(),
            change.cover_change.as_ref().unwrap().new[0].data.as_ptr()
        );
    }
    // A deliberate byte edit makes a private copy. Metadata changes need no
    // byte copy, and changing one picture can never mutate another file's plan.
    let changed = &mut cloned.changes[0].cover_change.as_mut().unwrap().new[0];
    std::sync::Arc::make_mut(&mut changed.data)[0] = 9;
    assert_ne!(art.data.as_ptr(), changed.data.as_ptr());
    assert_eq!(changed.data[0], 9);
    assert_eq!(
        plan.changes[0].cover_change.as_ref().unwrap().new[0].data[0],
        7
    );
    assert_eq!(
        cloned.changes[1].cover_change.as_ref().unwrap().new[0].data[0],
        7
    );
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
        data: vec![0x89, 0x50, 0x4e, 0x47, 9, 8, 7].into(),
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
                    text_to_unicode: false,
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
    // And a picture, which the switch used to drop (#607).
    let cover = tagrex_core::model::CoverArt {
        mime: "image/png".to_string(),
        data: vec![0x89, 0x50, 0x4e, 0x47, 9, 9].into(),
        ..tagrex_core::model::CoverArt::default()
    };
    TagEngine::write_covers(&track, std::slice::from_ref(&cover)).unwrap();
    TagEngine::set_id3v2_revision(&track, Id3v2Revision::V3).unwrap();
    assert_eq!(
        TagEngine::id3v2_revision(&track).unwrap(),
        Some(Id3v2Revision::V3)
    );
    assert_eq!(TagEngine::read_covers(&track).unwrap(), vec![cover.clone()]);

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
                text_to_unicode: false,
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
    assert_eq!(
        TagEngine::read_covers(&track).unwrap(),
        vec![cover.clone()],
        "the picture did not survive the revision switch"
    );

    Executor::undo(&mut journal, batch.id, &roots(dir.path())).unwrap();
    assert_eq!(
        TagEngine::id3v2_revision(&track).unwrap(),
        Some(Id3v2Revision::V3),
        "undo left the file in the revision it was converted to"
    );
    assert_eq!(private_frames(&track), 1);
    assert_eq!(TagEngine::read_covers(&track).unwrap(), vec![cover]);
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
                    text_to_unicode: false,
                },
                BlockChange {
                    kind: TagBlockKind::Id3v2,
                    revision: None,
                    old_revision: None,
                    old_bytes: Some(bytes),
                    old: Some(content),
                    new: None,
                    text_to_unicode: false,
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

#[test]
fn os_bookkeeping_files_are_recognised_by_name() {
    // #404: what a move neither carries nor lets keep an emptied folder alive.
    use tagrex_core::plan::is_os_junk;
    for junk in [
        ".DS_Store",
        "._01 track.flac",
        "Thumbs.db",
        "thumbs.db",
        "desktop.ini",
        "Desktop.ini",
        "ehthumbs.db",
    ] {
        assert!(is_os_junk(Path::new(junk)), "{junk}");
    }
    for real in [
        "folder.jpg",
        ".hidden-notes",
        "DS_Store",
        "rip.log",
        "Scans",
    ] {
        assert!(!is_os_junk(Path::new(real)), "{real}");
    }
}

#[test]
fn a_write_failing_mid_batch_leaves_the_library_as_it_was() {
    let dir = TempDir::new("midfail");
    let a = dir.flac("a.flac");
    let b = dir.flac("b.flac");
    let c = dir.flac("c.flac");
    let mut journal = VecJournal::new();

    let change = |path: &Path, field: TagField, value: &str| FileChange {
        path: path.to_path_buf(),
        tag_changes: vec![FieldChange {
            field,
            old: None,
            new: Some(value.to_string()),
        }],
        ..FileChange::default()
    };
    // The middle file's year is rejected when it is written, after the first
    // file has already been written.
    let plan = ChangePlan {
        description: "mid-batch failure".to_string(),
        changes: vec![
            change(&a, TagField::Artist, "A"),
            change(&b, TagField::Year, "222"),
            change(&c, TagField::Artist, "C"),
        ],
        ..ChangePlan::default()
    };
    assert!(Executor::apply(&plan, &mut journal, &roots(dir.path())).is_err());

    for path in [&a, &b, &c] {
        let tags = TagEngine::read(path).unwrap().tags;
        assert!(!tags.contains_key(&TagField::Artist), "{path:?}");
        assert!(!tags.contains_key(&TagField::Year), "{path:?}");
    }
    assert!(journal.batches().unwrap().is_empty());
}

#[test]
fn a_failure_after_the_copies_removes_them_and_their_folders() {
    let dir = TempDir::new("copyfail");
    let track = dir.flac("track.flac");
    let mut journal = VecJournal::new();

    let target = dir.path().join("Artist").join("Album").join("track.flac");
    // A copy's tags are written last, after the copy exists and its folders
    // have been created; the year is rejected at that point.
    let plan = ChangePlan {
        description: "copy then fail".to_string(),
        changes: vec![FileChange {
            path: track.clone(),
            tag_changes: vec![FieldChange {
                field: TagField::Year,
                old: None,
                new: Some("222".to_string()),
            }],
            rename_to: Some(target.clone()),
            copy: true,
            ..FileChange::default()
        }],
        ..ChangePlan::default()
    };
    assert!(Executor::apply(&plan, &mut journal, &roots(dir.path())).is_err());

    assert!(track.exists());
    assert!(!target.exists());
    assert!(!dir.path().join("Artist").exists());
    assert!(journal.batches().unwrap().is_empty());
}

fn set_artist_on(paths: &[&Path]) -> ChangePlan {
    ChangePlan {
        description: "set artist".to_string(),
        changes: paths
            .iter()
            .map(|path| FileChange {
                path: path.to_path_buf(),
                tag_changes: vec![FieldChange {
                    field: TagField::Artist,
                    old: None,
                    new: Some("Cancelled".to_string()),
                }],
                ..FileChange::default()
            })
            .collect(),
        ..ChangePlan::default()
    }
}

#[test]
fn apply_reports_each_step_with_a_running_total() {
    let dir = TempDir::new("progress");
    let files = [dir.flac("a.flac"), dir.flac("b.flac"), dir.flac("c.flac")];
    let mut journal = VecJournal::new();
    let plan = set_artist_on(&[&files[0], &files[1], &files[2]]);

    let mut seen = Vec::new();
    Executor::apply_with(
        &plan,
        &mut journal,
        &roots(dir.path()),
        &mut |progress| seen.push((progress.done, progress.total)),
        &AtomicBool::new(false),
    )
    .unwrap();

    assert_eq!(seen, vec![(0, 6), (1, 6), (2, 6), (3, 6), (4, 6), (5, 6)]);
}

#[test]
fn a_cancel_between_files_rolls_the_batch_back() {
    let dir = TempDir::new("cancel");
    let files = [dir.flac("a.flac"), dir.flac("b.flac"), dir.flac("c.flac")];
    let mut journal = VecJournal::new();
    let plan = set_artist_on(&[&files[0], &files[1], &files[2]]);

    // Cancel as the second file is reported: it is still written, and the flag
    // is read before the third, so two files are on disk when the batch stops.
    let cancel = AtomicBool::new(false);
    let mut written_when_stopped = 0;
    let result = Executor::apply_with(
        &plan,
        &mut journal,
        &roots(dir.path()),
        &mut |progress| {
            if progress.done == plan.changes.len() + 1 {
                cancel.store(true, Ordering::SeqCst);
            }
            written_when_stopped = progress.done.saturating_sub(plan.changes.len()) + 1;
        },
        &cancel,
    );

    assert!(matches!(result, Err(PlanError::Cancelled)));
    assert_eq!(written_when_stopped, 2);
    for path in &files {
        assert!(!TagEngine::read(path)
            .unwrap()
            .tags
            .contains_key(&TagField::Artist));
    }
    assert!(journal.batches().unwrap().is_empty());
}

#[test]
fn a_cancel_before_the_first_file_changes_nothing() {
    let dir = TempDir::new("cancel-early");
    let file = dir.flac("a.flac");
    let mut journal = VecJournal::new();
    let plan = set_artist_on(&[&file]);

    let result = Executor::apply_with(
        &plan,
        &mut journal,
        &roots(dir.path()),
        &mut |_| {},
        &AtomicBool::new(true),
    );

    assert!(matches!(result, Err(PlanError::Cancelled)));
    assert!(!TagEngine::read(&file)
        .unwrap()
        .tags
        .contains_key(&TagField::Artist));
    assert!(journal.batches().unwrap().is_empty());
}

#[test]
fn undo_reports_each_step_with_a_running_total() {
    let dir = TempDir::new("undo-progress");
    let files = [dir.flac("a.flac"), dir.flac("b.flac"), dir.flac("c.flac")];
    let mut journal = VecJournal::new();
    let plan = set_artist_on(&[&files[0], &files[1], &files[2]]);
    let batch = Executor::apply(&plan, &mut journal, &roots(dir.path())).unwrap();

    let mut seen = Vec::new();
    Executor::undo_with(
        &mut journal,
        batch.id,
        &roots(dir.path()),
        &mut |progress| seen.push((progress.done, progress.total)),
    )
    .unwrap();

    assert_eq!(seen, vec![(0, 3), (1, 3), (2, 3)]);
    assert!(journal.batches().unwrap().is_empty());
}

/// Whether `dir` sits on a file system that folds letter case, the default on
/// macOS and Windows.
fn folds_case(dir: &Path) -> bool {
    let probe = dir.join("case-probe");
    std::fs::write(&probe, b"").unwrap();
    let folds = dir.join("CASE-PROBE").exists();
    std::fs::remove_file(&probe).unwrap();
    folds
}

#[test]
fn two_targets_the_file_system_folds_together_are_refused() {
    let dir = TempDir::new("case-fold");
    if !folds_case(dir.path()) {
        // `Track.flac` and `track.flac` are two files here; nothing to refuse.
        return;
    }
    let a = dir.flac("a.flac");
    let b = dir.flac("b.flac");
    std::fs::write(&b, [MINIMAL_FLAC.as_slice(), b"b"].concat()).unwrap();
    let mut journal = VecJournal::new();
    let plan = ChangePlan {
        description: "fold".to_string(),
        changes: vec![
            FileChange {
                path: a.clone(),
                rename_to: Some(dir.path().join("Track.flac")),
                ..FileChange::default()
            },
            FileChange {
                path: b.clone(),
                rename_to: Some(dir.path().join("track.flac")),
                ..FileChange::default()
            },
        ],
        ..ChangePlan::default()
    };

    let err = Executor::apply(&plan, &mut journal, &roots(dir.path())).unwrap_err();

    assert!(matches!(err, PlanError::RenameCollision(_)));
    // Both files are back where they were, each with its own content.
    assert_eq!(std::fs::read(&a).unwrap(), MINIMAL_FLAC);
    assert_eq!(
        std::fs::read(&b).unwrap(),
        [MINIMAL_FLAC.as_slice(), b"b"].concat()
    );
    assert!(!dir.path().join("track.flac").exists());
    assert!(journal.batches().unwrap().is_empty());
}

/// The names in `dir` as the file system spells them, which on one that folds
/// letter case is the only way to tell `track.flac` from `Track.flac`.
fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_rename_that_only_changes_case_goes_through_and_undoes() {
    let dir = TempDir::new("case-only");
    let track = dir.flac("track.flac");
    let renamed = dir.path().join("Track.flac");
    let mut journal = VecJournal::new();
    let plan = ChangePlan {
        description: "case".to_string(),
        changes: vec![FileChange {
            path: track.clone(),
            rename_to: Some(renamed.clone()),
            ..FileChange::default()
        }],
        ..ChangePlan::default()
    };

    let batch = Executor::apply(&plan, &mut journal, &roots(dir.path())).unwrap();
    assert_eq!(names_in(dir.path()), ["Track.flac"]);
    assert_eq!(std::fs::read(&renamed).unwrap(), MINIMAL_FLAC);

    Executor::undo(&mut journal, batch.id, &roots(dir.path())).unwrap();
    assert_eq!(names_in(dir.path()), ["track.flac"]);
    assert_eq!(std::fs::read(&track).unwrap(), MINIMAL_FLAC);
}

#[test]
fn a_rollback_of_a_rename_that_only_changes_case_reports_no_failure() {
    let dir = TempDir::new("case-only-rollback");
    let first = dir.flac("first.flac");
    let second = dir.flac("second.flac");
    let mut journal = VecJournal::new();
    let plan = ChangePlan {
        description: "case".to_string(),
        changes: [&first, &second]
            .iter()
            .map(|path| FileChange {
                path: (*path).clone(),
                rename_to: Some(
                    dir.path()
                        .join(path.file_name().unwrap().to_string_lossy().to_uppercase()),
                ),
                ..FileChange::default()
            })
            .collect(),
        ..ChangePlan::default()
    };

    let cancel = AtomicBool::new(false);
    let result = Executor::apply_with(
        &plan,
        &mut journal,
        &roots(dir.path()),
        &mut |_| cancel.store(true, Ordering::SeqCst),
        &cancel,
    );

    assert!(matches!(result, Err(PlanError::Cancelled)), "{result:?}");
    assert_eq!(names_in(dir.path()), ["first.flac", "second.flac"]);
}

#[test]
fn undo_never_replaces_a_file_put_back_at_the_original_path() {
    let dir = TempDir::new("undo-occupied");
    let track = dir.flac("track.flac");
    let renamed = dir.path().join("renamed.flac");
    let mut journal = VecJournal::new();
    let plan = ChangePlan {
        description: "rename".to_string(),
        changes: vec![FileChange {
            path: track.clone(),
            rename_to: Some(renamed.clone()),
            ..FileChange::default()
        }],
        ..ChangePlan::default()
    };
    let batch = Executor::apply(&plan, &mut journal, &roots(dir.path())).unwrap();
    // Another file takes the old name.
    std::fs::write(&track, b"someone else").unwrap();

    let err = Executor::undo(&mut journal, batch.id, &roots(dir.path())).unwrap_err();

    assert!(matches!(err, PlanError::RenameCollision(_)));
    assert_eq!(std::fs::read(&track).unwrap(), b"someone else");
    assert_eq!(std::fs::read(&renamed).unwrap(), MINIMAL_FLAC);
    assert_eq!(journal.batches().unwrap().len(), 1);
}

#[test]
fn undo_removes_a_copy_only_while_it_is_still_the_copy() {
    let dir = TempDir::new("undo-copy-replaced");
    let track = dir.flac("track.flac");
    let target = dir.path().join("copy.flac");
    let mut journal = VecJournal::new();
    let plan = ChangePlan {
        description: "copy".to_string(),
        changes: vec![FileChange {
            path: track.clone(),
            rename_to: Some(target.clone()),
            copy: true,
            ..FileChange::default()
        }],
        ..ChangePlan::default()
    };
    let batch = Executor::apply(&plan, &mut journal, &roots(dir.path())).unwrap();
    // The copy is replaced by another file under the same name.
    std::fs::write(&target, b"a different file").unwrap();

    let err = Executor::undo(&mut journal, batch.id, &roots(dir.path())).unwrap_err();

    assert!(matches!(err, PlanError::Stale(_)));
    assert_eq!(std::fs::read(&target).unwrap(), b"a different file");
    assert_eq!(journal.batches().unwrap().len(), 1);

    // Once that file is out of the way, the undo goes through.
    std::fs::remove_file(&target).unwrap();
    Executor::undo(&mut journal, batch.id, &roots(dir.path())).unwrap();
    assert!(track.exists());
    assert!(journal.batches().unwrap().is_empty());
}

#[test]
fn undo_of_an_untouched_copy_still_removes_it() {
    let dir = TempDir::new("undo-copy");
    let track = dir.flac("track.flac");
    let target = dir.path().join("copy.flac");
    let mut journal = VecJournal::new();
    let mut plan = set_artist(&track, None, Some("Copied"));
    plan.changes[0].rename_to = Some(target.clone());
    plan.changes[0].copy = true;
    let batch = Executor::apply(&plan, &mut journal, &roots(dir.path())).unwrap();

    Executor::undo(&mut journal, batch.id, &roots(dir.path())).unwrap();

    assert!(!target.exists());
    assert_eq!(std::fs::read(&track).unwrap(), MINIMAL_FLAC);
}

#[test]
fn an_undo_that_stopped_half_way_can_be_finished() {
    let dir = TempDir::new("undo-resume");
    let a = dir.flac("a.flac");
    let b = dir.flac("b.flac");
    let a2 = dir.path().join("a2.flac");
    let b2 = dir.path().join("b2.flac");
    let mut journal = VecJournal::new();
    let mut plan = set_artist_on(&[&a, &b]);
    plan.changes[0].rename_to = Some(a2.clone());
    plan.changes[1].rename_to = Some(b2.clone());
    let batch = Executor::apply(&plan, &mut journal, &roots(dir.path())).unwrap();
    // The first attempt got as far as moving `a` back, then stopped.
    std::fs::rename(&a2, &a).unwrap();

    Executor::undo(&mut journal, batch.id, &roots(dir.path())).unwrap();

    assert!(a.exists() && b.exists());
    assert!(!a2.exists() && !b2.exists());
    for path in [&a, &b] {
        assert!(!TagEngine::read(path)
            .unwrap()
            .tags
            .contains_key(&TagField::Artist));
    }
    assert!(journal.batches().unwrap().is_empty());
}

/// #658: undoing an older batch wrote its `old` values over every later edit of
/// the same fields without a word.
#[test]
fn undo_refuses_to_overwrite_a_later_edit() {
    let dir = TempDir::new("undo-later-edit");
    let track = dir.flac("track.flac");
    let mut journal = VecJournal::new();
    let artist = |path: &Path| {
        TagEngine::read(path)
            .unwrap()
            .tags
            .get(&TagField::Artist)
            .cloned()
    };

    let first = Executor::apply(
        &set_artist(&track, None, Some("Y")),
        &mut journal,
        &roots(dir.path()),
    )
    .unwrap();
    let later = Executor::apply(
        &set_artist(&track, Some("Y"), Some("Z")),
        &mut journal,
        &roots(dir.path()),
    )
    .unwrap();

    let refused = Executor::undo(&mut journal, first.id, &roots(dir.path()));
    assert!(
        matches!(&refused, Err(PlanError::ChangedSinceApplied(path)) if path == &track),
        "{refused:?}"
    );
    assert_eq!(artist(&track).as_deref(), Some("Z"), "nothing was written");
    assert_eq!(journal.batches().unwrap().len(), 2, "both batches stay");

    // Undone newest first, both go through.
    Executor::undo(&mut journal, later.id, &roots(dir.path())).unwrap();
    Executor::undo(&mut journal, first.id, &roots(dir.path())).unwrap();
    assert_eq!(artist(&track), None);
    assert!(journal.batches().unwrap().is_empty());
}

/// #658, the other half: an edit outside the app is a later edit too, and a
/// batch recorded before the check existed is undone as it always was.
#[test]
fn undo_checks_only_what_the_batch_recorded() {
    let dir = TempDir::new("undo-unrecorded");
    let track = dir.flac("track.flac");
    let mut journal = VecJournal::new();
    let batch = Executor::apply(
        &set_artist(&track, None, Some("Y")),
        &mut journal,
        &roots(dir.path()),
    )
    .unwrap();
    assert_eq!(batch.applied_values.len(), 1);

    let mut file = TagEngine::read(&track).unwrap();
    file.tags.insert(TagField::Artist, "Elsewhere".to_string());
    TagEngine::write(&file).unwrap();
    assert!(matches!(
        Executor::undo(&mut journal, batch.id, &roots(dir.path())),
        Err(PlanError::ChangedSinceApplied(_))
    ));

    // The same batch as an older build recorded it: no values, no check.
    let mut old_journal = VecJournal::new();
    let mut unrecorded = batch.clone();
    unrecorded.applied_values.clear();
    let id = old_journal.record(&unrecorded).unwrap();
    Executor::undo(&mut old_journal, id, &roots(dir.path())).unwrap();
    assert!(!TagEngine::read(&track)
        .unwrap()
        .tags
        .contains_key(&TagField::Artist));
}

/// An MP3 whose ID3v2 block is in 2.3, carrying `artist`.
fn id3v23_mp3(dir: &TempDir, name: &str, artist: &str) -> PathBuf {
    use lofty::config::WriteOptions;
    use lofty::prelude::{Accessor, TagExt};

    let track = dir.path().join(name);
    let mut frame = vec![0xFF, 0xFB, 0x90, 0x00];
    frame.resize(417, 0);
    std::fs::write(&track, frame.repeat(5)).unwrap();
    let mut tag = lofty::id3::v2::Id3v2Tag::new();
    tag.set_artist(artist.to_string());
    tag.save_to_path(&track, WriteOptions::default().use_id3v23(true))
        .unwrap();
    track
}

fn artist_change(path: &Path, old: &str, new: &str) -> FileChange {
    FileChange {
        path: path.to_path_buf(),
        tag_changes: vec![FieldChange {
            field: TagField::Artist,
            old: Some(old.to_string()),
            new: Some(new.to_string()),
        }],
        ..FileChange::default()
    }
}

/// #666: an edit saves in the revision the app is set to, so a 2.3 file comes
/// out as 2.4. Undo puts it back in 2.3, the way it was.
#[test]
fn undo_puts_back_the_id3v2_revision_the_file_was_in() {
    use tagrex_core::model::Id3v2Revision;

    let dir = TempDir::new("undo-revision");
    let track = id3v23_mp3(&dir, "track.mp3", "Old");
    let plan = ChangePlan {
        description: "edit".to_string(),
        changes: vec![artist_change(&track, "Old", "New")],
        ..ChangePlan::default()
    };
    let mut journal = VecJournal::new();
    let batch = Executor::apply(&plan, &mut journal, &roots(dir.path())).unwrap();
    assert_eq!(
        TagEngine::id3v2_revision(&track).unwrap(),
        Some(Id3v2Revision::V4)
    );

    Executor::undo(&mut journal, batch.id, &roots(dir.path())).unwrap();

    assert_eq!(
        TagEngine::id3v2_revision(&track).unwrap(),
        Some(Id3v2Revision::V3)
    );
    let tags = TagEngine::read(&track).unwrap().tags;
    assert_eq!(tags.get(&TagField::Artist).map(String::as_str), Some("Old"));
}

/// #666: a batch that fails part-way puts the revision back too.
#[test]
fn a_rollback_puts_back_the_id3v2_revision_the_file_was_in() {
    use tagrex_core::model::Id3v2Revision;

    let dir = TempDir::new("rollback-revision");
    let track = id3v23_mp3(&dir, "track.mp3", "Old");
    let other = dir.flac("other.flac");
    // The second file's year is rejected after the first is written.
    let plan = ChangePlan {
        description: "edit".to_string(),
        changes: vec![
            artist_change(&track, "Old", "New"),
            FileChange {
                path: other,
                tag_changes: vec![FieldChange {
                    field: TagField::Year,
                    old: None,
                    new: Some("222".to_string()),
                }],
                ..FileChange::default()
            },
        ],
        ..ChangePlan::default()
    };
    let mut journal = VecJournal::new();
    assert!(Executor::apply(&plan, &mut journal, &roots(dir.path())).is_err());

    assert_eq!(
        TagEngine::id3v2_revision(&track).unwrap(),
        Some(Id3v2Revision::V3)
    );
    let tags = TagEngine::read(&track).unwrap().tags;
    assert_eq!(tags.get(&TagField::Artist).map(String::as_str), Some("Old"));
}

/// #669: removing a Vorbis comment block and undoing it puts back what the
/// model does not hold: the provenance comment, the vendor string, the order
/// and spelling of the names, the pictures beside them.
#[test]
fn undoing_a_vorbis_block_removal_puts_back_every_comment() {
    use lofty::config::WriteOptions;
    use lofty::ogg::tag::VorbisComments;
    use lofty::prelude::TagExt;
    use tagrex_core::model::{CoverArt, TagBlockContent, TagBlockKind};
    use tagrex_core::plan::BlockChange;

    let dir = TempDir::new("vorbis-bytes");
    let track = dir.flac("track.flac");
    let mut tag = VorbisComments::default();
    tag.set_vendor("Some Encoder 1.2.3".to_string());
    for (name, value) in [
        ("TITLE", "Kept"),
        ("ENCODER", "Some Encoder 1.2.3"),
        ("Mood", "Calm"),
        ("ARTIST", "Kept Artist"),
    ] {
        tag.push(name.to_string(), value.to_string());
    }
    tag.save_to_path(&track, WriteOptions::default()).unwrap();
    let cover = CoverArt {
        mime: "image/png".to_string(),
        data: vec![0x89, 0x50, 0x4e, 0x47, 1, 2].into(),
        ..CoverArt::default()
    };
    TagEngine::write_covers(&track, std::slice::from_ref(&cover)).unwrap();
    let before = TagEngine::dump_vorbis(&track).unwrap().expect("bytes");

    let content = TagEngine::read_block(&track, TagBlockKind::VorbisComments)
        .unwrap()
        .unwrap();
    let plan = ChangePlan {
        description: "Remove Vorbis comments".to_string(),
        changes: vec![FileChange {
            path: track.clone(),
            block_changes: vec![BlockChange {
                kind: TagBlockKind::VorbisComments,
                revision: None,
                old_revision: None,
                old_bytes: Some(before.clone()),
                old: Some(content),
                new: None,
                text_to_unicode: false,
            }],
            ..FileChange::default()
        }],
        ..ChangePlan::default()
    };
    let mut journal = VecJournal::new();
    let batch = Executor::apply(&plan, &mut journal, &roots(dir.path())).unwrap();
    assert!(TagEngine::read_block(&track, TagBlockKind::VorbisComments)
        .unwrap()
        .is_none_or(|content| content.tags.is_empty()));

    Executor::undo(&mut journal, batch.id, &roots(dir.path())).unwrap();

    assert_eq!(TagEngine::dump_vorbis(&track).unwrap(), Some(before));
    assert_eq!(TagEngine::read_covers(&track).unwrap(), vec![cover]);
    assert!(!TagBlockContent::exact(TagBlockKind::VorbisComments));
}

/// A custom field the file's tag cannot store (Vorbis Comments forbid
/// non-ASCII names and `=`) used to be written with `Ok` and be gone on the
/// next read. The whole plan is refused before anything is written instead.
#[test]
fn a_custom_field_the_tag_cannot_store_is_refused_before_anything_is_written() {
    let dir = TempDir::new("unstorable-custom");
    let first = dir.flac("first.flac");
    let second = dir.flac("second.flac");
    let before = std::fs::read(&first).unwrap();
    let mut journal = VecJournal::new();

    for name in ["ÜNI", "a=b"] {
        let mut plan = set_artist(&first, None, Some("Fine"));
        plan.changes.push(FileChange {
            path: second.clone(),
            tag_changes: vec![FieldChange {
                field: TagField::Custom(name.to_string()),
                old: None,
                new: Some("lost".to_string()),
            }],
            ..FileChange::default()
        });
        let result = Executor::apply(&plan, &mut journal, &roots(dir.path()));
        assert!(
            matches!(result, Err(PlanError::TagIo(_))),
            "{name:?}: {result:?}"
        );
        assert_eq!(std::fs::read(&first).unwrap(), before, "{name:?}");
        assert!(journal.batches().unwrap().is_empty());
    }

    // Clearing such a field is not refused: there is nothing to lose.
    let clear = ChangePlan {
        description: "clear".to_string(),
        changes: vec![FileChange {
            path: second.clone(),
            tag_changes: vec![FieldChange {
                field: TagField::Custom("ÜNI".to_string()),
                old: None,
                new: None,
            }],
            ..FileChange::default()
        }],
        ..ChangePlan::default()
    };
    Executor::apply(&clear, &mut journal, &roots(dir.path())).unwrap();
}

/// A refused undo (#658) had no way past it when the later edit came from
/// another program: there is no later batch to undo, and the history is a
/// stack. The person chooses instead: keep the newer values and undo the rest,
/// or put every old value back.
#[test]
fn a_refused_undo_can_keep_the_later_edit_or_overwrite_it() {
    for mode in [UndoMode::KeepChanged, UndoMode::Overwrite] {
        let dir = TempDir::new(&format!("undo-mode-{mode:?}"));
        let first = dir.flac("first.flac");
        let second = dir.flac("second.flac");
        let mut journal = VecJournal::new();
        let change = |path: &Path| FileChange {
            path: path.to_path_buf(),
            tag_changes: vec![
                FieldChange {
                    field: TagField::Artist,
                    old: None,
                    new: Some("Batch Artist".to_string()),
                },
                FieldChange {
                    field: TagField::Title,
                    old: None,
                    new: Some("Batch Title".to_string()),
                },
            ],
            ..FileChange::default()
        };
        let plan = ChangePlan {
            description: "two files".to_string(),
            changes: vec![change(&first), change(&second)],
            ..ChangePlan::default()
        };
        let batch = Executor::apply(&plan, &mut journal, &roots(dir.path())).unwrap();

        // Another program edits the first file's artist afterwards.
        let mut edited = TagEngine::read(&first).unwrap();
        edited
            .tags
            .insert(TagField::Artist, "Edited Elsewhere".to_string());
        TagEngine::write(&edited).unwrap();

        // Asked first, the undo is refused and writes nothing.
        assert!(matches!(
            Executor::undo(&mut journal, batch.id, &roots(dir.path())),
            Err(PlanError::ChangedSinceApplied(_))
        ));

        let report = Executor::undo_with_mode(
            &mut journal,
            batch.id,
            &roots(dir.path()),
            mode,
            &mut |_| {},
        )
        .unwrap();

        let first_now = TagEngine::read(&first).unwrap().tags;
        let second_now = TagEngine::read(&second).unwrap().tags;
        assert_eq!(
            first_now.get(&TagField::Title),
            None,
            "{mode:?}: title undone"
        );
        assert!(
            second_now.is_empty(),
            "{mode:?}: other file undone: {second_now:?}"
        );
        assert!(journal.batches().unwrap().is_empty(), "{mode:?}");
        match mode {
            UndoMode::KeepChanged => {
                assert_eq!(
                    first_now.get(&TagField::Artist).map(String::as_str),
                    Some("Edited Elsewhere")
                );
                assert_eq!(
                    report.kept,
                    vec![KeptField {
                        path: first.clone(),
                        field: TagField::Artist,
                    }]
                );
            }
            _ => {
                assert_eq!(first_now.get(&TagField::Artist), None, "overwritten");
                assert!(report.kept.is_empty());
            }
        }
    }
}

/// A failed final journal update plus an unavailable restoration path must keep
/// the original durable record, including after the journal is reopened.
#[test]
fn incomplete_rollback_retains_recovery_batch_after_reopen() {
    use tagrex_core::journal::{AppliedBatch, BatchId, JournalError, SqliteJournal};
    struct FailFinish {
        journal: SqliteJournal,
        path: PathBuf,
        saved: PathBuf,
    }
    impl UndoJournal for FailFinish {
        fn record(&mut self, b: &AppliedBatch) -> Result<BatchId, JournalError> {
            self.journal.record(b)
        }
        fn batches(&self) -> Result<Vec<AppliedBatch>, JournalError> {
            self.journal.batches()
        }
        fn finish(&mut self, id: BatchId, _: &AppliedBatch) -> Result<(), JournalError> {
            std::fs::rename(&self.path, &self.saved).unwrap();
            std::fs::create_dir(&self.path).unwrap();
            Err(JournalError::UnknownBatch(id))
        }
        fn retain_interrupted(
            &mut self,
            id: BatchId,
            batch: &AppliedBatch,
        ) -> Result<(), JournalError> {
            self.journal.retain_interrupted(id, batch)
        }
        fn rollback(&mut self, id: BatchId) -> Result<(), JournalError> {
            self.journal.rollback(id)
        }
    }
    let dir = TempDir::new("recovery-retention");
    let track = dir.flac("track.flac");
    let restored = dir.flac("restored.flac");
    let saved = dir.path().join("surviving.flac");
    let db = dir.path().join("journal.sqlite");
    let mut journal = FailFinish {
        journal: SqliteJournal::open(&db).unwrap(),
        path: track.clone(),
        saved: saved.clone(),
    };
    let mut plan = set_artist(&track, None, Some("CHANGED"));
    plan.changes
        .extend(set_artist(&restored, None, Some("CHANGED")).changes);
    let error = Executor::apply(&plan, &mut journal, &roots(dir.path())).unwrap_err();
    assert!(matches!(error, PlanError::RollbackIncomplete { .. }));
    drop(journal);
    let mut journal = SqliteJournal::open(&db).unwrap();
    let batches = journal.batches().unwrap();
    assert_eq!(
        batches.len(),
        1,
        "the surviving mutation needs durable recovery evidence"
    );
    assert!(batches[0].interrupted);
    assert!(!TagEngine::read(&restored)
        .unwrap()
        .tags
        .contains_key(&TagField::Artist));
    // A restored file is recorded in its *current* state; it must not make a
    // retry falsely look like an unrelated later edit.
    assert!(batches[0]
        .applied_values
        .iter()
        .any(|v| v.path == restored && v.field == TagField::Artist && v.value.is_none()));
    std::fs::remove_dir(&track).unwrap();
    std::fs::rename(saved, &track).unwrap();
    Executor::undo(&mut journal, batches[0].id, &roots(dir.path())).unwrap();
    assert!(!TagEngine::read(&track)
        .unwrap()
        .tags
        .contains_key(&TagField::Artist));
    assert!(journal.batches().unwrap().is_empty());
}

#[test]
fn write_phase_failure_retains_the_interrupted_batch_when_restore_is_blocked() {
    use tagrex_core::journal::SqliteJournal;
    let dir = TempDir::new("write-phase-recovery");
    let first = dir.flac("first.flac");
    let second = dir.flac("second.flac");
    let saved_first = dir.path().join("saved-first.flac");
    let saved_second = dir.path().join("saved-second.flac");
    let mut plan = set_artist(&first, None, Some("Changed"));
    plan.changes
        .extend(set_artist(&second, None, Some("Changed")).changes);
    let mut journal = SqliteJournal::open(&dir.path().join("journal.sqlite")).unwrap();
    let mut injected = false;
    let error = Executor::apply_with(
        &plan,
        &mut journal,
        &roots(dir.path()),
        &mut |progress| {
            // Preflight has finished and the first write is complete. Simulate a
            // share changing before the second write and restoration can run.
            if !injected && progress.done == plan.changes.len() + 1 {
                injected = true;
                for (from, to) in [(&first, &saved_first), (&second, &saved_second)] {
                    std::fs::rename(from, to).unwrap();
                    std::fs::create_dir(from).unwrap();
                }
            }
        },
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert!(injected);
    assert!(matches!(error, PlanError::RollbackIncomplete { .. }));
    let batch = journal.batches().unwrap().pop().unwrap();
    assert!(batch.interrupted);
    drop(journal);
    for (from, to) in [(&saved_first, &first), (&saved_second, &second)] {
        std::fs::remove_dir(to).unwrap();
        std::fs::rename(from, to).unwrap();
    }
    let mut journal = SqliteJournal::open(&dir.path().join("journal.sqlite")).unwrap();
    Executor::undo(&mut journal, batch.id, &roots(dir.path())).unwrap();
    for path in [&first, &second] {
        assert!(!TagEngine::read(path)
            .unwrap()
            .tags
            .contains_key(&TagField::Artist));
    }
    assert!(journal.batches().unwrap().is_empty());
}
