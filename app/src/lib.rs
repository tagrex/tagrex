//! Application command layer — the thin surface a GUI shell forwards user
//! intent to.
//!
//! Per architecture.md the shell stays thin: it renders state and forwards
//! intent, all logic lives in `tagrex-core`. [`App`] is that boundary. A Tauri
//! backend would hold one `App` in managed state and each `#[tauri::command]`
//! would be a one-line call into it; the webview + frontend are initialized
//! separately on a machine with a display (see `main.rs`). Every method here is
//! plain, testable Rust with no GUI dependency.
//!
//! Data crossing the (future) IPC boundary is expressed as serde DTOs rather
//! than core types, so `tagrex-core` stays serialization-agnostic. Tag map
//! keys use [`TagField`]'s lossless storage-key codec.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use base64::Engine as _;
use tagrex_core::export::{self, PlaylistTrack};
use tagrex_core::journal::{BatchId, SqliteJournal, UndoJournal};
use tagrex_core::mask::Mask;
use tagrex_core::matching::{self, MatchOptions, TrackRef};
use tagrex_core::model::{is_writable_value, CoverArt, TagEngine, TagField};
use tagrex_core::plan::{ChangePlan, CoverChange, Executor, FieldChange, FileChange};
use tagrex_core::provider::{MetadataProvider, ReleaseId, SearchQuery};
use tagrex_core::scanner::{self, ScanOptions};
use tagrex_core::transform::{
    CaseStyle, ChangeCase, RemoveDiacritics, Replace, ReplaceOptions, TransformChain,
};
use tagrex_providers_discogs::DiscogsProvider;

/// One audio file as the table view sees it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrackDto {
    pub path: String,
    pub format: String,
    /// Storage-key -> value (see [`TagField::to_storage_key`]).
    pub tags: std::collections::BTreeMap<String, String>,
}

/// A single planned field change: `old` is the current value, `new` what will
/// be written; `None` means absent/removed. `invalid` marks a `new` value the
/// backend rejected (see [`field_value_invalid`]): the preview flags the cell
/// and apply skips it, so `old` stays on disk. `#[serde(default)]` keeps plans
/// authored before the flag existed (and hand-built ones) deserializable.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FieldChangeDto {
    pub field: String,
    pub old: Option<String>,
    pub new: Option<String>,
    #[serde(default)]
    pub invalid: bool,
}

impl FieldChangeDto {
    /// Build a field change, validating the proposed `new` value. A rejected
    /// value is flagged `invalid` rather than dropped, so the preview can show
    /// it as an error while apply leaves the field untouched.
    fn new(field: String, old: Option<String>, new: Option<String>) -> Self {
        let invalid = field_value_invalid(&field, new.as_deref());
        Self {
            field,
            old,
            new,
            invalid,
        }
    }
}

/// Whether a proposed `new` value for `field` (a storage key) must be rejected
/// rather than written. Delegates to the tag engine's per-field rule
/// ([`is_writable_value`]) so the preview flags exactly what the writer would
/// mishandle: an invalid year (corrupts the file), a non-numeric track/disc/
/// total or BPM (silently dropped). Free-text fields accept anything; an
/// empty/absent value is always valid (it clears the field).
fn field_value_invalid(field: &str, new: Option<&str>) -> bool {
    match new {
        Some(value) => !is_writable_value(&TagField::from_storage_key(field), value),
        None => false,
    }
}

/// An embedded cover image crossing the IPC boundary: base64 data + MIME.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoverArtDto {
    pub mime: String,
    pub data_base64: String,
}

/// A planned cover-art change: `old` restored on undo, `new` embedded (or the
/// cover removed when `None`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoverChangeDto {
    pub old: Option<CoverArtDto>,
    pub new: Option<CoverArtDto>,
}

/// A planned change to one file: tag edits, a cover change, and/or a rename.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileChangeDto {
    pub path: String,
    pub rename_to: Option<String>,
    pub tag_changes: Vec<FieldChangeDto>,
    #[serde(default)]
    pub cover_change: Option<CoverChangeDto>,
}

/// A previewable plan, ready to render as a "current -> new" diff.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanDto {
    pub description: String,
    pub changes: Vec<FileChangeDto>,
}

/// One requested tag edit from the table: set `field` on `path` to `value`
/// (an empty/`None` value clears the field).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TagEditDto {
    pub path: String,
    /// Storage key (see [`TagField::to_storage_key`]).
    pub field: String,
    pub value: Option<String>,
}

/// Result of exporting embedded covers to disk: the image files written, and
/// the audio files skipped because they carried no embedded cover.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoverExportDto {
    pub written: Vec<String>,
    pub skipped_no_cover: Vec<String>,
}

/// A recorded batch, for the history/undo UI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BatchDto {
    pub id: i64,
    pub description: String,
    pub applied_at: i64,
}

/// What to search a provider for.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchQueryDto {
    pub artist: Option<String>,
    pub title: Option<String>,
    pub album: Option<String>,
    pub catalog_number: Option<String>,
}

/// A provider search hit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CandidateDto {
    pub id: String,
    pub artist: String,
    pub title: String,
    pub year: Option<u16>,
    pub score: f32,
    /// Small cover thumbnail URL (fetch bytes via [`App::fetch_discogs_image`]).
    pub thumb_url: Option<String>,
    /// Larger cover image URL, for a grid of tiles.
    pub cover_url: Option<String>,
    pub country: Option<String>,
    pub label: Option<String>,
    pub format: Option<String>,
    pub catalog_number: Option<String>,
}

/// One track of a fetched release.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseTrackDto {
    pub position: String,
    pub artist: Option<String>,
    pub title: String,
    /// Length the release lists for this track, in seconds, when it states one.
    pub duration_secs: Option<u64>,
}

/// A fully fetched release.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseDto {
    pub id: String,
    pub artist: String,
    pub title: String,
    pub year: Option<u16>,
    /// Broad Discogs genres (e.g. `Electronic`).
    pub genres: Vec<String>,
    /// Specific Discogs styles (e.g. `Trance`, `Tribal`, `Techno`) — what the
    /// import writes to the genre tag by preference (#26).
    pub styles: Vec<String>,
    pub tracks: Vec<ReleaseTrackDto>,
    /// URL of the release's primary image, if any. Fetch its bytes with
    /// [`App::fetch_discogs_image`] to preview or embed it.
    pub cover_image_url: Option<String>,
}

/// One release track the user chose to import, as sent back from the UI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportTrackDto {
    pub position: String,
    pub artist: String,
    pub title: String,
    /// Length from the release listing, used to corroborate a match (#64).
    #[serde(default)]
    pub duration_secs: Option<u64>,
}

/// A user-resolved import: the album-level fields plus the ordered list of
/// enabled release tracks to map onto the selected files (see
/// [`App::preview_import`]).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportSelectionDto {
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub year: Option<String>,
    pub genre: Option<String>,
    pub tracks: Vec<ImportTrackDto>,
}

/// One rule in a transformation chain, as the UI describes it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransformRuleDto {
    /// `replace`, `case` or `diacritics`.
    pub kind: String,
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub to: String,
    #[serde(default)]
    pub regex: bool,
    #[serde(default)]
    pub whole_word: bool,
    #[serde(default)]
    pub case_sensitive: bool,
    /// For `case`: `lower`, `upper`, `title` or `sentence`.
    #[serde(default)]
    pub style: String,
}

/// A tagging session rooted at one library directory. The root doubles as the
/// [`Executor`] `allowed_root`, so every write is confined to the opened
/// library.
pub struct App {
    library_root: PathBuf,
    journal: SqliteJournal,
}

impl App {
    /// Open a session for `library_root`, storing the undo journal at
    /// `journal_path` (typically inside the app's config dir).
    pub fn open(library_root: impl Into<PathBuf>, journal_path: &Path) -> Result<Self, AppError> {
        Ok(Self {
            library_root: library_root.into(),
            journal: SqliteJournal::open(journal_path)?,
        })
    }

