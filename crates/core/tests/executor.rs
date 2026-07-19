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
        }],
    }
}

#[test]
fn apply_writes_tags_and_records_the_batch() {
    let dir = TempDir::new("apply");
    let track = dir.flac("track.flac");
    let mut journal = VecJournal::new();

    let plan = set_artist(&track, None, Some("Boards of Canada"));
    let batch = Executor::apply(&plan, &mut journal, dir.path()).unwrap();

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
    let batch = Executor::apply(&plan, &mut journal, dir.path()).unwrap();
    assert!(TagEngine::read(&track)
        .unwrap()
        .tags
        .contains_key(&TagField::Artist));

    Executor::undo(&mut journal, batch.id, dir.path()).unwrap();

    assert!(!TagEngine::read(&track)
        .unwrap()
        .tags
        .contains_key(&TagField::Artist));
    assert!(journal.batches().unwrap().is_empty());
}

#[test]
fn rejects_a_path_outside_the_allowed_root() {
    let root = TempDir::new("root");
    let outside = TempDir::new("outside");
    let track = outside.flac("track.flac");
    let mut journal = VecJournal::new();

    let plan = set_artist(&track, None, Some("Nope"));
    let err = Executor::apply(&plan, &mut journal, root.path()).unwrap_err();

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
    let err = Executor::apply(&plan, &mut journal, dir.path()).unwrap_err();

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
    let batch = Executor::apply(&plan, &mut journal, dir.path()).unwrap();

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

    Executor::undo(&mut journal, batch.id, dir.path()).unwrap();

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
    let err = Executor::apply(&plan, &mut journal, dir.path()).unwrap_err();

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
            },
            FileChange {
                path: b,
                tag_changes: vec![],
                cover_change: None,
                rename_to: Some(target),
            },
        ],
    };
    let err = Executor::apply(&plan, &mut journal, dir.path()).unwrap_err();

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
    let err = Executor::apply(&plan, &mut journal, root.path()).unwrap_err();

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
    };
    let plan = ChangePlan {
        description: "embed cover".to_string(),
        changes: vec![FileChange {
            path: track.clone(),
            tag_changes: vec![],
            cover_change: Some(CoverChange {
                old: None,
                new: Some(cover.clone()),
            }),
            rename_to: None,
        }],
    };

    let batch = Executor::apply(&plan, &mut journal, dir.path()).unwrap();
    assert_eq!(
        TagEngine::read_cover(&track).unwrap().map(|c| c.data),
        Some(cover.data.clone())
    );

    Executor::undo(&mut journal, batch.id, dir.path()).unwrap();
    assert_eq!(TagEngine::read_cover(&track).unwrap(), None);
}
