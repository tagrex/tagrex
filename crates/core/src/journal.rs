//! Persistent undo journal.
//!
//! The one place where extra complexity is bought up front (architecture.md):
//! the journal must survive application restarts, record both tag writes and
//! renames, and allow rollback of a whole batch as a unit. The motivating
//! scenario: "renamed 8,000 files, closed the app, realized in the morning
//! the mask was wrong."

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::model::{CoverArt, TagField};
use crate::plan::{ChangePlan, CoverChange, FieldChange, FileChange};

/// Identifier of an applied batch, stable across restarts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchId(pub i64);

/// A batch that has been applied to disk, as recorded in the journal.
#[derive(Debug, Clone)]
pub struct AppliedBatch {
    pub id: BatchId,
    /// Description inherited from the plan source (for the history UI).
    pub description: String,
    /// Unix timestamp of when the batch was applied.
    pub applied_at: i64,
    /// The executed plan; `old` values are what rollback restores.
    pub plan: ChangePlan,
}

pub trait UndoJournal {
    /// Persist a batch and return the id the journal assigned to it.
    ///
    /// The journal owns id assignment (like a database autoincrement) so ids
    /// stay unique across application restarts; `batch.id` on the way in is
    /// ignored. The caller should adopt the returned id.
    fn record(&mut self, batch: &AppliedBatch) -> Result<BatchId, JournalError>;

    /// All recorded batches, newest first.
    fn batches(&self) -> Result<Vec<AppliedBatch>, JournalError>;

    /// Roll back a whole batch as a unit.
    fn rollback(&mut self, id: BatchId) -> Result<(), JournalError>;
}

/// In-memory journal: keeps recorded batches in a `Vec` for the lifetime of
/// the process, nothing more. It exists so the transaction pipeline
/// ([`Executor`](crate::plan::Executor)) can be built and tested end to end
/// before the persistent [`SqliteJournal`] lands. It deliberately does **not**
/// satisfy the cross-restart durability requirement in architecture.md — that
/// is `SqliteJournal`'s job.
#[derive(Debug, Default)]
pub struct VecJournal {
    batches: Vec<AppliedBatch>,
    next_id: i64,
}

impl VecJournal {
    pub fn new() -> Self {
        Self {
            batches: Vec::new(),
            next_id: 1,
        }
    }
}

impl UndoJournal for VecJournal {
    fn record(&mut self, batch: &AppliedBatch) -> Result<BatchId, JournalError> {
        let id = BatchId(self.next_id);
        self.next_id += 1;
        let mut stored = batch.clone();
        stored.id = id;
        self.batches.push(stored);
        Ok(id)
    }

    fn batches(&self) -> Result<Vec<AppliedBatch>, JournalError> {
        // Newest first, per the trait contract.
        Ok(self.batches.iter().rev().cloned().collect())
    }

    fn rollback(&mut self, id: BatchId) -> Result<(), JournalError> {
        let before = self.batches.len();
        self.batches.retain(|batch| batch.id != id);
        if self.batches.len() == before {
            return Err(JournalError::UnknownBatch(id));
        }
        Ok(())
    }
}

/// SQLite-backed journal, stored next to the application config.
///
/// This is the durable one: a batch survives an application restart, so the
/// "renamed 8,000 files, closed the app, realized in the morning the mask was
/// wrong" scenario stays recoverable. The plan is normalized across three
/// tables (`batches` -> `file_changes` -> `field_changes`) so old/new values
/// per field and old/new paths per file are first-class rows, not an opaque
/// blob.
///
/// Paths are stored as UTF-8 text via `to_string_lossy`; non-UTF-8 paths
/// (rare on the target platforms) would not round-trip byte-for-byte. Left as
/// a known limitation for now.
pub struct SqliteJournal {
    conn: rusqlite::Connection,
}

impl SqliteJournal {
    /// Open (creating if needed) the journal database at `path` and run
    /// migrations.
    pub fn open(path: &Path) -> Result<Self, JournalError> {
        let conn = rusqlite::Connection::open(path)?;
        // ON DELETE CASCADE below depends on this being enabled per connection.
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS batches (
                 id          INTEGER PRIMARY KEY AUTOINCREMENT,
                 description TEXT NOT NULL,
                 applied_at  INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS file_changes (
                 id        INTEGER PRIMARY KEY AUTOINCREMENT,
                 batch_id  INTEGER NOT NULL REFERENCES batches(id) ON DELETE CASCADE,
                 path      TEXT NOT NULL,
                 rename_to TEXT
             );
             CREATE TABLE IF NOT EXISTS field_changes (
                 id             INTEGER PRIMARY KEY AUTOINCREMENT,
                 file_change_id INTEGER NOT NULL REFERENCES file_changes(id) ON DELETE CASCADE,
                 field          TEXT NOT NULL,
                 old_value      TEXT,
                 new_value      TEXT
             );
             CREATE TABLE IF NOT EXISTS cover_changes (
                 file_change_id INTEGER NOT NULL REFERENCES file_changes(id) ON DELETE CASCADE,
                 old_mime       TEXT,
                 old_data       BLOB,
                 new_mime       TEXT,
                 new_data       BLOB
             );",
        )?;
        Ok(Self { conn })
    }
}