    /// Scan the library and read each file's tags. Files that can't be read
    /// (unsupported, corrupt, or walk errors like a permission-denied dir) are
    /// skipped rather than failing the whole scan. Results are sorted by path
    /// so the table has a stable order (the scanner yields filesystem order,
    /// which isn't alphabetical) — this order is also what mapping-by-position
    /// (rename masks, release import) lines up against.
    pub fn list_tracks(&self) -> Vec<TrackDto> {
        let mut tracks: Vec<TrackDto> = scanner::scan(&self.library_root, &ScanOptions::default())
            .filter_map(Result::ok)
            .filter_map(|path| TagEngine::read(&path).ok())
            .map(TrackDto::from)
            .collect();
        tracks.sort_by(|a, b| a.path.cmp(&b.path));
        tracks
    }

    /// Build a rename plan from a mask over the given files, without writing.
    /// The mask renders each file's new stem; the original extension is kept.
    /// Files whose tags can't satisfy the mask, or whose name wouldn't change,
    /// are left out of the plan.
    pub fn preview_rename(
        &self,
        mask_pattern: &str,
        paths: &[PathBuf],
    ) -> Result<PlanDto, AppError> {
        let mask = Mask::parse(mask_pattern)?;
        let mut changes = Vec::new();
        for path in paths {
            let Ok(track) = TagEngine::read(path) else {
                continue;
            };
            let Ok(stem) = mask.render(&track.tags) else {
                continue;
            };
            let new_name = match path.extension().and_then(|ext| ext.to_str()) {
                Some(ext) => format!("{stem}.{ext}"),
                None => stem,
            };
            let target = path.with_file_name(new_name);
            if target == *path {
                continue;
            }
            changes.push(FileChangeDto {
                path: path.to_string_lossy().into_owned(),
                rename_to: Some(target.to_string_lossy().into_owned()),
                tag_changes: Vec::new(),
                cover_change: None,
            });
        }
        Ok(PlanDto {
            description: format!("Rename by mask: {mask_pattern}"),
            changes,
        })
    }

    /// Preview applying a transformation chain, without writing (#34).
    ///
    /// `scope` is either `filename` — rewriting the file's stem, extension
    /// untouched — or a tag storage key, or `tags` for every text field the file
    /// carries. Producing a normal [`PlanDto`] means transformations preview,
    /// apply and undo through exactly the same journaled path as every other
    /// change; nothing here writes.
    pub fn preview_transform(
        &self,
        paths: &[PathBuf],
        rules: &[TransformRuleDto],
        scope: &str,
    ) -> Result<PlanDto, AppError> {
        let chain = build_chain(rules)?;
        let mut changes = Vec::new();

        for path in paths {
            let Ok(track) = TagEngine::read(path) else {
                continue;
            };

            if scope == "filename" {
                let stem = path
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let renamed = chain.apply(&stem);
                if renamed == stem || renamed.trim().is_empty() {
                    continue;
                }
                let file_name = match path.extension().and_then(|ext| ext.to_str()) {
                    Some(ext) => format!("{renamed}.{ext}"),
                    None => renamed,
                };
                changes.push(FileChangeDto {
                    path: path.to_string_lossy().into_owned(),
                    rename_to: Some(
                        path.with_file_name(file_name)
                            .to_string_lossy()
                            .into_owned(),
                    ),
                    tag_changes: Vec::new(),
                    cover_change: None,
                });
                continue;
            }

            let mut tag_changes = Vec::new();
            for (field, value) in &track.tags {
                let key = field.to_storage_key();
                if scope != "tags" && scope != key {
                    continue;
                }
                let transformed = chain.apply(value);
                if transformed != *value {
                    tag_changes.push(FieldChangeDto::new(
                        key,
                        Some(value.clone()),
                        Some(transformed),
                    ));
                }
            }
            if !tag_changes.is_empty() {
                changes.push(FileChangeDto {
                    path: path.to_string_lossy().into_owned(),
                    rename_to: None,
                    tag_changes,
                    cover_change: None,
                });
            }
        }

        Ok(PlanDto {
            description: format!("Transform ({scope})"),
            changes,
        })
    }

    /// Build a plan that moves files into a folder structure rendered from a
    /// mask, without writing (#37).
    ///
    /// Unlike [`preview_rename`](Self::preview_rename), the mask may contain `/`
    /// to denote directories — `%albumartist%/%year% - %album%/%track% - %title%`
    /// — and the result is anchored at the library root. Tag *values* still have
    /// their separators stripped by the mask engine, so only literal slashes in
    /// the pattern create folders; a value can't inject one.
    pub fn preview_move(&self, mask_pattern: &str, paths: &[PathBuf]) -> Result<PlanDto, AppError> {
        let mask = Mask::parse(mask_pattern)?;
        let mut changes = Vec::new();
        for path in paths {
            let Ok(track) = TagEngine::read(path) else {
                continue;
            };
            let Ok(rendered) = mask.render(&track.tags) else {
                continue;
            };
            // Both separators are accepted so a pattern stays portable and one
            // written on another platform still describes folders rather than
            // becoming a literal character in a file name (#71).
            let mut components: Vec<&str> = rendered.split(['/', '\\']).collect();
            // An empty component (from an empty tag) or a `..` would produce a
            // nonsense or escaping path. The executor would refuse the latter
            // anyway; rejecting here keeps the preview honest about what will
            // actually happen.
            if components
                .iter()
                .any(|part| part.trim().is_empty() || *part == "..")
            {
                continue;
            }
            // Extension goes on the last component, which is the file name.
            let last = match path.extension().and_then(|ext| ext.to_str()) {
                Some(ext) => format!("{}.{ext}", components.pop().unwrap_or_default()),
                None => components.pop().unwrap_or_default().to_string(),
            };
            // Pushed one at a time so the platform supplies its own separator
            // instead of us embedding one in the string.
            let mut target = self.library_root.clone();
            for component in components {
                target.push(component);
            }
            target.push(last);
            if target == *path {
                continue;
            }
            changes.push(FileChangeDto {
                path: path.to_string_lossy().into_owned(),
                rename_to: Some(target.to_string_lossy().into_owned()),
                tag_changes: Vec::new(),
                cover_change: None,
            });
        }
        Ok(PlanDto {
            description: format!("Reorganize by mask: {mask_pattern}"),
            changes,
        })
    }

    /// Build a tag-edit plan from requested cell edits, without writing. Reads
    /// each file's current value as the change's `old` (so preview shows the
    /// real diff and the executor's staleness check is accurate) and drops
    /// no-op edits. An empty requested value clears the field.
    pub fn preview_tag_edits(&self, edits: &[TagEditDto]) -> Result<PlanDto, AppError> {
        // Group edits by file so each file is read once and becomes one change.
        let mut by_path: std::collections::BTreeMap<&str, Vec<&TagEditDto>> =
            std::collections::BTreeMap::new();
        for edit in edits {
            by_path.entry(&edit.path).or_default().push(edit);
        }

        let mut changes = Vec::new();
        for (path, group) in by_path {
            let track = TagEngine::read(Path::new(path))?;
            let mut tag_changes = Vec::new();
            for edit in group {
                let field = TagField::from_storage_key(&edit.field);
                let old = track.tags.get(&field).cloned();
                let new = edit.value.clone().filter(|value| !value.is_empty());
                if old != new {
                    tag_changes.push(FieldChangeDto::new(edit.field.clone(), old, new));
                }
            }
            if !tag_changes.is_empty() {
                changes.push(FileChangeDto {
                    path: path.to_string(),
                    rename_to: None,
                    tag_changes,
                    cover_change: None,
                });
            }
        }
        Ok(PlanDto {
            description: "Edit tags".to_string(),
            changes,
        })
    }

