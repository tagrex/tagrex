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

/// A change to the file's embedded images (#56). Both sides are the WHOLE set:
/// `old` is what the file carries and what undo writes back, `new` is what
/// replaces it, and an empty side means no images at all.
///
/// A snapshot rather than a list of per-image operations, because an embedded
/// picture has no stable identity to address. Adding, replacing, removing,
/// reordering and retyping all become the same change, its undo is trivially
/// the other side, and the staleness check stays one comparison.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CoverChange {
    pub old: Vec<CoverArt>,
    pub new: Vec<CoverArt>,
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
    /// Whether `rename_to` is a COPY rather than a move (#153). The source is
    /// left exactly as it is — which is what makes copy the safe half of writing
    /// outside the library — so a copy change's tag and cover changes are
    /// written to the *copy*, never to the original. Undo removes the copy.
    pub copy: bool,
    /// Sidecar files that travel with this file's rename/move (#58): each a
    /// `(from, to)` pair. Detected at preview time and journaled with the plan so
    /// they move together and are restored together on undo. Empty when the file
    /// isn't renamed or the feature is off.
    pub sidecar_renames: Vec<(PathBuf, PathBuf)>,
}

/// A complete, previewable plan of changes over a set of files.
#[derive(Debug, Clone, Default)]
pub struct ChangePlan {
    /// Human-readable summary, inherited from the [`PlanSource`] that built
    /// the plan, and carried into the journal so history shows what a batch
    /// was (see [`AppliedBatch::description`]).
    pub description: String,
    pub changes: Vec<FileChange>,
    /// Whether to remove the folders a move empties (#153). Off by default, so
    /// a plain rename never prunes anything; the reorganize operation turns it
    /// on. Only directories this batch actually emptied are removed, and undo
    /// puts them back — see [`AppliedBatch::removed_dirs`].
    pub prune_empty_dirs: bool,
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
    /// Every source path and every rename target is confined to `allowed_roots`:
    /// a plan touching a path that resolves outside all of them is rejected
    /// wholesale before anything is written. The entire plan is validated up
    /// front (root containment + staleness + rename collisions) so a bad file
    /// cannot leave the batch half-applied.
    ///
    /// There is more than one root because a reorganize can file tracks into a
    /// folder outside the open library (#153). That second root is not inferred
    /// from the plan — which would make the containment check circular — it is a
    /// destination the user picked, handed down from the caller and recorded on
    /// the batch so undo is still authorized for it after a restart.
    pub fn apply(
        plan: &ChangePlan,
        journal: &mut dyn UndoJournal,
        allowed_roots: &[PathBuf],
    ) -> Result<AppliedBatch, PlanError> {
        let roots = canonical_roots(allowed_roots)?;

        // Pre-flight: validate the WHOLE plan before touching disk. Rename
        // targets are collected so two files can't be planned onto the same
        // destination.
        let mut planned_targets = HashSet::new();
        for change in &plan.changes {
            ensure_within_roots(&change.path, &roots)?;
            ensure_not_stale(change)?;
            if let Some(target) = effective_rename(change) {
                let canonical_target = resolve_target_within_roots(target, &roots)?;
                if canonical_target.exists() {
                    return Err(PlanError::RenameCollision(canonical_target));
                }
                if !planned_targets.insert(canonical_target.clone()) {
                    return Err(PlanError::RenameCollision(canonical_target));
                }
            }
            // Sidecars (#58) obey the same rules: source inside a root, target
            // inside a root, and never overwrite an existing file or another
            // planned destination.
            for (from, to) in &change.sidecar_renames {
                ensure_within_roots(from, &roots)?;
                let canonical_target = resolve_target_within_roots(to, &roots)?;
                if canonical_target.exists() {
                    return Err(PlanError::RenameCollision(canonical_target));
                }
                if !planned_targets.insert(canonical_target.clone()) {
                    return Err(PlanError::RenameCollision(canonical_target));
                }
            }
        }

        // Tags first, at each file's original path — but only for the changes
        // that move. A copy must not touch its source at all (#153), so its tag
        // changes wait and are written to the copy below.
        for change in &plan.changes {
            if change.copy {
                continue;
            }
            write_tag_changes(&change.path, change, Direction::Apply)?;
            apply_cover_change(&change.path, change, Direction::Apply)?;
        }
        // ...then the moves and copies, creating any folders the targets need.
        // Directories are created here rather than in the pre-flight so a
        // validation failure can't leave empty folders behind.
        let mut created_dirs = Vec::new();
        for change in &plan.changes {
            if let Some(target) = effective_rename(change) {
                if let Some(parent) = target.parent() {
                    created_dirs.extend(create_dirs_recording(parent)?);
                }
                transfer(&change.path, target, change.copy)?;
            }
        }
        // Sidecars follow their file, the same way (#58), after the main
        // transfers so their target directories already exist.
        for change in &plan.changes {
            for (from, to) in &change.sidecar_renames {
                if let Some(parent) = to.parent() {
                    created_dirs.extend(create_dirs_recording(parent)?);
                }
                transfer(from, to, change.copy)?;
            }
        }
        // Now that the source is a copy of its own, the copy gets the tag
        // changes the source was spared.
        for change in &plan.changes {
            if !change.copy {
                continue;
            }
            let Some(target) = effective_rename(change) else {
                continue;
            };
            write_tag_changes(target, change, Direction::Apply)?;
            apply_cover_change(target, change, Direction::Apply)?;
        }

        // A move can leave the folder it emptied behind (#153). Removing those
        // is opt-in per plan, and only ever the directories THIS batch emptied —
        // recorded so undo can put them back.
        let removed_dirs = if plan.prune_empty_dirs {
            prune_emptied_dirs(plan, &roots, &created_dirs)
        } else {
            Vec::new()
        };

        let mut batch = AppliedBatch {
            // Placeholder: the journal assigns the real id on record so ids
            // stay unique across restarts.
            id: BatchId(0),
            description: plan.description.clone(),
            applied_at: now_unix_secs(),
            plan: plan.clone(),
            created_dirs,
            removed_dirs,
            allowed_roots: roots,
        };
        batch.id = journal.record(&batch)?;
        Ok(batch)
    }

