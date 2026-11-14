//! Persistence tests for the SQLite journal: the whole point of #5 is that a
//! recorded batch survives an application restart. "Restart" here is dropping
//! the `SqliteJournal` (closing the connection) and reopening the same file.

use std::path::{Path, PathBuf};

use tagrex_core::journal::{
    AppliedBatch, AppliedValue, BatchId, CopyStamp, FileRevision, SqliteJournal, UndoJournal,
};
use tagrex_core::model::{Id3v2Revision, TagEngine, TagField};
use tagrex_core::plan::{ChangePlan, Executor, FieldChange, FileChange};

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
fn history_headers_never_hydrate_snapshot_tables() {
    let dir = TempDir::new("headers-only");
    let track = dir.flac("track.flac");
    let mut journal = SqliteJournal::open(&dir.db()).unwrap();
    let batch = Executor::apply(
        &set_artist(&track, None, Some("A")),
        &mut journal,
        &roots(dir.path()),
    )
    .unwrap();
    let conn = rusqlite::Connection::open(dir.db()).unwrap();
    // A header query must not need even the text-snapshot table, let alone
    // images. Damage is deliberate and confined to this disposable journal.
    conn.execute("DROP TABLE field_changes", []).unwrap();
    assert!(journal.batch(batch.id).is_err());
    let headers = journal.headers(&roots(dir.path())).unwrap();
    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0].id, batch.id);
    assert!(journal
        .headers(&[dir.path().join("other-library")])
        .unwrap()
        .is_empty());
}

#[test]
fn batch_survives_reopening_the_database() {
    let dir = TempDir::new("persist");
    let track = dir.flac("track.flac");

    let batch_id = {
        let mut journal = SqliteJournal::open(&dir.db()).unwrap();
        let plan = set_artist(&track, None, Some("Persisted Artist"));
        Executor::apply(&plan, &mut journal, &roots(dir.path()))
            .unwrap()
            .id
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
        Executor::apply(&plan, &mut journal, &roots(dir.path()))
            .unwrap()
            .id
    };
    assert!(TagEngine::read(&track)
        .unwrap()
        .tags
        .contains_key(&TagField::Artist));

    // Reopen and roll back -- the morning-after scenario.
    let mut journal = SqliteJournal::open(&dir.db()).unwrap();
    Executor::undo(&mut journal, batch_id, &roots(dir.path())).unwrap();

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
            cover_change: None,
            rename_to: None,
            sidecar_renames: Vec::new(),
            ..FileChange::default()
        }],
        ..ChangePlan::default()
    };

    {
        let mut journal = SqliteJournal::open(&dir.db()).unwrap();
        Executor::apply(&plan, &mut journal, &roots(dir.path())).unwrap();
    }

    let journal = SqliteJournal::open(&dir.db()).unwrap();
    let batches = journal.batches().unwrap();
    assert_eq!(
        batches[0].plan.changes[0].tag_changes[0].field,
        TagField::Custom("MOOD".to_string())
    );
}

/// #47: the snapshot of a stripped block is the only copy of it left, so it has
/// to survive a restart — otherwise the "realized in the morning" scenario this
/// journal exists for loses the block instead of restoring it.
#[test]
fn a_stripped_block_can_still_be_restored_after_a_restart() {
    use lofty::config::WriteOptions;
    use lofty::id3::v1::Id3v1Tag;
    use lofty::prelude::TagExt;
    use tagrex_core::model::TagBlockKind;
    use tagrex_core::plan::BlockChange;

    let dir = TempDir::new("block-restart");
    let track = dir.path().join("track.mp3");
    let mut frame = vec![0xFF, 0xFB, 0x90, 0x00];
    frame.resize(417, 0);
    std::fs::write(&track, frame.repeat(5)).unwrap();
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

    let batch_id = {
        let mut journal = SqliteJournal::open(&dir.db()).unwrap();
        Executor::apply(&plan, &mut journal, &roots(dir.path()))
            .unwrap()
            .id
    };
    assert!(TagEngine::read_block(&track, TagBlockKind::Id3v1)
        .unwrap()
        .is_none());

    // Reopen, as a fresh process would, and roll back.
    let mut journal = SqliteJournal::open(&dir.db()).unwrap();
    assert_eq!(
        journal.batches().unwrap()[0].plan.changes[0].block_changes,
        vec![BlockChange::removal(TagBlockKind::Id3v1, removed.clone())],
        "the snapshot did not survive the reopen"
    );
    Executor::undo(&mut journal, batch_id, &roots(dir.path())).unwrap();

    assert_eq!(
        TagEngine::read_block(&track, TagBlockKind::Id3v1).unwrap(),
        Some(removed)
    );
}

