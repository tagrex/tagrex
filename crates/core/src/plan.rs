//! The transactional change pipeline — the heart of TagRex.
//!
//! ```text
//! source of changes --> plan --> preview --> apply --> undo journal
//! ```
//!
//! Everything else (masks, transforms, online providers) is just a producer
//! of plans. **No module writes tags or renames files directly; all writes go
//! through [`Executor`].** This is an invariant, not a preference.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;

use crate::journal::{AppliedBatch, BatchId, JournalError, UndoJournal};
use crate::model::{CoverArt, TagEngine, TagField, TrackFile};

/// A change to a single tag field: `old` is what preview shows as "current",
/// `new` is what will be written. `None` means the field is absent/removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldChange {
    pub field: TagField,
    pub old: Option<String>,
    pub new: Option<String>,
}

/// A change to a file's embedded front cover. `old` is restored on undo;
/// `new` is embedded (or the cover removed when `None`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverChange {
    pub old: Option<CoverArt>,
    pub new: Option<CoverArt>,
}

/// All changes planned for one file.
#[derive(Debug, Clone, Default)]
pub struct FileChange {
    pub path: PathBuf,
    pub tag_changes: Vec<FieldChange>,
    /// Planned cover-art change, if any.
    pub cover_change: Option<CoverChange>,
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
/// Within a single file, tag writes happen before the rename: once a file is
/// renamed, the plan's path no longer points at it, so keeping the move last
/// means every tag write uses the original path and a failure mid-move leaves
/// the file at its old path with new tags — a recoverable state. Tag writing
/// and renaming are still separate *user* operations (like TagScanner's
/// separate tabs); a plan can carry either or both.
pub struct Executor;

impl Executor {
    /// Apply a plan's tag changes and renames to disk, then record the batch
    /// so it can be rolled back.
    ///
    /// Every write and every rename target is confined to `allowed_root`: a
    /// plan touching a path that resolves outside it is rejected wholesale
    /// before anything is written. The entire plan is validated up front
    /// (root containment + staleness + rename collisions) so a bad file
    /// cannot leave the batch half-applied.
    pub fn apply(
        plan: &ChangePlan,
        journal: &mut dyn UndoJournal,
        allowed_root: &Path,
    ) -> Result<AppliedBatch, PlanError> {
        let root = canonical_root(allowed_root)?;

        // Pre-flight: validate the WHOLE plan before touching disk. Rename
        // targets are collected so two files can't be planned onto the same
        // destination.
        let mut planned_targets = HashSet::new();
        for change in &plan.changes {
            ensure_within_root(&change.path, &root)?;
            ensure_not_stale(change)?;
            if let Some(target) = effective_rename(change) {
                let canonical_target = resolve_target_within_root(target, &root)?;
                if canonical_target.exists() {
                    return Err(PlanError::RenameCollision(canonical_target));
                }
                if !planned_targets.insert(canonical_target.clone()) {
                    return Err(PlanError::RenameCollision(canonical_target));
                }
            }
        }

        // Apply tags first (all files, at their original paths)...
        for change in &plan.changes {
            write_tag_changes(change, Direction::Apply)?;
            apply_cover_change(change, Direction::Apply)?;
        }
        // ...then renames, creating any folders the targets need. Directories
        // are created here rather than in the pre-flight so a validation
        // failure can't leave empty folders behind.
        let mut created_dirs = Vec::new();
        for change in &plan.changes {
            if let Some(target) = effective_rename(change) {
                if let Some(parent) = target.parent() {
                    created_dirs.extend(create_dirs_recording(parent)?);
                }
                std::fs::rename(&change.path, target).map_err(PlanError::Io)?;
            }
        }

        let mut batch = AppliedBatch {
            // Placeholder: the journal assigns the real id on record so ids
            // stay unique across restarts.
            id: BatchId(0),
            description: plan.description.clone(),
            applied_at: now_unix_secs(),
            plan: plan.clone(),
            created_dirs,
        };
        batch.id = journal.record(&batch)?;
        Ok(batch)
    }

    /// Roll a previously applied batch back: move every renamed file back to
    /// its original path, restore every field's `old` value, then remove the
    /// batch from the journal.
    ///
    /// Renames are reversed *before* tag restoration, mirroring apply in
    /// reverse: the file lives at its rename target now, so it has to move
    /// back before the original-path tag write can find it. Everything is
    /// confined to `allowed_root` the same way apply is.
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

        // Validate before touching disk: the file's current location (rename
        // target if it was renamed, else its path) and the restore
        // destination must both sit within root.
        for change in &batch.plan.changes {
            match effective_rename(change) {
                Some(target) => {
                    ensure_within_root(target, &root)?;
                    resolve_target_within_root(&change.path, &root)?;
                }
                None => ensure_within_root(&change.path, &root)?,
            }
        }

