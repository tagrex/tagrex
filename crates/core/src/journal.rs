//! Persistent undo journal.
//!
//! The one place where extra complexity is bought up front (architecture.md):
//! the journal must survive application restarts, record both tag writes and
//! renames, and allow rollback of a whole batch as a unit. The motivating
//! scenario: "renamed 8,000 files, closed the app, realized in the morning
//! the mask was wrong."

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::model::{
    CoverArt, CoverKind, Id3v2Revision, TagBlockContent, TagBlockKind, TagField, TagMap,
};
use crate::plan::{BlockChange, ChangePlan, CoverChange, FieldChange, FileChange};

/// Identifier of an applied batch, stable across restarts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchId(pub i64);

/// History metadata with source paths only. Listing this never hydrates tag,
/// cover or block snapshots; those are fetched only for an actual Undo.
#[derive(Debug, Clone)]
pub struct BatchHeader {
    pub id: BatchId,
    pub description: String,
    pub message: Option<String>,
    pub applied_at: i64,
    pub interrupted: bool,
    pub paths: Vec<PathBuf>,
}

/// A batch that has been applied to disk, as recorded in the journal.
#[derive(Debug, Clone)]
pub struct AppliedBatch {
    pub id: BatchId,
    /// Description inherited from the plan source (for the history UI).
    pub description: String,
    /// The same description as an opaque code-and-values descriptor (#268), so
    /// History can be rendered in the language of the day rather than in the
    /// one that happened to be active when the batch was applied. See
    /// [`ChangePlan::message`](crate::plan::ChangePlan::message).
    pub message: Option<String>,
    /// Unix timestamp of when the batch was applied.
    pub applied_at: i64,
    /// The executed plan; `old` values are what rollback restores.
    pub plan: ChangePlan,
    /// Directories this batch had to create to make room for its renames,
    /// shallowest first. Rollback removes exactly these (deepest first) so a
    /// reorganize can be undone without leaving empty folders behind — and
    /// without touching directories that already existed.
    #[allow(clippy::struct_field_names)]
    pub created_dirs: Vec<PathBuf>,
    /// Directories this batch emptied and then removed (#153), deepest first.
    /// Rollback recreates exactly these, before anything moves back, so a file
    /// has a folder to return to.
    #[allow(clippy::struct_field_names)]
    pub removed_dirs: Vec<PathBuf>,
    /// The roots this batch was authorized to write inside, canonicalized. Only
    /// interesting when a reorganize filed tracks outside the open library
    /// (#153): the destination was confirmed by the user at apply time, and
    /// recording it here is what lets undo still reach those files in a later
    /// session, when the caller knows nothing about that folder.
    pub allowed_roots: Vec<PathBuf>,
    /// What each copy this batch made looked like once the batch was done
    /// (#604). Undo removes a copy only while the file at its path still
    /// matches. Empty for a batch recorded before this existed; its copies are
    /// removed without the check, as they always were.
    pub copy_stamps: Vec<CopyStamp>,
    /// What each changed field read as once the batch was written (#658). Undo
    /// refuses a file whose fields have changed since, rather than writing the
    /// old values over somebody's later edit. Empty for a batch recorded before
    /// this existed; it is undone without the check, as it always was.
    pub applied_values: Vec<AppliedValue>,
    /// Recorded before its first write and never finished (#659): the process
    /// stopped part-way through the apply. Undoing it puts back what it reached.
    pub interrupted: bool,
    /// The ID3v2 revision each file it writes in place was in before the
    /// write (#666). An ordinary write saves in the revision the app is set
    /// to, so a 2.3 file comes out as 2.4; undo puts the recorded revision
    /// back. Empty for a batch recorded before this existed.
    pub id3v2_revisions: Vec<FileRevision>,
}

/// The ID3v2 revision one file was in before a batch wrote it (#666).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRevision {
    pub path: PathBuf,
    pub revision: Id3v2Revision,
}

/// One field of one file as a batch left it (#658).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedValue {
    /// Where the file was once the batch was written: its rename target if the
    /// batch moved it.
    pub path: PathBuf,
    pub field: TagField,
    /// `None` when the field read as absent.
    pub value: Option<String>,
}

/// A file's size and modification time (#604): enough to tell the copy a batch
/// made from a file put at the same path since, or the copy edited since.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyStamp {
    pub path: PathBuf,
    pub len: u64,
    /// Nanoseconds since the Unix epoch.
    pub modified_ns: i64,
}