/// #206: the bytes of a destroyed ID3v2 block are the only copy of its binary
/// frames, so they have to survive a restart along with everything else.
#[test]
fn the_bytes_of_a_destroyed_id3v2_block_survive_a_restart() {
    use lofty::config::WriteOptions;
    use lofty::prelude::{Accessor, TagExt};
    use tagrex_core::model::{TagBlockKind, TagEngine as Engine};
    use tagrex_core::plan::BlockChange;

    let dir = TempDir::new("block-bytes-restart");
    let track = dir.path().join("track.mp3");
    let mut frame = vec![0xFF, 0xFB, 0x90, 0x00];
    frame.resize(417, 0);
    std::fs::write(&track, frame.repeat(5)).unwrap();
    let mut tag = lofty::id3::v2::Id3v2Tag::new();
    tag.set_artist("Journaled".to_string());
    tag.save_to_path(&track, WriteOptions::default()).unwrap();

    let content = Engine::read_block(&track, TagBlockKind::Id3v2)
        .unwrap()
        .unwrap();
    let bytes = Engine::dump_id3v2(&track).unwrap().expect("bytes");
    let plan = ChangePlan {
        description: "Remove ID3v2 tag".to_string(),
        changes: vec![FileChange {
            path: track.clone(),
            block_changes: vec![BlockChange {
                kind: TagBlockKind::Id3v2,
                revision: None,
                old_revision: None,
                old_bytes: Some(bytes.clone()),
                old: Some(content),
                new: None,
                text_to_unicode: false,
            }],
            ..FileChange::default()
        }],
        ..ChangePlan::default()
    };

    {
        let mut journal = SqliteJournal::open(&dir.db()).unwrap();
        Executor::apply(&plan, &mut journal, &roots(dir.path())).unwrap();
    }

    let journal = SqliteJournal::open(&dir.db()).unwrap();
    assert_eq!(
        journal.batches().unwrap()[0].plan.changes[0].block_changes[0].old_bytes,
        Some(bytes),
        "the block's bytes did not survive the reopen"
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
            &roots(dir.path()),
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
            &roots(dir.path()),
        )
        .unwrap()
        .id
    };

    assert_ne!(first, second);
    assert!(second.0 > first.0);
}

#[test]
fn one_batch_can_be_fetched_without_loading_the_others() {
    let dir = TempDir::new("one-batch");
    let first = dir.flac("first.flac");
    let second = dir.flac("second.flac");
    let mut journal = SqliteJournal::open(&dir.db()).unwrap();
    let a = Executor::apply(
        &set_artist(&first, None, Some("First")),
        &mut journal,
        &roots(dir.path()),
    )
    .unwrap();
    let b = Executor::apply(
        &set_artist(&second, None, Some("Second")),
        &mut journal,
        &roots(dir.path()),
    )
    .unwrap();

    let fetched = journal.batch(a.id).unwrap().expect("the batch is there");
    assert_eq!(fetched.id, a.id);
    assert_eq!(fetched.plan.changes.len(), 1);
    assert_eq!(fetched.plan.changes[0].path, first);
    assert_eq!(
        journal.batch(b.id).unwrap().unwrap().plan.changes[0].path,
        second
    );
    assert!(journal.batch(BatchId(9999)).unwrap().is_none());
}