    /// Preview embedding `cover` as the front cover of each `paths` file,
    /// without writing. Reads each file's current cover as the change's `old`
    /// (for undo and staleness) and skips files that already have exactly this
    /// cover.
    pub fn preview_cover_embed(
        &self,
        paths: &[PathBuf],
        cover: &CoverArtDto,
    ) -> Result<PlanDto, AppError> {
        let new_art = cover_dto_to_art(cover);
        let mut changes = Vec::new();
        for path in paths {
            let old = TagEngine::read_cover(path)?;
            if old == new_art {
                continue; // already this exact cover
            }
            changes.push(FileChangeDto {
                path: path.to_string_lossy().into_owned(),
                rename_to: None,
                tag_changes: Vec::new(),
                cover_change: Some(CoverChangeDto {
                    old: old.as_ref().map(cover_art_to_dto),
                    new: Some(cover.clone()),
                }),
            });
        }
        Ok(PlanDto {
            description: "Embed cover art".to_string(),
            changes,
        })
    }

    /// Export the embedded front cover of each `paths` file to an image file
    /// next to it (`<basename>.<ext>`, the extension derived from the cover's
    /// MIME type — e.g. `cover.jpg`). Read-only for the audio files: this never
    /// goes through the [`Executor`], since it only reads embedded art and
    /// writes sidecar image files, so there is nothing to undo. Files with no
    /// embedded cover are reported in `skipped_no_cover` rather than failing the
    /// batch. Each target directory is the audio file's own, so writes stay
    /// within the opened library by construction; a target is still confined to
    /// the library root defensively.
    ///
    /// The sidecar name (`cover.jpg`) is per-directory, so selecting many tracks
    /// from one album folder yields a single file, not one write per track. The
    /// first selected file that resolves to a given target wins; later files
    /// resolving to the same path are not rewritten and don't inflate the count.
    pub fn export_cover(
        &self,
        paths: &[PathBuf],
        basename: &str,
    ) -> Result<CoverExportDto, AppError> {
        let root = std::fs::canonicalize(&self.library_root)?;
        let mut written = Vec::new();
        let mut skipped_no_cover = Vec::new();
        let mut seen_targets = std::collections::HashSet::new();
        for path in paths {
            match TagEngine::read_cover(path)? {
                Some(cover) => {
                    let ext = extension_for_mime(&cover.mime);
                    let target = path.with_file_name(format!("{basename}.{ext}"));
                    // Defensive containment: resolve the (existing) parent dir
                    // and require it inside the library root before writing.
                    let parent = target.parent().unwrap_or(Path::new("."));
                    let canonical_parent = std::fs::canonicalize(parent)?;
                    if !canonical_parent.starts_with(&root) {
                        return Err(AppError::OutsideRoot(target.to_string_lossy().into_owned()));
                    }
                    // Collapse duplicate targets: N tracks in one folder share a
                    // single `cover.jpg` rather than overwriting it N times.
                    let canonical_target = canonical_parent.join(target.file_name().unwrap());
                    if !seen_targets.insert(canonical_target) {
                        continue;
                    }
                    std::fs::write(&target, &cover.data)?;
                    written.push(target.to_string_lossy().into_owned());
                }
                None => skipped_no_cover.push(path.to_string_lossy().into_owned()),
            }
        }
        Ok(CoverExportDto {
            written,
            skipped_no_cover,
        })
    }

    /// Resolve an export target inside the opened library. The name must be a
    /// bare file name — no separators, no `..` — so an export can never be
    /// steered outside the library root.
    fn export_target(&self, file_name: &str) -> Result<PathBuf, AppError> {
        let name = file_name.trim();
        if name.is_empty() || name.contains('/') || name.contains('\\') || name.starts_with('.') {
            return Err(AppError::InvalidFileName(file_name.to_string()));
        }
        Ok(self.library_root.join(name))
    }

    /// Export `paths` as an extended M3U playlist written into the library root.
    /// Entry paths are relative to the playlist when the track sits inside the
    /// library (portable), absolute otherwise. Read-only for the audio files.
    pub fn export_playlist(&self, paths: &[PathBuf], file_name: &str) -> Result<String, AppError> {
        let target = self.export_target(file_name)?;
        let root = std::fs::canonicalize(&self.library_root)?;
        let entries: Vec<PlaylistTrack> = paths
            .iter()
            .filter_map(|path| {
                let track = TagEngine::read(path).ok()?;
                let duration = TagEngine::read_duration(path).unwrap_or_default();
                let display = std::fs::canonicalize(path)
                    .ok()
                    .and_then(|abs| {
                        abs.strip_prefix(&root)
                            .ok()
                            .map(|rel| rel.to_string_lossy().into_owned())
                    })
                    .unwrap_or_else(|| path.to_string_lossy().into_owned());
                let file_stem = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                Some(PlaylistTrack {
                    path: display,
                    artist: track
                        .tags
                        .get(&TagField::Artist)
                        .cloned()
                        .unwrap_or_default(),
                    // Fall back to the file name so an untagged entry still
                    // shows something useful in a player.
                    title: track
                        .tags
                        .get(&TagField::Title)
                        .cloned()
                        .unwrap_or(file_stem),
                    duration_secs: match duration.as_secs() {
                        0 => -1, // unknown
                        secs => secs as i64,
                    },
                })
            })
            .collect();
        std::fs::write(&target, export::m3u(&entries))?;
        Ok(target.to_string_lossy().into_owned())
    }

    /// Export the tag columns of `paths` as CSV into the library root.
    pub fn export_csv(&self, paths: &[PathBuf], file_name: &str) -> Result<String, AppError> {
        let target = self.export_target(file_name)?;
        let tracks = read_tracks(paths);
        std::fs::write(&target, export::csv(&tracks))?;
        Ok(target.to_string_lossy().into_owned())
    }

    /// Export a text report of `paths`, one mask-rendered line per track (same
    /// placeholders as rename masks), into the library root.
    pub fn export_report(
        &self,
        paths: &[PathBuf],
        mask_pattern: &str,
        file_name: &str,
    ) -> Result<String, AppError> {
        let target = self.export_target(file_name)?;
        let mask = Mask::parse(mask_pattern)?;
        let tracks = read_tracks(paths);
        std::fs::write(&target, export::report(&tracks, &mask))?;
        Ok(target.to_string_lossy().into_owned())
    }

    /// Apply a previewed plan to disk and record it for undo.
    pub fn apply(&mut self, plan: &PlanDto) -> Result<BatchDto, AppError> {
        let change_plan = plan.to_change_plan();
        let batch = Executor::apply(&change_plan, &mut self.journal, &self.library_root)?;
        Ok(BatchDto::from(&batch))
    }

    /// Roll back a previously applied batch.
    pub fn undo(&mut self, batch_id: i64) -> Result<(), AppError> {
        Executor::undo(&mut self.journal, BatchId(batch_id), &self.library_root)?;
        Ok(())
    }

    /// Recorded batches for the currently open library, newest first. The undo
    /// journal is shared across every library a user opens, so a batch from a
    /// previously opened library can linger in it; surfacing that here would let
    /// the UI offer an "undo" that then fails because the files live outside the
    /// current `allowed_root`. Filtering to batches whose paths sit under the
    /// open library keeps undo scoped to what the user is actually looking at.
    pub fn history(&self) -> Result<Vec<BatchDto>, AppError> {
        // A path belongs to the current library if it sits under the library
        // root in either its raw or canonicalized form. Checking both avoids
        // wrongly hiding a real batch when the two differ (e.g. a symlinked
        // path), while still filtering out batches from other libraries.
        let mut roots = vec![self.library_root.clone()];
        if let Ok(canon) = std::fs::canonicalize(&self.library_root) {
            if canon != self.library_root {
                roots.push(canon);
            }
        }
        let under_root = |path: &std::path::Path| roots.iter().any(|r| path.starts_with(r));
        Ok(self
            .journal
            .batches()?
            .iter()
            .filter(|batch| {
                batch.plan.changes.iter().all(|change| {
                    under_root(&change.path)
                        && change.rename_to.as_ref().is_none_or(|to| under_root(to))
                })
            })
            .map(BatchDto::from)
            .collect())
    }