impl CopyStamp {
    /// The stamp of the file at `path` as it is now, or `None` when there is no
    /// such file or its time cannot be read.
    pub fn of(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        let since_epoch = metadata
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?;
        Some(Self {
            path: path.to_path_buf(),
            len: metadata.len(),
            modified_ns: i64::try_from(since_epoch.as_nanos()).ok()?,
        })
    }
}

/// Whether `table` already has `column`. Used to make the `copied` migration
/// (#153) idempotent — `ALTER TABLE ADD COLUMN` has no `IF NOT EXISTS`, and
/// `open` runs on every start.
/// Whether the file at `path` is a damaged SQLite database: not a database at
/// all, or one whose pages fail SQLite's own check. A file that cannot be
/// opened or is locked is not called damaged, so it is never moved aside.
fn is_damaged_database(path: &Path) -> bool {
    use rusqlite::{ErrorCode, OpenFlags};
    let Ok(conn) = rusqlite::Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return false;
    };
    match conn.query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0)) {
        Ok(verdict) => verdict != "ok",
        Err(rusqlite::Error::SqliteFailure(failure, _)) => matches!(
            failure.code,
            ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt
        ),
        Err(_) => false,
    }
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn column_exists(
    conn: &rusqlite::Connection,
    table: &str,
    column: &str,
) -> Result<bool, JournalError> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        if row.get::<_, String>(1)? == column {
            return Ok(true);
        }
    }
    Ok(false)
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

    /// One batch by id, or `None` when there is no such batch. A journal that
    /// can fetch a single batch should say so: undoing one batch has no business
    /// loading every other (#449).
    fn batch(&self, id: BatchId) -> Result<Option<AppliedBatch>, JournalError> {
        Ok(self.batches()?.into_iter().find(|batch| batch.id == id))
    }

    /// Finish a batch recorded before its write began (#659): add what only the
    /// end of the write knows (the folders it created and removed, the copy
    /// stamps, the values it left) and clear its `interrupted` mark.
    fn finish(&mut self, id: BatchId, batch: &AppliedBatch) -> Result<(), JournalError>;

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

    fn finish(&mut self, id: BatchId, batch: &AppliedBatch) -> Result<(), JournalError> {
        let stored = self
            .batches
            .iter_mut()
            .find(|stored| stored.id == id)
            .ok_or(JournalError::UnknownBatch(id))?;
        *stored = AppliedBatch {
            id,
            interrupted: false,
            ..batch.clone()
        };
        Ok(())
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
    /// [`open`](Self::open), and when the file there is a damaged database, set
    /// it aside and start a fresh journal in its place.
    ///
    /// The journal is shared by every library, so a damaged one — a crash or a
    /// full disk part-way through a write, a sync tool's half copy — made every
    /// folder refuse to open, with nothing to do about it short of finding and
    /// deleting the file by hand. Only damage does this: a journal that is
    /// locked by another instance, or that cannot be read for want of
    /// permission, is an error as before, since moving a healthy history aside
    /// would lose it. The damaged file is renamed, not deleted, so it can still
    /// be recovered; its new path comes back so the caller can say so.
    pub fn open_or_recover(path: &Path) -> Result<(Self, Option<PathBuf>), JournalError> {
        let error = match Self::open(path) {
            Ok(journal) => return Ok((journal, None)),
            Err(error) => error,
        };
        if !is_damaged_database(path) {
            return Err(error);
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0);
        let aside = with_suffix(path, &format!(".corrupt-{stamp}"));
        std::fs::rename(path, &aside).map_err(|err| JournalError::Storage(err.to_string()))?;
        // SQLite's own companions of the damaged file go with it: left behind,
        // a hot rollback journal would be replayed into the fresh database.
        for companion in ["-journal", "-wal", "-shm"] {
            let from = with_suffix(path, companion);
            if from.exists() {
                std::fs::rename(&from, with_suffix(&aside, companion)).ok();
            }
        }
        Ok((Self::open(path)?, Some(aside)))
    }

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
             CREATE TABLE IF NOT EXISTS created_dirs (
                 id       INTEGER PRIMARY KEY AUTOINCREMENT,
                 batch_id INTEGER NOT NULL REFERENCES batches(id) ON DELETE CASCADE,
                 path     TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS cover_changes (
                 file_change_id INTEGER NOT NULL REFERENCES file_changes(id) ON DELETE CASCADE,
                 old_mime       TEXT,
                 old_data       BLOB,
                 new_mime       TEXT,
                 new_data       BLOB
             );
             CREATE TABLE IF NOT EXISTS sidecar_renames (
                 id             INTEGER PRIMARY KEY AUTOINCREMENT,
                 file_change_id INTEGER NOT NULL REFERENCES file_changes(id) ON DELETE CASCADE,
                 from_path      TEXT NOT NULL,
                 to_path        TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS cover_images (
                 id             INTEGER PRIMARY KEY AUTOINCREMENT,
                 file_change_id INTEGER NOT NULL REFERENCES file_changes(id) ON DELETE CASCADE,
                 side           TEXT NOT NULL,
                 kind           TEXT NOT NULL,
                 description    TEXT NOT NULL,
                 mime           TEXT NOT NULL,
                 data           BLOB NOT NULL
             );
             CREATE TABLE IF NOT EXISTS removed_dirs (
                 id       INTEGER PRIMARY KEY AUTOINCREMENT,
                 batch_id INTEGER NOT NULL REFERENCES batches(id) ON DELETE CASCADE,
                 path     TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS allowed_roots (
                 id       INTEGER PRIMARY KEY AUTOINCREMENT,
                 batch_id INTEGER NOT NULL REFERENCES batches(id) ON DELETE CASCADE,
                 path     TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS block_removals (
                 id             INTEGER PRIMARY KEY AUTOINCREMENT,
                 file_change_id INTEGER NOT NULL REFERENCES file_changes(id) ON DELETE CASCADE,
                 kind           TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS block_removal_fields (
                 id               INTEGER PRIMARY KEY AUTOINCREMENT,
                 block_removal_id INTEGER NOT NULL REFERENCES block_removals(id) ON DELETE CASCADE,
                 field            TEXT NOT NULL,
                 value            TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS block_removal_images (
                 id               INTEGER PRIMARY KEY AUTOINCREMENT,
                 block_removal_id INTEGER NOT NULL REFERENCES block_removals(id) ON DELETE CASCADE,
                 kind             TEXT NOT NULL,
                 description      TEXT NOT NULL,
                 mime             TEXT NOT NULL,
                 data             BLOB NOT NULL
             );
             CREATE TABLE IF NOT EXISTS block_changes (
                 id             INTEGER PRIMARY KEY AUTOINCREMENT,
                 file_change_id INTEGER NOT NULL REFERENCES file_changes(id) ON DELETE CASCADE,
                 kind           TEXT NOT NULL,
                 revision       TEXT,
                 old_revision   TEXT,
                 old_bytes      BLOB,
                 has_old        INTEGER NOT NULL,
                 has_new        INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS block_change_fields (
                 id              INTEGER PRIMARY KEY AUTOINCREMENT,
                 block_change_id INTEGER NOT NULL REFERENCES block_changes(id) ON DELETE CASCADE,
                 side            TEXT NOT NULL,
                 field           TEXT NOT NULL,
                 value           TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS block_change_images (
                 id              INTEGER PRIMARY KEY AUTOINCREMENT,
                 block_change_id INTEGER NOT NULL REFERENCES block_changes(id) ON DELETE CASCADE,
                 side            TEXT NOT NULL,
                 kind            TEXT NOT NULL,
                 description     TEXT NOT NULL,
                 mime            TEXT NOT NULL,
                 data            BLOB NOT NULL
             );
             CREATE TABLE IF NOT EXISTS copy_stamps (
                 id          INTEGER PRIMARY KEY AUTOINCREMENT,
                 batch_id    INTEGER NOT NULL REFERENCES batches(id) ON DELETE CASCADE,
                 path        TEXT NOT NULL,
                 len         INTEGER NOT NULL,
                 modified_ns INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS applied_values (
                 id       INTEGER PRIMARY KEY AUTOINCREMENT,
                 batch_id INTEGER NOT NULL REFERENCES batches(id) ON DELETE CASCADE,
                 path     TEXT NOT NULL,
                 field    TEXT NOT NULL,
                 value    TEXT
             );
             CREATE TABLE IF NOT EXISTS id3v2_revisions (
                 id       INTEGER PRIMARY KEY AUTOINCREMENT,
                 batch_id INTEGER NOT NULL REFERENCES batches(id) ON DELETE CASCADE,
                 path     TEXT NOT NULL,
                 revision TEXT NOT NULL
             );",
        )?;
        // `copied` (#153) arrived after the table did, and a journal from an
        // older build is a real thing to open — a whole point of this file is
        // surviving restarts. `CREATE TABLE IF NOT EXISTS` cannot add it, so add
        // the column only when it is missing; every existing row is a move.
        if !column_exists(&conn, "file_changes", "copied")? {
            conn.execute_batch(
                "ALTER TABLE file_changes ADD COLUMN copied INTEGER NOT NULL DEFAULT 0",
            )?;
        }
        // The translatable descriptor (#268), added the same way. A row written
        // before it holds NULL and keeps showing the English it was written
        // with — there is nothing to recover a code from, and matching the
        // prose to guess one would be inventing what a batch was.
        if !column_exists(&conn, "batches", "message")? {
            conn.execute_batch("ALTER TABLE batches ADD COLUMN message TEXT")?;
        }
        // The write-ahead mark (#659), added the same way. Every existing row
        // was recorded after its write had finished.
        if !column_exists(&conn, "batches", "interrupted")? {
            conn.execute_batch(
                "ALTER TABLE batches ADD COLUMN interrupted INTEGER NOT NULL DEFAULT 0",
            )?;
        }
        // The rewrite of a block's text in Unicode (#698), added the same way.
        // Every existing row is some other change.
        if !column_exists(&conn, "block_changes", "text_to_unicode")? {
            conn.execute_batch(
                "ALTER TABLE block_changes ADD COLUMN text_to_unicode INTEGER NOT NULL DEFAULT 0",
            )?;
        }
        // Every child table is read and deleted by its parent's id, and SQLite
        // does not index a foreign key by itself. Without these, each of the
        // lookups below — one per file, several per file — scanned its whole
        // table, so loading or rolling back a batch cost the square of the
        // journal's size: minutes for a few batches of 20,000 files (#449). An
        // existing journal gets them on its next open.
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_file_changes_batch ON file_changes(batch_id);
             CREATE INDEX IF NOT EXISTS idx_field_changes_file ON field_changes(file_change_id);
             CREATE INDEX IF NOT EXISTS idx_cover_changes_file ON cover_changes(file_change_id);
             CREATE INDEX IF NOT EXISTS idx_cover_images_file ON cover_images(file_change_id);
             CREATE INDEX IF NOT EXISTS idx_sidecar_renames_file ON sidecar_renames(file_change_id);
             CREATE INDEX IF NOT EXISTS idx_block_changes_file ON block_changes(file_change_id);
             CREATE INDEX IF NOT EXISTS idx_block_change_fields_block ON block_change_fields(block_change_id);
             CREATE INDEX IF NOT EXISTS idx_block_change_images_block ON block_change_images(block_change_id);
             CREATE INDEX IF NOT EXISTS idx_block_removals_file ON block_removals(file_change_id);
             CREATE INDEX IF NOT EXISTS idx_block_removal_fields_block ON block_removal_fields(block_removal_id);
             CREATE INDEX IF NOT EXISTS idx_block_removal_images_block ON block_removal_images(block_removal_id);
             CREATE INDEX IF NOT EXISTS idx_created_dirs_batch ON created_dirs(batch_id);
             CREATE INDEX IF NOT EXISTS idx_removed_dirs_batch ON removed_dirs(batch_id);
             CREATE INDEX IF NOT EXISTS idx_allowed_roots_batch ON allowed_roots(batch_id);
             CREATE INDEX IF NOT EXISTS idx_copy_stamps_batch ON copy_stamps(batch_id);
             CREATE INDEX IF NOT EXISTS idx_applied_values_batch ON applied_values(batch_id);
             CREATE INDEX IF NOT EXISTS idx_id3v2_revisions_batch ON id3v2_revisions(batch_id);",
        )?;
        Ok(Self { conn })
    }
}

impl UndoJournal for SqliteJournal {
    fn record(&mut self, batch: &AppliedBatch) -> Result<BatchId, JournalError> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO batches (description, message, applied_at, interrupted) \
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                batch.description,
                batch.message,
                batch.applied_at,
                batch.interrupted
            ],
        )?;
        let batch_id = tx.last_insert_rowid();

        for change in &batch.plan.changes {
            tx.execute(
                "INSERT INTO file_changes (batch_id, path, rename_to, copied) \
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    batch_id,
                    change.path.to_string_lossy(),
                    change
                        .rename_to
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned()),
                    change.copy,
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

            // Both whole sides of the image set (#56), in order. The legacy
            // `cover_changes` table held one old/new pair and is no longer
            // written — only read, so a journal from an older build still undoes.
            if let Some(cover) = &change.cover_change {
                for (side, images) in [("old", &cover.old), ("new", &cover.new)] {
                    for image in images {
                        tx.execute(
                            "INSERT INTO cover_images \
                             (file_change_id, side, kind, description, mime, data) \
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                            rusqlite::params![
                                file_change_id,
                                side,
                                image.kind.to_storage_key(),
                                image.description,
                                image.mime,
                                image.data,
                            ],
                        )?;
                    }
                }
            }

            for (from, to) in &change.sidecar_renames {
                tx.execute(
                    "INSERT INTO sidecar_renames (file_change_id, from_path, to_path) \
                     VALUES (?1, ?2, ?3)",
                    rusqlite::params![
                        file_change_id,
                        from.to_string_lossy(),
                        to.to_string_lossy(),
                    ],
                )?;
            }

            // Both whole sides of every block change (#47, #205), field rows and
            // picture rows. A stripped block's snapshot is the only copy left
            // once it is off the file, so nothing here is stored by reference.
            // `has_old`/`has_new` are separate from the row counts because a
            // block that exists and holds nothing is not the same as no block.
            for block_change in &change.block_changes {
                tx.execute(
                    "INSERT INTO block_changes \
                     (file_change_id, kind, revision, old_revision, old_bytes, has_old, has_new, \
                     text_to_unicode) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    rusqlite::params![
                        file_change_id,
                        block_change.kind.to_storage_key(),
                        block_change.revision.map(Id3v2Revision::to_storage_key),
                        block_change.old_revision.map(Id3v2Revision::to_storage_key),
                        block_change.old_bytes,
                        block_change.old.is_some(),
                        block_change.new.is_some(),
                        block_change.text_to_unicode,
                    ],
                )?;
                let block_change_id = tx.last_insert_rowid();
                let sides = [("old", &block_change.old), ("new", &block_change.new)];
                for (side, content) in sides {
                    let Some(content) = content else { continue };
                    for (field, value) in &content.tags {
                        tx.execute(
                            "INSERT INTO block_change_fields \
                             (block_change_id, side, field, value) VALUES (?1, ?2, ?3, ?4)",
                            rusqlite::params![block_change_id, side, field.to_storage_key(), value],
                        )?;
                    }
                    for image in &content.covers {
                        tx.execute(
                            "INSERT INTO block_change_images \
                             (block_change_id, side, kind, description, mime, data) \
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                            rusqlite::params![
                                block_change_id,
                                side,
                                image.kind.to_storage_key(),
                                image.description,
                                image.mime,
                                image.data,
                            ],
                        )?;
                    }
                }
            }
        }

        // Known before the first write, unlike the outcome below, so recorded
        // here and never again on `finish`.
        for file in &batch.id3v2_revisions {
            tx.execute(
                "INSERT INTO id3v2_revisions (batch_id, path, revision) VALUES (?1, ?2, ?3)",
                rusqlite::params![
                    batch_id,
                    file.path.to_string_lossy(),
                    file.revision.to_storage_key(),
                ],
            )?;
        }

        insert_outcome(&tx, batch_id, batch)?;

        tx.commit()?;
        Ok(BatchId(batch_id))
    }

    fn batches(&self) -> Result<Vec<AppliedBatch>, JournalError> {
        let mut batch_stmt = self.conn.prepare(
            "SELECT id, description, message, applied_at, interrupted FROM batches \
                 ORDER BY id DESC",
        )?;
        let rows = batch_stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, bool>(4)?,
            ))
        })?;

        let mut batches = Vec::new();
        for row in rows {
            let (id, description, message, applied_at, interrupted) = row?;
            let mut batch = self.load_batch(id, description, message, applied_at)?;
            batch.interrupted = interrupted;
            batches.push(batch);
        }
        Ok(batches)
    }

    fn batch(&self, id: BatchId) -> Result<Option<AppliedBatch>, JournalError> {
        let row = self
            .conn
            .query_row(
                "SELECT description, message, applied_at, interrupted FROM batches WHERE id = ?1",
                rusqlite::params![id.0],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, bool>(3)?,
                    ))
                },
            )
            .map(Some)
            .or_else(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })?;
        row.map(|(description, message, applied_at, interrupted)| {
            let mut batch = self.load_batch(id.0, description, message, applied_at)?;
            batch.interrupted = interrupted;
            Ok(batch)
        })
        .transpose()
    }

    fn finish(&mut self, id: BatchId, batch: &AppliedBatch) -> Result<(), JournalError> {
        let tx = self.conn.transaction()?;
        let affected = tx.execute(
            "UPDATE batches SET interrupted = 0 WHERE id = ?1",
            rusqlite::params![id.0],
        )?;
        if affected == 0 {
            return Err(JournalError::UnknownBatch(id));
        }
        insert_outcome(&tx, id.0, batch)?;
        tx.commit()?;
        Ok(())
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

/// The rows only the end of a batch's write knows (#659): the folders it
/// created and removed, its copy stamps (#604) and the values it left (#658).
fn insert_outcome(
    tx: &rusqlite::Transaction<'_>,
    batch_id: i64,
    batch: &AppliedBatch,
) -> Result<(), JournalError> {
    for dir in &batch.created_dirs {
        tx.execute(
            "INSERT INTO created_dirs (batch_id, path) VALUES (?1, ?2)",
            rusqlite::params![batch_id, dir.to_string_lossy()],
        )?;
    }
    for dir in &batch.removed_dirs {
        tx.execute(
            "INSERT INTO removed_dirs (batch_id, path) VALUES (?1, ?2)",
            rusqlite::params![batch_id, dir.to_string_lossy()],
        )?;
    }
    for root in &batch.allowed_roots {
        tx.execute(
            "INSERT INTO allowed_roots (batch_id, path) VALUES (?1, ?2)",
            rusqlite::params![batch_id, root.to_string_lossy()],
        )?;
    }
    for stamp in &batch.copy_stamps {
        tx.execute(
            "INSERT INTO copy_stamps (batch_id, path, len, modified_ns) \
                 VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                batch_id,
                stamp.path.to_string_lossy(),
                i64::try_from(stamp.len).unwrap_or(i64::MAX),
                stamp.modified_ns,
            ],
        )?;
    }

    for applied in &batch.applied_values {
        tx.execute(
            "INSERT INTO applied_values (batch_id, path, field, value) \
                 VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                batch_id,
                applied.path.to_string_lossy(),
                applied.field.to_storage_key(),
                applied.value,
            ],
        )?;
    }
    Ok(())
}