        // Reverse renames first, so tag restoration finds each file back at
        // its original path.
        for change in &batch.plan.changes {
            if let Some(target) = effective_rename(change) {
                std::fs::rename(target, &change.path).map_err(PlanError::Io)?;
            }
        }
        for change in &batch.plan.changes {
            write_tag_changes(change, Direction::Undo)?;
            apply_cover_change(change, Direction::Undo)?;
        }

        // Finally remove the folders this batch created, deepest first. Only
        // empty ones go: if the user has since put something in there, the
        // directory stays and rollback is still considered successful.
        for dir in batch.created_dirs.iter().rev() {
            if ensure_within_root(dir, &root).is_ok() {
                std::fs::remove_dir(dir).ok();
            }
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
/// `../../etc` from steering a write outside the scanned library. Requires the
/// path to exist, since it canonicalizes the file itself.
fn ensure_within_root(path: &Path, root: &Path) -> Result<(), PlanError> {
    let canonical = std::fs::canonicalize(path).map_err(PlanError::Io)?;
    if canonical.starts_with(root) {
        Ok(())
    } else {
        Err(PlanError::OutsideRoot(canonical))
    }
}

/// A rename destination that doesn't exist yet can't be canonicalized directly.
/// Resolve it against its nearest *existing* ancestor — which may be several
/// levels up when the target lands in folders that still have to be created —
/// and require that ancestor to sit inside `root`. Returns the resolved
/// absolute target path.
///
/// Canonicalizing the existing part is what makes the containment check
/// meaningful: it collapses `..` and follows symlinks, so a crafted mask can't
/// steer a move outside the library by way of a path that only looks contained.
fn resolve_target_within_root(target: &Path, root: &Path) -> Result<PathBuf, PlanError> {
    let mut existing = target
        .parent()
        .ok_or_else(|| PlanError::OutsideRoot(target.to_path_buf()))?;
    let mut trailing: Vec<&std::ffi::OsStr> = Vec::new();
    // `target`'s own file name is always part of the not-yet-existing tail.
    trailing.push(
        target
            .file_name()
            .ok_or_else(|| PlanError::OutsideRoot(target.to_path_buf()))?,
    );
    while !existing.exists() {
        let name = existing
            .file_name()
            .ok_or_else(|| PlanError::OutsideRoot(target.to_path_buf()))?;
        trailing.push(name);
        existing = existing
            .parent()
            .ok_or_else(|| PlanError::OutsideRoot(target.to_path_buf()))?;
    }

    let canonical = std::fs::canonicalize(existing).map_err(PlanError::Io)?;
    if !canonical.starts_with(root) {
        return Err(PlanError::OutsideRoot(target.to_path_buf()));
    }
    let mut resolved = canonical;
    for name in trailing.iter().rev() {
        resolved.push(name);
    }
    Ok(resolved)
}

/// Create `dir` and any missing ancestors, returning the directories actually
/// created, shallowest first. Recording exactly what was created lets undo
/// remove those and only those — a directory that already existed is never
/// touched, even if the rollback leaves it empty.
fn create_dirs_recording(dir: &Path) -> Result<Vec<PathBuf>, PlanError> {
    let mut missing = Vec::new();
    let mut current = Some(dir);
    while let Some(path) = current {
        if path.exists() {
            break;
        }
        missing.push(path.to_path_buf());
        current = path.parent();
    }
    missing.reverse();
    for path in &missing {
        std::fs::create_dir(path).map_err(PlanError::Io)?;
    }
    Ok(missing)
}

/// The rename this change actually performs, if any: `rename_to` unless it's
/// a no-op (equal to the current path).
fn effective_rename(change: &FileChange) -> Option<&Path> {
    change
        .rename_to
        .as_deref()
        .filter(|target| *target != change.path)
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
    if let Some(cover_change) = &change.cover_change {
        if TagEngine::read_cover(&change.path)? != cover_change.old {
            return Err(PlanError::Stale(change.path.clone()));
        }
    }
    Ok(())
}

/// Embed the `new` cover (or the `old` one on undo), or remove it when the
/// target side is `None`. No-op when the change carries no cover.
fn apply_cover_change(change: &FileChange, direction: Direction) -> Result<(), PlanError> {
    let Some(cover_change) = &change.cover_change else {
        return Ok(());
    };
    let target = match direction {
        Direction::Apply => &cover_change.new,
        Direction::Undo => &cover_change.old,
    };
    match target {
        Some(cover) => TagEngine::embed_cover(&change.path, cover)?,
        None => TagEngine::remove_cover(&change.path)?,
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
    #[error("journal error: {0}")]
    Journal(#[from] JournalError),
    #[error("tag I/O error: {0}")]
    TagIo(#[from] crate::model::TagIoError),
    #[error("I/O error: {0}")]
    Io(#[source] std::io::Error),
}