    /// Search a metadata provider (Discogs) with the given personal token.
    ///
    /// Results are re-scored against the query text and re-sorted: the provider
    /// score is only "the API returned this one first", which is not evidence of
    /// a better match (#53).
    pub fn search_discogs(
        &self,
        token: &str,
        query: &SearchQueryDto,
    ) -> Result<Vec<CandidateDto>, AppError> {
        let provider = DiscogsProvider::new(token);
        let candidates = provider.search(&query.to_search_query())?;
        let mut results: Vec<CandidateDto> = candidates.iter().map(CandidateDto::from).collect();

        let wanted = [
            query.artist.as_deref(),
            query.album.as_deref(),
            query.title.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
        if !wanted.trim().is_empty() {
            for candidate in &mut results {
                let label = format!("{} {}", candidate.artist, candidate.title);
                candidate.score = matching::text_similarity(&wanted, &label);
            }
            results.sort_by(|a, b| b.score.total_cmp(&a.score));
        }
        Ok(results)
    }

    /// Fetch a full release from Discogs.
    pub fn fetch_discogs_release(&self, token: &str, id: &str) -> Result<ReleaseDto, AppError> {
        let provider = DiscogsProvider::new(token);
        let release = provider.fetch_release(&ReleaseId(id.to_string()))?;
        Ok(ReleaseDto::from(&release))
    }

    /// Download a Discogs image (e.g. a release's cover) and return it as a
    /// cover DTO, ready to feed straight into [`App::preview_cover_embed`] — the
    /// same shape a locally chosen file produces, so the fetched art flows
    /// through the identical preview/apply/undo path.
    pub fn fetch_discogs_image(&self, token: &str, url: &str) -> Result<CoverArtDto, AppError> {
        let provider = DiscogsProvider::new(token);
        let image = provider.fetch_image(url)?;
        Ok(CoverArtDto {
            mime: image.mime,
            data_base64: base64::engine::general_purpose::STANDARD.encode(&image.data),
        })
    }

    /// Align the selected files to a release's tracks by content rather than by
    /// position (#53).
    ///
    /// Returns, for each file, the index of the release track it matches (or
    /// `None`). Blind positional mapping is what silently tags a whole album
    /// one title out of step when the file order doesn't happen to match the
    /// release; this lets the UI line them up from the actual metadata instead.
    /// Untagged files fall back to their file name, which is usually where the
    /// information hides in a messy library.
    pub fn auto_align(
        &self,
        paths: &[PathBuf],
        tracks: &[ImportTrackDto],
    ) -> Result<Vec<Option<usize>>, AppError> {
        let locals: Vec<(String, String, Option<u64>)> = paths
            .iter()
            .map(|path| {
                let track = TagEngine::read(path).ok();
                let title = track
                    .as_ref()
                    .and_then(|track| track.tags.get(&TagField::Title).cloned())
                    .filter(|title| !title.is_empty())
                    .unwrap_or_else(|| {
                        path.file_stem()
                            .map(|stem| stem.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    });
                let artist = track
                    .as_ref()
                    .and_then(|track| track.tags.get(&TagField::Artist).cloned())
                    .unwrap_or_default();
                let duration = TagEngine::read_duration(path)
                    .ok()
                    .map(|duration| duration.as_secs())
                    .filter(|secs| *secs > 0);
                (title, artist, duration)
            })
            .collect();

        let local_refs: Vec<TrackRef> = locals
            .iter()
            .map(|(title, artist, duration)| TrackRef {
                title,
                artist: Some(artist.as_str()).filter(|artist| !artist.is_empty()),
                duration_secs: *duration,
            })
            .collect();
        let candidate_refs: Vec<TrackRef> = tracks
            .iter()
            .map(|track| TrackRef {
                title: &track.title,
                artist: Some(track.artist.as_str()).filter(|artist| !artist.is_empty()),
                duration_secs: track.duration_secs,
            })
            .collect();

        let by_content = matching::align(&local_refs, &candidate_refs, &MatchOptions::default());
        let content_hits = by_content.iter().flatten().count();

        // If titles only carried us part of the way, the folder probably has
        // none worth matching (`track01.mp3` and friends). The ordered vector of
        // lengths needs no titles at all, so try it and keep whichever aligned
        // more files (#64).
        let reachable = local_refs.len().min(candidate_refs.len());
        if content_hits * 2 < reachable {
            let by_duration = matching::align_by_duration_sequence(
                &local_refs,
                &candidate_refs,
                matching::DURATION_SEQUENCE_TOLERANCE_SECS,
            );
            if by_duration.iter().flatten().count() > content_hits {
                return Ok(by_duration);
            }
        }
        Ok(by_content)
    }

    /// Preview importing a user-resolved release selection onto `paths`,
    /// without writing. The frontend decides the mapping (TagScanner-style):
    /// the user toggles which release tracks participate and orders the files
    /// to match, so here the i-th enabled track simply maps onto the i-th
    /// file. Album-level fields go to every file; per-track fields
    /// (title/artist/track number) to files that line up with a selected
    /// track. The track number comes from the release track's own position,
    /// not the selection index, so an aligned file keeps its real number.
    /// Reads current values for `old` and drops no-op edits, flowing through
    /// the same preview/apply/undo path as manual edits.
    pub fn preview_import(
        &self,
        paths: &[PathBuf],
        selection: &ImportSelectionDto,
    ) -> Result<PlanDto, AppError> {
        let mut changes = Vec::new();
        for (index, path) in paths.iter().enumerate() {
            let current = TagEngine::read(path)?;

            // (field, desired new value) — album-level first, then per-track.
            let mut desired: Vec<(TagField, Option<String>)> = vec![
                (TagField::Album, non_empty(selection.album.clone())),
                (
                    TagField::AlbumArtist,
                    non_empty(selection.album_artist.clone()),
                ),
                (TagField::Year, non_empty(selection.year.clone())),
                (TagField::Genre, non_empty(selection.genre.clone())),
            ];
            if let Some(track) = selection.tracks.get(index) {
                let artist = non_empty(Some(track.artist.clone()))
                    .or_else(|| non_empty(selection.album_artist.clone()));
                desired.push((TagField::Title, non_empty(Some(track.title.clone()))));
                desired.push((TagField::Artist, artist));

                // Track number from the release position, but leave the file's
                // existing number alone if it already means the same thing --
                // so an aligned "01" isn't reformatted to "1". Compare
                // numerically (both normalized) and only change on a real
                // difference.
                let position_number = track_number_from_position(&track.position)
                    .unwrap_or_else(|| (index + 1).to_string());
                let current_number = current
                    .tags
                    .get(&TagField::TrackNumber)
                    .and_then(|value| track_number_from_position(value));
                if current_number.as_deref() != Some(position_number.as_str()) {
                    desired.push((TagField::TrackNumber, Some(position_number)));
                }
            }

            let mut tag_changes = Vec::new();
            for (field, new) in desired {
                let new = new.filter(|value| !value.is_empty());
                let old = current.tags.get(&field).cloned();
                if new.is_some() && old != new {
                    tag_changes.push(FieldChangeDto::new(field.to_storage_key(), old, new));
                }
            }
            if !tag_changes.is_empty() {
                changes.push(FileChangeDto {
                    path: path.to_string_lossy().into_owned(),
                    rename_to: None,
                    tag_changes,
                    cover_change: None,
                });
            }
        }
        Ok(PlanDto {
            description: "Import Discogs release".to_string(),
            changes,
        })
    }
}

/// Extract a track number from a Discogs position: take the *trailing* run of
/// digits, so "5" -> 5, "A1" -> 1, "1-05" -> 5, "12" -> 12. Returns `None` for
/// positions with no trailing digits (e.g. a heading), letting the caller fall
/// back to the selection index.
fn track_number_from_position(position: &str) -> Option<String> {
    let digits: String = position
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    // Normalize leading zeros ("05" -> "5") via a round-trip through u32.
    digits.parse::<u32>().ok().map(|n| n.to_string())
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.is_empty())
}

/// Turn the UI's rule list into a transform chain, rejecting a malformed rule
/// rather than silently dropping it — a rule that quietly does nothing is worse
/// than an error, because the preview would look like a no-op.
fn build_chain(rules: &[TransformRuleDto]) -> Result<TransformChain, AppError> {
    let mut chain = TransformChain::default();
    for rule in rules {
        match rule.kind.as_str() {
            "replace" => chain.push(Box::new(Replace::new(
                &rule.from,
                &rule.to,
                ReplaceOptions {
                    regex: rule.regex,
                    whole_word: rule.whole_word,
                    case_sensitive: rule.case_sensitive,
                },
            )?)),
            "case" => {
                let style = match rule.style.as_str() {
                    "lower" => CaseStyle::Lower,
                    "upper" => CaseStyle::Upper,
                    "title" => CaseStyle::Title,
                    "sentence" => CaseStyle::Sentence,
                    other => return Err(AppError::UnknownTransform(other.to_string())),
                };
                chain.push(Box::new(ChangeCase::new(style)));
            }
            "diacritics" => chain.push(Box::new(RemoveDiacritics)),
            other => return Err(AppError::UnknownTransform(other.to_string())),
        }
    }
    Ok(chain)
}

/// Read the given files, skipping any that can't be parsed — an export should
/// cover what it can rather than failing wholesale on one bad file.
fn read_tracks(paths: &[PathBuf]) -> Vec<tagrex_core::model::TrackFile> {
    paths
        .iter()
        .filter_map(|path| TagEngine::read(path).ok())
        .collect()
}

/// A file extension for an embedded cover's MIME type. Known image types map to
/// their conventional extension; anything else falls back to the MIME subtype
/// when it's a clean alphanumeric token, else `jpg` (the overwhelmingly common
/// cover format).
fn extension_for_mime(mime: &str) -> String {
    match mime.trim().to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => "jpg".to_string(),
        "image/png" => "png".to_string(),
        "image/gif" => "gif".to_string(),
        "image/webp" => "webp".to_string(),
        "image/bmp" => "bmp".to_string(),
        "image/tiff" | "image/tif" => "tiff".to_string(),
        other => other
            .strip_prefix("image/")
            .filter(|sub| !sub.is_empty() && sub.chars().all(|c| c.is_ascii_alphanumeric()))
            .unwrap_or("jpg")
            .to_string(),
    }
}

