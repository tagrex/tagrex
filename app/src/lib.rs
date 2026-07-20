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
use tagrex_core::journal::{BatchId, SqliteJournal, UndoJournal};
use tagrex_core::mask::Mask;
use tagrex_core::model::{CoverArt, TagEngine, TagField};
use tagrex_core::plan::{ChangePlan, CoverChange, Executor, FieldChange, FileChange};
use tagrex_core::provider::{MetadataProvider, ReleaseId, SearchQuery};
use tagrex_core::scanner::{self, ScanOptions};
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
/// be written; `None` means absent/removed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FieldChangeDto {
    pub field: String,
    pub old: Option<String>,
    pub new: Option<String>,
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
}

/// One track of a fetched release.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseTrackDto {
    pub position: String,
    pub artist: Option<String>,
    pub title: String,
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
                    tag_changes.push(FieldChangeDto {
                        field: edit.field.clone(),
                        old,
                        new,
                    });
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

    /// Recorded batches, newest first.
    pub fn history(&self) -> Result<Vec<BatchDto>, AppError> {
        Ok(self.journal.batches()?.iter().map(BatchDto::from).collect())
    }

    /// Search a metadata provider (Discogs) with the given personal token.
    pub fn search_discogs(
        &self,
        token: &str,
        query: &SearchQueryDto,
    ) -> Result<Vec<CandidateDto>, AppError> {
        let provider = DiscogsProvider::new(token);
        let candidates = provider.search(&query.to_search_query())?;
        Ok(candidates.iter().map(CandidateDto::from).collect())
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
                    tag_changes.push(FieldChangeDto {
                        field: field.to_storage_key(),
                        old,
                        new,
                    });
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
                },
                ImportTrackDto {
                    position: "5".into(),
                    artist: "Guest".into(),
                    title: "Fifth".into(),
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