/// Every child table is read and deleted by its parent's id; without an index
/// each of those lookups scans the whole table, and a journal with a few large
/// batches took minutes to load or roll back (#449).
#[test]
fn the_journal_tables_are_indexed_by_their_parent() {
    let dir = TempDir::new("indexes");
    drop(SqliteJournal::open(&dir.db()).unwrap());
    let conn = rusqlite::Connection::open(dir.db()).unwrap();
    for query in [
        "SELECT * FROM file_changes WHERE batch_id = 1",
        "SELECT * FROM field_changes WHERE file_change_id = 1",
        "SELECT * FROM cover_images WHERE file_change_id = 1",
        "SELECT * FROM sidecar_renames WHERE file_change_id = 1",
        "SELECT * FROM block_changes WHERE file_change_id = 1",
        "SELECT * FROM block_change_fields WHERE block_change_id = 1",
        "SELECT * FROM allowed_roots WHERE batch_id = 1",
        "SELECT * FROM copy_stamps WHERE batch_id = 1",
        "SELECT * FROM applied_values WHERE batch_id = 1",
    ] {
        let plan: String = conn
            .query_row(&format!("EXPLAIN QUERY PLAN {query}"), [], |row| row.get(3))
            .unwrap();
        assert!(plan.contains("USING INDEX"), "{query}: {plan}");
    }
}

#[test]
fn the_stamp_of_a_copy_survives_a_restart() {
    let dir = TempDir::new("copy-stamp");
    let track = dir.flac("track.flac");
    let target = dir.path().join("copy.flac");
    let plan = ChangePlan {
        description: "copy".to_string(),
        changes: vec![FileChange {
            path: track,
            rename_to: Some(target.clone()),
            copy: true,
            ..FileChange::default()
        }],
        ..ChangePlan::default()
    };
    let id = {
        let mut journal = SqliteJournal::open(&dir.db()).unwrap();
        Executor::apply(&plan, &mut journal, &roots(dir.path()))
            .unwrap()
            .id
    };

    let journal = SqliteJournal::open(&dir.db()).unwrap();
    let batch = journal.batch(id).unwrap().unwrap();
    assert_eq!(batch.copy_stamps, vec![CopyStamp::of(&target).unwrap()]);
}

/// What a batch left each field as (#658) is what undo checks the file
/// against, so it has to be there after a restart.
#[test]
fn what_a_batch_left_survives_a_restart() {
    let dir = TempDir::new("applied-values");
    let track = dir.flac("track.flac");
    let plan = ChangePlan {
        description: "set".to_string(),
        changes: vec![FileChange {
            path: track.clone(),
            tag_changes: vec![
                FieldChange {
                    field: TagField::Artist,
                    old: None,
                    new: Some("Y".to_string()),
                },
                FieldChange {
                    field: TagField::Album,
                    old: None,
                    new: None,
                },
            ],
            ..FileChange::default()
        }],
        ..ChangePlan::default()
    };
    let id = {
        let mut journal = SqliteJournal::open(&dir.db()).unwrap();
        Executor::apply(&plan, &mut journal, &roots(dir.path()))
            .unwrap()
            .id
    };

    let journal = SqliteJournal::open(&dir.db()).unwrap();
    let batch = journal.batch(id).unwrap().unwrap();
    assert_eq!(
        batch.applied_values,
        vec![
            AppliedValue {
                path: track.clone(),
                field: TagField::Artist,
                value: Some("Y".to_string()),
            },
            AppliedValue {
                path: track,
                field: TagField::Album,
                value: None,
            },
        ]
    );
}