fn cover_dto_to_art(dto: &CoverArtDto) -> Option<CoverArt> {
    let data = base64::engine::general_purpose::STANDARD
        .decode(dto.data_base64.as_bytes())
        .ok()?;
    Some(CoverArt {
        mime: dto.mime.clone(),
        data,
    })
}

fn cover_art_to_dto(art: &CoverArt) -> CoverArtDto {
    CoverArtDto {
        mime: art.mime.clone(),
        data_base64: base64::engine::general_purpose::STANDARD.encode(&art.data),
    }
}

impl From<tagrex_core::model::TrackFile> for TrackDto {
    fn from(track: tagrex_core::model::TrackFile) -> Self {
        Self {
            path: track.path.to_string_lossy().into_owned(),
            format: format!("{:?}", track.format),
            tags: track
                .tags
                .into_iter()
                .map(|(field, value)| (field.to_storage_key(), value))
                .collect(),
        }
    }
}

impl PlanDto {
    fn to_change_plan(&self) -> ChangePlan {
        ChangePlan {
            description: self.description.clone(),
            changes: self
                .changes
                .iter()
                .map(|change| FileChange {
                    path: PathBuf::from(&change.path),
                    rename_to: change.rename_to.as_ref().map(PathBuf::from),
                    tag_changes: change
                        .tag_changes
                        .iter()
                        // A rejected value is display-only; never write it, so
                        // the field keeps its current on-disk value.
                        .filter(|field_change| !field_change.invalid)
                        .map(|field_change| FieldChange {
                            field: TagField::from_storage_key(&field_change.field),
                            old: field_change.old.clone(),
                            new: field_change.new.clone(),
                        })
                        .collect(),
                    cover_change: change.cover_change.as_ref().map(|c| CoverChange {
                        old: c.old.as_ref().and_then(cover_dto_to_art),
                        new: c.new.as_ref().and_then(cover_dto_to_art),
                    }),
                })
                .collect(),
        }
    }
}

impl From<&tagrex_core::journal::AppliedBatch> for BatchDto {
    fn from(batch: &tagrex_core::journal::AppliedBatch) -> Self {
        Self {
            id: batch.id.0,
            description: batch.description.clone(),
            applied_at: batch.applied_at,
        }
    }
}

impl SearchQueryDto {
    fn to_search_query(&self) -> SearchQuery {
        SearchQuery {
            artist: self.artist.clone(),
            title: self.title.clone(),
            album: self.album.clone(),
            catalog_number: self.catalog_number.clone(),
        }
    }
}

impl From<&tagrex_core::provider::ReleaseCandidate> for CandidateDto {
    fn from(candidate: &tagrex_core::provider::ReleaseCandidate) -> Self {
        Self {
            id: candidate.id.0.clone(),
            artist: candidate.artist.clone(),
            title: candidate.title.clone(),
            year: candidate.year,
            score: candidate.score,
            thumb_url: candidate.thumb_url.clone(),
            cover_url: candidate.cover_url.clone(),
            country: candidate.country.clone(),
            label: candidate.label.clone(),
            format: candidate.format.clone(),
            catalog_number: candidate.catalog_number.clone(),
        }
    }
}

