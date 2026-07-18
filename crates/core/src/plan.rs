//! The transactional change pipeline — the heart of TagRex.
//!
//! ```text
//! source of changes --> plan --> preview --> apply --> undo journal
//! ```
//!
//! Everything else (masks, transforms, online providers) is just a producer
//! of plans. **No module writes tags or renames files directly; all writes go
//! through [`Executor`].** This is an invariant, not a preference.

use std::path::PathBuf;

use thiserror::Error;

use crate::journal::{AppliedBatch, JournalError, UndoJournal};
use crate::model::{TagField, TrackFile};

/// A change to a single tag field: `old` is what preview shows as "current",
/// `new` is what will be written. `None` means the field is absent/removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldChange {
    pub field: TagField,
    pub old: Option<String>,
    pub new: Option<String>,
}

/// All changes planned for one file.
#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: PathBuf,
    pub tag_changes: Vec<FieldChange>,
    /// Planned rename, if any.
    pub rename_to: Option<PathBuf>,
}

/// A complete, previewable plan of changes over a set of files.
#[derive(Debug, Clone, Default)]
pub struct ChangePlan {
    pub changes: Vec<FileChange>,
}

impl ChangePlan {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub fn file_count(&self) -> usize {
        self.changes.len()
    }
}

/// Anything that can produce a plan: a mask rename, a filename-to-tags
/// import, a provider release import, a transform chain over selected fields.
pub trait PlanSource {
    /// Human-readable description for the preview header and the journal.
    fn describe(&self) -> String;

    /// Build a plan against the current state of `files`. Must be pure:
    /// no I/O side effects, no writes.
    fn build_plan(&self, files: &[TrackFile]) -> Result<ChangePlan, PlanError>;
}

/// The only component allowed to write. Applies a plan and records the
/// applied batch in the journal so it can be rolled back — including after
/// an application restart.
pub struct Executor;

impl Executor {
    pub fn apply(
        _plan: &ChangePlan,
        _journal: &mut dyn UndoJournal,
    ) -> Result<AppliedBatch, PlanError> {
        todo!(
            "validate plan against current disk state, write tags via TagEngine, \
             perform renames, record the batch in the journal"
        )
    }
}

#[derive(Debug, Error)]
pub enum PlanError {
    #[error("file changed on disk since the plan was built: {0}")]
    Stale(PathBuf),
    #[error("rename target already exists: {0}")]
    RenameCollision(PathBuf),
    #[error("journal error: {0}")]
    Journal(#[from] JournalError),
    #[error("tag I/O error: {0}")]
    TagIo(#[from] crate::model::TagIoError),
}