/// #659: the batch is in the journal, marked interrupted, before the first file
/// is written, and loses the mark once every file is.
#[test]
fn a_batch_is_journaled_before_its_first_write() {
    use std::sync::atomic::AtomicBool;

    let dir = TempDir::new("write-ahead");
    let tracks = [dir.flac("a.flac"), dir.flac("b.flac")];
    let plan = ChangePlan {
        description: "set".to_string(),
        changes: tracks
            .iter()
            .map(|path| FileChange {
                path: path.clone(),
                tag_changes: vec![FieldChange {
                    field: TagField::Artist,
                    old: None,
                    new: Some("Y".to_string()),
                }],
                ..FileChange::default()
            })
            .collect(),
        ..ChangePlan::default()
    };
    let mut journal = SqliteJournal::open(&dir.db()).unwrap();
    let mut seen = Vec::new();
    let batch = Executor::apply_with(
        &plan,
        &mut journal,
        &roots(dir.path()),
        &mut |progress| {
            // Preflight reports before a batch exists and writes nothing.
            if progress.done < plan.changes.len() {
                return;
            }
            // What another process would find, were this one to stop here.
            let other = SqliteJournal::open(&dir.db()).unwrap();
            seen.push(
                other
                    .batches()
                    .unwrap()
                    .iter()
                    .map(|batch| batch.interrupted)
                    .collect::<Vec<_>>(),
            );
        },
        &AtomicBool::new(false),
    )
    .unwrap();

    assert!(!seen.is_empty());
    assert!(seen.iter().all(|batches| batches == &[true]), "{seen:?}");
    assert!(!batch.interrupted);
    let reopened = SqliteJournal::open(&dir.db()).unwrap();
    let stored = reopened.batch(batch.id).unwrap().unwrap();
    assert!(!stored.interrupted);
    assert_eq!(stored.applied_values.len(), 2);
}

/// #659: a batch the process never finished is still there at the next start,
/// marked, and undoing it puts back what it reached and skips what it did not.
#[test]
fn an_interrupted_batch_can_be_undone_after_a_restart() {
    let dir = TempDir::new("interrupted");
    let reached = dir.flac("reached.flac");
    let moved = dir.path().join("moved.flac");
    let unreached = dir.flac("unreached.flac");
    let never_moved = dir.path().join("never-moved.flac");
    let change = |path: &PathBuf, rename_to: &PathBuf| FileChange {
        path: path.clone(),
        tag_changes: vec![FieldChange {
            field: TagField::Artist,
            old: None,
            new: Some("Y".to_string()),
        }],
        rename_to: Some(rename_to.clone()),
        ..FileChange::default()
    };
    let plan = ChangePlan {
        description: "set and move".to_string(),
        changes: vec![change(&reached, &moved), change(&unreached, &never_moved)],
        ..ChangePlan::default()
    };
    // What the process got done before it stopped: the first file written and
    // moved, the second not touched.
    let mut file = TagEngine::read(&reached).unwrap();
    file.tags.insert(TagField::Artist, "Y".to_string());
    TagEngine::write(&file).unwrap();
    std::fs::rename(&reached, &moved).unwrap();
    SqliteJournal::open(&dir.db())
        .unwrap()
        .record(&AppliedBatch {
            id: BatchId(0),
            description: plan.description.clone(),
            message: None,
            applied_at: 0,
            plan,
            created_dirs: Vec::new(),
            removed_dirs: Vec::new(),
            allowed_roots: roots(dir.path()),
            copy_stamps: Vec::new(),
            applied_values: Vec::new(),
            interrupted: true,
            id3v2_revisions: Vec::new(),
        })
        .unwrap();

    let mut journal = SqliteJournal::open(&dir.db()).unwrap();
    let batches = journal.batches().unwrap();
    assert_eq!(batches.len(), 1);
    assert!(batches[0].interrupted);
    Executor::undo(&mut journal, batches[0].id, &roots(dir.path())).unwrap();

    assert!(reached.exists() && !moved.exists());
    assert!(unreached.exists() && !never_moved.exists());
    for path in [&reached, &unreached] {
        assert!(!TagEngine::read(path)
            .unwrap()
            .tags
            .contains_key(&TagField::Artist));
    }
    assert!(journal.batches().unwrap().is_empty());
}

