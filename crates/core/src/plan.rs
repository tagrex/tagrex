//! The transactional change pipeline — the heart of TagRex.
//!
//! ```text
//! source of changes --> plan --> preview --> apply --> undo journal
//! ```
//!
//! Everything else (masks, transforms, online providers) is just a producer
//! of plans. **No module writes tags or renames files directly; all writes go
//! through [`Executor`].** This is an invariant, not a preference.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;

use crate::journal::{
    AppliedBatch, AppliedValue, BatchId, CopyStamp, FileRevision, JournalError, UndoJournal,
};
// [2026-11-03 08:03] Import the custom-name rule and the tag I/O error for the pre-flight guard.
use crate::model::{
    is_storable_custom_name_at, CoverArt, Id3v2Revision, TagBlockContent, TagBlockKind, TagEngine,
    TagField, TagIoError, TrackFile,
};

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

/// A change to one whole tag block (#47, #205). Both sides are the WHOLE block,
/// the way [`CoverChange`]'s are the whole image set: `old` is what the file
/// carries and what undo writes back, `new` is what replaces it, and `None` on
/// either side means no block of that kind at all.
///
/// One shape for three operations, because they are the same write with
/// different ends: stripping a block is `new: None`, giving a file a block it
/// lacked is `old: None`, and rewriting one in place — a different ID3v2
/// revision, say — is both sides present. A conversion between two kinds is two
/// of these, one per kind, which is why this is per-kind rather than a
/// from/to pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockChange {
    pub kind: TagBlockKind,
    /// Which ID3v2 revision to write, for the ID3v2 block only. `None` follows
    /// the app-wide preference (#79), which is what a removal's undo and an
    /// ordinary write both want; a conversion sets it deliberately.
    pub revision: Option<Id3v2Revision>,
    /// The revision the block was in, so undo puts that back rather than
    /// whatever the app-wide preference happens to say. A revision is part of
    /// what the block *was*, and an undo that quietly leaves a 2.3 file as 2.4
    /// has not undone anything the user can see.
    pub old_revision: Option<Id3v2Revision>,
    /// The ID3v2 block as it was, in bytes, so undo puts it back frame for
    /// frame instead of rebuilding it from `old` (#206); likewise the Vorbis
    /// comment block, comment for comment (#669). `None` for every other kind,
    /// and for a change that creates a block rather than replacing one.
    ///
    /// Pictures are not in here — they are in `old` — so restoring takes both.
    pub old_bytes: Option<Vec<u8>>,
    pub old: Option<TagBlockContent>,
    pub new: Option<TagBlockContent>,
}

impl BlockChange {
    /// Strip a block, keeping what it held so undo can rebuild it.
    pub fn removal(kind: TagBlockKind, removed: TagBlockContent) -> Self {
        Self {
            kind,
            revision: None,
            old_revision: None,
            old_bytes: None,
            old: Some(removed),
            new: None,
        }
    }

    /// Whether this only changes which revision the ID3v2 block is written in,
    /// leaving its contents alone — the 2.3 <-> 2.4 switch (#205).
    ///
    /// Worth telling apart because it can be done without rebuilding the block,
    /// and so without losing the frames a rebuild cannot carry.
    fn is_revision_only(&self) -> bool {
        self.kind == TagBlockKind::Id3v2 && self.old.is_some() && self.old == self.new
    }

    /// Whether undoing this change would put the block back whole.
    ///
    /// True when the bytes of the old block were kept (#206), which makes the
    /// restore frame-for-frame whatever the kind; otherwise it falls back to
    /// what a rebuild from the model's view can manage, which is ID3v1 alone —
    /// see [`TagBlockContent::exact`]. The preview says so before the change is
    /// staged, which is the only warning the user gets.
    pub fn exact(&self) -> bool {
        self.old_bytes.is_some() || TagBlockContent::exact(self.kind)
    }
}

