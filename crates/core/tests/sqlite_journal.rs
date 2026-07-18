//! Persistence tests for the SQLite journal: the whole point of #5 is that a
//! recorded batch survives an application restart. "Restart" here is dropping
//! the `SqliteJournal` (closing the connection) and reopening the same file.

use std::path::{Path, PathBuf};

use tagrex_core::journal::{SqliteJournal, UndoJournal};
use tagrex_core::model::{TagEngine, TagField};
use tagrex_core::plan::{ChangePlan, Executor, FieldChange, FileChange};

const MINIMAL_FLAC: [u8; 62] = [
    0x66, 0x4c, 0x61, 0x43, 0x00, 0x00, 0x00, 0x22, 0x10, 0x00, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x0a, 0xc4, 0x42, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x81, 0x00, 0x00, 0x10, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "tagrex-sqlite-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn flac(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, MINIMAL_FLAC).unwrap();
        path
    }

    fn db(&self) -> PathBuf {
        self.0.join("journal.sqlite")
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
            rename_to: None,
        }],
    }
}

#[test]
fn batch_survives_reopening_the_database() {
    let dir = TempDir::new("persist");
    let track = dir.flac("track.flac");

    let batch_id = {
        let mut journal = SqliteJournal::open(&dir.db()).unwrap();
        let plan = set_artist(&track, None, Some("Persisted Artist"));
        Executor::apply(&plan, &mut journal, dir.path()).unwrap().id
        // journal dropped here -> connection closed, simulating app exit
    };

    // Reopen the same database file, as a fresh process would.
    let journal = SqliteJournal::open(&dir.db()).unwrap();
    let batches = journal.batches().unwrap();

    assert_eq!(batches.len(), 1);
    let batch = &batches[0];
    assert_eq!(batch.id, batch_id);
    assert_eq!(batch.description, "set artist");
    assert_eq!(batch.plan.changes.len(), 1);
    let change = &batch.plan.changes[0];
    assert_eq!(change.path, track);
    assert_eq!(change.tag_changes[0].field, TagField::Artist);
    assert_eq!(change.tag_changes[0].old, None);
    assert_eq!(
        change.tag_changes[0].new.as_deref(),
        Some("Persisted Artist")
    );
}

#[test]
fn undo_works_against_a_reopened_journal() {
    let dir = TempDir::new("undo-restart");
    let track = dir.flac("track.flac");

    let batch_id = {
        let mut journal = SqliteJournal::open(&dir.db()).unwrap();
        let plan = set_artist(&track, None, Some("Temporary"));
        Executor::apply(&plan, &mut journal, dir.path()).unwrap().id
    };
    assert!(TagEngine::read(&track)
        .unwrap()
        .tags
        .contains_key(&TagField::Artist));

    // Reopen and roll back -- the morning-after scenario.
    let mut journal = SqliteJournal::open(&dir.db()).unwrap();
    Executor::undo(&mut journal, batch_id, dir.path()).unwrap();

    assert!(!TagEngine::read(&track)
        .unwrap()
        .tags
        .contains_key(&TagField::Artist));
    assert!(journal.batches().unwrap().is_empty());
}

#[test]
fn custom_field_round_trips_through_storage() {
    let dir = TempDir::new("custom");
    let track = dir.flac("track.flac");

    let plan = ChangePlan {
        description: "set custom".to_string(),
        changes: vec![FileChange {
            path: track.clone(),
            tag_changes: vec![FieldChange {
                field: TagField::Custom("MOOD".to_string()),
                old: None,
                new: Some("Energetic".to_string()),
            }],
            rename_to: None,
        }],
    };

    {
        let mut journal = SqliteJournal::open(&dir.db()).unwrap();
        Executor::apply(&plan, &mut journal, dir.path()).unwrap();
    }

    let journal = SqliteJournal::open(&dir.db()).unwrap();
    let batches = journal.batches().unwrap();
    assert_eq!(
        batches[0].plan.changes[0].tag_changes[0].field,
        TagField::Custom("MOOD".to_string())
    );
}

#[test]
fn ids_keep_climbing_across_reopens() {
    let dir = TempDir::new("ids");
    let track = dir.flac("track.flac");

    let first = {
        let mut journal = SqliteJournal::open(&dir.db()).unwrap();
        Executor::apply(
            &set_artist(&track, None, Some("A")),
            &mut journal,
            dir.path(),
        )
        .unwrap()
        .id
    };

    // A fresh connection must not restart ids from 1 and collide.
    let second = {
        let mut journal = SqliteJournal::open(&dir.db()).unwrap();
        Executor::apply(
            &set_artist(&track, Some("A"), Some("B")),
            &mut journal,
            dir.path(),
        )
        .unwrap()
        .id
    };

    assert_ne!(first, second);
    assert!(second.0 > first.0);
}