/// #666: the revision each file was in survives a restart, recorded once even
/// though the batch is written to the journal twice.
#[test]
fn the_id3v2_revisions_survive_a_restart() {
    let dir = TempDir::new("revisions");
    let revisions = vec![
        FileRevision {
            path: dir.path().join("a.mp3"),
            revision: Id3v2Revision::V3,
        },
        FileRevision {
            path: dir.path().join("b.mp3"),
            revision: Id3v2Revision::V4,
        },
    ];
    let mut batch = AppliedBatch {
        id: BatchId(0),
        description: "edit".to_string(),
        message: None,
        applied_at: 0,
        plan: ChangePlan::default(),
        created_dirs: Vec::new(),
        removed_dirs: Vec::new(),
        allowed_roots: Vec::new(),
        copy_stamps: Vec::new(),
        applied_values: Vec::new(),
        interrupted: true,
        id3v2_revisions: revisions.clone(),
    };
    let mut journal = SqliteJournal::open(&dir.db()).unwrap();
    batch.id = journal.record(&batch).unwrap();
    batch.interrupted = false;
    journal.finish(batch.id, &batch).unwrap();
    drop(journal);

    let reopened = SqliteJournal::open(&dir.db()).unwrap();
    let stored = reopened.batch(batch.id).unwrap().unwrap();
    assert_eq!(stored.id3v2_revisions, revisions);
}

/// A damaged journal made every library refuse to open. It is set aside, byte
/// for byte, and a fresh journal takes its place.
#[test]
fn a_damaged_journal_is_set_aside_and_started_afresh() {
    let dir = TempDir::new("damaged");
    let db = dir.db();
    {
        let mut journal = SqliteJournal::open(&db).unwrap();
        let track = dir.flac("a.flac");
        let plan = ChangePlan {
            description: "seed".to_string(),
            changes: vec![FileChange {
                path: track,
                tag_changes: vec![FieldChange {
                    field: TagField::Artist,
                    old: None,
                    new: Some("A".to_string()),
                }],
                ..FileChange::default()
            }],
            ..ChangePlan::default()
        };
        Executor::apply(&plan, &mut journal, &[dir.path().to_path_buf()]).unwrap();
    }
    let healthy = std::fs::read(&db).unwrap();
    let mut header_corrupt = healthy.clone();
    for byte in &mut header_corrupt[16..100] {
        *byte = 0xFF;
    }
    let cases: [(&str, Vec<u8>); 3] = [
        ("garbage", b"this is not a database".repeat(100)),
        ("truncated", healthy[..100].to_vec()),
        ("header", header_corrupt),
    ];
    for (name, bytes) in cases {
        std::fs::write(&db, &bytes).unwrap();
        assert!(
            SqliteJournal::open(&db).is_err(),
            "{name}: fixture must be damaged"
        );

        let (journal, aside) = SqliteJournal::open_or_recover(&db).unwrap();
        let aside = aside.unwrap_or_else(|| panic!("{name}: nothing set aside"));
        assert!(journal.batches().unwrap().is_empty(), "{name}");
        assert_eq!(
            std::fs::read(&aside).unwrap(),
            bytes,
            "{name}: kept as it was"
        );
        std::fs::remove_file(&aside).ok();
        drop(journal);
        std::fs::remove_file(&db).ok();
    }
}

/// Only damage sets a journal aside: a healthy one opens as it is, and one that
/// cannot be opened for another reason is an error with nothing moved.
#[test]
fn a_healthy_or_unavailable_journal_is_never_set_aside() {
    let dir = TempDir::new("not-damaged");
    let db = dir.db();
    drop(SqliteJournal::open(&db).unwrap());
    let (_, aside) = SqliteJournal::open_or_recover(&db).unwrap();
    assert_eq!(aside, None);

    // A folder where the file should be cannot be opened, and is no database
    // to recover either.
    let folder = dir.path().join("journal-folder.sqlite");
    std::fs::create_dir_all(&folder).unwrap();
    assert!(SqliteJournal::open_or_recover(&folder).is_err());
    assert!(folder.is_dir(), "an unavailable journal is not moved");
}

