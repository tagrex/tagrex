//! Persistent undo journal.
//!
//! The one place where extra complexity is bought up front (architecture.md):
//! the journal must survive application restarts, record both tag writes and
//! renames, and allow rollback of a whole batch as a unit. The motivating
//! scenario: "renamed 8,000 files, closed the app, realized in the morning
//! the mask was wrong."

use std::path::Path;

use thiserror::Error;

use crate::plan::ChangePlan;

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
    fn record(&mut self, batch: &AppliedBatch) -> Result<(), JournalError>;

    /// All recorded batches, newest first.
    fn batches(&self) -> Result<Vec<AppliedBatch>, JournalError>;

    /// Roll back a whole batch as a unit.
    fn rollback(&mut self, id: BatchId) -> Result<(), JournalError>;
}

/// SQLite-backed journal, stored next to the application config.
pub struct SqliteJournal;

impl SqliteJournal {
    pub fn open(_path: &Path) -> Result<Self, JournalError> {
        todo!("open/create the SQLite file, run migrations")
    }
}

impl UndoJournal for SqliteJournal {
    fn record(&mut self, _batch: &AppliedBatch) -> Result<(), JournalError> {
        todo!()
    }

    fn batches(&self) -> Result<Vec<AppliedBatch>, JournalError> {
        todo!()
    }

    fn rollback(&mut self, _id: BatchId) -> Result<(), JournalError> {
        todo!()
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
