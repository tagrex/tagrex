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
    };

    let mut journal = VecJournal::default();
    let batch = Executor::apply(&plan, &mut journal, dir.path()).unwrap();

    assert!(target.exists(), "file moved into the new folder tree");
    assert!(!track.exists());
    assert!(target_in_existing.exists());
    // Two levels were created for the first target; `Existing` was not.
    assert_eq!(batch.created_dirs.len(), 2);
    assert!(batch.created_dirs.iter().all(|d| d.starts_with(dir.path())));

    Executor::undo(&mut journal, batch.id, dir.path()).unwrap();

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
    };

    let mut journal = VecJournal::default();
    let result = Executor::apply(&plan, &mut journal, dir.path());
    assert!(matches!(result, Err(PlanError::OutsideRoot(_))));
    assert!(track.exists(), "nothing moved");
    assert!(
        !dir.path().join("../outside").exists(),
        "no folders created"
    );

    std::fs::remove_dir_all(dir.path()).ok();
}