#[test]
fn new_artwork_snapshots_remain_inline_and_share_memory_after_loading() {
    use tagrex_core::model::{CoverArt, CoverKind, TagBlockContent, TagBlockKind};
    use tagrex_core::plan::{BlockChange, CoverChange};
    let dir = TempDir::new("inline-compatible");
    let art = CoverArt {
        mime: "image/jpeg".into(),
        data: vec![7; 65536].into(),
        kind: CoverKind::Back,
        description: "Sleeve".into(),
    };
    let plan = ChangePlan {
        changes: [dir.flac("a.flac"), dir.flac("b.flac")]
            .iter()
            .map(|path| FileChange {
                path: path.clone(),
                cover_change: Some(CoverChange {
                    old: vec![],
                    new: vec![art.clone()],
                }),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    let mut journal = SqliteJournal::open(&dir.db()).unwrap();
    let batch = Executor::apply(&plan, &mut journal, &roots(dir.path())).unwrap();
    drop(journal);
    let connection = rusqlite::Connection::open(dir.db()).unwrap();
    // The exact stable-reader columns must hold real BLOBs, never sentinels or references.
    let rows = connection
        .prepare("SELECT kind, description, mime, data FROM cover_images ORDER BY id")
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Vec<u8>>(3)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(rows.len(), 2);
    for (kind, description, mime, bytes) in rows {
        assert_eq!(
            (kind, description, mime),
            ("back".into(), "Sleeve".into(), "image/jpeg".into())
        );
        assert_eq!(bytes.as_slice(), art.data.as_ref());
    }
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name = 'journal_image_blobs'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    let mut reopened = SqliteJournal::open(&dir.db()).unwrap();
    let loaded = reopened.batch(batch.id).unwrap().unwrap();
    assert_eq!(
        loaded.plan.changes[0].cover_change.as_ref().unwrap().new[0]
            .data
            .as_ptr(),
        loaded.plan.changes[1].cover_change.as_ref().unwrap().new[0]
            .data
            .as_ptr()
    );
    let mut with_block = batch.clone();
    with_block.plan.changes[0]
        .block_changes
        .push(BlockChange::removal(
            TagBlockKind::VorbisComments,
            TagBlockContent {
                covers: vec![art.clone()],
                ..Default::default()
            },
        ));
    let block_id = reopened.record(&with_block).unwrap();
    let stored: Vec<u8> = connection
        .query_row("SELECT data FROM block_change_images LIMIT 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(stored.as_slice(), art.data.as_ref());
    let cross_table = reopened.batch(block_id).unwrap().unwrap();
    assert_eq!(
        cross_table.plan.changes[0]
            .cover_change
            .as_ref()
            .unwrap()
            .new[0]
            .data
            .as_ptr(),
        cross_table.plan.changes[0].block_changes[0]
            .old
            .as_ref()
            .unwrap()
            .covers[0]
            .data
            .as_ptr()
    );
    reopened.rollback(block_id).unwrap();
    Executor::undo(&mut reopened, batch.id, &roots(dir.path())).unwrap();
    for change in plan.changes {
        assert!(TagEngine::read_covers(&change.path).unwrap().is_empty());
    }
}

#[test]
fn separate_batches_keep_exact_inline_artwork_until_their_own_undo() {
    use tagrex_core::model::{CoverArt, CoverKind};
    use tagrex_core::plan::CoverChange;
    let dir = TempDir::new("inline-batches");
    let art = CoverArt {
        mime: "image/jpeg".into(),
        data: vec![7; 64].into(),
        kind: CoverKind::Back,
        description: "Back".into(),
    };
    let mut journal = SqliteJournal::open(&dir.db()).unwrap();
    let make_plan = |path| ChangePlan {
        changes: vec![FileChange {
            path,
            cover_change: Some(CoverChange {
                old: vec![],
                new: vec![art.clone()],
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    let first = Executor::apply(
        &make_plan(dir.flac("a.flac")),
        &mut journal,
        &roots(dir.path()),
    )
    .unwrap();
    let second = Executor::apply(
        &make_plan(dir.flac("b.flac")),
        &mut journal,
        &roots(dir.path()),
    )
    .unwrap();
    Executor::undo(&mut journal, second.id, &roots(dir.path())).unwrap();
    assert_eq!(
        journal.batch(first.id).unwrap().unwrap().plan.changes[0]
            .cover_change
            .as_ref()
            .unwrap()
            .new,
        [art]
    );
    Executor::undo(&mut journal, first.id, &roots(dir.path())).unwrap();
    assert!(journal.batches().unwrap().is_empty());
}