/// All changes planned for one file.
#[derive(Debug, Clone, Default)]
pub struct FileChange {
    pub path: PathBuf,
    pub tag_changes: Vec<FieldChange>,
    /// Planned cover-art change, if any.
    pub cover_change: Option<CoverChange>,
    /// Whole-block changes for this file (#47, #205). A list because a file can
    /// carry several blocks and one operation can touch more than one of them —
    /// a conversion writes the target block and drops the source in the same
    /// change, and undoing half of that would be worse than not offering it.
    pub block_changes: Vec<BlockChange>,
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
    /// The same description as a code and its values, encoded by the layer that
    /// knows what the encoding means (#268).
    ///
    /// Opaque here on purpose: the journal has to persist it so History can be
    /// rendered in whatever language is chosen *later*, but nothing in this
    /// crate reads it, and giving the core a serialization format for it would
    /// buy a dependency for a string it only ever stores and hands back.
    /// `None` for a plan built before this existed.
    pub message: Option<String>,
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
/// and renaming are still separate *user* operations, each with its own tab in
/// the UI as taggers conventionally present them; a plan can carry either or
/// both.
pub struct Executor;

impl Executor {
    /// Apply a plan's tag changes and renames to disk, then record the batch
    /// so it can be rolled back.
    ///
    /// Every source path and every rename target is confined to `allowed_roots`:
    /// a plan touching a path that resolves outside all of them is rejected
    /// wholesale before anything is written. The entire plan is validated up
    /// front (root containment + staleness + rename collisions) so a bad file
    /// cannot leave the batch half-applied. A write that fails at run time
    /// anyway (a locked file, a share dropping out) is rolled back, best-effort,
    /// before the error is returned, so the library is left as it was (#441).
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
        Self::apply_with(
            plan,
            journal,
            allowed_roots,
            &mut |_| {},
            &AtomicBool::new(false),
        )
    }