impl SqliteJournal {
    /// Fetch history in one metadata-only query. Path membership stays exact
    /// (`Path::starts_with`, not SQL wildcard/prefix matching), including empty
    /// batches and symlink-root spellings supported by the command layer.
    pub fn headers(&self, roots: &[PathBuf]) -> Result<Vec<BatchHeader>, JournalError> {
        let mut statement = self.conn.prepare("SELECT b.id, b.description, b.message, b.applied_at, b.interrupted, f.path FROM batches b LEFT JOIN file_changes f ON f.batch_id = b.id ORDER BY b.id DESC, f.id")?;
        let mut rows = statement.query([])?;
        let mut headers: Vec<BatchHeader> = Vec::new();
        while let Some(row) = rows.next()? {
            let id = BatchId(row.get(0)?);
            if headers.last().is_none_or(|h| h.id != id) {
                headers.push(BatchHeader {
                    id,
                    description: row.get(1)?,
                    message: row.get(2)?,
                    applied_at: row.get(3)?,
                    interrupted: row.get(4)?,
                    paths: Vec::new(),
                });
            }
            if let Some(path) = row.get::<_, Option<String>>(5)? {
                headers
                    .last_mut()
                    .expect("header inserted above")
                    .paths
                    .push(PathBuf::from(path));
            }
        }
        // #153: membership is judged by where the files CAME FROM, not where
        // they went. A reorganize can move files outside the library; its batch
        // must still appear in history so those changes can be undone.
        headers.retain(|header| {
            header
                .paths
                .iter()
                .all(|path| roots.iter().any(|root| path.starts_with(root)))
        });
        Ok(headers)
    }