impl UndoJournal for SqliteJournal {
    fn record(&mut self, batch: &AppliedBatch) -> Result<BatchId, JournalError> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO batches (description, applied_at) VALUES (?1, ?2)",
            rusqlite::params![batch.description, batch.applied_at],
        )?;
        let batch_id = tx.last_insert_rowid();

        for change in &batch.plan.changes {
            tx.execute(
                "INSERT INTO file_changes (batch_id, path, rename_to) VALUES (?1, ?2, ?3)",
                rusqlite::params![
                    batch_id,
                    change.path.to_string_lossy(),
                    change
                        .rename_to
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned()),
                ],
            )?;
            let file_change_id = tx.last_insert_rowid();

            for field_change in &change.tag_changes {
                tx.execute(
                    "INSERT INTO field_changes (file_change_id, field, old_value, new_value) \
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![
                        file_change_id,
                        field_change.field.to_storage_key(),
                        field_change.old,
                        field_change.new,
                    ],
                )?;
            }

            if let Some(cover) = &change.cover_change {
                tx.execute(
                    "INSERT INTO cover_changes \
                     (file_change_id, old_mime, old_data, new_mime, new_data) \
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![
                        file_change_id,
                        cover.old.as_ref().map(|c| c.mime.clone()),
                        cover.old.as_ref().map(|c| c.data.clone()),
                        cover.new.as_ref().map(|c| c.mime.clone()),
                        cover.new.as_ref().map(|c| c.data.clone()),
                    ],
                )?;
            }
        }

        tx.commit()?;
        Ok(BatchId(batch_id))
    }

    fn batches(&self) -> Result<Vec<AppliedBatch>, JournalError> {
        let mut batch_stmt = self
            .conn
            .prepare("SELECT id, description, applied_at FROM batches ORDER BY id DESC")?;
        let rows = batch_stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;

        let mut batches = Vec::new();
        for row in rows {
            let (id, description, applied_at) = row?;
            let changes = self.load_file_changes(id)?;
            batches.push(AppliedBatch {
                id: BatchId(id),
                description: description.clone(),
                applied_at,
                plan: ChangePlan {
                    description,
                    changes,
                },
            });
        }
        Ok(batches)
    }

    fn rollback(&mut self, id: BatchId) -> Result<(), JournalError> {
        // Children cascade away via the foreign keys enabled in `open`.
        let affected = self
            .conn
            .execute("DELETE FROM batches WHERE id = ?1", rusqlite::params![id.0])?;
        if affected == 0 {
            return Err(JournalError::UnknownBatch(id));
        }
        Ok(())
    }
}

impl SqliteJournal {
    fn load_file_changes(&self, batch_id: i64) -> Result<Vec<FileChange>, JournalError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, rename_to FROM file_changes WHERE batch_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(rusqlite::params![batch_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?;

        let mut changes = Vec::new();
        for row in rows {
            let (file_change_id, path, rename_to) = row?;
            changes.push(FileChange {
                path: PathBuf::from(path),
                tag_changes: self.load_field_changes(file_change_id)?,
                cover_change: self.load_cover_change(file_change_id)?,
                rename_to: rename_to.map(PathBuf::from),
            });
        }
        Ok(changes)
    }

    fn load_cover_change(&self, file_change_id: i64) -> Result<Option<CoverChange>, JournalError> {
        let mut stmt = self.conn.prepare(
            "SELECT old_mime, old_data, new_mime, new_data FROM cover_changes \
             WHERE file_change_id = ?1",
        )?;
        let mut rows = stmt.query_map(rusqlite::params![file_change_id], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<Vec<u8>>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<Vec<u8>>>(3)?,
            ))
        })?;
        let Some(row) = rows.next() else {
            return Ok(None);
        };
        let (old_mime, old_data, new_mime, new_data) = row?;
        let cover = |mime: Option<String>, data: Option<Vec<u8>>| match (mime, data) {
            (Some(mime), Some(data)) => Some(CoverArt { mime, data }),
            _ => None,
        };
        Ok(Some(CoverChange {
            old: cover(old_mime, old_data),
            new: cover(new_mime, new_data),
        }))
    }

    fn load_field_changes(&self, file_change_id: i64) -> Result<Vec<FieldChange>, JournalError> {
        let mut stmt = self.conn.prepare(
            "SELECT field, old_value, new_value FROM field_changes \
             WHERE file_change_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(rusqlite::params![file_change_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?;

        let mut field_changes = Vec::new();
        for row in rows {
            let (field, old, new) = row?;
            field_changes.push(FieldChange {
                field: TagField::from_storage_key(&field),
                old,
                new,
            });
        }
        Ok(field_changes)
    }
}

#[derive(Debug, Error)]
pub enum JournalError {
    #[error("journal storage error: {0}")]
    Storage(String),
    #[error("unknown batch id: {0:?}")]
    UnknownBatch(BatchId),
    #[error("batch cannot be rolled back: {0}")]
    NotRollbackable(String),
}

impl From<rusqlite::Error> for JournalError {
    fn from(err: rusqlite::Error) -> Self {
        JournalError::Storage(err.to_string())
    }
}