    /// [`Executor::apply`] that reports how far it has got and can be stopped
    /// (#437). `progress` is called before each step of the write phase;
    /// `cancel` is read between steps, never inside one, so a cancelled batch
    /// cannot leave a half-written file behind. A cancel is rolled back exactly
    /// like a failure and comes back as [`PlanError::Cancelled`], with nothing
    /// changed and nothing journaled. Once every file is written the batch is
    /// committed whatever the flag says.
    pub fn apply_with(
        plan: &ChangePlan,
        journal: &mut dyn UndoJournal,
        allowed_roots: &[PathBuf],
        progress: &mut dyn FnMut(&ApplyProgress),
        cancel: &AtomicBool,
    ) -> Result<AppliedBatch, PlanError> {
        let roots = canonical_roots(allowed_roots)?;

        // Pre-flight: validate the WHOLE plan before touching disk. Rename
        // targets are collected so two files can't be planned onto the same
        // destination.
        let mut planned_targets = HashSet::new();
        for change in &plan.changes {
            // Reading every file to check it is not stale is the long part of a
            // big apply on a slow disk, and it writes nothing, so a cancel here
            // has nothing to put back (#437).
            if cancel.load(Ordering::SeqCst) {
                return Err(PlanError::Cancelled);
            }
            ensure_within_roots(&change.path, &roots)?;
            ensure_not_stale(change)?;
            // [2026-11-03 08:03] Refuse a custom field the file's tag would silently drop.
            ensure_storable_custom_names(change)?;
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

        // Everything below touches disk. A failure part-way must not leave the
        // library half-applied with nothing in the journal for Undo to find
        // (#441), so what was done is tracked and put back before the error goes
        // up. Rollback is best-effort: it tries every step and keeps going.
        //
        // A rollback needs a process to run it, though. So the batch is in the
        // journal before the first write, marked interrupted (#659): should the
        // process stop part-way, the next start finds it there to undo. A
        // journal that cannot record it means nothing is written at all.
        let mut batch = AppliedBatch {
            // Placeholder: the journal assigns the real id on record so ids
            // stay unique across restarts.
            id: BatchId(0),
            description: plan.description.clone(),
            message: plan.message.clone(),
            applied_at: now_unix_secs(),
            plan: plan.clone(),
            created_dirs: Vec::new(),
            removed_dirs: Vec::new(),
            allowed_roots: roots.clone(),
            copy_stamps: Vec::new(),
            applied_values: Vec::new(),
            interrupted: true,
            id3v2_revisions: id3v2_revisions(plan),
        };
        batch.id = journal.record(&batch)?;

        let mut written = Written::new(plan.changes.len());
        written.revisions.clone_from(&batch.id3v2_revisions);
        let mut steps = Steps::new(count_steps(plan), progress, cancel);
        let removed_dirs = match Self::write_phase(plan, &roots, &mut written, &mut steps) {
            Ok(removed_dirs) => removed_dirs,
            Err(error) => {
                let error = written.roll_back_after(plan, &[], error);
                let _ = journal.rollback(batch.id);
                return Err(error);
            }
        };
        batch.created_dirs = std::mem::take(&mut written.created_dirs);
        batch.removed_dirs = removed_dirs;
        batch.copy_stamps = copy_stamps(plan);
        batch.applied_values = applied_values(plan);
        batch.interrupted = false;
        if let Err(error) = journal.finish(batch.id, &batch) {
            written.created_dirs = batch.created_dirs;
            let error = written.roll_back_after(plan, &batch.removed_dirs, error.into());
            let _ = journal.rollback(batch.id);
            return Err(error);
        }
        Ok(batch)
    }

    /// The write half of [`Executor::apply`]: tags, then moves and copies, then
    /// sidecars, then the tags of copies. Progress is recorded in `written` as it
    /// goes — before each step is attempted, so a step that fails half-way is
    /// still covered by the rollback. Returns the directories pruned afterwards.
    fn write_phase(
        plan: &ChangePlan,
        roots: &[PathBuf],
        written: &mut Written,
        steps: &mut Steps,
    ) -> Result<Vec<PathBuf>, PlanError> {
        // Tags first, at each file's original path — but only for the changes
        // that move. A copy must not touch its source at all (#153), so its tag
        // changes wait and are written to the copy below.
        for (index, change) in plan.changes.iter().enumerate() {
            if change.copy || !writes_in_place(change) {
                continue;
            }
            steps.begin(&change.path)?;
            written.tag_steps[index] = 1;
            write_tag_changes(&change.path, change, Direction::Apply)?;
            written.tag_steps[index] = 2;
            apply_cover_change(&change.path, change, Direction::Apply)?;
            written.tag_steps[index] = 3;
            write_blocks(&change.path, change, Direction::Apply)?;
            steps.done();
        }
        // ...then the moves and copies, creating any folders the targets need.
        // Directories are created here rather than in the pre-flight so a
        // validation failure can't leave empty folders behind.
        for (index, change) in plan.changes.iter().enumerate() {
            if let Some(target) = effective_rename(change) {
                steps.begin(&change.path)?;
                if let Some(parent) = target.parent() {
                    written.created_dirs.extend(create_dirs_recording(parent)?);
                }
                // Checked before the transfer is recorded: a file found there is
                // not this batch's, and rolling back must not move it (#605).
                ensure_target_free(&change.path, target, change.copy)?;
                written.transferred.push(index);
                transfer(&change.path, target, change.copy)?;
                steps.done();
            }
        }
        // Sidecars follow their file, the same way (#58), after the main
        // transfers so their target directories already exist.
        for change in &plan.changes {
            for (from, to) in &change.sidecar_renames {
                if let Some(parent) = to.parent() {
                    written.created_dirs.extend(create_dirs_recording(parent)?);
                }
                ensure_target_free(from, to, change.copy)?;
                written
                    .sidecars
                    .push((from.clone(), to.clone(), change.copy));
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
            if !writes_in_place(change) {
                continue;
            }
            steps.begin(target)?;
            write_tag_changes(target, change, Direction::Apply)?;
            apply_cover_change(target, change, Direction::Apply)?;
            write_blocks(target, change, Direction::Apply)?;
            steps.done();
        }

        // A move can leave the folder it emptied behind (#153). Removing those
        // is opt-in per plan, and only ever the directories THIS batch emptied —
        // recorded so undo can put them back.
        Ok(if plan.prune_empty_dirs {
            prune_emptied_dirs(plan, roots, &written.created_dirs)
        } else {
            Vec::new()
        })
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
        Self::undo_with(journal, batch_id, allowed_roots, &mut |_| {})
    }

    /// [`Executor::undo`] that reports how far it has got (#448), the same way
    /// [`Executor::apply_with`] does. There is no cancel: stopping half-way would
    /// leave the batch partly undone, which is the state #441 removed for apply.
    pub fn undo_with(
        journal: &mut dyn UndoJournal,
        batch_id: BatchId,
        allowed_roots: &[PathBuf],
        progress: &mut dyn FnMut(&ApplyProgress),
    ) -> Result<(), PlanError> {
        let batch = journal
            .batch(batch_id)?
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
        // A transfer that is already undone — by an earlier attempt that stopped
        // half-way (#604) — has nothing left to check or do.
        for change in &batch.plan.changes {
            match effective_rename(change) {
                Some(target) if is_untransferred(target, &change.path, change.copy) => {
                    resolve_target_within_roots(&change.path, &roots)?;
                }
                Some(target) => {
                    ensure_within_roots(target, &roots)?;
                    resolve_target_within_roots(&change.path, &roots)?;
                    ensure_untransfer_safe(target, &change.path, change.copy, &batch.copy_stamps)?;
                }
                None => ensure_within_roots(&change.path, &roots)?,
            }
            // A sidecar currently lives at its `to`; it must go back to `from`,
            // and both must sit within a root (#58).
            for (from, to) in &change.sidecar_renames {
                if is_untransferred(to, from, change.copy) {
                    resolve_target_within_roots(from, &roots)?;
                    continue;
                }
                ensure_within_roots(to, &roots)?;
                resolve_target_within_roots(from, &roots)?;
                ensure_untransfer_safe(to, from, change.copy, &batch.copy_stamps)?;
            }
        }

        ensure_unchanged_since_applied(&batch)?;

        // Reverse the transfers first, so tag restoration finds each file back
        // at its original path. Undoing a copy means removing the copy — the
        // source was never touched, so there is nothing else to put back.
        let never = AtomicBool::new(false);
        let mut steps = Steps::new(count_undo_steps(&batch.plan), progress, &never);
        for change in &batch.plan.changes {
            if let Some(target) = effective_rename(change) {
                steps.begin(target)?;
                if !is_untransferred(target, &change.path, change.copy) {
                    untransfer(target, &change.path, change.copy)?;
                }
                steps.done();
            }
            for (from, to) in &change.sidecar_renames {
                steps.begin(to)?;
                if !is_untransferred(to, from, change.copy) {
                    untransfer(to, from, change.copy)?;
                }
                steps.done();
            }
        }
        for change in &batch.plan.changes {
            // A copy's tag changes were written to the copy, which no longer
            // exists; the source still holds its original values.
            if change.copy || !writes_in_place(change) {
                continue;
            }
            steps.begin(&change.path)?;
            // Blocks come back before the field and image values are restored,
            // mirroring apply in reverse. It matters when the block that was
            // stripped is the one the app reads from: put it back first and the
            // restoring writes land on the same block they were taken from.
            write_blocks(&change.path, change, Direction::Undo)?;
            write_tag_changes(&change.path, change, Direction::Undo)?;
            apply_cover_change(&change.path, change, Direction::Undo)?;
            restore_revision(&change.path, &batch.id3v2_revisions)?;
            steps.done();
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

/// How far a running [`Executor::apply_with`] has got (#437). `total` counts the
/// write steps of the whole plan (tags of a file, a move or copy, a sidecar),
/// `done` those finished before `path`'s step, which is about to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyProgress {
    pub done: usize,
    pub total: usize,
    pub path: PathBuf,
}

/// Counts the steps of the write phase and is the one place it asks whether to
/// go on: before every step, never inside one.
struct Steps<'a> {
    total: usize,
    done: usize,
    progress: &'a mut dyn FnMut(&ApplyProgress),
    cancel: &'a AtomicBool,
}

impl<'a> Steps<'a> {
    fn new(
        total: usize,
        progress: &'a mut dyn FnMut(&ApplyProgress),
        cancel: &'a AtomicBool,
    ) -> Self {
        Self {
            total,
            done: 0,
            progress,
            cancel,
        }
    }

    /// Called before a step: stops the batch if cancelled, else reports it.
    fn begin(&mut self, path: &Path) -> Result<(), PlanError> {
        if self.cancel.load(Ordering::SeqCst) {
            return Err(PlanError::Cancelled);
        }
        (self.progress)(&ApplyProgress {
            done: self.done,
            total: self.total,
            path: path.to_path_buf(),
        });
        Ok(())
    }

    fn done(&mut self) {
        self.done += 1;
    }
}

/// Whether the change writes tag fields, images or blocks into the file.
fn writes_in_place(change: &FileChange) -> bool {
    !change.tag_changes.is_empty()
        || change.cover_change.is_some()
        || !change.block_changes.is_empty()
}

/// The steps [`Executor::write_phase`] will take, counted the way it takes them.
fn count_steps(plan: &ChangePlan) -> usize {
    plan.changes
        .iter()
        .map(|change| {
            let in_place = usize::from(writes_in_place(change));
            let moves = usize::from(effective_rename(change).is_some());
            // A copy's tags are written to the copy, a move's to the original:
            // one in-place step either way, but only a copy needs a target.
            let in_place = if change.copy && moves == 0 {
                0
            } else {
                in_place
            };
            in_place + moves + change.sidecar_renames.len()
        })
        .sum()
}

/// The steps [`Executor::undo_with`] will take: each transfer and sidecar put
/// back, then each file whose fields, images or blocks are restored.
fn count_undo_steps(plan: &ChangePlan) -> usize {
    plan.changes
        .iter()
        .map(|change| {
            let restores = usize::from(!change.copy && writes_in_place(change));
            restores
                + usize::from(effective_rename(change).is_some())
                + change.sidecar_renames.len()
        })
        .sum()
}

/// What a partially failed [`Executor::apply`] has done to disk so far, so it
/// can be put back (#441). Mirrors [`Executor::undo`], over only the steps that
/// were reached.
struct Written {
    /// Per change: how far its in-place writes got. 0 = untouched, 1 = tag
    /// fields attempted, 2 = + images attempted, 3 = + blocks attempted.
    tag_steps: Vec<u8>,
    /// Indices of changes whose move or copy was attempted.
    transferred: Vec<usize>,
    /// Sidecar (from, to, copy) transfers that were attempted.
    sidecars: Vec<(PathBuf, PathBuf, bool)>,
    created_dirs: Vec<PathBuf>,
    /// The ID3v2 revision of each file before the batch, as journaled (#666).
    revisions: Vec<FileRevision>,
}

impl Written {
    fn new(changes: usize) -> Self {
        Self {
            tag_steps: vec![0; changes],
            transferred: Vec::new(),
            sidecars: Vec::new(),
            created_dirs: Vec::new(),
            revisions: Vec::new(),
        }
    }

    /// Roll back, then pair `cause` with whatever the rollback could not undo
    /// (#442): `cause` itself when everything is back, otherwise
    /// [`PlanError::RollbackIncomplete`], so the user is told the library needs a
    /// look instead of only that the write failed.
    fn roll_back_after(
        &self,
        plan: &ChangePlan,
        removed_dirs: &[PathBuf],
        cause: PlanError,
    ) -> PlanError {
        let mut failed = self.roll_back(plan, removed_dirs);
        if failed.is_empty() {
            return cause;
        }
        PlanError::RollbackIncomplete {
            cause: Box::new(cause),
            count: failed.len(),
            first: failed.swap_remove(0),
        }
    }

    /// Reverse what was done, in the order undo uses: restore pruned folders,
    /// move or remove transfers, restore blocks/fields/images, drop created
    /// folders. Every step is attempted and its error ignored: the caller is
    /// already returning the failure that started this. Whether something is
    /// back is decided by looking at the disk afterwards, not by the result of
    /// the undo call — the step that failed usually fails to undo too, having
    /// done nothing to undo. Returns the paths that are still not as they were.
    fn roll_back(&self, plan: &ChangePlan, removed_dirs: &[PathBuf]) -> Vec<PathBuf> {
        let mut failed = Vec::new();
        for dir in removed_dirs {
            let _ = std::fs::create_dir_all(dir);
            if !dir.is_dir() {
                failed.push(dir.clone());
            }
        }
        for (from, to, copy) in self.sidecars.iter().rev() {
            let _ = untransfer(to, from, *copy);
            if !is_untransferred(to, from, *copy) {
                failed.push(from.clone());
            }
        }
        for &index in self.transferred.iter().rev() {
            let change = &plan.changes[index];
            if let Some(target) = effective_rename(change) {
                let _ = untransfer(target, &change.path, change.copy);
                if !is_untransferred(target, &change.path, change.copy) {
                    failed.push(change.path.clone());
                }
            }
        }
        for (index, change) in plan.changes.iter().enumerate() {
            let steps = self.tag_steps[index];
            if change.copy || steps == 0 {
                continue;
            }
            if steps >= 3 {
                let _ = write_blocks(&change.path, change, Direction::Undo);
            }
            let _ = write_tag_changes(&change.path, change, Direction::Undo);
            if steps >= 2 {
                let _ = apply_cover_change(&change.path, change, Direction::Undo);
            }
            let _ = restore_revision(&change.path, &self.revisions);
            // The file holds the values the plan recorded as `old` again, or it
            // does not.
            if ensure_not_stale(change).is_err() {
                failed.push(change.path.clone());
            }
        }
        for dir in self.created_dirs.iter().rev() {
            let _ = std::fs::remove_dir(dir);
        }
        failed
    }
}

/// Whether one [`transfer`] is undone: a move is back at `original` with nothing
/// left at `target`, a copy is gone from `target`.
fn is_untransferred(target: &Path, original: &Path, copy: bool) -> bool {
    if copy {
        !target.exists()
    } else {
        original.exists() && !target.exists()
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

/// A transfer never replaces a file (#605). The pre-flight already refused
/// targets that exist, but it compares paths as strings: on a case-insensitive
/// file system `Track.mp3` and `track.mp3` (or the NFC and NFD spelling of one
/// name) are the same file, so the second of two such moves would land on the
/// first. Checked again right before each transfer, where the file system has
/// the last word. The one file allowed to be there already is the one being
/// moved: a case-only rename of a single file.
fn ensure_target_free(from: &Path, to: &Path, copy: bool) -> Result<(), PlanError> {
    match std::fs::symlink_metadata(to) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PlanError::Io(error)),
        Ok(_) if !copy && same_file::is_same_file(from, to).unwrap_or(false) => Ok(()),
        Ok(_) => Err(PlanError::RenameCollision(to.to_path_buf())),
    }
}

/// Whether undoing one transfer only touches what the batch made (#604).
///
/// A move goes back only onto a free path: a file put at the original path
/// since is not the batch's to replace (the file itself is fine, which is what a
/// case-only rename looks like). A copy is removed only while the file at its
/// path is still the copy as the batch left it; a batch recorded before stamps
/// existed has none, and its copies are removed as before.
fn ensure_untransfer_safe(
    target: &Path,
    original: &Path,
    copy: bool,
    stamps: &[CopyStamp],
) -> Result<(), PlanError> {
    if copy {
        let Some(stamp) = stamps.iter().find(|stamp| stamp.path == target) else {
            return Ok(());
        };
        if CopyStamp::of(target).as_ref() == Some(stamp) {
            Ok(())
        } else {
            Err(PlanError::Stale(target.to_path_buf()))
        }
    } else {
        ensure_target_free(target, original, false)
    }
}

/// The ID3v2 revision of every file `plan` writes in place, read before it is
/// written (#666). A file with no ID3v2 block, or one that cannot be read, gets
/// nothing, and keeps whatever revision undo writes.
fn id3v2_revisions(plan: &ChangePlan) -> Vec<FileRevision> {
    plan.changes
        .iter()
        .filter(|change| !change.copy && writes_in_place(change))
        .filter_map(|change| {
            let revision = TagEngine::id3v2_revision(&change.path).ok()??;
            Some(FileRevision {
                path: change.path.clone(),
                revision,
            })
        })
        .collect()
}

/// Put `path` back in the ID3v2 revision journaled for it (#666). The writes
/// that restore its values save in the revision the app is set to, which need
/// not be the one the file was in. A 2.2 file stays as they left it: the backend
/// does not write 2.2.
fn restore_revision(path: &Path, revisions: &[FileRevision]) -> Result<(), PlanError> {
    let Some(recorded) = revisions
        .iter()
        .find(|file| file.path == path)
        .map(|file| file.revision)
    else {
        return Ok(());
    };
    if !Id3v2Revision::WRITABLE.contains(&recorded) {
        return Ok(());
    }
    if TagEngine::id3v2_revision(path)?.is_some_and(|current| current != recorded) {
        TagEngine::set_id3v2_revision(path, recorded)?;
    }
    Ok(())
}

/// What each changed field of every file `plan` wrote in place reads as, once
/// the batch is written (#658). A file that cannot be read gets nothing, and is
/// undone unchecked. A copy is not here: undo removes it, under its stamp.
fn applied_values(plan: &ChangePlan) -> Vec<AppliedValue> {
    let mut values = Vec::new();
    for change in &plan.changes {
        if change.copy || change.tag_changes.is_empty() {
            continue;
        }
        let path = effective_rename(change).unwrap_or(&change.path);
        let Ok(current) = TagEngine::read(path) else {
            continue;
        };
        values.extend(change.tag_changes.iter().map(|field_change| AppliedValue {
            path: path.to_path_buf(),
            field: field_change.field.clone(),
            value: current.tags.get(&field_change.field).cloned(),
        }));
    }
    values
}

/// Refuse to undo `batch` over a later edit (#658): every field it changed must
/// still read as the batch left it, or as its `old` value already, where an
/// earlier undo stopped part-way (#604). Checked for every file before any is
/// touched, so a refusal writes nothing.
fn ensure_unchanged_since_applied(batch: &AppliedBatch) -> Result<(), PlanError> {
    let mut recorded: HashMap<&Path, Vec<&AppliedValue>> = HashMap::new();
    for applied in &batch.applied_values {
        recorded.entry(&applied.path).or_default().push(applied);
    }
    for change in &batch.plan.changes {
        if change.copy {
            continue;
        }
        let path = effective_rename(change).unwrap_or(&change.path);
        let Some(values) = recorded.get(path) else {
            continue;
        };
        // Where the file is now: back at its own path when an earlier undo
        // already moved it there.
        let now_at = match effective_rename(change) {
            Some(target) if is_untransferred(target, &change.path, false) => &change.path,
            _ => path,
        };
        let current = TagEngine::read(now_at)?;
        for applied in values {
            let now = current.tags.get(&applied.field);
            let old = change
                .tag_changes
                .iter()
                .find(|field_change| field_change.field == applied.field)
                .and_then(|field_change| field_change.old.as_ref());
            if now != applied.value.as_ref() && now != old {
                return Err(PlanError::ChangedSinceApplied(now_at.to_path_buf()));
            }
        }
    }
    Ok(())
}

/// The stamp of every copy `plan` made, read once the batch is written (#604).
/// A copy whose stamp cannot be read gets none, and is undone unchecked.
fn copy_stamps(plan: &ChangePlan) -> Vec<CopyStamp> {
    let mut stamps = Vec::new();
    for change in plan.changes.iter().filter(|change| change.copy) {
        let targets = effective_rename(change)
            .into_iter()
            .chain(change.sidecar_renames.iter().map(|(_, to)| to.as_path()));
        stamps.extend(targets.filter_map(CopyStamp::of));
    }
    stamps
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
        // The file's own folder, and every folder a file travelling with it came
        // out of (#161): carrying a `Scans/` subfolder away empties it, and an
        // empty subfolder left behind would keep its parent from being pruned.
        let sources = std::iter::once(change.path.as_path()).chain(
            change
                .sidecar_renames
                .iter()
                .map(|(from, _)| from.as_path()),
        );
        for source in sources {
            let mut dir = source.parent();
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
    }
    // Deepest first, so a parent is only considered once its child is gone.
    candidates.sort_by_key(|dir| std::cmp::Reverse(dir.components().count()));

    let mut removed = Vec::new();
    for dir in candidates {
        if created.contains(&dir) {
            continue;
        }
        // A folder the batch emptied of everything the user keeps may still
        // hold the OS's own bookkeeping (#404) — a `.DS_Store` Finder wrote
        // while it was open. That is not content, and it is not carried either,
        // so it goes with the folder instead of keeping it from being tidied.
        if clear_os_junk(&dir) && std::fs::remove_dir(&dir).is_ok() {
            removed.push(dir);
        }
    }
    removed
}

/// Whether `path` is a file the OS writes into folders for its own bookkeeping
/// (#404): Finder's `.DS_Store` and AppleDouble `._*` companions, Windows'
/// `Thumbs.db`, `ehthumbs.db` and `desktop.ini`. None of it is the user's —
/// each is rewritten by the OS on demand — so a move neither carries it nor
/// lets it keep an emptied folder from being removed.
pub fn is_os_junk(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    name == ".DS_Store"
        || name.starts_with("._")
        || name.eq_ignore_ascii_case("Thumbs.db")
        || name.eq_ignore_ascii_case("ehthumbs.db")
        || name.eq_ignore_ascii_case("desktop.ini")
}

/// Delete `dir`'s OS bookkeeping files if that is all it holds, recursing into
/// subfolders that hold nothing else either, and report whether `dir` is empty
/// now. A folder with anything real in it is left exactly as it was.
fn clear_os_junk(dir: &Path) -> bool {
    fn only_junk(dir: &Path) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return false;
        };
        entries.flatten().all(|entry| {
            let path = entry.path();
            if path.is_dir() {
                only_junk(&path)
            } else {
                is_os_junk(&path)
            }
        })
    }
    if !only_junk(dir) {
        return false;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            std::fs::remove_dir_all(&path).ok();
        } else {
            std::fs::remove_file(&path).ok();
        }
    }
    true
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
    // A block that has changed — or is already gone — since the plan was built
    // would be journaled with a snapshot undo could not put back (#47).
    for block_change in &change.block_changes {
        if TagEngine::read_block(&change.path, block_change.kind)? != block_change.old {
            return Err(PlanError::Stale(change.path.clone()));
        }
    }
    Ok(())
}