    /// Roll a previously applied batch back: move every renamed file back to
    /// its original path, remove every copy it made, restore every field's
    /// `old` value, then remove the batch from the journal.
    ///
    /// Renames are reversed *before* tag restoration, mirroring apply in
    /// reverse: the file lives at its rename target now, so it has to move
    /// back before the original-path tag write can find it.
    ///
    /// Containment works the same way as apply, over `allowed_roots` *plus the
    /// roots the batch itself recorded*. That second part is what lets a
    /// reorganize into a folder outside the library be undone in a later
    /// session (#153), when the caller no longer knows about that folder: the
    /// destination was confirmed by the user when the batch was applied, and
    /// the journal is where that authorization lives. Undo can still only move
    /// files back along paths the journal recorded, so this widens what undo
    /// may touch without widening what a plan may reach.
    pub fn undo(
        journal: &mut dyn UndoJournal,
        batch_id: BatchId,
        allowed_roots: &[PathBuf],
    ) -> Result<(), PlanError> {
        let batch = journal
            .batches()?
            .into_iter()
            .find(|batch| batch.id == batch_id)
            .ok_or(PlanError::Journal(JournalError::UnknownBatch(batch_id)))?;

        let mut roots = canonical_roots(allowed_roots)?;
        for root in &batch.allowed_roots {
            if !roots.contains(root) {
                roots.push(root.clone());
            }
        }

        // Put back the folders the batch emptied, before anything moves: a file
        // has to have a directory to move back into (#153).
        for dir in &batch.removed_dirs {
            if resolve_target_within_roots(dir, &roots).is_ok() {
                std::fs::create_dir_all(dir).map_err(PlanError::Io)?;
            }
        }

        // Validate before touching disk: the file's current location (rename
        // target if it was renamed, else its path) and the restore
        // destination must both sit within a root.
        for change in &batch.plan.changes {
            match effective_rename(change) {
                Some(target) => {
                    ensure_within_roots(target, &roots)?;
                    resolve_target_within_roots(&change.path, &roots)?;
                }
                None => ensure_within_roots(&change.path, &roots)?,
            }
            // A sidecar currently lives at its `to`; it must go back to `from`,
            // and both must sit within a root (#58).
            for (from, to) in &change.sidecar_renames {
                ensure_within_roots(to, &roots)?;
                resolve_target_within_roots(from, &roots)?;
            }
        }

        // Reverse the transfers first, so tag restoration finds each file back
        // at its original path. Undoing a copy means removing the copy — the
        // source was never touched, so there is nothing else to put back.
        for change in &batch.plan.changes {
            if let Some(target) = effective_rename(change) {
                untransfer(target, &change.path, change.copy)?;
            }
            for (from, to) in &change.sidecar_renames {
                untransfer(to, from, change.copy)?;
            }
        }
        for change in &batch.plan.changes {
            // A copy's tag changes were written to the copy, which no longer
            // exists; the source still holds its original values.
            if change.copy {
                continue;
            }
            write_tag_changes(&change.path, change, Direction::Undo)?;
            apply_cover_change(&change.path, change, Direction::Undo)?;
        }

        // Finally remove the folders this batch created, deepest first. Only
        // empty ones go: if the user has since put something in there, the
        // directory stays and rollback is still considered successful.
        for dir in batch.created_dirs.iter().rev() {
            if ensure_within_roots(dir, &roots).is_ok() {
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

/// Canonicalize every allowed root, dropping the ones that no longer resolve.
/// A root that has been deleted or renamed since it was chosen simply stops
/// authorizing anything, which fails safe; only when NONE of them resolve is
/// this an error, since then nothing could be written anyway.
fn canonical_roots(allowed_roots: &[PathBuf]) -> Result<Vec<PathBuf>, PlanError> {
    let mut roots = Vec::with_capacity(allowed_roots.len());
    let mut first_error = None;
    for root in allowed_roots {
        match std::fs::canonicalize(root) {
            Ok(canonical) => {
                if !roots.contains(&canonical) {
                    roots.push(canonical);
                }
            }
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    match first_error {
        Some(error) if roots.is_empty() => Err(PlanError::Io(error)),
        _ => Ok(roots),
    }
}

/// Move or copy one file. The one place the two differ, so nothing above has to
/// branch on it twice.
fn transfer(from: &Path, to: &Path, copy: bool) -> Result<(), PlanError> {
    if copy {
        std::fs::copy(from, to).map(|_| ()).map_err(PlanError::Io)
    } else {
        std::fs::rename(from, to).map_err(PlanError::Io)
    }
}

/// Reverse one [`transfer`]: a move goes back, a copy is removed.
fn untransfer(target: &Path, original: &Path, copy: bool) -> Result<(), PlanError> {
    if copy {
        // The copy may already be gone (deleted by hand between apply and
        // undo); that is the state undo wanted, not a failure.
        match std::fs::remove_file(target) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(PlanError::Io(error)),
        }
    } else {
        std::fs::rename(target, original).map_err(PlanError::Io)
    }
}

/// Remove the directories this batch's moves emptied (#153), deepest first, and
/// report them so undo can put them back.
///
/// Only a directory that is empty *now*, sits inside an allowed root, and is not
/// one this same batch just created. Walking up from each source folder means a
/// move that empties `Artist/Album` also clears `Artist` when nothing else is
/// left in it — which is the point, since an unsorted folder is usually a tree.
/// Anything that fails to remove is simply left alone: a folder that still holds
/// something the plan never touched is not this batch's to delete.
fn prune_emptied_dirs(plan: &ChangePlan, roots: &[PathBuf], created: &[PathBuf]) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    for change in &plan.changes {
        if change.copy || effective_rename(change).is_none() {
            continue;
        }
        let mut dir = change.path.parent();
        while let Some(current) = dir {
            let owned = current.to_path_buf();
            // A root is the boundary, not something to remove.
            if roots.contains(&owned) {
                break;
            }
            if ensure_within_roots(current, roots).is_err() {
                break;
            }
            if !candidates.contains(&owned) {
                candidates.push(owned);
            }
            dir = current.parent();
        }
    }
    // Deepest first, so a parent is only considered once its child is gone.
    candidates.sort_by_key(|dir| std::cmp::Reverse(dir.components().count()));

    let mut removed = Vec::new();
    for dir in candidates {
        if created.contains(&dir) {
            continue;
        }
        if std::fs::remove_dir(&dir).is_ok() {
            removed.push(dir);
        }
    }
    removed
}

/// Resolve `path` (following symlinks, collapsing `..`) and require the result
/// to sit inside one of `roots`. This is what stops a crafted mask literal like
/// `../../etc` from steering a write outside the scanned library. Requires the
/// path to exist, since it canonicalizes the file itself.
fn ensure_within_roots(path: &Path, roots: &[PathBuf]) -> Result<(), PlanError> {
    let canonical = std::fs::canonicalize(path).map_err(PlanError::Io)?;
    if roots.iter().any(|root| canonical.starts_with(root)) {
        Ok(())
    } else {
        Err(PlanError::OutsideRoot(canonical))
    }
}

/// A rename destination that doesn't exist yet can't be canonicalized directly.
/// Resolve it against its nearest *existing* ancestor — which may be several
/// levels up when the target lands in folders that still have to be created —
/// and require that ancestor to sit inside one of `roots`. Returns the resolved
/// absolute target path.
///
/// Canonicalizing the existing part is what makes the containment check
/// meaningful: it collapses `..` and follows symlinks, so a crafted mask can't
/// steer a move outside the library by way of a path that only looks contained.
fn resolve_target_within_roots(target: &Path, roots: &[PathBuf]) -> Result<PathBuf, PlanError> {
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
    if !roots.iter().any(|root| canonical.starts_with(root)) {
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
        if TagEngine::read_covers(&change.path)? != cover_change.old {
            return Err(PlanError::Stale(change.path.clone()));
        }
    }
    Ok(())
}

/// Write the `new` image set (or the `old` one on undo). No-op when the change
/// carries no cover change; an empty target set removes every image.
fn apply_cover_change(
    path: &Path,
    change: &FileChange,
    direction: Direction,
) -> Result<(), PlanError> {
    let Some(cover_change) = &change.cover_change else {
        return Ok(());
    };
    let target = match direction {
        Direction::Apply => &cover_change.new,
        Direction::Undo => &cover_change.old,
    };
    TagEngine::write_covers(path, target)?;
    Ok(())
}

fn write_tag_changes(
    path: &Path,
    change: &FileChange,
    direction: Direction,
) -> Result<(), PlanError> {
    if change.tag_changes.is_empty() {
        return Ok(());
    }
    let mut track = TagEngine::read(path)?;
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