impl From<&tagrex_core::provider::Release> for ReleaseDto {
    fn from(release: &tagrex_core::provider::Release) -> Self {
        Self {
            id: release.id.0.clone(),
            artist: release.artist.clone(),
            title: release.title.clone(),
            year: release.year,
            genres: release.genres.clone(),
            styles: release.styles.clone(),
            tracks: release
                .tracks
                .iter()
                .map(|track| ReleaseTrackDto {
                    position: track.position.clone(),
                    artist: track.artist.clone(),
                    title: track.title.clone(),
                    duration_secs: track.duration_secs,
                })
                .collect(),
            cover_image_url: release.cover_image_url.clone(),
        }
    }
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    TagIo(#[from] tagrex_core::model::TagIoError),
    #[error(transparent)]
    Mask(#[from] tagrex_core::mask::MaskError),
    #[error(transparent)]
    Plan(#[from] tagrex_core::plan::PlanError),
    #[error(transparent)]
    Journal(#[from] tagrex_core::journal::JournalError),
    #[error(transparent)]
    Provider(#[from] tagrex_core::provider::ProviderError),
    #[error("path resolves outside the opened library: {0}")]
    OutsideRoot(String),
    #[error("invalid export file name: {0}")]
    InvalidFileName(String),
    #[error("unknown transformation: {0}")]
    UnknownTransform(String),
    #[error(transparent)]
    Transform(#[from] tagrex_core::transform::TransformError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_FLAC: [u8; 62] = [
        0x66, 0x4c, 0x61, 0x43, 0x00, 0x00, 0x00, 0x22, 0x10, 0x00, 0x10, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x0a, 0xc4, 0x42, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x81, 0x00, 0x00,
        0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00,
    ];

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "tagrex-app-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        /// Write a minimal FLAC with the given artist/title set.
        fn tagged_flac(&self, name: &str, artist: &str, title: &str) -> PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, MINIMAL_FLAC).unwrap();
            let mut tags = std::collections::BTreeMap::new();
            tags.insert(TagField::Artist, artist.to_string());
            tags.insert(TagField::Title, title.to_string());
            TagEngine::write(&tagrex_core::model::TrackFile {
                path: path.clone(),
                format: tagrex_core::model::AudioFormat::Flac,
                tags,
            })
            .unwrap();
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn open_app(dir: &TempDir) -> App {
        App::open(dir.0.clone(), &dir.0.join("journal.sqlite")).unwrap()
    }

    #[test]
    fn lists_tagged_tracks() {
        let dir = TempDir::new("list");
        dir.tagged_flac("x.flac", "Boards of Canada", "Roygbiv");
        let app = open_app(&dir);

        let tracks = app.list_tracks();
        assert_eq!(tracks.len(), 1);
        assert_eq!(
            tracks[0].tags.get("artist").map(String::as_str),
            Some("Boards of Canada")
        );
        assert_eq!(tracks[0].format, "Flac");
    }

    #[test]
    fn preview_apply_undo_rename_round_trip() {
        let dir = TempDir::new("rename");
        let track = dir.tagged_flac("original.flac", "Boards of Canada", "Roygbiv");
        let mut app = open_app(&dir);

        let plan = app
            .preview_rename("%artist% - %title%", std::slice::from_ref(&track))
            .unwrap();
        assert_eq!(plan.changes.len(), 1);
        let expected = dir.0.join("Boards of Canada - Roygbiv.flac");
        assert_eq!(
            plan.changes[0].rename_to.as_deref(),
            Some(expected.to_string_lossy().as_ref())
        );

        let batch = app.apply(&plan).unwrap();
        assert!(expected.exists());
        assert!(!track.exists());

        // History shows the batch; undo puts the file back.
        assert_eq!(app.history().unwrap().len(), 1);
        app.undo(batch.id).unwrap();
        assert!(track.exists());
        assert!(!expected.exists());
        assert!(app.history().unwrap().is_empty());
    }

    #[test]
    fn edit_tags_preview_apply_undo_round_trip() {
        let dir = TempDir::new("edit");
        let track = dir.tagged_flac("x.flac", "Old Artist", "Title");
        let mut app = open_app(&dir);

        let path = track.to_string_lossy().into_owned();
        let edits = vec![
            TagEditDto {
                path: path.clone(),
                field: "artist".into(),
                value: Some("New Artist".into()),
            },
            // No-op (same value) — must be dropped from the plan.
            TagEditDto {
                path: path.clone(),
                field: "title".into(),
                value: Some("Title".into()),
            },
        ];
        let plan = app.preview_tag_edits(&edits).unwrap();
        assert_eq!(plan.changes.len(), 1);
        assert_eq!(plan.changes[0].tag_changes.len(), 1);
        assert_eq!(plan.changes[0].tag_changes[0].field, "artist");
        assert_eq!(
            plan.changes[0].tag_changes[0].old.as_deref(),
            Some("Old Artist")
        );
        assert_eq!(
            plan.changes[0].tag_changes[0].new.as_deref(),
            Some("New Artist")
        );

        let batch = app.apply(&plan).unwrap();
        assert_eq!(
            TagEngine::read(&track)
                .unwrap()
                .tags
                .get(&TagField::Artist)
                .map(String::as_str),
            Some("New Artist")
        );

        app.undo(batch.id).unwrap();
        assert_eq!(
            TagEngine::read(&track)
                .unwrap()
                .tags
                .get(&TagField::Artist)
                .map(String::as_str),
            Some("Old Artist")
        );
    }

    #[test]
    fn invalid_tag_value_is_flagged_in_preview_and_skipped_on_apply() {
        let dir = TempDir::new("invalid");
        let track = dir.tagged_flac("x.flac", "Artist", "Title");
        let mut app = open_app(&dir);

        let path = track.to_string_lossy().into_owned();
        let edits = vec![
            // A non-numeric year — must be flagged, not written.
            TagEditDto {
                path: path.clone(),
                field: "year".into(),
                value: Some("19x6".into()),
            },
            // A valid change alongside it — must still apply.
            TagEditDto {
                path: path.clone(),
                field: "album".into(),
                value: Some("New Album".into()),
            },
        ];
        let plan = app.preview_tag_edits(&edits).unwrap();
        let by_field = |c: &FileChangeDto, f: &str| {
            c.tag_changes
                .iter()
                .find(|fc| fc.field == f)
                .cloned()
                .unwrap()
        };
        let year = by_field(&plan.changes[0], "year");
        let album = by_field(&plan.changes[0], "album");
        // Rejected value is present in the preview (so the cell can show it) but
        // flagged; the valid change is not.
        assert!(year.invalid);
        assert_eq!(year.new.as_deref(), Some("19x6"));
        assert!(!album.invalid);

        app.apply(&plan).unwrap();
        let tags = TagEngine::read(&track).unwrap().tags;
        // The valid change landed; the invalid year was never written.
        assert_eq!(
            tags.get(&TagField::Album).map(String::as_str),
            Some("New Album")
        );
        assert_eq!(tags.get(&TagField::Year), None);
    }

    #[test]
    fn field_value_invalid_rejects_bad_typed_values() {
        // Year: exactly 4 digits (optionally a date suffix); anything else fails.
        assert!(!field_value_invalid("year", Some("1996")));
        assert!(!field_value_invalid("year", Some("1996-05-01")));
        assert!(!field_value_invalid("year", None));
        assert!(!field_value_invalid("year", Some(""))); // clearing is valid
        assert!(field_value_invalid("year", Some("19x6")));
        assert!(field_value_invalid("year", Some("MCMXCVI")));
        assert!(field_value_invalid("year", Some("222"))); // short year poisons the file
        assert!(field_value_invalid("year", Some("96")));
        assert!(field_value_invalid("year", Some("12345")));

        // Track / disc / total: a plain integer (a non-numeric one is dropped
        // by the writer).
        assert!(!field_value_invalid("track", Some("7")));
        assert!(!field_value_invalid("disc", Some("2")));
        assert!(!field_value_invalid("tracktotal", Some("12")));
        assert!(field_value_invalid("track", Some("A1"))); // vinyl-style — dropped
        assert!(field_value_invalid("track", Some("7/12"))); // pair belongs in two fields
        assert!(field_value_invalid("disc", Some("one")));

        // BPM: numeric, integer or decimal (DJ tools store fractional BPM).
        assert!(!field_value_invalid("bpm", Some("128")));
        assert!(!field_value_invalid("bpm", Some("128.5")));
        assert!(field_value_invalid("bpm", Some("fast")));
        assert!(field_value_invalid("bpm", Some("128bpm")));

        // Free-text fields accept anything.
        assert!(!field_value_invalid("artist", Some("19x6")));
        assert!(!field_value_invalid("title", Some("A1")));
        assert!(!field_value_invalid("comment", Some("anything at all")));
    }

    #[test]
    fn preview_import_maps_selected_tracks_onto_files() {
        let dir = TempDir::new("import");
        let a = dir.tagged_flac("a.flac", "Old A", "Old Title A");
        let b = dir.tagged_flac("b.flac", "Old B", "Old Title B");
        let app = open_app(&dir);

        // User kept two release tracks whose positions are 1 and 5 (a subset),
        // aligned onto the two files in order.
        let selection = ImportSelectionDto {
            album: Some("Some Compilation".into()),
            album_artist: Some("Various".into()),
            year: Some("1996".into()),
            genre: Some("House".into()),
            tracks: vec![
                ImportTrackDto {
                    position: "1".into(),
                    artist: String::new(),
                    title: "First".into(),
                    duration_secs: None,
                },
                ImportTrackDto {
                    position: "5".into(),
                    artist: "Guest".into(),
                    title: "Fifth".into(),
                    duration_secs: None,
                },
            ],
        };

        let plan = app.preview_import(&[a, b], &selection).unwrap();
        assert_eq!(plan.changes.len(), 2);

        let fields = |c: &FileChangeDto| {
            c.tag_changes
                .iter()
                .map(|fc| (fc.field.clone(), fc.new.clone().unwrap()))
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        let first = fields(&plan.changes[0]);
        assert_eq!(
            first.get("album").map(String::as_str),
            Some("Some Compilation")
        );
        assert_eq!(
            first.get("albumartist").map(String::as_str),
            Some("Various")
        );
        assert_eq!(first.get("year").map(String::as_str), Some("1996"));
        assert_eq!(first.get("genre").map(String::as_str), Some("House"));
        assert_eq!(first.get("title").map(String::as_str), Some("First"));
        // No track artist -> falls back to the album artist.
        assert_eq!(first.get("artist").map(String::as_str), Some("Various"));
        // Track number comes from the release position (1), not the index.
        assert_eq!(first.get("track").map(String::as_str), Some("1"));

        let second = fields(&plan.changes[1]);
        assert_eq!(second.get("artist").map(String::as_str), Some("Guest"));
        // Position 5, not selection index 2.
        assert_eq!(second.get("track").map(String::as_str), Some("5"));
    }

    #[test]
    fn import_leaves_a_matching_track_number_untouched() {
        let dir = TempDir::new("import-track");
        let path = dir.tagged_flac("t.flac", "Artist", "Title");
        // Give the file a zero-padded track number.
        let mut track = TagEngine::read(&path).unwrap();
        track.tags.insert(TagField::TrackNumber, "05".into());
        TagEngine::write(&track).unwrap();
        let app = open_app(&dir);

        let selection = ImportSelectionDto {
            album: Some("Album".into()),
            tracks: vec![ImportTrackDto {
                position: "5".into(),
                artist: "Artist".into(),
                title: "Title".into(),
                duration_secs: None,
            }],
            ..ImportSelectionDto::default()
        };
        let plan = app.preview_import(&[path], &selection).unwrap();
        let changed_fields: Vec<&str> = plan.changes[0]
            .tag_changes
            .iter()
            .map(|fc| fc.field.as_str())
            .collect();
        // Album changes; the track number ("05" vs position "5") must NOT,
        // since they mean the same number.
        assert!(changed_fields.contains(&"album"));
        assert!(!changed_fields.contains(&"track"));
    }

    #[test]
    fn track_number_parsing_handles_vinyl_and_padding() {
        assert_eq!(track_number_from_position("5").as_deref(), Some("5"));
        assert_eq!(track_number_from_position("A1").as_deref(), Some("1"));
        assert_eq!(track_number_from_position("1-05").as_deref(), Some("5"));
        assert_eq!(track_number_from_position("12").as_deref(), Some("12"));
        assert_eq!(track_number_from_position(""), None);
    }

    fn replace_rule(from: &str, to: &str) -> TransformRuleDto {
        TransformRuleDto {
            kind: "replace".into(),
            from: from.into(),
            to: to.into(),
            regex: false,
            whole_word: false,
            case_sensitive: false,
            style: String::new(),
        }
    }

    fn case_rule(style: &str) -> TransformRuleDto {
        TransformRuleDto {
            kind: "case".into(),
            style: style.into(),
            ..replace_rule("", "")
        }
    }

    #[test]
    fn preview_transform_rewrites_tags_and_skips_unchanged_ones() {
        let dir = TempDir::new("transform-tags");
        let track = dir.tagged_flac("x.flac", "the_x_factor", "desert_rain");
        let app = open_app(&dir);

        let rules = vec![replace_rule("_", " "), case_rule("title")];
        let plan = app
            .preview_transform(std::slice::from_ref(&track), &rules, "tags")
            .unwrap();

        let changed: std::collections::BTreeMap<_, _> = plan.changes[0]
            .tag_changes
            .iter()
            .map(|c| (c.field.clone(), c.new.clone().unwrap()))
            .collect();
        assert_eq!(
            changed.get("artist").map(String::as_str),
            Some("The X Factor")
        );
        assert_eq!(
            changed.get("title").map(String::as_str),
            Some("Desert Rain")
        );
    }

    #[test]
    fn preview_transform_can_target_one_field_or_the_filename() {
        let dir = TempDir::new("transform-scope");
        let track = dir.tagged_flac("the_x_factor_-_desert_rain.flac", "a_b", "c_d");
        let app = open_app(&dir);
        let rules = vec![replace_rule("_", " ")];

        // A single field: the others are left alone.
        let one = app
            .preview_transform(std::slice::from_ref(&track), &rules, "artist")
            .unwrap();
        assert_eq!(one.changes[0].tag_changes.len(), 1);
        assert_eq!(one.changes[0].tag_changes[0].field, "artist");

        // The filename scope renames instead, keeping the extension.
        let renamed = app
            .preview_transform(std::slice::from_ref(&track), &rules, "filename")
            .unwrap();
        assert_eq!(
            renamed.changes[0].rename_to.as_deref(),
            Some(
                dir.0
                    .join("the x factor - desert rain.flac")
                    .to_string_lossy()
                    .as_ref()
            )
        );
        assert!(renamed.changes[0].tag_changes.is_empty());
    }

    #[test]
    fn preview_transform_reports_a_bad_rule_instead_of_ignoring_it() {
        let dir = TempDir::new("transform-bad");
        let track = dir.tagged_flac("x.flac", "Artist", "Title");
        let app = open_app(&dir);

        // A rule that silently did nothing would show an empty preview and look
        // like "no changes needed", which is the wrong story to tell.
        let unknown = vec![TransformRuleDto {
            kind: "nonsense".into(),
            ..replace_rule("a", "b")
        }];
        assert!(matches!(
            app.preview_transform(std::slice::from_ref(&track), &unknown, "tags"),
            Err(AppError::UnknownTransform(_))
        ));

        let bad_regex = vec![TransformRuleDto {
            regex: true,
            ..replace_rule("(unclosed", "x")
        }];
        assert!(matches!(
            app.preview_transform(std::slice::from_ref(&track), &bad_regex, "tags"),
            Err(AppError::Transform(_))
        ));
    }

    #[test]
    fn preview_move_builds_folder_paths_under_the_library() {
        let dir = TempDir::new("move");
        let track = dir.tagged_flac("x.flac", "Plastic", "Sexy Groove");
        let mut file = TagEngine::read(&track).unwrap();
        file.tags.insert(TagField::Album, "La Bush".into());
        file.tags.insert(TagField::Year, "1996".into());
        TagEngine::write(&file).unwrap();
        let app = open_app(&dir);

        let plan = app
            .preview_move(
                "%year% - %album%/%artist% - %title%",
                std::slice::from_ref(&track),
            )
            .unwrap();
        assert_eq!(plan.changes.len(), 1);
        assert_eq!(
            plan.changes[0].rename_to.as_deref(),
            Some(
                dir.0
                    .join("1996 - La Bush/Plastic - Sexy Groove.flac")
                    .to_string_lossy()
                    .as_ref()
            )
        );
    }

    #[test]
    fn preview_move_handles_multi_disc_pattern_with_adjacent_placeholders() {
        let dir = TempDir::new("move-multidisc");
        let track = dir.tagged_flac("x.flac", "The X Factor", "Desert Rain");
        let mut file = TagEngine::read(&track).unwrap();
        file.tags.insert(TagField::Album, "La Bush".into());
        file.tags.insert(TagField::AlbumArtist, "Various".into());
        file.tags.insert(TagField::Year, "1996".into());
        file.tags.insert(TagField::DiscNumber, "1".into());
        file.tags.insert(TagField::TrackNumber, "1".into());
        TagEngine::write(&file).unwrap();
        let app = open_app(&dir);

        // `%disc%%track%` has no separator between the placeholders: fine to
        // render, and the track pads to two digits so disc 1 track 1 reads as
        // 101 rather than 11.
        let plan = app
            .preview_move(
                "%albumartist% - %album% (%year%)/%disc%%track%. %artist% - %title%",
                std::slice::from_ref(&track),
            )
            .unwrap();
        assert_eq!(plan.changes.len(), 1);
        assert_eq!(
            plan.changes[0].rename_to.as_deref(),
            Some(
                dir.0
                    .join("Various - La Bush (1996)/101. The X Factor - Desert Rain.flac")
                    .to_string_lossy()
                    .as_ref()
            )
        );
    }

    #[test]
    fn preview_move_accepts_either_folder_separator() {
        let dir = TempDir::new("move-sep");
        let track = dir.tagged_flac("x.flac", "Plastic", "Sexy Groove");
        let mut file = TagEngine::read(&track).unwrap();
        file.tags.insert(TagField::Album, "La Bush".into());
        TagEngine::write(&file).unwrap();
        let app = open_app(&dir);

        // A backslash pattern (natural on Windows, and what an imported config
        // carries) must describe folders, not become part of a file name.
        let expected = dir.0.join("La Bush/Plastic - Sexy Groove.flac");
        for pattern in ["%album%/%artist% - %title%", "%album%\\%artist% - %title%"] {
            let plan = app
                .preview_move(pattern, std::slice::from_ref(&track))
                .unwrap();
            assert_eq!(plan.changes.len(), 1, "pattern {pattern:?}");
            assert_eq!(
                plan.changes[0].rename_to.as_deref(),
                Some(expected.to_string_lossy().as_ref()),
                "pattern {pattern:?}"
            );
        }
    }

    #[test]
    fn preview_move_refuses_escaping_and_empty_components() {
        let dir = TempDir::new("move-guard");
        let track = dir.tagged_flac("x.flac", "Plastic", "Sexy Groove");
        let app = open_app(&dir);

        // `%album%` is unset here, so the folder component would be empty.
        let empty = app
            .preview_move("%album%/%title%", std::slice::from_ref(&track))
            .unwrap();
        assert!(
            empty.changes.is_empty(),
            "empty folder component is skipped"
        );

        // A literal `..` in the pattern must never produce a plan, with either
        // separator.
        for pattern in ["../%title%", "..\\%title%"] {
            let escaping = app
                .preview_move(pattern, std::slice::from_ref(&track))
                .unwrap();
            assert!(
                escaping.changes.is_empty(),
                "climbing out refused: {pattern:?}"
            );
        }
    }

    #[test]
    fn preview_skips_files_missing_mask_tags() {
        let dir = TempDir::new("skip");
        // Has artist+title, but the mask needs album.
        let track = dir.tagged_flac("x.flac", "Artist", "Title");
        let app = open_app(&dir);

        let plan = app.preview_rename("%album% - %title%", &[track]).unwrap();
        assert!(plan.changes.is_empty());
    }

    #[test]
    fn export_cover_writes_sidecar_and_skips_files_without_cover() {
        let dir = TempDir::new("export");
        let with_cover = dir.tagged_flac("a.flac", "Artist", "Has Cover");
        let without_cover = dir.tagged_flac("b.flac", "Artist", "No Cover");
        // Embed a distinctively-typed cover into the first file only.
        let art = CoverArt {
            mime: "image/png".to_string(),
            data: vec![1, 2, 3, 4, 5],
        };
        TagEngine::embed_cover(&with_cover, &art).unwrap();
        let app = open_app(&dir);

        let result = app
            .export_cover(&[with_cover.clone(), without_cover.clone()], "cover")
            .unwrap();

        // The file with a cover produced `cover.png` next to it, byte-for-byte.
        assert_eq!(result.written.len(), 1);
        let expected = dir.0.join("cover.png");
        assert_eq!(result.written[0], expected.to_string_lossy());
        assert_eq!(std::fs::read(&expected).unwrap(), art.data);
        // The audio files themselves were not modified (read-only, no journal).
        assert!(app.history().unwrap().is_empty());
        // The cover-less file is reported as skipped, not an error.
        assert_eq!(
            result.skipped_no_cover,
            vec![without_cover.to_string_lossy()]
        );
    }

    #[test]
    fn export_cover_collapses_same_folder_targets_to_one_file() {
        let dir = TempDir::new("export-dedup");
        let a = dir.tagged_flac("a.flac", "Artist", "A");
        let b = dir.tagged_flac("b.flac", "Artist", "B");
        let art = CoverArt {
            mime: "image/jpeg".to_string(),
            data: vec![9, 8, 7],
        };
        TagEngine::embed_cover(&a, &art).unwrap();
        TagEngine::embed_cover(&b, &art).unwrap();
        let app = open_app(&dir);

        // Both files sit in the same folder, so both resolve to the same
        // `cover.jpg`: exactly one file is written, not two.
        let result = app.export_cover(&[a, b], "cover").unwrap();
        assert_eq!(result.written.len(), 1);
        assert_eq!(result.written[0], dir.0.join("cover.jpg").to_string_lossy());
        assert!(result.skipped_no_cover.is_empty());
    }

    #[test]
    fn exports_playlist_csv_and_report_into_the_library() {
        let dir = TempDir::new("export-files");
        let a = dir.tagged_flac("a.flac", "Plastic", "Sexy Groove");
        let b = dir.tagged_flac("b.flac", "B.B.E.", "Seven Days");
        let app = open_app(&dir);
        let paths = [a, b];

        // Playlist: entries are relative to the library root (portable).
        let written = app.export_playlist(&paths, "list.m3u").unwrap();
        assert_eq!(written, dir.0.join("list.m3u").to_string_lossy());
        let m3u = std::fs::read_to_string(dir.0.join("list.m3u")).unwrap();
        assert!(m3u.starts_with("#EXTM3U\n"));
        assert!(m3u.contains("Plastic - Sexy Groove"));
        assert!(m3u.contains("\na.flac\n"), "relative entry path: {m3u}");

        // CSV: header plus one row per track.
        app.export_csv(&paths, "tags.csv").unwrap();
        let csv = std::fs::read_to_string(dir.0.join("tags.csv")).unwrap();
        assert!(csv.starts_with("File,Artist,Title,"));
        assert_eq!(csv.trim_end().lines().count(), 3);

        // Report: one mask-rendered line per track.
        app.export_report(&paths, "%artist% - %title%", "report.txt")
            .unwrap();
        let report = std::fs::read_to_string(dir.0.join("report.txt")).unwrap();
        assert_eq!(report, "Plastic - Sexy Groove\nB.B.E. - Seven Days\n");
    }

    #[test]
    fn export_rejects_file_names_that_would_escape_the_library() {
        let dir = TempDir::new("export-escape");
        let track = dir.tagged_flac("a.flac", "Artist", "Title");
        let app = open_app(&dir);

        for name in ["../evil.csv", "sub/evil.csv", "", ".hidden"] {
            assert!(
                matches!(
                    app.export_csv(std::slice::from_ref(&track), name),
                    Err(AppError::InvalidFileName(_))
                ),
                "should reject {name:?}"
            );
        }
    }

    #[test]
    fn auto_align_matches_files_to_release_tracks_by_content() {
        let dir = TempDir::new("align");
        // File order deliberately does NOT match the release order.
        let a = dir.tagged_flac("a.flac", "Plastic", "Sexy Groove");
        let b = dir.tagged_flac("b.flac", "B.B.E.", "Seven Days And One Week");
        let app = open_app(&dir);

        let tracks = vec![
            ImportTrackDto {
                position: "11".into(),
                artist: "B.B.E.".into(),
                // Punctuation/decoration differs from the local tag.
                title: "Seven Days & One Week (Original Mix)".into(),
                duration_secs: None,
            },
            ImportTrackDto {
                position: "14".into(),
                artist: "Plastic".into(),
                title: "Sexy Groove".into(),
                duration_secs: None,
            },
        ];

        // Each file finds its own track despite the order and the decoration.
        assert_eq!(
            app.auto_align(&[a, b], &tracks).unwrap(),
            vec![Some(1), Some(0)]
        );
    }

    #[test]
    fn extension_for_mime_maps_known_and_falls_back() {
        assert_eq!(extension_for_mime("image/jpeg"), "jpg");
        assert_eq!(extension_for_mime("IMAGE/PNG"), "png");
        assert_eq!(extension_for_mime("image/webp"), "webp");
        // Unknown but clean subtype passes through.
        assert_eq!(extension_for_mime("image/heic"), "heic");
        // Garbage / non-image falls back to jpg.
        assert_eq!(extension_for_mime("application/octet-stream"), "jpg");
        assert_eq!(extension_for_mime(""), "jpg");
    }
}