// [2026-11-03 08:03] Added: pre-flight guard for custom field names.
/// Refuse a change that sets a custom field under a name the file's tag type
/// cannot store. The backend skips such a name without an error, so the write
/// would report success and the value be gone on the next read. Clearing one
/// (`new` empty or `None`) is always fine. The preview already flags these
/// ([`is_writable_value_at`](crate::model::is_writable_value_at)); this keeps
/// any other plan source from getting one past it.
fn ensure_storable_custom_names(change: &FileChange) -> Result<(), PlanError> {
    for field_change in &change.tag_changes {
        let (TagField::Custom(name), Some(new)) = (&field_change.field, &field_change.new) else {
            continue;
        };
        if !new.is_empty() && !is_storable_custom_name_at(&change.path, name) {
            return Err(PlanError::TagIo(TagIoError::Malformed(format!(
                "the field name {name:?} cannot be stored in {}",
                change.path.display()
            ))));
        }
    }
    Ok(())
}

/// Write this change's blocks (#47, #205). Last of the per-file writes, so the
/// field and image writes above still land on the block they were planned
/// against even when that block is the one going away.
///
/// A conversion's two halves are ordered: every block being written goes first,
/// then the ones being dropped. The other order would leave a file with no tag
/// block at all between the two writes, and a crash in that gap would lose
/// everything rather than half of it.
fn write_blocks(path: &Path, change: &FileChange, direction: Direction) -> Result<(), PlanError> {
    let target = |block_change: &BlockChange| match direction {
        Direction::Apply => block_change.new.clone(),
        Direction::Undo => block_change.old.clone(),
    };
    let revision = |block_change: &BlockChange| match direction {
        Direction::Apply => block_change.revision,
        Direction::Undo => block_change.old_revision,
    };
    for block_change in &change.block_changes {
        // A revision switch keeps the block as it is and only restamps the
        // header, so it never goes through the rebuild — see
        // [`TagEngine::set_id3v2_revision`] for why that matters.
        if block_change.is_revision_only() {
            if let Some(revision) = revision(block_change) {
                TagEngine::set_id3v2_revision(path, revision)?;
                continue;
            }
        }
        if let Some(content) = target(block_change) {
            // Undo puts an ID3v2 block back from its bytes when they were kept
            // (#206), so the frames the model cannot express come back too.
            // Apply never has bytes to write: it is producing a block, not
            // putting one back.
            match (direction, &block_change.old_bytes) {
                (Direction::Undo, Some(bytes)) if block_change.kind == TagBlockKind::Id3v2 => {
                    TagEngine::restore_id3v2_bytes(
                        path,
                        bytes,
                        &content.covers,
                        block_change.old_revision,
                    )?;
                }
                (Direction::Undo, Some(bytes))
                    if block_change.kind == TagBlockKind::VorbisComments =>
                {
                    TagEngine::restore_vorbis_bytes(path, bytes, &content.covers)?;
                }
                _ => {
                    TagEngine::write_block(
                        path,
                        block_change.kind,
                        revision(block_change),
                        &content,
                    )?;
                }
            }
        }
    }
    for block_change in &change.block_changes {
        if target(block_change).is_none() {
            TagEngine::remove_block(path, block_change.kind)?;
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
    /// Undo found a field that has changed since the batch was applied (#658),
    /// and would have written the old value over that later edit.
    #[error("file changed since the batch was applied: {0}")]
    ChangedSinceApplied(PathBuf),
    #[error("path resolves outside the allowed root: {0}")]
    OutsideRoot(PathBuf),
    #[error("journal error: {0}")]
    Journal(#[from] JournalError),
    #[error("tag I/O error: {0}")]
    TagIo(#[from] crate::model::TagIoError),
    #[error("I/O error: {0}")]
    Io(#[source] std::io::Error),
    /// The apply was cancelled between two files and rolled back (#437).
    #[error("apply cancelled")]
    Cancelled,
    /// A failed apply could not be fully rolled back (#442): `count` files,
    /// folders or sidecars are not as they were, `first` being the first.
    #[error("{cause}; {count} item(s) could not be put back, the first: {}", .first.display())]
    RollbackIncomplete {
        cause: Box<PlanError>,
        count: usize,
        first: PathBuf,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change_for(path: &Path, artist: &str) -> FileChange {
        FileChange {
            path: path.to_path_buf(),
            tag_changes: vec![FieldChange {
                field: TagField::Artist,
                old: None,
                new: Some(artist.to_string()),
            }],
            ..FileChange::default()
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tagrex-plan-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A file the plan reached but that is no longer there cannot be put back:
    /// the rollback has to say so.
    #[test]
    fn a_rollback_that_cannot_restore_a_file_reports_it() {
        let dir = temp_dir("incomplete");
        let gone = dir.join("gone.flac");
        let plan = ChangePlan {
            changes: vec![change_for(&gone, "A")],
            ..ChangePlan::default()
        };
        let mut written = Written::new(1);
        written.tag_steps[0] = 3;

        let error = written.roll_back_after(&plan, &[], PlanError::Io(std::io::Error::other("x")));

        match error {
            PlanError::RollbackIncomplete { count, first, .. } => {
                assert_eq!(count, 1);
                assert_eq!(first, gone);
            }
            other => panic!("expected RollbackIncomplete, got {other:?}"),
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Nothing was reached, so there is nothing to put back and nothing to report.
    #[test]
    fn an_untouched_plan_rolls_back_silently() {
        let dir = temp_dir("untouched");
        let plan = ChangePlan {
            changes: vec![change_for(&dir.join("never.flac"), "A")],
            ..ChangePlan::default()
        };
        let written = Written::new(1);

        let error = written.roll_back_after(&plan, &[], PlanError::Io(std::io::Error::other("x")));

        assert!(matches!(error, PlanError::Io(_)));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A transfer onto an existing file is refused, unless that file is the one
    /// being moved (#605).
    #[test]
    fn a_transfer_target_must_be_free_or_the_file_itself() {
        let dir = temp_dir("target-free");
        let from = dir.join("a.flac");
        let other = dir.join("b.flac");
        std::fs::write(&from, b"a").unwrap();
        std::fs::write(&other, b"b").unwrap();

        assert!(ensure_target_free(&from, &dir.join("free.flac"), false).is_ok());
        assert!(ensure_target_free(&from, &dir.join("free.flac"), true).is_ok());
        assert!(matches!(
            ensure_target_free(&from, &other, false),
            Err(PlanError::RenameCollision(_))
        ));
        assert!(matches!(
            ensure_target_free(&from, &other, true),
            Err(PlanError::RenameCollision(_))
        ));
        // A move onto itself is a rename in place; a copy onto itself would
        // truncate the source.
        assert!(ensure_target_free(&from, &from, false).is_ok());
        assert!(matches!(
            ensure_target_free(&from, &from, true),
            Err(PlanError::RenameCollision(_))
        ));
        std::fs::remove_dir_all(&dir).ok();
    }
}
