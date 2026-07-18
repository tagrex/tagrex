//! The transactional change pipeline — the heart of TagRex.
//!
//! ```text
//! source of changes --> plan --> preview --> apply --> undo journal
//! ```
//!
//! Everything else (masks, transforms, online providers) is just a producer
//! of plans. **No module writes tags or renames files directly; all writes go
//! through [`Executor`].** This is an invariant, not a preference.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;

use crate::journal::{AppliedBatch, BatchId, JournalError, UndoJournal};
use crate::model::{TagEngine, TagField, TrackFile};

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
    /// Human-readable summary, inherited from the [`PlanSource`] that built
    /// the plan, and carried into the journal so history shows what a batch
    /// was (see [`AppliedBatch::description`]).
    pub description: String,
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
///
/// Rename execution is deliberately not here yet: tag writing and renaming
/// are separate user operations (like TagScanner's separate tabs), so this
/// increment handles tag writes only and rejects any plan carrying a
/// `rename_to`. Rename lands as its own increment.
pub struct Executor;

impl Executor {
    /// Apply a plan's tag changes to disk, then record the batch so it can be
    /// rolled back.
    ///
    /// Every write is confined to `allowed_root`: a plan touching a path that
    /// resolves outside it is rejected wholesale before anything is written.
    /// The entire plan is validated up front (root containment + staleness)
    /// so a bad file cannot leave the batch half-applied.
    pub fn apply(
        plan: &ChangePlan,
        journal: &mut dyn UndoJournal,
        allowed_root: &Path,
    ) -> Result<AppliedBatch, PlanError> {
        let root = canonical_root(allowed_root)?;

        // Pre-flight: validate the WHOLE plan before touching disk.
        for change in &plan.changes {
            if change.rename_to.is_some() {
                return Err(PlanError::RenameNotSupported(change.path.clone()));
            }
            ensure_within_root(&change.path, &root)?;
            ensure_not_stale(change)?;
        }

        // Apply: write the `new` values.
        for change in &plan.changes {
            write_tag_changes(change, Direction::Apply)?;
        }

        let batch = AppliedBatch {
            id: next_batch_id(),
            description: plan.description.clone(),
            applied_at: now_unix_secs(),
            plan: plan.clone(),
        };
        journal.record(&batch)?;
        Ok(batch)
    }

    /// Roll a previously applied batch back, restoring every field's `old`
    /// value, then remove it from the journal.
    ///
    /// Restoration goes through the same `TagEngine`-only write path as
    /// [`apply`](Self::apply) and is confined to `allowed_root` the same way.
    pub fn undo(
        journal: &mut dyn UndoJournal,
        batch_id: BatchId,
        allowed_root: &Path,
    ) -> Result<(), PlanError> {
        let root = canonical_root(allowed_root)?;

        let batch = journal
            .batches()?
            .into_iter()
            .find(|batch| batch.id == batch_id)
            .ok_or(PlanError::Journal(JournalError::UnknownBatch(batch_id)))?;

        for change in &batch.plan.changes {
            ensure_within_root(&change.path, &root)?;
        }
        for change in &batch.plan.changes {
            write_tag_changes(change, Direction::Undo)?;
        }

        journal.rollback(batch_id)?;
        Ok(())
    }
}

/// Which side of a [`FieldChange`] to write: `new` when applying, `old` when
/// undoing. Both directions share one write path so they can't diverge.
#[derive(Clone, Copy)]
enum Direction {
    Apply,
    Undo,
}

fn canonical_root(allowed_root: &Path) -> Result<PathBuf, PlanError> {
    std::fs::canonicalize(allowed_root).map_err(PlanError::Io)
}

/// Resolve `path` (following symlinks, collapsing `..`) and require the result
/// to sit inside `root`. This is what stops a crafted mask literal like
/// `../../etc` from steering a write outside the scanned library.
fn ensure_within_root(path: &Path, root: &Path) -> Result<(), PlanError> {
    let canonical = std::fs::canonicalize(path).map_err(PlanError::Io)?;
    if canonical.starts_with(root) {
        Ok(())
    } else {
        Err(PlanError::OutsideRoot(canonical))
    }
}

/// Guard against TOCTOU: if the file's current on-disk value for any changed
/// field no longer matches what the plan recorded as `old`, the plan was
/// built against a stale snapshot and must not be applied.
fn ensure_not_stale(change: &FileChange) -> Result<(), PlanError> {
    let current = TagEngine::read(&change.path)?;
    for field_change in &change.tag_changes {
        let on_disk = current.tags.get(&field_change.field);
        if on_disk != field_change.old.as_ref() {
            return Err(PlanError::Stale(change.path.clone()));
        }
    }
    Ok(())
}

fn write_tag_changes(change: &FileChange, direction: Direction) -> Result<(), PlanError> {
    let mut track = TagEngine::read(&change.path)?;
    for field_change in &change.tag_changes {
        let value = match direction {
            Direction::Apply => &field_change.new,
            Direction::Undo => &field_change.old,
        };
        match value {
            Some(value) => {
                track.tags.insert(field_change.field.clone(), value.clone());
            }
            None => {
                track.tags.remove(&field_change.field);
            }
        }
    }
    TagEngine::write(&track)?;
    Ok(())
}

/// Monotonic within a process. The persistent [`SqliteJournal`] will source
/// batch ids from the database instead once it lands (see the journal module).
static NEXT_BATCH_ID: AtomicI64 = AtomicI64::new(1);

fn next_batch_id() -> BatchId {
    BatchId(NEXT_BATCH_ID.fetch_add(1, Ordering::Relaxed))
}

fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Error)]
pub enum PlanError {
    #[error("file changed on disk since the plan was built: {0}")]
    Stale(PathBuf),
    #[error("rename target already exists: {0}")]
    RenameCollision(PathBuf),
    #[error("path resolves outside the allowed root: {0}")]
    OutsideRoot(PathBuf),
    #[error("renames are not supported yet (planned for a later increment): {0}")]
    RenameNotSupported(PathBuf),
    #[error("journal error: {0}")]
    Journal(#[from] JournalError),
    #[error("tag I/O error: {0}")]
    TagIo(#[from] crate::model::TagIoError),
    #[error("I/O error: {0}")]
    Io(#[source] std::io::Error),
}