    /// One of the batch's plain path lists, in insertion order. `table` is a
    /// literal from this module, never anything a caller supplies.
    fn load_paths(&self, table: &str, batch_id: i64) -> Result<Vec<PathBuf>, JournalError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT path FROM {table} WHERE batch_id = ?1 ORDER BY id"
        ))?;
        let rows = stmt.query_map(rusqlite::params![batch_id], |row| {
            row.get::<_, String>(0).map(PathBuf::from)
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// A batch row with everything that hangs off it.
    fn load_batch(
        &self,
        id: i64,
        description: String,
        message: Option<String>,
        applied_at: i64,
    ) -> Result<AppliedBatch, JournalError> {
        let changes = self.load_file_changes(id)?;
        Ok(AppliedBatch {
            id: BatchId(id),
            description: description.clone(),
            message: message.clone(),
            applied_at,
            plan: ChangePlan {
                description,
                message,
                changes,
                // Apply-time only: what it caused is in `removed_dirs`.
                prune_empty_dirs: false,
            },
            created_dirs: self.load_paths("created_dirs", id)?,
            removed_dirs: self.load_paths("removed_dirs", id)?,
            allowed_roots: self.load_paths("allowed_roots", id)?,
            copy_stamps: self.load_copy_stamps(id)?,
            applied_values: self.load_applied_values(id)?,
            id3v2_revisions: self.load_id3v2_revisions(id)?,
            // Set by the caller, which read the batch row.
            interrupted: false,
        })
    }

    fn load_copy_stamps(&self, batch_id: i64) -> Result<Vec<CopyStamp>, JournalError> {
        let mut stmt = self.conn.prepare(
            "SELECT path, len, modified_ns FROM copy_stamps WHERE batch_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(rusqlite::params![batch_id], |row| {
            Ok(CopyStamp {
                path: PathBuf::from(row.get::<_, String>(0)?),
                len: u64::try_from(row.get::<_, i64>(1)?).unwrap_or(0),
                modified_ns: row.get::<_, i64>(2)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn load_applied_values(&self, batch_id: i64) -> Result<Vec<AppliedValue>, JournalError> {
        let mut stmt = self.conn.prepare(
            "SELECT path, field, value FROM applied_values WHERE batch_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(rusqlite::params![batch_id], |row| {
            Ok(AppliedValue {
                path: PathBuf::from(row.get::<_, String>(0)?),
                field: TagField::from_storage_key(&row.get::<_, String>(1)?),
                value: row.get::<_, Option<String>>(2)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// A revision this build does not know is left out: undo cannot write it.
    fn load_id3v2_revisions(&self, batch_id: i64) -> Result<Vec<FileRevision>, JournalError> {
        let mut stmt = self.conn.prepare(
            "SELECT path, revision FROM id3v2_revisions WHERE batch_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(rusqlite::params![batch_id], |row| {
            Ok((
                PathBuf::from(row.get::<_, String>(0)?),
                row.get::<_, String>(1)?,
            ))
        })?;
        let mut revisions = Vec::new();
        for row in rows {
            let (path, key) = row?;
            if let Some(revision) = Id3v2Revision::from_storage_key(&key) {
                revisions.push(FileRevision { path, revision });
            }
        }
        Ok(revisions)
    }

    fn load_file_changes(&self, batch_id: i64) -> Result<Vec<FileChange>, JournalError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, rename_to, copied FROM file_changes WHERE batch_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(rusqlite::params![batch_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, bool>(3)?,
            ))
        })?;

        let mut changes = Vec::new();
        for row in rows {
            let (file_change_id, path, rename_to, copied) = row?;
            changes.push(FileChange {
                path: PathBuf::from(path),
                tag_changes: self.load_field_changes(file_change_id)?,
                cover_change: self.load_cover_change(file_change_id)?,
                rename_to: rename_to.map(PathBuf::from),
                copy: copied,
                sidecar_renames: self.load_sidecar_renames(file_change_id)?,
                block_changes: self.load_block_changes(file_change_id)?,
            });
        }
        Ok(changes)
    }

    fn load_sidecar_renames(
        &self,
        file_change_id: i64,
    ) -> Result<Vec<(PathBuf, PathBuf)>, JournalError> {
        let mut stmt = self.conn.prepare(
            "SELECT from_path, to_path FROM sidecar_renames \
             WHERE file_change_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(rusqlite::params![file_change_id], |row| {
            Ok((
                PathBuf::from(row.get::<_, String>(0)?),
                PathBuf::from(row.get::<_, String>(1)?),
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// The whole-block changes one file change carries (#47, #205), both sides
    /// of each.
    ///
    /// A row whose `kind` this build has no name for is skipped rather than
    /// guessed at: it comes from a newer build, and undoing it would mean
    /// writing a block this one cannot address. The rest of the batch still
    /// rolls back. A file change with no rows here falls back to the legacy
    /// removal-only tables, so a batch recorded before conversion landed can
    /// still be undone.
    fn load_block_changes(&self, file_change_id: i64) -> Result<Vec<BlockChange>, JournalError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, revision, old_revision, old_bytes, has_old, has_new, \
             text_to_unicode FROM block_changes WHERE file_change_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(rusqlite::params![file_change_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<Vec<u8>>>(4)?,
                row.get::<_, bool>(5)?,
                row.get::<_, bool>(6)?,
                row.get::<_, bool>(7)?,
            ))
        })?;

        let mut changes = Vec::new();
        let mut any = false;
        for row in rows {
            let (id, kind, revision, old_revision, old_bytes, has_old, has_new, text_to_unicode) =
                row?;
            any = true;
            let Some(kind) = TagBlockKind::from_storage_key(&kind) else {
                continue;
            };
            let side = |side: &str, present: bool| -> Result<_, JournalError> {
                if !present {
                    return Ok(None);
                }
                Ok(Some(TagBlockContent {
                    tags: self.load_block_change_fields(id, side)?,
                    covers: self.load_block_change_images(id, side)?,
                }))
            };
            changes.push(BlockChange {
                kind,
                revision: revision
                    .as_deref()
                    .and_then(Id3v2Revision::from_storage_key),
                old_revision: old_revision
                    .as_deref()
                    .and_then(Id3v2Revision::from_storage_key),
                old_bytes,
                old: side("old", has_old)?,
                new: side("new", has_new)?,
                text_to_unicode,
            });
        }
        if any {
            return Ok(changes);
        }
        self.load_legacy_block_removals(file_change_id)
    }

    fn load_block_change_fields(&self, id: i64, side: &str) -> Result<TagMap, JournalError> {
        let mut stmt = self.conn.prepare(
            "SELECT field, value FROM block_change_fields \
             WHERE block_change_id = ?1 AND side = ?2 ORDER BY id",
        )?;
        let rows = stmt.query_map(rusqlite::params![id, side], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut tags = TagMap::new();
        for row in rows {
            let (field, value) = row?;
            tags.insert(TagField::from_storage_key(&field), value);
        }
        Ok(tags)
    }

    fn load_block_change_images(&self, id: i64, side: &str) -> Result<Vec<CoverArt>, JournalError> {
        let mut stmt = self.conn.prepare(
            "SELECT kind, description, mime, data FROM block_change_images \
             WHERE block_change_id = ?1 AND side = ?2 ORDER BY id",
        )?;
        let rows = stmt.query_map(rusqlite::params![id, side], |row| {
            Ok(CoverArt {
                kind: CoverKind::from_storage_key(&row.get::<_, String>(0)?),
                description: row.get::<_, String>(1)?,
                mime: row.get::<_, String>(2)?,
                data: row.get::<_, Vec<u8>>(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// A batch recorded when a block change could only ever be a removal. Read
    /// only, never written: the tables stay so a pending undo from that build
    /// still works.
    fn load_legacy_block_removals(
        &self,
        file_change_id: i64,
    ) -> Result<Vec<BlockChange>, JournalError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, kind FROM block_removals WHERE file_change_id = ?1 ORDER BY id")?;
        let rows = stmt.query_map(rusqlite::params![file_change_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut removals = Vec::new();
        for row in rows {
            let (removal_id, kind) = row?;
            let Some(kind) = TagBlockKind::from_storage_key(&kind) else {
                continue;
            };
            let mut fields = self.conn.prepare(
                "SELECT field, value FROM block_removal_fields \
                 WHERE block_removal_id = ?1 ORDER BY id",
            )?;
            let mut tags = TagMap::new();
            for row in fields.query_map(rusqlite::params![removal_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })? {
                let (field, value) = row?;
                tags.insert(TagField::from_storage_key(&field), value);
            }
            let mut images = self.conn.prepare(
                "SELECT kind, description, mime, data FROM block_removal_images \
                 WHERE block_removal_id = ?1 ORDER BY id",
            )?;
            let covers = images
                .query_map(rusqlite::params![removal_id], |row| {
                    Ok(CoverArt {
                        kind: CoverKind::from_storage_key(&row.get::<_, String>(0)?),
                        description: row.get::<_, String>(1)?,
                        mime: row.get::<_, String>(2)?,
                        data: row.get::<_, Vec<u8>>(3)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            removals.push(BlockChange::removal(kind, TagBlockContent { tags, covers }));
        }
        Ok(removals)
    }

    /// The image-set change for one file change (#56), or `None` when it carries
    /// none. Reads the current `cover_images` rows; a file change with none of
    /// those falls back to the legacy single-pair `cover_changes` row, so a
    /// batch recorded before the set model can still be rolled back.
    fn load_cover_change(&self, file_change_id: i64) -> Result<Option<CoverChange>, JournalError> {
        let mut stmt = self.conn.prepare(
            "SELECT side, kind, description, mime, data FROM cover_images \
             WHERE file_change_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(rusqlite::params![file_change_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                CoverArt {
                    kind: CoverKind::from_storage_key(&row.get::<_, String>(1)?),
                    description: row.get::<_, String>(2)?,
                    mime: row.get::<_, String>(3)?,
                    data: row.get::<_, Vec<u8>>(4)?,
                },
            ))
        })?;
        let mut change = CoverChange::default();
        let mut any = false;
        for row in rows {
            let (side, image) = row?;
            any = true;
            if side == "old" {
                change.old.push(image);
            } else {
                change.new.push(image);
            }
        }
        if any {
            return Ok(Some(change));
        }
        self.load_legacy_cover_change(file_change_id)
    }

    fn load_legacy_cover_change(
        &self,
        file_change_id: i64,
    ) -> Result<Option<CoverChange>, JournalError> {
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
        // A pre-#56 pair: one front cover, or none on that side.
        let cover = |mime: Option<String>, data: Option<Vec<u8>>| match (mime, data) {
            (Some(mime), Some(data)) => vec![CoverArt {
                mime,
                data,
                kind: CoverKind::Front,
                description: String::new(),
            }],
            _ => Vec::new(),
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
