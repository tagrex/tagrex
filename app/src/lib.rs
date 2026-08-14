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

use std::path::{Component, Path, PathBuf};

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use base64::Engine as _;
use tagrex_core::export::{self, PlaylistTrack};
use tagrex_core::journal::{BatchId, SqliteJournal, UndoJournal};
use tagrex_core::mask::{FileContext, Mask, MaskError};
use tagrex_core::matching::{self, MatchOptions, TrackRef};
use tagrex_core::model::{is_writable_value, CoverArt, CoverKind, TagEngine, TagField};
use tagrex_core::plan::{ChangePlan, CoverChange, Executor, FieldChange, FileChange};
use tagrex_core::provider::{MetadataProvider, ReleaseId, SearchQuery};
use tagrex_core::scanner::{self, ScanOptions};
use tagrex_core::transform::{
    CaseStyle, ChangeCase, KeyNotation, KeyStyle, RemoveDiacritics, Replace, ReplaceOptions,
    TransformChain, Transliterate, Untransliterate,
};
use tagrex_providers_discogs::DiscogsProvider;
use tagrex_providers_musicbrainz::MusicBrainzProvider;

/// One audio file as the table view sees it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrackDto {
    pub path: String,
    pub format: String,
    /// Storage-key -> value (see [`TagField::to_storage_key`]).
    pub tags: std::collections::BTreeMap<String, String>,
    /// The file is on disk but its tags couldn't be parsed (e.g. a malformed
    /// frame). It's listed anyway — greyed and non-editable — so it never
    /// silently vanishes from the library. `#[serde(default)]` = false.
    #[serde(default)]
    pub unreadable: bool,
}

impl TrackDto {
    /// A placeholder row for a file whose tags failed to read: path only, format
    /// guessed from the extension, no tags.
    fn unreadable(path: &Path) -> Self {
        let format = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_uppercase)
            .unwrap_or_default();
        Self {
            path: path.to_string_lossy().into_owned(),
            format,
            tags: std::collections::BTreeMap::new(),
            unreadable: true,
        }
    }
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

/// What a mask sees in one file's name (#139): the string being matched — the
/// stem, plus as many parent folders as the pattern asks for — and the
/// storage-key/value pairs it pulls out, in the model's field order. `matched`
/// is false when the name doesn't fit the pattern at all, which is a normal
/// state while a pattern is half-typed, not an error.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NameProbeDto {
    pub subject: String,
    pub fields: Vec<(String, String)>,
    pub matched: bool,
}

/// An embedded image crossing the IPC boundary: base64 data + MIME, plus what
/// it depicts and its description (#56).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CoverArtDto {
    pub mime: String,
    pub data_base64: String,
    /// A [`CoverKind`] storage key (`front`, `back`, `media`, …). Absent or
    /// unknown reads as the front cover, which is what an image picked, dropped
    /// or fetched without saying otherwise is meant as.
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub description: String,
}

/// A planned change to a file's embedded images (#56). Both sides are the whole
/// set: `old` is restored on undo, `new` is written; an empty side means no
/// images at all.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CoverChangeDto {
    #[serde(default)]
    pub old: Vec<CoverArtDto>,
    #[serde(default)]
    pub new: Vec<CoverArtDto>,
}

/// A planned change to one file: tag edits, a cover change, and/or a rename.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileChangeDto {
    pub path: String,
    pub rename_to: Option<String>,
    /// Whether `rename_to` is a copy rather than a move (#153). The source is
    /// left untouched; undo removes the copy.
    #[serde(default)]
    pub copy: bool,
    pub tag_changes: Vec<FieldChangeDto>,
    #[serde(default)]
    pub cover_change: Option<CoverChangeDto>,
    /// Sidecar files travelling with this rename/move (#58): `(from, to)` pairs,
    /// filled by `attach_sidecars` at preview time. Serialized as `[[from, to], …]`.
    #[serde(default)]
    pub sidecar_renames: Vec<(String, String)>,
}

/// A previewable plan, ready to render as a "current -> new" diff.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanDto {
    pub description: String,
    pub changes: Vec<FileChangeDto>,
    /// Whether applying this plan should remove the folders its moves empty
    /// (#153). Defaults off, so every plan that isn't a reorganize behaves
    /// exactly as before.
    #[serde(default)]
    pub prune_empty_dirs: bool,
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

/// Result of saving a release's images to disk (#102). `conflicts` non-empty
/// means NOTHING was written — the named files already exist and the caller
/// should confirm before re-saving with `overwrite`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SaveImagesDto {
    pub written: Vec<String>,
    pub conflicts: Vec<String>,
}

/// Cover state across a selection, for the EDITOR cover well: how many files
/// carry any image, whether they differ, and either the exact set they all
/// share or a small fan of the distinct front covers when they don't.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoverSummaryDto {
    pub total: usize,
    pub with_cover: usize,
    pub distinct: bool,
    /// Distinct front covers to fan out when the selection is mixed.
    pub samples: Vec<CoverArtDto>,
    /// The whole image set every selected file carries, when they all carry the
    /// same one (#56). Empty when they differ — there is no single set to edit
    /// then, only the fan above to show.
    #[serde(default)]
    pub shared_set: Vec<CoverArtDto>,
}

/// One file in a duplicate group (#40), with the columns needed to tell copies
/// apart. Read-only — detection never modifies or deletes anything.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DuplicateFileDto {
    pub path: String,
    pub artist: String,
    pub title: String,
    pub album: String,
    pub duration_secs: u64,
    pub size_bytes: u64,
    pub bitrate_kbps: Option<u32>,
}

/// A set of files judged duplicates of each other under the chosen criterion
/// (#40): the shared key and its ≥2 members.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DuplicateGroupDto {
    /// Human-readable key the group shares (e.g. `"artist — title"`).
    pub key: String,
    pub files: Vec<DuplicateFileDto>,
}

/// App-wide preferences (Settings, #79), persisted as JSON in the config dir.
/// Every field has a serde default so an older/partial file still loads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SettingsDto {
    /// HTTP/SOCKS proxy URL for Discogs requests; empty = direct connection.
    #[serde(default)]
    pub proxy: String,
    /// Client-side throttle for Discogs requests, in requests/minute; 0 = no
    /// throttle (the server's 429/Retry-After is still honored either way).
    #[serde(default)]
    pub rate_limit_per_min: u32,
    /// Write ID3v2 tags as v2.3 instead of the default v2.4.
    #[serde(default)]
    pub id3_v23: bool,
    /// Ordered tag-block read priority (#84): keys like `"id3v2"`, `"vorbis"`,
    /// `"ape"`. When a file carries more than one tag block, values are read
    /// from the first listed block that is present. Empty = the backend's
    /// default order (primary tag, then the first present).
    #[serde(default)]
    pub read_priority: Vec<String>,
    /// Max cover dimension in pixels before embedding (#41); a larger image is
    /// downscaled to fit and re-encoded as JPEG. 0 = don't resize.
    #[serde(default)]
    pub cover_max_px: u32,
    /// JPEG quality used when a cover is resized (#41). 0 (unset) means the
    /// default 85; otherwise 1..=100.
    #[serde(default)]
    pub cover_quality: u8,
    /// Saved transform-chain action groups (#57): each a named, ordered set of
    /// steps + scope. Stored data only — `apply_settings` ignores it; the UI
    /// reads it from `load_settings` and rewrites it via `save_settings`.
    #[serde(default)]
    pub action_groups: Vec<ActionGroupDto>,
    /// Whether a rename/move carries matching sidecar files along (#58).
    /// Defaults on.
    #[serde(default = "default_carry_sidecars")]
    pub carry_sidecars: bool,
    /// Extensions (without the dot) whose same-stem files count as sidecars to
    /// carry (#58). Case-insensitive. Defaults to a lyrics/cue/text/image set.
    #[serde(default = "default_sidecar_extensions")]
    pub sidecar_extensions: Vec<String>,
    /// Storage keys an online import must NOT write (#152), e.g. `"genre"` or
    /// `"custom:RELEASECOUNTRY"`.
    ///
    /// A *deny* list rather than an allow list, deliberately. Empty means "write
    /// everything", which is both the historical behaviour and what an absent or
    /// older settings.json deserializes to — an allow list would have to be
    /// populated to mean the same thing, and would silently write nothing for
    /// anyone upgrading. It also means a field added to the import later is
    /// written by default instead of being invisibly excluded.
    #[serde(default)]
    pub import_skip_fields: Vec<String>,
    /// What joins the several values of a multi-value field into the one string
    /// the app edits, and splits them apart again on write (#46). Empty = the
    /// backend's default (`"; "`), which is also what an older settings.json
    /// deserializes to.
    #[serde(default)]
    pub multi_value_separator: String,
}

fn default_carry_sidecars() -> bool {
    true
}

/// The default sidecar extension set (#58): lyrics, cue sheets, text notes, and
/// per-track cover images.
fn default_sidecar_extensions() -> Vec<String> {
    ["lrc", "cue", "txt", "jpg", "jpeg", "png"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

// A manual `Default` (rather than derived) so `carry_sidecars`/`sidecar_extensions`
// default to the same values serde fills in for a partial file — keeping
// `SettingsDto::default()` and `from_str("{}")` identical.
impl Default for SettingsDto {
    fn default() -> Self {
        Self {
            proxy: String::new(),
            rate_limit_per_min: 0,
            id3_v23: false,
            read_priority: Vec::new(),
            cover_max_px: 0,
            cover_quality: 0,
            action_groups: Vec::new(),
            carry_sidecars: default_carry_sidecars(),
            sidecar_extensions: default_sidecar_extensions(),
            import_skip_fields: Vec::new(),
            multi_value_separator: String::new(),
        }
    }
}

/// The effective JPEG quality for cover resize: the setting, or 85 when unset.
fn effective_cover_quality(quality: u8) -> u8 {
    if quality == 0 {
        85
    } else {
        quality
    }
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
    /// Media-type filter (#103): `CD` / `Vinyl` / `LP` / `File`; absent = all.
    #[serde(default)]
    pub format: Option<String>,
    /// 1-based page for "Load more" pagination (#95). Defaults to 0 (page 1)
    /// so older callers that omit it keep working.
    #[serde(default)]
    pub page: u32,
    /// Results per page; 0 lets the provider use its default page size.
    #[serde(default)]
    pub per_page: u32,
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
    /// Which disc of the set the track sits on, when the release says (#146).
    #[serde(default)]
    pub disc: Option<u32>,
    pub artist: Option<String>,
    pub title: String,
    /// Length the release lists for this track, in seconds, when it states one.
    pub duration_secs: Option<u64>,
    /// Per-recording ISRC, when the provider exposes it (#54).
    #[serde(default)]
    pub isrc: Option<String>,
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
    /// Label / catalogue-number pairs (#90); the UI picks which one to import.
    #[serde(default)]
    pub labels: Vec<ReleaseLabelDto>,
    /// Release country, e.g. `Belgium`.
    #[serde(default)]
    pub country: Option<String>,
    /// Physical/source format descriptor, e.g. `Vinyl, 12", 33 ⅓ RPM` (#106).
    #[serde(default)]
    pub format: Option<String>,
    /// How many discs the set holds, when the release states it (#146).
    #[serde(default)]
    pub disc_total: Option<u32>,
    /// Public webpage for the release, if any.
    #[serde(default)]
    pub url: Option<String>,
    /// URL of the release's primary image, if any. Fetch its bytes with
    /// [`App::fetch_discogs_image`] to preview or embed it.
    pub cover_image_url: Option<String>,
    /// Every image the release carries, primary first (#102) — for the cover
    /// resolution + count display and the save-to-disk actions.
    #[serde(default)]
    pub images: Vec<ReleaseImageDto>,
}

/// One release image: a fetch handle plus its dimensions (`0` = unknown) (#102).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseImageDto {
    pub url: String,
    pub width: u32,
    pub height: u32,
}

/// One label / catalogue-number pair of a release (#90).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseLabelDto {
    pub name: String,
    pub catalog_number: Option<String>,
}

/// One release track the user chose to import, as sent back from the UI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportTrackDto {
    pub position: String,
    /// The disc this track sits on, as the release states it (#146). Used when
    /// the position carries no vinyl side to map instead.
    #[serde(default)]
    pub disc: Option<u32>,
    pub artist: String,
    pub title: String,
    /// Length from the release listing, used to corroborate a match (#64).
    #[serde(default)]
    pub duration_secs: Option<u64>,
    /// Per-recording ISRC from the provider (#54): an exact match key, and
    /// written to the file on import when it's missing one.
    #[serde(default)]
    pub isrc: Option<String>,
}

/// One file's auto-align result (#53/#54): the release-track index it lined up
/// with, and whether the match was driven by an exact ISRC hit (so the UI can
/// say *why* it matched).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct AlignMatchDto {
    pub track: usize,
    pub by_isrc: bool,
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
    /// The chosen release's provider id, stored as an album-level tag so the
    /// table can group by release (#20). Empty/absent writes nothing.
    #[serde(default)]
    pub release_id: Option<String>,
    /// Which provider the id came from ("discogs" | "musicbrainz"), selecting
    /// the tag key it's written under. Defaults to Discogs.
    #[serde(default)]
    pub source: Option<String>,
    /// The chosen label imprint → written to the Publisher tag (#90).
    #[serde(default)]
    pub label: Option<String>,
    /// The chosen catalogue number → written to the CatalogNumber tag (#90).
    #[serde(default)]
    pub catalog_number: Option<String>,
    /// Release country → written to a `RELEASECOUNTRY` tag (the portable,
    /// cross-format key). Full name as the provider states it (e.g. `Belgium`).
    #[serde(default)]
    pub country: Option<String>,
    /// Total number of tracks on the release → written to the TrackTotal tag, so
    /// a file's track reads as N/total. Album-level (same for every file).
    #[serde(default)]
    pub track_total: Option<String>,
    /// The release's public webpage → written to the URL frame (`WOAF`).
    #[serde(default)]
    pub url: Option<String>,
    /// Physical medium of the release (Vinyl / CD / Cassette / File) → written to
    /// the media tag (`TMED`/`MEDIA`). Album-level; drives the vinyl side view.
    #[serde(default)]
    pub media_type: Option<String>,
    /// Number of discs in the set → written to the DiscTotal tag, so a file's
    /// disc reads as N/total (#146). Album-level, like `track_total`.
    #[serde(default)]
    pub disc_total: Option<String>,
}

/// One rule in a transformation chain, as the UI describes it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransformRuleDto {
    /// `replace`, `case`, `diacritics`, `transliterate`, `untransliterate` or
    /// `key`.
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
    /// For `case`: `lower`, `upper`, `title` or `sentence`. For `key`: the
    /// target notation `camelot`, `openkey` or `musical`.
    #[serde(default)]
    pub style: String,
    /// Whether this step runs (#57). A disabled step stays in the chain / saved
    /// group but is skipped. Defaults true so chains and groups without the field
    /// stay enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// A named, saved chain of transform steps — an "action group" (#57). Persisted
/// in settings.json; run over a selection to produce one previewable ChangePlan
/// (the whole group applies and undoes as one batch).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionGroupDto {
    pub name: String,
    /// The transform scope the group runs at (`tags`, `filename`, `fileext`, or
    /// a single field key) — mirrors the GENERATOR scope selector.
    #[serde(default)]
    pub scope: String,
    pub rules: Vec<TransformRuleDto>,
    /// What the group is for, in one line (#137). Carried by the shipped
    /// presets, where the name alone doesn't say what the chain does; empty for
    /// groups the user saved, whose name is their own shorthand.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// What a drag-and-drop of paths resolves to (#127), reported to the frontend so
/// it can open the right kind of session and group the table accordingly.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DropResultDto {
    /// `"library"` (a single dropped folder) or `"files"` (a file-set).
    pub mode: String,
    /// The session root: the dropped folder, or the files' common ancestor.
    pub root: String,
    /// In `"files"` mode, the dropped directories — each becomes a table group;
    /// files under none of them collect under the "Files" group. Empty in
    /// `"library"` mode.
    pub folders: Vec<String>,
}

/// How a set of dropped paths resolves into a session (#127).
#[derive(Debug)]
enum DropPlan {
    /// Exactly one dropped directory → open it as a library rooted there.
    Library { root: PathBuf },
    /// Files and/or several directories → a file-set session over `files`,
    /// rooted at their common ancestor. `folders` are the dropped directories,
    /// surfaced so the frontend can group by drop origin.
    FileSet {
        root: PathBuf,
        files: Vec<PathBuf>,
        folders: Vec<PathBuf>,
    },
    /// Nothing usable was dropped (no readable audio, only empty folders, etc.).
    Empty,
}

/// Classify dropped `paths` into a [`DropPlan`]. A lone directory opens as a
/// library; anything else (loose files, several folders, a mix) becomes a
/// file-set: every folder is expanded into its audio files, loose files are kept
/// if they're supported audio, and the whole set is de-duplicated and sorted.
/// Non-existent or non-audio entries are skipped.
fn resolve_drop(paths: &[PathBuf]) -> DropPlan {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut files: Vec<PathBuf> = Vec::new();
    for path in paths {
        match std::fs::metadata(path) {
            Ok(meta) if meta.is_dir() => dirs.push(path.clone()),
            Ok(_) if scanner::is_supported_audio(path) => files.push(path.clone()),
            _ => {} // missing, or a non-audio file — ignore
        }
    }

    // A single folder alone is an ordinary library, rooted at that folder.
    if dirs.len() == 1 && files.is_empty() {
        return DropPlan::Library {
            root: dirs.into_iter().next().unwrap(),
        };
    }

    // Otherwise expand every folder into its audio files and fold in loose files.
    let mut all: Vec<PathBuf> = files;
    for dir in &dirs {
        all.extend(scanner::scan(dir, &ScanOptions::default()).filter_map(Result::ok));
    }
    all.sort();
    all.dedup();

    if all.is_empty() {
        return DropPlan::Empty;
    }

    let root = common_ancestor(&all).unwrap_or_else(|| PathBuf::from("/"));
    DropPlan::FileSet {
        root,
        files: all,
        folders: dirs,
    }
}

/// The deepest directory that is an ancestor of every path in `paths`. For a
/// single file it's that file's parent. `None` only if `paths` is empty.
fn common_ancestor(paths: &[PathBuf]) -> Option<PathBuf> {
    let mut iter = paths.iter();
    let first = iter.next()?;
    // Seed with the first path's parent (a file contributes its directory).
    let mut prefix: Vec<Component> = first.parent().unwrap_or(first).components().collect();
    for path in iter {
        let parent = path.parent().unwrap_or(path);
        let shared = prefix
            .iter()
            .zip(parent.components())
            .take_while(|(a, b)| *a == b)
            .count();
        prefix.truncate(shared);
        if prefix.is_empty() {
            break;
        }
    }
    Some(prefix.iter().map(|c| c.as_os_str()).collect())
}

/// A tagging session rooted at one library directory. The root doubles as the
/// [`Executor`] `allowed_root`, so every write is confined to the opened
/// library.
pub struct App {
    library_root: PathBuf,
    /// When set, the session operates on this explicit list of files (a
    /// drag-and-drop of files and/or several folders, #127) instead of scanning
    /// `library_root`. `library_root` still bounds every write as the executor's
    /// `allowed_root`; the filter only narrows *which* files are listed and
    /// operated on. `None` = an ordinary library rooted at `library_root`.
    file_filter: Option<Vec<PathBuf>>,
    journal: SqliteJournal,
    /// Live Discogs proxy URL (None = direct), from settings.
    discogs_proxy: RefCell<Option<String>>,
    /// Minimum spacing between Discogs requests (None = no throttle), from the
    /// rate-limit setting. Enforced in [`throttle_discogs`](App::throttle_discogs).
    discogs_min_interval: Cell<Option<Duration>>,
    /// When the last Discogs request went out, for the throttle. Interior
    /// mutability so the read-only command path can update it.
    last_discogs_request: Cell<Option<Instant>>,
    /// When the last MusicBrainz request went out (#33). MusicBrainz asks
    /// clients to stay under ~1 req/s regardless of any user rate-limit setting,
    /// so it gets its own timestamp and a hard 1s floor in
    /// [`throttle_musicbrainz`](App::throttle_musicbrainz).
    last_musicbrainz_request: Cell<Option<Instant>>,
    /// Max cover dimension before embedding, 0 = off (#41), from settings.
    cover_max_px: Cell<u32>,
    /// JPEG quality for a resized cover (#41), from settings.
    cover_quality: Cell<u8>,
    /// Whether a rename/move carries matching sidecar files (#58), from settings.
    carry_sidecars: Cell<bool>,
    /// Sidecar extensions to carry (#58), from settings.
    sidecar_extensions: RefCell<Vec<String>>,
    /// Storage keys an online import must not write (#152), from settings.
    import_skip_fields: RefCell<HashSet<String>>,
    /// Destinations outside `library_root` the user has explicitly chosen to
    /// reorganize into during this session (#153), and which therefore bound
    /// writes alongside the library root.
    ///
    /// Never inferred from a plan — that would make the containment check
    /// circular — and never persisted: an external destination is authorized by
    /// the user picking it in the folder chooser, for as long as the session
    /// lasts. Undo of an older batch does not need this list, because the batch
    /// records the roots it was applied under.
    extra_roots: RefCell<Vec<PathBuf>>,
}

impl App {
    /// Open a session for `library_root`, storing the undo journal at
    /// `journal_path` (typically inside the app's config dir). Settings default
    /// until [`apply_settings`](App::apply_settings) is called.
    pub fn open(library_root: impl Into<PathBuf>, journal_path: &Path) -> Result<Self, AppError> {
        Ok(Self {
            library_root: library_root.into(),
            file_filter: None,
            journal: SqliteJournal::open(journal_path)?,
            discogs_proxy: RefCell::new(None),
            discogs_min_interval: Cell::new(None),
            last_discogs_request: Cell::new(None),
            last_musicbrainz_request: Cell::new(None),
            cover_max_px: Cell::new(0),
            cover_quality: Cell::new(85),
            carry_sidecars: Cell::new(default_carry_sidecars()),
            sidecar_extensions: RefCell::new(default_sidecar_extensions()),
            import_skip_fields: RefCell::new(HashSet::new()),
            extra_roots: RefCell::new(Vec::new()),
        })
    }

    /// Open a file-set session (#127): the session lists and operates on exactly
    /// `files`, while `root` — their common ancestor — bounds every write as the
    /// executor's `allowed_root`. Used when a drag-and-drop resolves to loose
    /// files and/or several folders rather than a single library directory.
    pub fn open_file_set(
        root: impl Into<PathBuf>,
        files: Vec<PathBuf>,
        journal_path: &Path,
    ) -> Result<Self, AppError> {
        let mut app = Self::open(root, journal_path)?;
        app.file_filter = Some(files);
        Ok(app)
    }

    /// Resolve a drag-and-drop of `paths` into a session (#127) and open it: a
    /// lone folder becomes a library, anything else a file-set. Returns the
    /// session together with a [`DropResultDto`] telling the frontend which mode
    /// it got and how to group the table. Errors with [`AppError::EmptyDrop`]
    /// when nothing usable was dropped.
    pub fn open_drop(
        paths: Vec<PathBuf>,
        journal_path: &Path,
    ) -> Result<(Self, DropResultDto), AppError> {
        match resolve_drop(&paths) {
            DropPlan::Library { root } => {
                let dto = DropResultDto {
                    mode: "library".to_string(),
                    root: root.to_string_lossy().into_owned(),
                    folders: Vec::new(),
                };
                Ok((Self::open(root, journal_path)?, dto))
            }
            DropPlan::FileSet {
                root,
                files,
                folders,
            } => {
                let dto = DropResultDto {
                    mode: "files".to_string(),
                    root: root.to_string_lossy().into_owned(),
                    folders: folders
                        .iter()
                        .map(|f| f.to_string_lossy().into_owned())
                        .collect(),
                };
                Ok((Self::open_file_set(root, files, journal_path)?, dto))
            }
            DropPlan::Empty => Err(AppError::EmptyDrop),
        }
    }

    /// The files this session operates on: the explicit filter when set (a
    /// file-set drop), otherwise a fresh recursive scan of `library_root`.
    fn source_paths(&self) -> Vec<PathBuf> {
        match &self.file_filter {
            Some(files) => files.clone(),
            None => scanner::scan(&self.library_root, &ScanOptions::default())
                .filter_map(Result::ok)
                .collect(),
        }
    }

    /// Apply saved settings (#79): the Discogs proxy + rate-limit throttle, the
    /// app-wide ID3v2 write version, and the tag-read priority (#84). Called on
    /// open and whenever settings are saved.
    pub fn apply_settings(&self, settings: &SettingsDto) {
        let proxy = settings.proxy.trim();
        *self.discogs_proxy.borrow_mut() = (!proxy.is_empty()).then(|| proxy.to_string());
        self.discogs_min_interval.set(
            (settings.rate_limit_per_min > 0)
                .then(|| Duration::from_secs_f64(60.0 / settings.rate_limit_per_min as f64)),
        );
        tagrex_core::model::set_write_id3v23(settings.id3_v23);
        tagrex_core::model::set_read_priority(&settings.read_priority);
        tagrex_core::model::set_multi_value_separator(&settings.multi_value_separator);
        self.cover_max_px.set(settings.cover_max_px);
        self.cover_quality
            .set(effective_cover_quality(settings.cover_quality));
        self.carry_sidecars.set(settings.carry_sidecars);
        *self.sidecar_extensions.borrow_mut() = settings.sidecar_extensions.clone();
        *self.import_skip_fields.borrow_mut() =
            settings.import_skip_fields.iter().cloned().collect();
    }

    /// Build a Discogs provider using the current proxy setting.
    fn discogs_provider(&self, token: &str) -> Result<DiscogsProvider, AppError> {
        Ok(DiscogsProvider::with_proxy(
            token,
            self.discogs_proxy.borrow().as_deref(),
        )?)
    }

    /// Build a MusicBrainz provider using the current proxy setting (#33). No
    /// token — MusicBrainz is unauthenticated. Reuses the same network proxy the
    /// Discogs provider uses.
    fn musicbrainz_provider(&self) -> Result<MusicBrainzProvider, AppError> {
        Ok(MusicBrainzProvider::with_proxy(
            self.discogs_proxy.borrow().as_deref(),
        )?)
    }

    /// Throttle the next provider request for `source`.
    fn throttle(&self, source: &str) {
        match source {
            "musicbrainz" => self.throttle_musicbrainz(),
            _ => self.throttle_discogs(),
        }
    }

    /// Sleep just enough to honor the rate-limit setting before a Discogs
    /// request. Discogs calls are already serialized by the app lock, so a
    /// single shared timestamp is enough to space them out.
    fn throttle_discogs(&self) {
        let Some(min) = self.discogs_min_interval.get() else {
            return;
        };
        if let Some(last) = self.last_discogs_request.get() {
            let elapsed = last.elapsed();
            if elapsed < min {
                std::thread::sleep(min - elapsed);
            }
        }
        self.last_discogs_request.set(Some(Instant::now()));
    }

    /// Space MusicBrainz requests out (#33). MusicBrainz etiquette is ~1 req/s
    /// for anonymous clients, and honoring it is not optional, so the interval
    /// is the *stricter* of a hard 1s floor and any user rate-limit setting.
    fn throttle_musicbrainz(&self) {
        let one_sec = Duration::from_secs(1);
        let min = self
            .discogs_min_interval
            .get()
            .map_or(one_sec, |user| user.max(one_sec));
        if let Some(last) = self.last_musicbrainz_request.get() {
            let elapsed = last.elapsed();
            if elapsed < min {
                std::thread::sleep(min - elapsed);
            }
        }
        self.last_musicbrainz_request.set(Some(Instant::now()));
    }

    /// Scan the library and read each file's tags. A file whose tags can't be
    /// parsed (e.g. a malformed frame) is still listed — as an `unreadable`
    /// placeholder — rather than silently dropped, so it never looks like the
    /// file went missing (#83). Walk errors (a permission-denied dir) are the
    /// only thing skipped. Results are sorted by path so the table has a stable
    /// order (the scanner yields filesystem order, which isn't alphabetical) —
    /// this order is also what mapping-by-position (rename masks, release
    /// import) lines up against.
    pub fn list_tracks(&self) -> Vec<TrackDto> {
        let mut tracks: Vec<TrackDto> = self
            .source_paths()
            .into_iter()
            .map(|path| match TagEngine::read(&path) {
                Ok(track) => TrackDto::from(track),
                Err(_) => TrackDto::unreadable(&path),
            })
            .collect();
        tracks.sort_by(|a, b| a.path.cmp(&b.path));
        tracks
    }

    /// Find likely duplicate files across the opened library (#40), by
    /// `criterion`: `"artist_title"`, `"album_track"`, `"duration"`, `"size"`,
    /// or `"hash"` (identical bytes). Strictly read-only — returns groups of two
    /// or more files sharing the key, each with the columns needed to tell
    /// copies apart, largest groups first. Unreadable files, and files lacking
    /// the data a criterion needs (e.g. no artist/title), are skipped.
    pub fn find_duplicates(&self, criterion: &str) -> Result<Vec<DuplicateGroupDto>, AppError> {
        use std::collections::BTreeMap;
        use std::hash::{Hash, Hasher};

        // internal key -> (human-readable key, members)
        let mut groups: BTreeMap<String, (String, Vec<DuplicateFileDto>)> = BTreeMap::new();

        for path in self.source_paths() {
            let Ok(track) = TagEngine::read(&path) else {
                continue;
            };
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let props = TagEngine::read_audio_props(&path).ok();
            let duration = props.map(|p| p.duration_secs).unwrap_or(0);
            let bitrate = props.and_then(|p| p.bitrate_kbps);
            let field = |key| track.tags.get(key).cloned().unwrap_or_default();
            let artist: String = field(&TagField::Artist);
            let title: String = field(&TagField::Title);
            let album: String = field(&TagField::Album);

            let keyed = match criterion {
                "album_track" => {
                    let trackno = field(&TagField::TrackNumber);
                    let (a, t) = (norm_key(&album), norm_key(&trackno));
                    (!a.is_empty() && !t.is_empty()).then(|| {
                        (
                            format!("album_track:{a}\u{1}{t}"),
                            format!("{album} · #{trackno}"),
                        )
                    })
                }
                "duration" => {
                    (duration > 0).then(|| (format!("duration:{duration}"), format!("{duration}s")))
                }
                "size" => (size > 0).then(|| (format!("size:{size}"), format!("{size} bytes"))),
                "hash" => std::fs::read(&path).ok().map(|bytes| {
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    bytes.hash(&mut hasher);
                    (
                        format!("hash:{:016x}", hasher.finish()),
                        "identical bytes".to_string(),
                    )
                }),
                // default: artist + title
                _ => {
                    let (a, t) = (norm_key(&artist), norm_key(&title));
                    (!a.is_empty() && !t.is_empty())
                        .then(|| (format!("at:{a}\u{1}{t}"), format!("{artist} — {title}")))
                }
            };
            let Some((key, display)) = keyed else {
                continue;
            };

            groups
                .entry(key)
                .or_insert_with(|| (display, Vec::new()))
                .1
                .push(DuplicateFileDto {
                    path: path.to_string_lossy().into_owned(),
                    artist,
                    title,
                    album,
                    duration_secs: duration,
                    size_bytes: size,
                    bitrate_kbps: bitrate,
                });
        }

        let mut result: Vec<DuplicateGroupDto> = groups
            .into_values()
            .filter(|(_, files)| files.len() >= 2)
            .map(|(key, mut files)| {
                files.sort_by(|a, b| a.path.cmp(&b.path));
                DuplicateGroupDto { key, files }
            })
            .collect();
        // Largest groups first; stable by key.
        result.sort_by(|a, b| b.files.len().cmp(&a.files.len()).then(a.key.cmp(&b.key)));
        Ok(result)
    }

    /// Build a rename plan from a mask over the given files, without writing.
    /// The mask renders each file's new stem; the original extension is kept.
    /// Files whose tags can't satisfy the mask, or whose name wouldn't change,
    /// are left out of the plan.
    /// Detect sidecar files that should travel with `change`'s rename/move (#58)
    /// and record them on it. A sidecar is a file in the source directory whose
    /// stem matches the audio file's and whose extension is in the configured
    /// set; its target keeps the destination directory and the new stem. No-op
    /// when the feature is off or the change carries no real rename.
    fn attach_sidecars(&self, change: &mut FileChangeDto) {
        if !self.carry_sidecars.get() {
            return;
        }
        let Some(rename_to) = change.rename_to.as_deref() else {
            return;
        };
        let src = Path::new(&change.path);
        let dst = Path::new(rename_to);
        if src == dst {
            return;
        }
        let (Some(src_dir), Some(src_stem)) =
            (src.parent(), src.file_stem().and_then(|s| s.to_str()))
        else {
            return;
        };
        let (Some(dst_dir), Some(dst_stem)) =
            (dst.parent(), dst.file_stem().and_then(|s| s.to_str()))
        else {
            return;
        };
        let exts = self.sidecar_extensions.borrow();
        if exts.is_empty() {
            return;
        }
        let Ok(entries) = std::fs::read_dir(src_dir) else {
            return;
        };
        let mut pairs = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            // Never the audio file itself, and only real files.
            if path == *src || !path.is_file() {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if stem != src_stem {
                continue;
            }
            let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                continue; // an extension-less file can't match the set
            };
            if !exts.iter().any(|e| e.eq_ignore_ascii_case(ext)) {
                continue;
            }
            let from = path.to_string_lossy().into_owned();
            let to = dst_dir
                .join(format!("{dst_stem}.{ext}"))
                .to_string_lossy()
                .into_owned();
            pairs.push((from, to));
        }
        // Deterministic order so preview and journal read consistently.
        pairs.sort();
        change.sidecar_renames = pairs;
    }

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
            let Ok(stem) = mask.render_with(&track.tags, &FileContext::read(&mask, &track)) else {
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
            let mut change = FileChangeDto {
                path: path.to_string_lossy().into_owned(),
                rename_to: Some(target.to_string_lossy().into_owned()),
                tag_changes: Vec::new(),
                cover_change: None,
                sidecar_renames: Vec::new(),
                copy: false,
            };
            self.attach_sidecars(&mut change);
            changes.push(change);
        }
        Ok(PlanDto {
            description: format!("Rename by mask: {mask_pattern}"),
            changes,
            prune_empty_dirs: false,
        })
    }

    /// Build a tag plan by reading each file's own name through a mask (#139) —
    /// the extract direction of the grammar [`preview_rename`](Self::preview_rename)
    /// renders with. Files that arrived named `01 - Artist - Title.flac` and
    /// empty already carry their metadata; this is how it gets back into tags.
    ///
    /// A pattern carrying folder separators (`%albumartist%/%album%/%title%`)
    /// matches that many parent directories ahead of the stem, because that is
    /// where artist and album usually live; without one, only the stem is
    /// matched. Either separator is accepted so a pattern stays portable (#71).
    ///
    /// A name that does not fit the pattern is skipped rather than failing the
    /// batch — a selection is rarely uniform — but a pattern the extract
    /// direction refuses outright (`%side%`, adjacent placeholders) is an error
    /// before anything is previewed, as it is for rename. The result is an
    /// ordinary [`PlanDto`], so this previews, applies and undoes through the
    /// same journaled path as every other change.
    ///
    /// The values come out as the name spells them (#159). Cleaning them up is
    /// a chain over the staged plan — [`preview_transform_over_plan`](Self::preview_transform_over_plan),
    /// #142 — which is the one mechanism every producer of values shares, so
    /// there is no second one here.
    pub fn preview_tags_from_name(
        &self,
        mask_pattern: &str,
        paths: &[PathBuf],
    ) -> Result<PlanDto, AppError> {
        // The mask engine sees one separator; the user may type either (#71).
        let normalized = mask_pattern.replace('\\', "/");
        let mask = Mask::parse(&normalized)?;
        let depth = normalized.matches('/').count();

        let mut changes = Vec::new();
        for path in paths {
            let Ok(track) = TagEngine::read(path) else {
                continue;
            };
            let Some(subject) = name_subject(path, depth) else {
                continue; // shallower than the pattern asks for
            };
            let extracted = match mask.extract(&subject) {
                Ok(tags) => tags,
                // This one name doesn't fit the pattern; the next one may.
                Err(MaskError::NoMatch) => continue,
                // Anything else is a property of the pattern itself, and would
                // be just as wrong for every remaining file.
                Err(err) => return Err(err.into()),
            };

            let mut tag_changes = Vec::new();
            for (field, value) in &extracted {
                let value = normalize_extracted(field, value);
                if value.is_empty() {
                    continue;
                }
                let old = track.tags.get(field).cloned();
                if old.as_deref() == Some(value.as_str()) {
                    continue;
                }
                tag_changes.push(FieldChangeDto::new(
                    field.to_storage_key(),
                    old,
                    Some(value),
                ));
            }
            if tag_changes.is_empty() {
                continue;
            }
            changes.push(FileChangeDto {
                path: path.to_string_lossy().into_owned(),
                rename_to: None,
                tag_changes,
                cover_change: None,
                sidecar_renames: Vec::new(),
                copy: false,
            });
        }
        Ok(PlanDto {
            description: format!("Tags from name: {mask_pattern}"),
            changes,
            prune_empty_dirs: false,
        })
    }

    /// What a mask pulls out of one file's name (#139), for the live probe
    /// beside the pattern box.
    ///
    /// The same extraction [`preview_tags_from_name`](Self::preview_tags_from_name)
    /// does, for a single file and *without* comparing against the file's
    /// current tags: while a pattern is being typed the question is what it
    /// sees, not what would change. A name that doesn't fit comes back
    /// `matched: false` rather than as an error, because the subject string is
    /// exactly what the user needs to see in that case — it shows how much of
    /// the path the pattern is being matched against.
    pub fn probe_tags_from_name(
        &self,
        mask_pattern: &str,
        path: &Path,
    ) -> Result<NameProbeDto, AppError> {
        let normalized = mask_pattern.replace('\\', "/");
        let mask = Mask::parse(&normalized)?;
        let depth = normalized.matches('/').count();
        let subject = name_subject(path, depth).unwrap_or_default();
        let fields = match mask.extract(&subject) {
            Ok(tags) => tags
                .iter()
                .map(|(field, value)| (field.to_storage_key(), normalize_extracted(field, value)))
                .filter(|(_, value)| !value.is_empty())
                .collect(),
            Err(MaskError::NoMatch) => {
                return Ok(NameProbeDto {
                    subject,
                    fields: Vec::new(),
                    matched: false,
                })
            }
            Err(err) => return Err(err.into()),
        };
        Ok(NameProbeDto {
            subject,
            fields,
            matched: true,
        })
    }

    /// Preview applying a transformation chain, without writing (#34).
    ///
    /// `scope` is either `filename` — rewriting the file's stem, extension
    /// untouched — `fileext` (#137), its mirror image: the extension alone,
    /// stem untouched — or a tag storage key, or `tags` for every text field the
    /// file carries. Producing a normal [`PlanDto`] means transformations
    /// preview, apply and undo through exactly the same journaled path as every
    /// other change; nothing here writes.
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
                let mut change = FileChangeDto {
                    path: path.to_string_lossy().into_owned(),
                    rename_to: Some(
                        path.with_file_name(file_name)
                            .to_string_lossy()
                            .into_owned(),
                    ),
                    tag_changes: Vec::new(),
                    cover_change: None,
                    sidecar_renames: Vec::new(),
                    copy: false,
                };
                self.attach_sidecars(&mut change);
                changes.push(change);
                continue;
            }

            // The extension alone (#137) — `.MP3` -> `.mp3` is the case that
            // motivates it, so the chain sees the extension without its dot and
            // the stem is carried through untouched. A file with no extension has
            // nothing to transform: skipped rather than given one.
            if scope == "fileext" {
                let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
                    continue;
                };
                let renamed = chain.apply(ext);
                // A separator would move the file, and a dot would change how many
                // extensions the name has -- neither is a rename this scope offers.
                if renamed == ext || renamed.trim().is_empty() || renamed.contains(['/', '\\', '.'])
                {
                    continue;
                }
                let stem = path
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let mut change = FileChangeDto {
                    path: path.to_string_lossy().into_owned(),
                    rename_to: Some(
                        path.with_file_name(format!("{stem}.{renamed}"))
                            .to_string_lossy()
                            .into_owned(),
                    ),
                    tag_changes: Vec::new(),
                    cover_change: None,
                    sidecar_renames: Vec::new(),
                    copy: false,
                };
                self.attach_sidecars(&mut change);
                changes.push(change);
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
                    sidecar_renames: Vec::new(),
                    copy: false,
                });
            }
        }

        Ok(PlanDto {
            description: format!("Transform ({scope})"),
            changes,
            prune_empty_dirs: false,
        })
    }

    /// Preview several action groups run in order as **one** plan (#137).
    ///
    /// Not the same as previewing each group separately and stacking the
    /// results: every group after the first sees what the ones before it did.
    /// A group that lower-cases the file name followed by one that rewrites the
    /// extension has to compose into a single rename, and two groups touching
    /// the Artist field have to compose into a single edit — otherwise the
    /// second group is computed against the file on disk and silently undoes the
    /// first. So each file is carried through the groups as in-memory state and
    /// only the net difference becomes a change.
    ///
    /// The result is an ordinary [`PlanDto`], so the whole checklist previews,
    /// applies and undoes as one batch, exactly like a single group.
    pub fn preview_transform_groups(
        &self,
        paths: &[PathBuf],
        groups: &[ActionGroupDto],
    ) -> Result<PlanDto, AppError> {
        // Build every chain up front: a malformed rule should be an error before
        // anything is previewed, not halfway through the file list.
        let chains = groups
            .iter()
            .map(|group| Ok((group.scope.as_str(), build_chain(&group.rules)?)))
            .collect::<Result<Vec<_>, AppError>>()?;

        let mut changes = Vec::new();
        for path in paths {
            let Ok(track) = TagEngine::read(path) else {
                continue;
            };

            let stem = |p: &Path| {
                p.file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            };
            let mut name = stem(path);
            let mut ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_string);
            let mut tags = track.tags.clone();

            for (scope, chain) in &chains {
                match *scope {
                    "filename" => {
                        let next = chain.apply(&name);
                        if !next.trim().is_empty() {
                            name = next;
                        }
                    }
                    "fileext" => {
                        if let Some(current) = &ext {
                            let next = chain.apply(current);
                            if !next.trim().is_empty() && !next.contains(['/', '\\', '.']) {
                                ext = Some(next);
                            }
                        }
                    }
                    scope => {
                        for (field, value) in tags.iter_mut() {
                            if scope == "tags" || scope == field.to_storage_key() {
                                *value = chain.apply(value);
                            }
                        }
                    }
                }
            }

            let tag_changes: Vec<FieldChangeDto> = tags
                .iter()
                .filter(|(field, value)| track.tags.get(field) != Some(value))
                .map(|(field, value)| {
                    FieldChangeDto::new(
                        field.to_storage_key(),
                        track.tags.get(field).cloned(),
                        Some(value.clone()),
                    )
                })
                .collect();

            let file_name = match &ext {
                Some(ext) => format!("{name}.{ext}"),
                None => name.clone(),
            };
            let renamed = path.file_name().map(|n| n.to_string_lossy().into_owned())
                != Some(file_name.clone());

            if tag_changes.is_empty() && !renamed {
                continue;
            }
            let mut change = FileChangeDto {
                path: path.to_string_lossy().into_owned(),
                rename_to: renamed.then(|| {
                    path.with_file_name(&file_name)
                        .to_string_lossy()
                        .into_owned()
                }),
                tag_changes,
                cover_change: None,
                sidecar_renames: Vec::new(),
                copy: false,
            };
            if renamed {
                self.attach_sidecars(&mut change);
            }
            changes.push(change);
        }

        let names: Vec<&str> = groups.iter().map(|g| g.name.as_str()).collect();
        Ok(PlanDto {
            description: format!("Transform ({})", names.join(", ")),
            changes,
            prune_empty_dirs: false,
        })
    }

    /// Run action groups over a **staged plan** rather than over the files (#142).
    ///
    /// Every flow that *produces* values needs the same second step — clean them
    /// up — and until now that step could only read what was already on disk. So
    /// producing and cleaning meant writing first and transforming afterwards:
    /// two plans and two undo entries for what the user did as one operation,
    /// and no way at all to clean values that exist nowhere yet (tags a mask has
    /// just read out of a file name).
    ///
    /// The input here is the plan's own proposal, not the file. Each chain sees
    /// the `new` value of a change it is scoped to, and the revised value goes
    /// back into the same change — `old` is untouched, because it is still what
    /// is on disk and still what the diff and the executor's staleness check
    /// must compare against. The result is an ordinary [`PlanDto`] that replaces
    /// the staged one, so the whole thing still applies and undoes as one batch.
    ///
    /// A change the cleanup turns back into its `old` value is no longer a
    /// change and is dropped, as is a file left with nothing to do. Values are
    /// re-validated on the way out: a cleanup can just as easily rescue a
    /// rejected value as break a good one.
    pub fn preview_transform_over_plan(
        &self,
        plan: &PlanDto,
        groups: &[ActionGroupDto],
    ) -> Result<PlanDto, AppError> {
        // Same as the on-disk runner: build every chain up front so a malformed
        // rule is an error before anything is revised.
        let chains = groups
            .iter()
            .map(|group| Ok((group.scope.as_str(), build_chain(&group.rules)?)))
            .collect::<Result<Vec<_>, AppError>>()?;

        let mut changes = Vec::new();
        for change in &plan.changes {
            let path = Path::new(&change.path);

            // The name the plan proposes — the rename it already carries, or the
            // file's own name when it proposes none. A file-scoped chain that
            // changes it turns into (or replaces) this file's rename.
            let proposed = change
                .rename_to
                .clone()
                .unwrap_or_else(|| change.path.clone());
            let proposed = Path::new(&proposed);
            let mut name = proposed
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut ext = proposed
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_string);

            let mut tag_changes = change.tag_changes.clone();
            for (scope, chain) in &chains {
                match *scope {
                    "filename" => {
                        let next = chain.apply(&name);
                        if !next.trim().is_empty() {
                            name = next;
                        }
                    }
                    "fileext" => {
                        if let Some(current) = &ext {
                            let next = chain.apply(current);
                            if !next.trim().is_empty() && !next.contains(['/', '\\', '.']) {
                                ext = Some(next);
                            }
                        }
                    }
                    scope => {
                        for field in tag_changes.iter_mut() {
                            if scope != "tags" && scope != field.field {
                                continue;
                            }
                            // Only a value the plan actually proposes. A change
                            // that clears a field has nothing to clean up.
                            if let Some(value) = &field.new {
                                field.new = Some(chain.apply(value));
                            }
                        }
                    }
                }
            }

            // Re-validate, then drop what the cleanup turned back into a no-op.
            let tag_changes: Vec<FieldChangeDto> = tag_changes
                .into_iter()
                .filter(|field| field.old != field.new)
                .map(|field| FieldChangeDto::new(field.field, field.old, field.new))
                .collect();

            let file_name = match &ext {
                Some(ext) => format!("{name}.{ext}"),
                None => name.clone(),
            };
            let renamed = path.file_name().map(|n| n.to_string_lossy().into_owned())
                != Some(file_name.clone());

            if tag_changes.is_empty() && !renamed && change.cover_change.is_none() {
                continue;
            }
            let mut revised = FileChangeDto {
                path: change.path.clone(),
                rename_to: renamed.then(|| {
                    path.with_file_name(&file_name)
                        .to_string_lossy()
                        .into_owned()
                }),
                tag_changes,
                cover_change: change.cover_change.clone(),
                // Recomputed below rather than carried over: the sidecars follow
                // the destination name, which a file-scoped chain just changed.
                sidecar_renames: Vec::new(),
                copy: false,
            };
            if renamed {
                self.attach_sidecars(&mut revised);
            }
            changes.push(revised);
        }

        Ok(PlanDto {
            description: cleaned_up_description(&plan.description),
            changes,
            prune_empty_dirs: false,
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
    /// `destination` is the folder the rendered path is built under. `None` is
    /// the library root, which is what this always did (#153); a folder outside
    /// it is the point of the option — an unsorted download folder is tagged in
    /// place and then filed into the library, which lives somewhere else. Such a
    /// destination is remembered as a second allowed root for the session, so
    /// the executor will accept the writes it authorizes; it is the user's own
    /// pick from the folder chooser, never something read out of a mask.
    ///
    /// `copy` leaves every source file where it is. `prune_empty_dirs` removes
    /// the folders a move empties, and is meaningless for a copy.
    pub fn preview_move(
        &self,
        mask_pattern: &str,
        paths: &[PathBuf],
        destination: Option<&Path>,
        copy: bool,
        prune_empty_dirs: bool,
    ) -> Result<PlanDto, AppError> {
        let mask = Mask::parse(mask_pattern)?;
        let root = match destination {
            Some(destination) => {
                let destination = destination.to_path_buf();
                if !destination.is_dir() {
                    return Err(AppError::MissingDestination(
                        destination.to_string_lossy().into_owned(),
                    ));
                }
                self.authorize_root(&destination);
                destination
            }
            None => self.library_root.clone(),
        };
        let mut changes = Vec::new();
        for path in paths {
            let Ok(track) = TagEngine::read(path) else {
                continue;
            };
            let Ok(rendered) = mask.render_with(&track.tags, &FileContext::read(&mask, &track))
            else {
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
            let mut target = root.clone();
            for component in components {
                target.push(component);
            }
            target.push(last);
            if target == *path {
                continue;
            }
            let mut change = FileChangeDto {
                path: path.to_string_lossy().into_owned(),
                rename_to: Some(target.to_string_lossy().into_owned()),
                tag_changes: Vec::new(),
                cover_change: None,
                sidecar_renames: Vec::new(),
                copy,
            };
            self.attach_sidecars(&mut change);
            changes.push(change);
        }
        Ok(PlanDto {
            description: format!(
                "{} by mask: {mask_pattern}",
                if copy { "Copy" } else { "Reorganize" }
            ),
            changes,
            // A copy empties nothing, so there is nothing to prune either way.
            prune_empty_dirs: prune_empty_dirs && !copy,
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
                    sidecar_renames: Vec::new(),
                    copy: false,
                });
            }
        }
        Ok(PlanDto {
            description: "Edit tags".to_string(),
            changes,
            prune_empty_dirs: false,
        })
    }

    /// Build a plan that wipes every modeled text field from each selected file
    /// for a fresh start (#94), through the normal preview/apply/**undo** path so
    /// it stays reversible and journaled. Only the text tags TagRex models are
    /// cleared — the embedded cover and the non-text binary frames the write
    /// pipeline preserves (`PRIV`/`GEOB` DJ cue points/loops, ReplayGain,
    /// ratings) are deliberately left intact, so it is safe on a DJ library. Use
    /// the cover well's Remove to drop the cover as well. Files that already have
    /// no modeled tags are skipped.
    pub fn preview_clear_tags(&self, paths: &[PathBuf]) -> Result<PlanDto, AppError> {
        let mut changes = Vec::new();
        for path in paths {
            let track = TagEngine::read(path)?;
            // One clearing change per field the file actually carries, so the
            // preview diff and the undo journal record the real old values.
            let tag_changes: Vec<FieldChangeDto> = track
                .tags
                .iter()
                .map(|(field, value)| {
                    FieldChangeDto::new(field.to_storage_key(), Some(value.clone()), None)
                })
                .collect();
            if !tag_changes.is_empty() {
                changes.push(FileChangeDto {
                    path: path.to_string_lossy().into_owned(),
                    rename_to: None,
                    tag_changes,
                    cover_change: None,
                    sidecar_renames: Vec::new(),
                    copy: false,
                });
            }
        }
        Ok(PlanDto {
            description: "Clear tags".to_string(),
            changes,
            prune_empty_dirs: false,
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
        // Resize/recompress once, up front (#41), so every file embeds the same
        // trimmed image and the preview shows exactly what will be written.
        let new_art = cover_dto_to_art(cover).map(|art| {
            tagrex_core::cover::resize_cover(
                &art,
                self.cover_max_px.get(),
                self.cover_quality.get(),
            )
        });
        let mut changes = Vec::new();
        for path in paths {
            let old = TagEngine::read_covers(path)?;
            // Front cover in, every other image left as it is (#56): a picked,
            // dropped or fetched image means "this is the cover", never "throw
            // away the back and the disc".
            let mut new = old.clone();
            match &new_art {
                Some(art) => {
                    let front = art.clone();
                    match new.iter().position(|c| c.kind == CoverKind::Front) {
                        Some(at) => new[at] = front,
                        None => new.insert(0, front),
                    }
                }
                None => new.retain(|c| c.kind != CoverKind::Front),
            }
            if old == new {
                continue; // already this exact cover
            }
            changes.push(FileChangeDto {
                path: path.to_string_lossy().into_owned(),
                rename_to: None,
                tag_changes: Vec::new(),
                cover_change: Some(CoverChangeDto {
                    old: cover_arts_to_dto(&old),
                    new: cover_arts_to_dto(&new),
                }),
                sidecar_renames: Vec::new(),
                copy: false,
            });
        }
        Ok(PlanDto {
            description: "Embed cover art".to_string(),
            changes,
            prune_empty_dirs: false,
        })
    }

    /// Preview replacing every selected file's whole image set with `covers`
    /// (#56), without writing.
    ///
    /// The one command behind add, remove, reorder and retype: the panel edits
    /// its copy of the set and sends the result, which is also exactly what the
    /// plan stores and what undo writes back. Files that already carry this set
    /// are skipped. An empty `covers` removes every image.
    pub fn preview_cover_set(
        &self,
        paths: &[PathBuf],
        covers: &[CoverArtDto],
    ) -> Result<PlanDto, AppError> {
        // Resize once, up front (#41), so every file gets the same trimmed
        // images and the preview shows what will really be written.
        let new: Vec<CoverArt> = cover_dtos_to_art(covers)
            .iter()
            .map(|art| {
                tagrex_core::cover::resize_cover(
                    art,
                    self.cover_max_px.get(),
                    self.cover_quality.get(),
                )
            })
            .collect();
        let new_dtos = cover_arts_to_dto(&new);
        let mut changes = Vec::new();
        for path in paths {
            let old = TagEngine::read_covers(path)?;
            if old == new {
                continue;
            }
            changes.push(FileChangeDto {
                path: path.to_string_lossy().into_owned(),
                rename_to: None,
                tag_changes: Vec::new(),
                cover_change: Some(CoverChangeDto {
                    old: cover_arts_to_dto(&old),
                    new: new_dtos.clone(),
                }),
                sidecar_renames: Vec::new(),
                copy: false,
            });
        }
        Ok(PlanDto {
            description: if new.is_empty() {
                "Remove cover art".to_string()
            } else {
                format!("Set {} cover image(s)", new.len())
            },
            changes,
            prune_empty_dirs: false,
        })
    }

    /// Summarize the cover state across `paths` for the cover well: the total,
    /// how many carry any image, whether they differ, and either the exact set
    /// they all share (#56) or a small fan of distinct front covers when they
    /// don't.
    pub fn read_cover_summary(&self, paths: &[PathBuf]) -> Result<CoverSummaryDto, AppError> {
        let mut sets: Vec<Vec<CoverArt>> = Vec::with_capacity(paths.len());
        for path in paths {
            sets.push(TagEngine::read_covers(path)?);
        }
        let total = sets.len();
        let with_cover = sets.iter().filter(|set| !set.is_empty()).count();

        // Distinct unless the whole selection carries one identical image SET —
        // same images, same types, same order. Anything less is not a set the
        // panel could offer to edit as one.
        let mut unique: Vec<&Vec<CoverArt>> = Vec::new();
        for set in &sets {
            if !unique.contains(&set) {
                unique.push(set);
            }
        }
        let distinct = unique.len() > 1;
        let shared_set = if distinct {
            Vec::new()
        } else {
            sets.first()
                .map(|set| cover_arts_to_dto(set))
                .unwrap_or_default()
        };

        // Up to three distinct front covers, for the mixed fan. `read_cover`'s
        // rule (the front one, else the first) is what "the cover" means here.
        let mut samples: Vec<CoverArtDto> = Vec::new();
        for set in &sets {
            let Some(front) = set
                .iter()
                .find(|c| c.kind == CoverKind::Front)
                .or_else(|| set.first())
            else {
                continue;
            };
            let dto = cover_art_to_dto(front);
            if !samples.contains(&dto) {
                samples.push(dto);
                if samples.len() == 3 {
                    break;
                }
            }
        }

        Ok(CoverSummaryDto {
            total,
            with_cover,
            distinct,
            samples,
            shared_set,
        })
    }

    /// Find an external cover file (`cover.jpg` / `folder.jpg`, and the `.jpeg` /
    /// `.png` variants) sitting next to the selected tracks (#41) — the inverse
    /// of the sidecar export. Returns the first match across the selection's
    /// distinct directories, ready to feed [`preview_cover_embed`]. Read-only.
    pub fn read_external_cover(&self, paths: &[PathBuf]) -> Result<Option<CoverArtDto>, AppError> {
        // Distinct parent directories, in selection order.
        let mut dirs: Vec<&Path> = Vec::new();
        for path in paths {
            if let Some(dir) = path.parent() {
                if !dirs.contains(&dir) {
                    dirs.push(dir);
                }
            }
        }
        for dir in dirs {
            if let Some(found) = external_cover_in(dir) {
                let data = std::fs::read(&found).map_err(AppError::Io)?;
                return Ok(Some(CoverArtDto {
                    mime: mime_for_cover_path(&found).to_string(),
                    data_base64: base64::engine::general_purpose::STANDARD.encode(&data),
                    ..CoverArtDto::default()
                }));
            }
        }
        Ok(None)
    }

    /// Preview removing the front cover from every `paths` file that has one,
    /// through the normal preview/apply/undo path (files without a cover are
    /// skipped).
    pub fn preview_cover_remove(&self, paths: &[PathBuf]) -> Result<PlanDto, AppError> {
        let mut changes = Vec::new();
        for path in paths {
            // Every image, not just the front one (#56) — this is the panel's
            // "remove all", and per-image removal goes through `preview_cover_set`.
            let old = TagEngine::read_covers(path)?;
            if old.is_empty() {
                continue;
            }
            changes.push(FileChangeDto {
                path: path.to_string_lossy().into_owned(),
                rename_to: None,
                tag_changes: Vec::new(),
                cover_change: Some(CoverChangeDto {
                    old: cover_arts_to_dto(&old),
                    new: Vec::new(),
                }),
                sidecar_renames: Vec::new(),
                copy: false,
            });
        }
        Ok(PlanDto {
            description: "Remove cover art".to_string(),
            changes,
            prune_empty_dirs: false,
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

    /// Export `paths` as a self-contained HTML table into the library root (#42).
    pub fn export_html(&self, paths: &[PathBuf], file_name: &str) -> Result<String, AppError> {
        let target = self.export_target(file_name)?;
        let tracks = read_tracks(paths);
        std::fs::write(&target, export::html(&tracks))?;
        Ok(target.to_string_lossy().into_owned())
    }

    /// Export `paths` as an XML document into the library root (#42).
    pub fn export_xml(&self, paths: &[PathBuf], file_name: &str) -> Result<String, AppError> {
        let target = self.export_target(file_name)?;
        let tracks = read_tracks(paths);
        std::fs::write(&target, export::xml(&tracks))?;
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

    /// Remember a destination the user has chosen as a place this session may
    /// write into (#153), beside the library root.
    fn authorize_root(&self, root: &Path) {
        let mut roots = self.extra_roots.borrow_mut();
        if !roots.iter().any(|existing| existing == root) {
            roots.push(root.to_path_buf());
        }
    }

    /// Everywhere this session is allowed to write: the open library, plus any
    /// destination the user picked for a reorganize (#153).
    fn allowed_roots(&self) -> Vec<PathBuf> {
        let mut roots = vec![self.library_root.clone()];
        roots.extend(self.extra_roots.borrow().iter().cloned());
        roots
    }

    /// Apply a previewed plan to disk and record it for undo.
    pub fn apply(&mut self, plan: &PlanDto) -> Result<BatchDto, AppError> {
        let change_plan = plan.to_change_plan();
        let roots = self.allowed_roots();
        let batch = Executor::apply(&change_plan, &mut self.journal, &roots)?;
        Ok(BatchDto::from(&batch))
    }

    /// Roll back a previously applied batch.
    pub fn undo(&mut self, batch_id: i64) -> Result<(), AppError> {
        let roots = self.allowed_roots();
        Executor::undo(&mut self.journal, BatchId(batch_id), &roots)?;
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
            // Judged by where the files CAME FROM, not where they went (#153):
            // a reorganize can now file them into a folder outside the library,
            // and testing the destination would hide exactly the batch the user
            // is most likely to want back.
            .filter(|batch| {
                batch
                    .plan
                    .changes
                    .iter()
                    .all(|change| under_root(&change.path))
            })
            .map(BatchDto::from)
            .collect())
    }

    /// Search a metadata provider (`source` = "discogs" | "musicbrainz") with
    /// the given personal token (ignored by token-less providers).
    ///
    /// Results are re-scored against the query text and re-sorted: the provider
    /// score is only "the API returned this one first", which is not evidence of
    /// a better match (#53).
    pub fn provider_search(
        &self,
        source: &str,
        token: &str,
        query: &SearchQueryDto,
    ) -> Result<Vec<CandidateDto>, AppError> {
        self.throttle(source);
        let search = query.to_search_query();
        let candidates = match source {
            "musicbrainz" => self.musicbrainz_provider()?.search(&search)?,
            _ => self.discogs_provider(token)?.search(&search)?,
        };
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

    /// Fetch a full release from a provider (`source` selects it).
    pub fn provider_fetch_release(
        &self,
        source: &str,
        token: &str,
        id: &str,
    ) -> Result<ReleaseDto, AppError> {
        self.throttle(source);
        let rid = ReleaseId(id.to_string());
        let release = match source {
            "musicbrainz" => self.musicbrainz_provider()?.fetch_release(&rid)?,
            _ => self.discogs_provider(token)?.fetch_release(&rid)?,
        };
        Ok(ReleaseDto::from(&release))
    }

    /// Download a provider image (e.g. a release's cover) and return it as a
    /// cover DTO, ready to feed straight into [`App::preview_cover_embed`] — the
    /// same shape a locally chosen file produces, so the fetched art flows
    /// through the identical preview/apply/undo path. `source` selects the
    /// provider so its image fetch uses the right host/headers (Discogs' CDN
    /// needs the token + User-Agent; the Cover Art Archive needs neither).
    pub fn provider_fetch_image(
        &self,
        source: &str,
        token: &str,
        url: &str,
    ) -> Result<CoverArtDto, AppError> {
        self.throttle(source);
        let image = match source {
            "musicbrainz" => self.musicbrainz_provider()?.fetch_image(url)?,
            _ => self.discogs_provider(token)?.fetch_image(url)?,
        };
        Ok(CoverArtDto {
            mime: image.mime,
            data_base64: base64::engine::general_purpose::STANDARD.encode(&image.data),
            ..CoverArtDto::default()
        })
    }

    /// Save a release's images to disk next to the selected tracks (#102).
    ///
    /// Names them positionally, as the user chose: the primary -> `folder.<ext>`,
    /// the rest -> `cover.<ext>`, `cover-1.<ext>`, `cover-2.<ext>`… (extension
    /// from each image's MIME). Confined to the opened library root. Fetches
    /// every image first (so extensions are known), then, if any target already
    /// exists and `overwrite` is false, writes NOTHING and returns those names so
    /// the UI can confirm — otherwise writes them all.
    pub fn save_release_images(
        &self,
        source: &str,
        token: &str,
        track: &Path,
        urls: &[String],
        overwrite: bool,
    ) -> Result<SaveImagesDto, AppError> {
        let root = std::fs::canonicalize(&self.library_root)?;
        let dir = track.parent().unwrap_or(Path::new("."));
        let canonical_dir = std::fs::canonicalize(dir)?;
        if !canonical_dir.starts_with(&root) {
            return Err(AppError::OutsideRoot(track.to_string_lossy().into_owned()));
        }
        let mut planned: Vec<(PathBuf, Vec<u8>)> = Vec::new();
        for (index, url) in urls.iter().enumerate() {
            self.throttle(source);
            let image = match source {
                "musicbrainz" => self.musicbrainz_provider()?.fetch_image(url)?,
                _ => self.discogs_provider(token)?.fetch_image(url)?,
            };
            let ext = extension_for_mime(&image.mime);
            let name = format!("{}.{ext}", image_basename(index));
            planned.push((canonical_dir.join(name), image.data));
        }
        let conflicts: Vec<String> = planned
            .iter()
            .filter(|(target, _)| target.exists())
            .filter_map(|(target, _)| target.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .collect();
        if !conflicts.is_empty() && !overwrite {
            return Ok(SaveImagesDto {
                written: Vec::new(),
                conflicts,
            });
        }
        let mut written = Vec::new();
        for (target, data) in &planned {
            std::fs::write(target, data)?;
            written.push(target.to_string_lossy().into_owned());
        }
        Ok(SaveImagesDto {
            written,
            conflicts: Vec::new(),
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
    ) -> Result<Vec<Option<AlignMatchDto>>, AppError> {
        let locals: Vec<(String, String, Option<u64>, Option<String>)> = paths
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
                let isrc = track
                    .as_ref()
                    .and_then(|track| track.tags.get(&TagField::Isrc).cloned())
                    .filter(|isrc| !isrc.is_empty());
                let duration = TagEngine::read_duration(path)
                    .ok()
                    .map(|duration| duration.as_secs())
                    .filter(|secs| *secs > 0);
                (title, artist, duration, isrc)
            })
            .collect();

        let local_refs: Vec<TrackRef> = locals
            .iter()
            .map(|(title, artist, duration, isrc)| TrackRef {
                title,
                artist: Some(artist.as_str()).filter(|artist| !artist.is_empty()),
                duration_secs: *duration,
                isrc: isrc.as_deref(),
            })
            .collect();
        let candidate_refs: Vec<TrackRef> = tracks
            .iter()
            .map(|track| TrackRef {
                title: &track.title,
                artist: Some(track.artist.as_str()).filter(|artist| !artist.is_empty()),
                duration_secs: track.duration_secs,
                isrc: track.isrc.as_deref().filter(|isrc| !isrc.is_empty()),
            })
            .collect();

        // Tag each aligned pair with whether an ISRC drove it, so the UI can say
        // why a file matched (#54). Duration-sequence matches are never ISRC.
        let annotate = |aligned: Vec<Option<usize>>| -> Vec<Option<AlignMatchDto>> {
            aligned
                .into_iter()
                .enumerate()
                .map(|(i, slot)| {
                    slot.map(|track| AlignMatchDto {
                        track,
                        by_isrc: matching::is_isrc_match(&local_refs[i], &candidate_refs[track]),
                    })
                })
                .collect()
        };

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
                return Ok(annotate(by_duration));
            }
        }
        Ok(annotate(by_content))
    }

    /// Render `pattern` over `paths` for a mask-defined table column (#150) —
    /// one string per path, in the order given.
    ///
    /// Batched deliberately. The mask engine lives in Rust (one grammar, two
    /// directions — architecture.md), so a column that reimplemented rendering
    /// in the frontend would be a second grammar drifting from the first. But a
    /// round-trip per cell per repaint is not an option either, so the frontend
    /// asks once per pattern and caches the answers against the paths.
    ///
    /// Rendering is lenient in the same way a report is: a placeholder with
    /// nothing behind it becomes empty rather than dropping the row, because a
    /// column must show something for every file. A file that cannot be read at
    /// all, or a pattern that refuses to render (an extract-only `%skip%`),
    /// yields an empty string for that row rather than failing the batch — a
    /// half-typed pattern is a normal state while someone is typing one.
    pub fn render_column(&self, pattern: &str, paths: &[PathBuf]) -> Result<Vec<String>, AppError> {
        let mask = Mask::parse(pattern)?;
        Ok(paths
            .iter()
            .map(|path| {
                let Ok(track) = TagEngine::read(path) else {
                    return String::new();
                };
                let file = FileContext::read(&mask, &track);
                mask.render_with(&export::lenient_tags(&track.tags), &file)
                    .unwrap_or_default()
            })
            .collect())
    }

    /// Preview importing a user-resolved release selection onto `paths`,
    /// without writing. The frontend decides the mapping, the way a manual
    /// tagger does: the user toggles which release tracks participate and
    /// orders the files to match, so here the i-th enabled track simply maps
    /// onto the i-th file. Album-level fields go to every file; per-track fields
    /// (title/artist/track number) to files that line up with a selected
    /// track. The track number comes from the release track's own position,
    /// not the selection index, so an aligned file keeps its real number.
    /// Reads current values for `old` and drops no-op edits, flowing through
    /// the same preview/apply/undo path as manual edits.
    pub fn preview_import(
        &self,
        paths: &[PathBuf],
        selection: &ImportSelectionDto,
        vinyl_sides_to_disc: bool,
    ) -> Result<PlanDto, AppError> {
        let mut changes = Vec::new();
        // Per-side running track number, used only for vinyl positions that carry
        // a side but no digit ("B" = the single track on side B): each side (disc)
        // restarts at 1, in the order the tracks are imported.
        let mut side_track_counters: std::collections::HashMap<u32, u32> =
            std::collections::HashMap::new();
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
                // Release id (#20): an album-level identifier the table can group
                // by, written under the provider's conventional tag key.
                (
                    release_id_field(selection.source.as_deref()),
                    non_empty(selection.release_id.clone()),
                ),
                // Label + catalogue number (#90): the user's chosen single pair
                // (label → Publisher, catno → CatalogNumber), never merged.
                (TagField::Publisher, non_empty(selection.label.clone())),
                (
                    TagField::CatalogNumber,
                    non_empty(selection.catalog_number.clone()),
                ),
                // Album-level total track count → TrackTotal (so a file reads
                // N/total), and the release country → a portable RELEASECOUNTRY
                // custom tag.
                (
                    TagField::TrackTotal,
                    non_empty(selection.track_total.clone()),
                ),
                (
                    TagField::Custom("RELEASECOUNTRY".to_string()),
                    non_empty(selection.country.clone()),
                ),
                // Release webpage → WOAF URL frame.
                (TagField::Url, non_empty(selection.url.clone())),
                // Physical medium (Vinyl / CD / Cassette / File) → media frame.
                (TagField::MediaType, non_empty(selection.media_type.clone())),
            ];
            if let Some(track) = selection.tracks.get(index) {
                let artist = non_empty(Some(track.artist.clone()))
                    .or_else(|| non_empty(selection.album_artist.clone()));
                desired.push((TagField::Title, non_empty(Some(track.title.clone()))));
                desired.push((TagField::Artist, artist));
                // Write the provider's ISRC when the file is missing one (#54);
                // preview_import only emits a change when it actually differs.
                desired.push((TagField::Isrc, non_empty(track.isrc.clone())));

                // Vinyl side -> disc (#105): a vinyl-side position ("A1", the
                // reverse "1A", or a bare side "B") can't keep its letter in the
                // integer track-number tag, so -- when the toggle is on -- the
                // side maps to a disc number (A->1, B->2, ...) and the track
                // number restarts per side. This is an explicit per-import opt-in,
                // so it overwrites a disc (a file's default "disc 1" must not
                // block side B becoming disc 2).
                let side_disc = if vinyl_sides_to_disc {
                    side_disc_from_position(&track.position)
                } else {
                    None
                };
                let position_number = match side_disc {
                    // Digits in the position are already per-side ("A1" -> 1);
                    // a bare side ("B") has none, so take the next per-side number.
                    Some(disc) => {
                        track_number_from_position(&track.position).unwrap_or_else(|| {
                            let counter = side_track_counters.entry(disc).or_insert(0);
                            *counter += 1;
                            counter.to_string()
                        })
                    }
                    // Not a vinyl side: the position's number, else the row index.
                    None => track_number_from_position(&track.position)
                        .unwrap_or_else(|| (index + 1).to_string()),
                };

                // Track number: leave the file's existing number alone when it
                // already means the same thing (an aligned "01" isn't reformatted
                // to "1") -- compare numerically, change only on a real difference.
                let current_number = current
                    .tags
                    .get(&TagField::TrackNumber)
                    .and_then(|value| track_number_from_position(value));
                if current_number.as_deref() != Some(position_number.as_str()) {
                    desired.push((TagField::TrackNumber, Some(position_number)));
                }
                // Disc number, in precedence order (#146):
                //
                // 1. the vinyl side, when that opt-in is on — an explicit
                //    instruction, so it wins and overwrites;
                // 2. the disc the release states (a Discogs `1-05` position, a
                //    MusicBrainz medium) — the provider's own answer;
                // 3. the file's folder, a guess about the file rather than a
                //    statement about the release, so only when it has no disc yet;
                // 4. plain `1`, but ONLY when the release states it holds exactly
                //    one disc (#157).
                //
                // That last one is not the invention it looks like. A release
                // whose format quantity reads 1 is not silent about the disc —
                // it says there is exactly one, which puts every track on disc 1
                // of 1. The count is what licenses it: on a release stating two
                // or more, a track whose position carries no prefix is genuinely
                // unplaced, and 1 would be a guess. A release stating no count at
                // all still writes nothing.
                let disc = side_disc
                    .or(track.disc)
                    .or_else(|| {
                        disc_from_folder_name(path, current.tags.get(&TagField::DiscNumber))
                    })
                    .or_else(|| single_disc_release(selection).then_some(1));
                if let Some(disc) = disc {
                    let disc = disc.to_string();
                    if current.tags.get(&TagField::DiscNumber).map(String::as_str)
                        != Some(disc.as_str())
                    {
                        desired.push((TagField::DiscNumber, Some(disc)));
                    }
                }
                // "of N" — but only where there is a disc for it to complete
                // (#146). Unlike a track number, a disc number can be absent
                // altogether, and a DiscTotal on its own is half a pair. Since
                // #157 a stated single-disc release does get a disc, so those
                // files now read 1/1 rather than nothing at all. Album-level
                // value, applied per file for that reason.
                if disc.is_some() || current.tags.contains_key(&TagField::DiscNumber) {
                    desired.push((TagField::DiscTotal, non_empty(selection.disc_total.clone())));
                }
            }

            let mut tag_changes = Vec::new();
            // One gate for every field the import produces (#152), here at the
            // end rather than beside each push: a field added to `desired` later
            // is covered without anyone having to remember this exists.
            let skip = self.import_skip_fields.borrow();
            for (field, new) in desired {
                let key = field.to_storage_key();
                if skip.contains(&key) {
                    continue;
                }
                let new = new.filter(|value| !value.is_empty());
                let old = current.tags.get(&field).cloned();
                if new.is_some() && old != new {
                    tag_changes.push(FieldChangeDto::new(key, old, new));
                }
            }
            drop(skip);
            if !tag_changes.is_empty() {
                changes.push(FileChangeDto {
                    path: path.to_string_lossy().into_owned(),
                    rename_to: None,
                    tag_changes,
                    cover_change: None,
                    sidecar_renames: Vec::new(),
                    copy: false,
                });
            }
        }
        Ok(PlanDto {
            description: "Import Discogs release".to_string(),
            changes,
            prune_empty_dirs: false,
        })
    }
}

/// The description a plan carries once a cleanup chain has been run over it
/// (#142). The plan is still the one the user staged — the bar should keep
/// saying where it came from — with a note that it has been cleaned up. Running
/// a second chain over it does not stack a second note: it is the same staged
/// plan either way.
fn cleaned_up_description(description: &str) -> String {
    const SUFFIX: &str = " · cleaned up";
    if description.ends_with(SUFFIX) {
        return description.to_string();
    }
    format!("{description}{SUFFIX}")
}

/// The tag field a release id is stored under, by provider source (#20).
/// `MUSICBRAINZ_ALBUMID` is the de-facto standard (what Picard writes); Discogs
/// has no standard tag, so a matching `DISCOGS_RELEASE_ID` custom field is used.
/// A `Custom` field round-trips as a TXXX frame / Vorbis comment on every
/// format, and the table groups on whichever is present.
fn release_id_field(source: Option<&str>) -> TagField {
    match source {
        Some("musicbrainz") => TagField::Custom("MUSICBRAINZ_ALBUMID".to_string()),
        _ => TagField::Custom("DISCOGS_RELEASE_ID".to_string()),
    }
}

/// The string a mask is matched against when reading tags out of a name
/// (#139): the file's stem, preceded by `depth` parent directory names joined
/// with `/`. `depth` is how many separators the pattern carries, so
/// `%album%/%title%` sees `Album/01 - Title` while `%title%` sees only the
/// stem. `None` when the path is shallower than the pattern asks for, or when
/// any component isn't valid UTF-8.
fn name_subject(path: &Path, depth: usize) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let mut parts = Vec::with_capacity(depth + 1);
    let mut dir = path.parent();
    for _ in 0..depth {
        let current = dir?;
        parts.push(current.file_name()?.to_str()?);
        dir = current.parent();
    }
    parts.reverse();
    parts.push(stem);
    Some(parts.join("/"))
}

/// Clean up one value pulled out of a file name (#139) before it becomes a tag
/// change. Whitespace around a capture is an artifact of the separators in the
/// pattern, never part of the value. Beyond that only the integer fields are
/// touched: the tag backend stores them as numbers, so a name's `05` is written
/// as `5` either way — normalizing here keeps the preview honest about what
/// will end up on disk instead of showing a diff that then writes something
/// else. A non-numeric value (a vinyl `A1`) is left as it is, to be flagged by
/// the same validation the editor uses rather than silently reinterpreted.
fn normalize_extracted(field: &TagField, value: &str) -> String {
    let value = value.trim();
    match field {
        TagField::TrackNumber
        | TagField::TrackTotal
        | TagField::DiscNumber
        | TagField::DiscTotal => value
            .parse::<u32>()
            .map(|n| n.to_string())
            .unwrap_or_else(|_| value.to_string()),
        _ => value.to_string(),
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

/// Whether the release states it holds exactly one disc (#157).
///
/// `None` — the provider said nothing about a count — is deliberately NOT the
/// same as `Some(1)`: the first is silence, the second is a statement, and only
/// a statement licenses defaulting a track to disc 1.
fn single_disc_release(selection: &ImportSelectionDto) -> bool {
    selection
        .disc_total
        .as_deref()
        .map(str::trim)
        .and_then(|total| total.parse::<u32>().ok())
        == Some(1)
}

/// Last-resort disc number from the file's own folder (#146): `CD2`, `CD 2`,
/// `Disc 3`, `disk_1`.
///
/// Only reached when neither the vinyl-side opt-in nor the release itself gave
/// an answer, and only when the file carries no disc yet — `existing` short-
/// circuits it, because a folder name is a guess about one file, not a statement
/// about the release, and it must never overwrite something real.
///
/// A **keyword is required**. A folder merely *ending* in a number is not a
/// disc: the compilation series that prompted this issue is filed as
/// `…_(as_5606)_(1996) 2`, where the 2 is the volume, and reading that as disc 2
/// would tag every track of a single-CD release wrongly. Guessing is worse than
/// leaving the tag alone.
fn disc_from_folder_name(path: &Path, existing: Option<&String>) -> Option<u32> {
    if existing.is_some_and(|value| !value.trim().is_empty()) {
        return None;
    }
    let folder = path.parent()?.file_name()?.to_str()?.to_ascii_lowercase();
    // The keyword has to start a word. Without that check `cd`/`disc` match
    // inside ordinary words — `Discography`, or a folder named `nodisc-2` — and
    // whatever digits happen to follow become a disc number.
    let start = ["cd", "disc", "disk"]
        .iter()
        .flat_map(|word| {
            folder
                .match_indices(word)
                .map(move |(at, _)| (at, at + word.len()))
        })
        .filter(|(at, _)| *at == 0 || !folder.as_bytes()[at - 1].is_ascii_alphanumeric())
        .map(|(_, end)| end)
        .max()?;
    let digits: String = folder[start..]
        .trim_start_matches([' ', '_', '-', '.'])
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    // A plausible disc number, not any number that follows the word. Box sets
    // top out in the tens, so a folder yielding 99831 is not naming a disc --
    // it is a keyword that happened to be followed by digits, which is exactly
    // what a directory called `…-single-disc-99831` is.
    digits
        .parse::<u32>()
        .ok()
        .filter(|disc| (1..=99).contains(disc))
}

/// Map a vinyl-side track *position* to a disc number (#105): the side letter
/// becomes A->1, B->2, ... . Handles a bare side ("B"), the common side-first
/// "A1", and the reverse "1A"; any numeric part must be plain digits. Returns
/// `None` for a plain number ("5"), a disc-track pair ("1-05"), or anything
/// without a single A-Z side. Only the side matters here -- the track number
/// itself comes from [`track_number_from_position`] (or a per-side counter).
fn side_disc_from_position(position: &str) -> Option<u32> {
    let bytes = position.trim().as_bytes();
    if bytes.is_empty() {
        return None;
    }
    // Side-first: a leading A-Z followed by nothing or only digits ("B", "A1").
    let side = if bytes[0].is_ascii_alphabetic() && bytes[1..].iter().all(u8::is_ascii_digit) {
        bytes[0]
    // Reverse: one or more digits ending in a single A-Z ("1A", "12B").
    } else if bytes.len() >= 2
        && bytes[bytes.len() - 1].is_ascii_alphabetic()
        && bytes[..bytes.len() - 1].iter().all(u8::is_ascii_digit)
    {
        bytes[bytes.len() - 1]
    } else {
        return None;
    };
    let ordinal = u32::from(side.to_ascii_uppercase() - b'A') + 1;
    (1..=26).contains(&ordinal).then_some(ordinal)
}

/// Normalize a tag value into a duplicate-grouping key (#40): lower-cased with
/// runs of whitespace collapsed, so "The  Field" and "the field" group together.
fn norm_key(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The external cover file in `dir`, if any (#41): a `cover`/`folder`/`front`
/// named `.jpg`/`.jpeg`/`.png`, matched case-insensitively (so it works on
/// case-sensitive filesystems too) and preferring `cover` over `folder` over
/// `front`.
fn external_cover_in(dir: &Path) -> Option<PathBuf> {
    const PREFERRED: [&str; 3] = ["cover", "folder", "front"];
    const EXTS: [&str; 3] = ["jpg", "jpeg", "png"];
    let mut best: Option<(usize, PathBuf)> = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase());
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        let (Some(stem), Some(ext)) = (stem, ext) else {
            continue;
        };
        if !EXTS.contains(&ext.as_str()) {
            continue;
        }
        if let Some(rank) = PREFERRED.iter().position(|p| *p == stem) {
            if best.as_ref().is_none_or(|(r, _)| rank < *r) {
                best = Some((rank, path));
            }
        }
    }
    best.map(|(_, path)| path)
}

/// The MIME type for an external cover path, by extension (defaults to JPEG).
fn mime_for_cover_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        _ => "image/jpeg",
    }
}

/// MIME type for a would-be cover image by extension, or `None` when the
/// extension isn't a recognized image format. Broader than
/// [`mime_for_cover_path`] because a dropped cover (#133) can be any image the
/// user chose, not just the `cover.jpg` / `folder.png` sidecar set.
fn mime_for_image_path(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("jpg" | "jpeg") => Some("image/jpeg"),
        Some("png") => Some("image/png"),
        Some("webp") => Some("image/webp"),
        Some("gif") => Some("image/gif"),
        Some("bmp") => Some("image/bmp"),
        Some("tif" | "tiff") => Some("image/tiff"),
        _ => None,
    }
}

/// Read an image file into a [`CoverArtDto`] (#133), ready to feed
/// [`App::preview_cover_embed`]. Used when the user drops an image onto the
/// cover well: the drag-drop event gives a path, not a browser `File`. The
/// extension must be a recognized image format, or this errors rather than
/// embedding arbitrary bytes.
pub fn read_cover_image(path: &Path) -> Result<CoverArtDto, AppError> {
    let mime = mime_for_image_path(path)
        .ok_or_else(|| AppError::NotAnImage(path.display().to_string()))?;
    let data = std::fs::read(path)?;
    Ok(CoverArtDto {
        mime: mime.to_string(),
        data_base64: base64::engine::general_purpose::STANDARD.encode(&data),
        ..CoverArtDto::default()
    })
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.is_empty())
}

/// One step of a shipped preset, spelled out positionally so the tables below
/// read as data. `style` doubles as the case style / key notation, unused by
/// `replace`.
fn preset_rule(kind: &str, from: &str, to: &str, regex: bool, style: &str) -> TransformRuleDto {
    TransformRuleDto {
        kind: kind.into(),
        from: from.into(),
        to: to.into(),
        regex,
        whole_word: false,
        case_sensitive: false,
        style: style.into(),
        enabled: true,
    }
}

/// One row of the "what may an online import write" setting (#152).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportFieldDto {
    /// The storage keys this row governs. Usually one; the release id is two,
    /// since the key depends on which provider the release came from and a user
    /// unticking it means both.
    pub keys: Vec<String>,
    pub label: String,
}

/// Every tag field an online import can write (#152), in the order the setting
/// lists them.
///
/// The import itself builds its fields inline, per file, so this cannot be
/// derived from that code — but it must not drift from it either. The test
/// `import_field_catalogue_covers_everything_an_import_writes` runs a maximal
/// import and asserts the two agree in both directions, which is what keeps a
/// newly added field from being unlistable and an obsolete row from lingering.
///
/// The cover is deliberately absent: embedding one is its own action on the
/// release card, not something `preview_import` does, so there is nothing here
/// to switch off.
pub fn import_fields() -> Vec<ImportFieldDto> {
    let one = |key: &str, label: &str| ImportFieldDto {
        keys: vec![key.to_string()],
        label: label.to_string(),
    };
    vec![
        one("title", "Title"),
        one("artist", "Artist"),
        one("album", "Album"),
        one("albumartist", "Album artist"),
        one("track", "Track number"),
        one("tracktotal", "Track total"),
        one("disc", "Disc number"),
        one("disctotal", "Disc total"),
        one("year", "Year"),
        one("genre", "Genre"),
        one("publisher", "Label / publisher"),
        one("catalognumber", "Catalogue number"),
        one("isrc", "ISRC"),
        one("url", "Release webpage"),
        one("media", "Media type"),
        one("custom:RELEASECOUNTRY", "Release country"),
        ImportFieldDto {
            keys: vec![
                "custom:DISCOGS_RELEASE_ID".to_string(),
                "custom:MUSICBRAINZ_ALBUMID".to_string(),
            ],
            label: "Release id".to_string(),
        },
    ]
}

/// One placeholder as the in-app reference shows it (#148).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlaceholderDto {
    /// Ready to insert, percent signs included — `%catalognumber%`.
    pub token: String,
    pub name: String,
    pub description: String,
    /// Section heading: Tags / File / Technical / Special.
    pub group: String,
    pub render: bool,
    pub extract: bool,
}

/// Every placeholder the mask parser accepts (#148).
///
/// Read-only and stateless, like the preset library: it describes the grammar,
/// not the open library, so the reference works before a folder is even opened —
/// which is the point. Being offline is exactly when there is no documentation
/// to fall back on.
pub fn mask_placeholders() -> Vec<PlaceholderDto> {
    tagrex_core::mask::placeholder_reference()
        .into_iter()
        .map(|doc| PlaceholderDto {
            token: format!("%{}%", doc.name),
            name: doc.name.to_string(),
            description: doc.description.to_string(),
            group: doc.group.label().to_string(),
            render: doc.render,
            extract: doc.extract,
        })
        .collect()
}

/// The preset library that ships with the app (#137).
///
/// These are ordinary action groups — same rule shape, same scopes, run through
/// the same preview/apply/undo path as a group the user saved. They differ only
/// in where they live: in the binary rather than in settings.json, so they can't
/// be deleted and can't drift. Loading one copies its steps into the live chain
/// to edit and save under a new name; the preset itself stays as shipped.
///
/// Every one is a chain a user could have built by hand. Nothing here needs a
/// rule kind that doesn't already exist — a preset that would is a feature
/// request, not a preset. `builtin_presets_all_build` keeps that honest.
pub fn builtin_action_groups() -> Vec<ActionGroupDto> {
    let group =
        |name: &str, scope: &str, note: &str, rules: Vec<TransformRuleDto>| ActionGroupDto {
            name: name.into(),
            scope: scope.into(),
            rules,
            note: note.into(),
        };
    let re = |from: &str, to: &str| preset_rule("replace", from, to, true, "");
    let case = |style: &str| preset_rule("case", "", "", false, style);
    let step = |kind: &str| preset_rule(kind, "", "", false, "");

    vec![
        group(
            "Standard values",
            "tags",
            "Collapse runs of whitespace and trim the ends.",
            vec![re(r"\s+", " "), re(r"^\s+|\s+$", "")],
        ),
        group(
            "Discogs cleanup",
            "tags",
            "Drop the numeric disambiguator on artist names — \"Sunbeam (2)\" becomes \"Sunbeam\".",
            vec![re(r"\s*\(\d+\)$", "")],
        ),
        group(
            "Normalize english",
            "tags",
            "Title-case, keeping the acronyms and roman numerals the case step knows about.",
            vec![case("title")],
        ),
        group(
            "General Latin",
            "tags",
            "Romanize non-Latin scripts, then strip any accents left behind.",
            vec![step("transliterate"), step("diacritics")],
        ),
        group(
            "No dash",
            "tags",
            "Replace dashes with spaces, then collapse the gaps that leaves.",
            vec![
                re(r"[-\x{2013}\x{2014}]+", " "),
                re(r"\s{2,}", " "),
                re(r"^\s+|\s+$", ""),
            ],
        ),
        group(
            "Lower case",
            "filename",
            "Lower-case the file name's stem, extension untouched.",
            vec![case("lower")],
        ),
        group(
            "File extension",
            "fileext",
            "Lower-case the extension alone — \".FLAC\" becomes \".flac\".",
            vec![case("lower")],
        ),
        group(
            "FTP format",
            "filename",
            "Plain ASCII, no spaces — a file name that survives any server.",
            vec![
                step("transliterate"),
                step("diacritics"),
                re(r"[^A-Za-z0-9._-]+", "_"),
                re(r"_{2,}", "_"),
                re(r"^_+|_+$", ""),
            ],
        ),
    ]
}

/// Turn the UI's rule list into a transform chain, rejecting a malformed rule
/// rather than silently dropping it — a rule that quietly does nothing is worse
/// than an error, because the preview would look like a no-op.
fn build_chain(rules: &[TransformRuleDto]) -> Result<TransformChain, AppError> {
    let mut chain = TransformChain::default();
    for rule in rules {
        // A disabled step (#57) stays in the chain but contributes nothing.
        if !rule.enabled {
            continue;
        }
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
            "transliterate" => chain.push(Box::new(Transliterate)),
            "untransliterate" => chain.push(Box::new(Untransliterate)),
            "key" => {
                let style = match rule.style.as_str() {
                    "camelot" => KeyStyle::Camelot,
                    "openkey" => KeyStyle::OpenKey,
                    "musical" => KeyStyle::Musical,
                    other => return Err(AppError::UnknownTransform(other.to_string())),
                };
                chain.push(Box::new(KeyNotation::new(style)));
            }
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
/// Positional base name for a saved release image (#102): the primary (index 0)
/// becomes `folder` — the de-facto external-cover name the app auto-reads — and
/// the rest become `cover`, `cover-1`, `cover-2`, … The caller appends the
/// MIME-derived extension.
fn image_basename(index: usize) -> String {
    match index {
        0 => "folder".to_string(),
        1 => "cover".to_string(),
        n => format!("cover-{}", n - 1),
    }
}

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

fn cover_dtos_to_art(dtos: &[CoverArtDto]) -> Vec<CoverArt> {
    dtos.iter().filter_map(cover_dto_to_art).collect()
}

fn cover_dto_to_art(dto: &CoverArtDto) -> Option<CoverArt> {
    let data = base64::engine::general_purpose::STANDARD
        .decode(dto.data_base64.as_bytes())
        .ok()?;
    Some(CoverArt {
        mime: dto.mime.clone(),
        data,
        kind: CoverKind::from_storage_key(&dto.kind),
        description: dto.description.clone(),
    })
}

fn cover_art_to_dto(art: &CoverArt) -> CoverArtDto {
    CoverArtDto {
        mime: art.mime.clone(),
        data_base64: base64::engine::general_purpose::STANDARD.encode(&art.data),
        kind: art.kind.to_storage_key().to_string(),
        description: art.description.clone(),
    }
}

fn cover_arts_to_dto(arts: &[CoverArt]) -> Vec<CoverArtDto> {
    arts.iter().map(cover_art_to_dto).collect()
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
            unreadable: false,
        }
    }
}

impl PlanDto {
    fn to_change_plan(&self) -> ChangePlan {
        ChangePlan {
            description: self.description.clone(),
            prune_empty_dirs: self.prune_empty_dirs,
            changes: self
                .changes
                .iter()
                .map(|change| FileChange {
                    path: PathBuf::from(&change.path),
                    rename_to: change.rename_to.as_ref().map(PathBuf::from),
                    copy: change.copy,
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
                        old: cover_dtos_to_art(&c.old),
                        new: cover_dtos_to_art(&c.new),
                    }),
                    sidecar_renames: change
                        .sidecar_renames
                        .iter()
                        .map(|(from, to)| (PathBuf::from(from), PathBuf::from(to)))
                        .collect(),
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
            format: self.format.clone(),
            page: self.page,
            per_page: self.per_page,
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
                    disc: track.disc,
                    artist: track.artist.clone(),
                    title: track.title.clone(),
                    duration_secs: track.duration_secs,
                    isrc: track.isrc.clone(),
                })
                .collect(),
            labels: release
                .labels
                .iter()
                .map(|label| ReleaseLabelDto {
                    name: label.name.clone(),
                    catalog_number: label.catalog_number.clone(),
                })
                .collect(),
            country: release.country.clone(),
            format: release.format.clone(),
            disc_total: release.disc_total,
            url: release.url.clone(),
            cover_image_url: release.cover_image_url.clone(),
            images: release
                .images
                .iter()
                .map(|image| ReleaseImageDto {
                    url: image.url.clone(),
                    width: image.width,
                    height: image.height,
                })
                .collect(),
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
    #[error("nothing to open: the drop contained no audio files")]
    EmptyDrop,
    #[error("not an image file: {0}")]
    NotAnImage(String),
    #[error("the destination folder does not exist: {0}")]
    MissingDestination(String),
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

        /// Write a minimal tagged FLAC at `rel` (relative to the temp root),
        /// creating any parent directories. Returns the absolute path.
        fn tagged_flac_at(&self, rel: &str, artist: &str, title: &str) -> PathBuf {
            let path = self.0.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
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

    // #46: editing one field must not flatten a multi-value one on the way past.
    // This is the loss the issue reports — a file with two artists came back
    // with only the last, and the next write put that one value over both — so
    // the guard belongs on the path an ordinary cell edit takes, not only on
    // the engine.
    #[test]
    fn an_edit_leaves_the_other_fields_multi_values_intact() {
        let dir = TempDir::new("multi-value-edit");
        let path = dir.tagged_flac("x.flac", "placeholder", "Old Title");
        // Seed a genuine multi-value artist, the way a file from elsewhere has it.
        let mut seeded = TagEngine::read(&path).unwrap();
        seeded
            .tags
            .insert(TagField::Artist, "Autechre; Gescom".to_string());
        seeded
            .tags
            .insert(TagField::Genre, "Electronic; IDM".to_string());
        TagEngine::write(&seeded).unwrap();

        let mut app = open_app(&dir);
        let plan = app
            .preview_tag_edits(&[TagEditDto {
                path: path.to_string_lossy().into_owned(),
                field: "title".into(),
                value: Some("New Title".into()),
            }])
            .unwrap();
        // Only the title changed — the artist is not part of the plan at all.
        assert_eq!(plan.changes[0].tag_changes.len(), 1);
        let batch = app.apply(&plan).unwrap();

        let after = TagEngine::read(&path).unwrap();
        assert_eq!(
            after.tags.get(&TagField::Title).map(String::as_str),
            Some("New Title")
        );
        assert_eq!(
            after.tags.get(&TagField::Artist).map(String::as_str),
            Some("Autechre; Gescom"),
            "editing the title flattened the multi-value artist"
        );
        assert_eq!(
            after.tags.get(&TagField::Genre).map(String::as_str),
            Some("Electronic; IDM")
        );

        // And undo restores the title without disturbing them either.
        app.undo(batch.id).unwrap();
        let undone = TagEngine::read(&path).unwrap();
        assert_eq!(
            undone.tags.get(&TagField::Title).map(String::as_str),
            Some("Old Title")
        );
        assert_eq!(
            undone.tags.get(&TagField::Artist).map(String::as_str),
            Some("Autechre; Gescom")
        );
    }

    // #46: a multi-value field reaches masks and exports as the one joined
    // string the table shows — the point of the canonical form is that nothing
    // downstream has to learn about it.
    #[test]
    fn a_multi_value_field_renders_into_a_mask_as_the_joined_string() {
        let dir = TempDir::new("multi-value-mask");
        let path = dir.tagged_flac("x.flac", "placeholder", "Gantz Graf");
        let mut seeded = TagEngine::read(&path).unwrap();
        seeded
            .tags
            .insert(TagField::Artist, "Autechre; Gescom".to_string());
        TagEngine::write(&seeded).unwrap();

        let app = open_app(&dir);
        let plan = app
            .preview_rename("%artist% - %title%", std::slice::from_ref(&path))
            .unwrap();
        assert!(
            plan.changes[0]
                .rename_to
                .as_deref()
                .unwrap()
                .ends_with("Autechre; Gescom - Gantz Graf.flac"),
            "got {:?}",
            plan.changes[0].rename_to
        );
    }

    // #142: a cleanup chain over a STAGED plan cleans the values the plan
    // proposes, not the ones on disk. This is the case that could not be done
    // at all before — the extracted values exist nowhere yet, so a chain run
    // against the file would see the old tags and clean the wrong thing.
    #[test]
    fn a_chain_over_a_staged_plan_cleans_the_proposed_values() {
        let dir = TempDir::new("plan-cleanup");
        let track = dir.tagged_flac("the_x_factor_-_desert_rain.flac", "Old Artist", "Old Title");
        let app = open_app(&dir);

        let staged = app
            .preview_tags_from_name("%artist%_-_%title%", std::slice::from_ref(&track))
            .unwrap();
        let value = |plan: &PlanDto, field: &str| {
            plan.changes[0]
                .tag_changes
                .iter()
                .find(|c| c.field == field)
                .and_then(|c| c.new.clone())
        };
        assert_eq!(value(&staged, "artist").as_deref(), Some("the_x_factor"));

        let cleaned = app
            .preview_transform_over_plan(
                &staged,
                &[
                    ActionGroupDto {
                        name: "separators".into(),
                        scope: "tags".into(),
                        rules: vec![replace_rule("_", " ")],
                        note: String::new(),
                    },
                    // Scoped, and applied over the result of the group before it.
                    ActionGroupDto {
                        name: "artist upper".into(),
                        scope: "artist".into(),
                        rules: vec![case_rule("upper")],
                        note: String::new(),
                    },
                ],
            )
            .unwrap();
        assert_eq!(value(&cleaned, "artist").as_deref(), Some("THE X FACTOR"));
        assert_eq!(value(&cleaned, "title").as_deref(), Some("desert rain"));
        // `old` still describes the file, so the diff and the staleness check
        // keep comparing against what is really there.
        assert_eq!(
            cleaned.changes[0]
                .tag_changes
                .iter()
                .find(|c| c.field == "artist")
                .and_then(|c| c.old.clone())
                .as_deref(),
            Some("Old Artist")
        );
        assert!(cleaned.description.ends_with(" · cleaned up"));

        // It stays one plan and one undo entry: apply the cleaned one and the
        // cleaned values are what land on disk.
        let mut app = app;
        app.apply(&cleaned).unwrap();
        let after = TagEngine::read(&track).unwrap();
        assert_eq!(
            after.tags.get(&TagField::Artist).map(String::as_str),
            Some("THE X FACTOR")
        );
    }

    // #142: a cleanup that undoes the change is not a change any more, and a
    // file left with nothing to do leaves the plan.
    #[test]
    fn a_cleanup_that_restores_the_old_value_drops_the_change() {
        let dir = TempDir::new("plan-cleanup-noop");
        let track = dir.tagged_flac("x.flac", "Aphex Twin", "Xtal");
        let app = open_app(&dir);

        let staged = app
            .preview_tag_edits(&[TagEditDto {
                path: track.to_string_lossy().into_owned(),
                field: "artist".into(),
                value: Some("APHEX TWIN".into()),
            }])
            .unwrap();
        assert_eq!(staged.changes.len(), 1);

        // Title-casing the proposal lands back on what the file already says.
        let cleaned = app
            .preview_transform_over_plan(
                &staged,
                &[ActionGroupDto {
                    name: "title case".into(),
                    scope: "artist".into(),
                    rules: vec![case_rule("title")],
                    note: String::new(),
                }],
            )
            .unwrap();
        assert!(
            cleaned.changes.is_empty(),
            "a no-op should leave the plan, got {:?}",
            cleaned.changes
        );
    }

    // #142: a file-scoped chain acts on the name the plan PROPOSES, not the one
    // on disk — so a rename and its cleanup compose into a single rename instead
    // of the second undoing the first. The sidecars follow the revised name.
    #[test]
    fn a_file_scoped_chain_revises_the_rename_the_plan_proposes() {
        let dir = TempDir::new("plan-cleanup-rename");
        let track = dir.tagged_flac("x.flac", "Autechre", "Gantz Graf");
        std::fs::write(dir.0.join("x.lrc"), "lyrics").unwrap();
        let app = open_app(&dir);

        let staged = app
            .preview_rename("%artist% - %title%", std::slice::from_ref(&track))
            .unwrap();
        assert!(staged.changes[0]
            .rename_to
            .as_deref()
            .unwrap()
            .ends_with("Autechre - Gantz Graf.flac"));

        let cleaned = app
            .preview_transform_over_plan(
                &staged,
                &[ActionGroupDto {
                    name: "lower".into(),
                    scope: "filename".into(),
                    rules: vec![case_rule("lower")],
                    note: String::new(),
                }],
            )
            .unwrap();
        let renamed = cleaned.changes[0].rename_to.as_deref().unwrap();
        assert!(
            renamed.ends_with("autechre - gantz graf.flac"),
            "got {renamed}"
        );
        assert_eq!(
            cleaned.changes[0]
                .sidecar_renames
                .iter()
                .map(|(_, to)| to.rsplit('/').next().unwrap().to_string())
                .collect::<Vec<_>>(),
            vec!["autechre - gantz graf.lrc"],
            "the sidecar must follow the revised name, not the staged one"
        );
    }

    // #153: the destination can be a folder outside the open library, which the
    // executor would otherwise refuse. The user picking it is the authorization,
    // and it has to survive into apply.
    #[test]
    fn a_reorganize_can_file_tracks_outside_the_library() {
        let dir = TempDir::new("move-outside");
        let library = dir.0.join("incoming");
        let destination = dir.0.join("library");
        std::fs::create_dir_all(&library).unwrap();
        std::fs::create_dir_all(&destination).unwrap();
        let track = dir.tagged_flac_at("incoming/x.flac", "Autechre", "Gantz Graf");
        let mut app = App::open(&library, &dir.0.join("journal.sqlite")).unwrap();

        let plan = app
            .preview_move(
                "%artist%/%title%",
                std::slice::from_ref(&track),
                Some(&destination),
                false,
                false,
            )
            .unwrap();
        let target = PathBuf::from(plan.changes[0].rename_to.clone().unwrap());
        assert!(target.starts_with(&destination), "got {target:?}");

        let batch = app.apply(&plan).unwrap();
        assert!(destination.join("Autechre/Gantz Graf.flac").exists());
        assert!(!track.exists(), "a move must not leave the source behind");

        // And it comes back — the destination is journaled with the batch, so
        // undo is authorized for it.
        app.undo(batch.id).unwrap();
        assert!(track.exists());
        assert!(!destination.join("Autechre/Gantz Graf.flac").exists());
    }

    // #153: an unauthorized destination is still refused. Only a folder the
    // user picked becomes a root — a plan cannot nominate its own.
    #[test]
    fn an_unauthorized_destination_is_still_refused() {
        let dir = TempDir::new("move-unauthorized");
        let library = dir.0.join("incoming");
        let elsewhere = dir.0.join("elsewhere");
        std::fs::create_dir_all(&library).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        let track = dir.tagged_flac_at("incoming/x.flac", "Autechre", "Gantz Graf");
        let mut app = App::open(&library, &dir.0.join("journal.sqlite")).unwrap();

        // A hand-built plan aimed outside, as a crafted mask would produce.
        let plan = PlanDto {
            description: "sneaky".into(),
            prune_empty_dirs: false,
            changes: vec![FileChangeDto {
                path: track.to_string_lossy().into_owned(),
                rename_to: Some(elsewhere.join("stolen.flac").to_string_lossy().into_owned()),
                tag_changes: Vec::new(),
                cover_change: None,
                sidecar_renames: Vec::new(),
                copy: false,
            }],
        };
        assert!(app.apply(&plan).is_err());
        assert!(track.exists());
        assert!(!elsewhere.join("stolen.flac").exists());
    }

    // #153: a copy leaves the source alone, and undoing it removes the copy.
    #[test]
    fn a_copy_keeps_the_source_and_undo_removes_the_copy() {
        let dir = TempDir::new("move-copy");
        let library = dir.0.join("incoming");
        let destination = dir.0.join("library");
        std::fs::create_dir_all(&library).unwrap();
        std::fs::create_dir_all(&destination).unwrap();
        let track = dir.tagged_flac_at("incoming/x.flac", "Autechre", "Gantz Graf");
        let mut app = App::open(&library, &dir.0.join("journal.sqlite")).unwrap();

        let plan = app
            .preview_move(
                "%artist%/%title%",
                std::slice::from_ref(&track),
                Some(&destination),
                true,
                // Asked for, but a copy empties nothing — the plan must say so.
                true,
            )
            .unwrap();
        assert!(plan.changes[0].copy);
        assert!(!plan.prune_empty_dirs);

        let batch = app.apply(&plan).unwrap();
        let copy = destination.join("Autechre/Gantz Graf.flac");
        assert!(copy.exists());
        assert!(track.exists(), "a copy must leave the source in place");

        app.undo(batch.id).unwrap();
        assert!(!copy.exists());
        assert!(track.exists(), "undoing a copy must not touch the source");
    }

    // #153: a move can be asked to clear up after itself, and undo puts the
    // folders back so the files have somewhere to return to.
    #[test]
    fn pruning_removes_the_folders_a_move_empties_and_undo_restores_them() {
        let dir = TempDir::new("move-prune");
        let library = dir.0.join("incoming");
        std::fs::create_dir_all(&library).unwrap();
        let track = dir.tagged_flac_at("incoming/dump/deep/x.flac", "Autechre", "Gantz Graf");
        let mut app = App::open(&library, &dir.0.join("journal.sqlite")).unwrap();

        let plan = app
            .preview_move(
                "%artist%/%title%",
                std::slice::from_ref(&track),
                None,
                false,
                true,
            )
            .unwrap();
        assert!(plan.prune_empty_dirs);
        let batch = app.apply(&plan).unwrap();

        assert!(library.join("Autechre/Gantz Graf.flac").exists());
        assert!(!library.join("dump/deep").exists(), "emptied folder left");
        assert!(!library.join("dump").exists(), "its emptied parent left");
        // The library root itself is a boundary, never something to remove.
        assert!(library.exists());

        app.undo(batch.id).unwrap();
        assert!(track.exists());
        assert!(library.join("dump/deep").exists());
    }

    // #153: a folder that still holds something is not this batch's to delete.
    #[test]
    fn pruning_leaves_a_folder_that_still_has_something_in_it() {
        let dir = TempDir::new("move-prune-busy");
        let library = dir.0.join("incoming");
        std::fs::create_dir_all(&library).unwrap();
        let track = dir.tagged_flac_at("incoming/dump/x.flac", "Autechre", "Gantz Graf");
        std::fs::write(library.join("dump/notes.txt"), "keep me").unwrap();
        let mut app = App::open(&library, &dir.0.join("journal.sqlite")).unwrap();

        let plan = app
            .preview_move(
                "%artist%/%title%",
                std::slice::from_ref(&track),
                None,
                false,
                true,
            )
            .unwrap();
        app.apply(&plan).unwrap();
        assert!(library.join("dump").exists());
        assert!(library.join("dump/notes.txt").exists());
    }

    // #56: embedding a cover replaces the FRONT image and leaves the rest of
    // the set alone. "Here is the cover" must never mean "throw away the back
    // and the disc".
    #[test]
    fn embedding_a_cover_keeps_the_other_images() {
        let dir = TempDir::new("cover-set-embed");
        let track = dir.tagged_flac("x.flac", "Autechre", "Gantz Graf");
        let image = |kind: CoverKind, byte: u8| CoverArt {
            mime: "image/png".to_string(),
            data: vec![0x89, 0x50, byte],
            kind,
            description: String::new(),
        };
        TagEngine::write_covers(
            &track,
            &[image(CoverKind::Front, 1), image(CoverKind::Back, 2)],
        )
        .unwrap();

        let mut app = open_app(&dir);
        let replacement = cover_art_to_dto(&image(CoverKind::Front, 9));
        let plan = app
            .preview_cover_embed(std::slice::from_ref(&track), &replacement)
            .unwrap();
        app.apply(&plan).unwrap();

        let after = TagEngine::read_covers(&track).unwrap();
        assert_eq!(after.len(), 2, "the back cover was dropped: {after:?}");
        assert_eq!(after[0], image(CoverKind::Front, 9));
        assert_eq!(after[1], image(CoverKind::Back, 2));
    }

    // #56: the whole set is one change — add, reorder and retype at once — and
    // undo puts the previous set back exactly.
    #[test]
    fn setting_the_image_set_is_one_undoable_change() {
        let dir = TempDir::new("cover-set");
        let track = dir.tagged_flac("x.flac", "Autechre", "Gantz Graf");
        let image = |kind: CoverKind, byte: u8| CoverArt {
            mime: "image/png".to_string(),
            data: vec![0x89, 0x50, byte],
            kind,
            description: String::new(),
        };
        let before = vec![image(CoverKind::Front, 1), image(CoverKind::Back, 2)];
        TagEngine::write_covers(&track, &before).unwrap();

        let mut app = open_app(&dir);
        // Reordered, retyped and grown, in one send.
        let wanted = vec![
            image(CoverKind::Front, 2),
            image(CoverKind::Media, 1),
            image(CoverKind::Artist, 3),
        ];
        let plan = app
            .preview_cover_set(
                std::slice::from_ref(&track),
                &wanted.iter().map(cover_art_to_dto).collect::<Vec<_>>(),
            )
            .unwrap();
        assert_eq!(plan.changes.len(), 1);
        let batch = app.apply(&plan).unwrap();
        assert_eq!(TagEngine::read_covers(&track).unwrap(), wanted);

        app.undo(batch.id).unwrap();
        assert_eq!(TagEngine::read_covers(&track).unwrap(), before);

        // An empty set is how the panel removes every image.
        let plan = app
            .preview_cover_set(std::slice::from_ref(&track), &[])
            .unwrap();
        app.apply(&plan).unwrap();
        assert!(TagEngine::read_covers(&track).unwrap().is_empty());
    }

    // #56: the well can only offer a set to edit when every selected file
    // carries the same one — same images, same types, same order.
    #[test]
    fn the_summary_reports_a_shared_set_only_when_it_is_really_shared() {
        let dir = TempDir::new("cover-summary");
        let a = dir.tagged_flac("a.flac", "Autechre", "A");
        let b = dir.tagged_flac("b.flac", "Autechre", "B");
        let image = |kind: CoverKind, byte: u8| CoverArt {
            mime: "image/png".to_string(),
            data: vec![0x89, 0x50, byte],
            kind,
            description: String::new(),
        };
        let set = vec![image(CoverKind::Front, 1), image(CoverKind::Back, 2)];
        TagEngine::write_covers(&a, &set).unwrap();
        TagEngine::write_covers(&b, &set).unwrap();
        let app = open_app(&dir);

        let shared = app.read_cover_summary(&[a.clone(), b.clone()]).unwrap();
        assert!(!shared.distinct);
        assert_eq!(shared.with_cover, 2);
        assert_eq!(shared.shared_set.len(), 2);
        assert_eq!(shared.shared_set[1].kind, "back");

        // Same images, different order — not the same set, so nothing to edit.
        TagEngine::write_covers(&b, &[set[1].clone(), set[0].clone()]).unwrap();
        let mixed = app.read_cover_summary(&[a, b]).unwrap();
        assert!(mixed.distinct);
        assert!(mixed.shared_set.is_empty());
        // The fan still shows one entry: both files' FRONT cover is the same.
        assert_eq!(mixed.samples.len(), 1);
    }

    #[test]
    fn settings_deserialize_fills_defaults_and_applies() {
        // A partial (older) settings file still loads — every field defaults.
        let partial: SettingsDto = serde_json::from_str(r#"{"proxy":"http://p:8080"}"#).unwrap();
        assert_eq!(partial.proxy, "http://p:8080");
        assert_eq!(partial.rate_limit_per_min, 0);
        assert!(!partial.id3_v23);
        assert_eq!(
            SettingsDto::default(),
            serde_json::from_str::<SettingsDto>("{}").unwrap()
        );

        // Applying settings wires the proxy/throttle onto the app without panic.
        let dir = TempDir::new("settings");
        let app = open_app(&dir);
        app.apply_settings(&SettingsDto {
            proxy: "http://host:3128".into(),
            rate_limit_per_min: 120,
            id3_v23: true,
            read_priority: vec!["vorbis".into(), "id3v2".into()],
            cover_max_px: 500,
            cover_quality: 90,
            action_groups: Vec::new(),
            carry_sidecars: true,
            sidecar_extensions: Vec::new(),
            import_skip_fields: Vec::new(),
            multi_value_separator: String::new(),
        });
        assert_eq!(app.cover_max_px.get(), 500);
        assert_eq!(app.cover_quality.get(), 90);
        assert_eq!(
            app.discogs_proxy.borrow().as_deref(),
            Some("http://host:3128")
        );
        assert_eq!(
            app.discogs_min_interval.get(),
            Some(Duration::from_secs_f64(0.5))
        );
        // An empty proxy clears it back to a direct connection.
        app.apply_settings(&SettingsDto::default());
        assert!(app.discogs_proxy.borrow().is_none());
        assert!(app.discogs_min_interval.get().is_none());
    }

    #[test]
    fn settings_round_trip_action_groups_and_default_step_enabled() {
        // A saved group persists in settings.json; a step without `enabled`
        // deserializes as enabled (so older data / omitted fields stay on) (#57).
        let json = r#"{
            "action_groups": [
                { "name": "Cleanup", "scope": "tags",
                  "rules": [ { "kind": "case", "style": "title" } ] }
            ]
        }"#;
        let settings: SettingsDto = serde_json::from_str(json).unwrap();
        assert_eq!(settings.action_groups.len(), 1);
        let group = &settings.action_groups[0];
        assert_eq!(group.name, "Cleanup");
        assert_eq!(group.scope, "tags");
        assert!(group.rules[0].enabled, "omitted `enabled` defaults to true");
    }

    #[test]
    fn build_chain_skips_disabled_steps() {
        let rule = |kind: &str, style: &str, enabled: bool| TransformRuleDto {
            kind: kind.into(),
            from: String::new(),
            to: String::new(),
            regex: false,
            whole_word: false,
            case_sensitive: false,
            style: style.into(),
            enabled,
        };
        // An enabled upper-case step plus a *disabled* title-case step: only the
        // enabled one runs, so the result is upper, not title.
        let chain =
            build_chain(&[rule("case", "upper", true), rule("case", "title", false)]).unwrap();
        assert_eq!(chain.apply("hello world"), "HELLO WORLD");
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
    fn unreadable_file_is_listed_not_dropped() {
        let dir = TempDir::new("unreadable");
        dir.tagged_flac("good.flac", "Artist", "Title");
        // A supported extension with garbage content: the tag reader fails on it.
        std::fs::write(dir.0.join("bad.flac"), b"not a real flac file").unwrap();
        let app = open_app(&dir);

        let tracks = app.list_tracks();
        // Both are listed — the unreadable one is a placeholder, not dropped.
        assert_eq!(tracks.len(), 2);
        let bad = tracks
            .iter()
            .find(|t| t.path.ends_with("bad.flac"))
            .unwrap();
        assert!(bad.unreadable);
        assert!(bad.tags.is_empty());
        assert_eq!(bad.format, "FLAC");
        let good = tracks
            .iter()
            .find(|t| t.path.ends_with("good.flac"))
            .unwrap();
        assert!(!good.unreadable);
    }

    /// The file placeholders (#147) reach the rename path with a real file
    /// behind them: the folder name and the container come from the file rather
    /// than from any tag, and the technical ones resolve off the actual probe.
    #[test]
    fn preview_rename_resolves_file_and_technical_placeholders() {
        let dir = TempDir::new("filemask");
        let track = dir.tagged_flac_at("Blue Lines/original.flac", "Massive Attack", "Safe");
        let app = open_app(&dir);

        let plan = app
            .preview_rename(
                "%foldername% - %title% (%_codec%)",
                std::slice::from_ref(&track),
            )
            .unwrap();
        assert_eq!(plan.changes.len(), 1);
        let expected = track.with_file_name("Blue Lines - Safe (FLAC).flac");
        assert_eq!(
            plan.changes[0].rename_to.as_deref(),
            Some(expected.to_string_lossy().as_ref())
        );

        // An audio property costs a probe, which only a pattern asking for one
        // pays for -- and it has to come back with a real value, not an empty
        // string standing in for a read that never happened.
        let probed = app
            .preview_rename("%title% %_samplerate%", std::slice::from_ref(&track))
            .unwrap();
        let renamed = probed.changes[0].rename_to.as_deref().unwrap();
        assert!(renamed.ends_with("Safe 44100.flac"), "got {renamed}");
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

    // #139: the extract direction — the name carries the metadata, and it comes
    // back as an ordinary tag plan that applies and undoes like any other.
    #[test]
    fn tags_from_name_preview_apply_undo_round_trip() {
        let dir = TempDir::new("fromname");
        let track = dir.tagged_flac("05 - Boards of Canada - Roygbiv.flac", "Unknown", "Track 5");
        let mut app = open_app(&dir);

        let plan = app
            .preview_tags_from_name("%track% - %artist% - %title%", std::slice::from_ref(&track))
            .unwrap();
        assert_eq!(plan.changes.len(), 1);
        let by_field = |field: &str| {
            plan.changes[0]
                .tag_changes
                .iter()
                .find(|c| c.field == field)
                .unwrap_or_else(|| panic!("no change for {field}"))
        };
        // "05" is normalized to "5": the tag stores an integer either way, so
        // the preview shows what will actually be on disk.
        assert_eq!(by_field("track").new.as_deref(), Some("5"));
        assert_eq!(by_field("artist").old.as_deref(), Some("Unknown"));
        assert_eq!(by_field("artist").new.as_deref(), Some("Boards of Canada"));
        assert_eq!(by_field("title").new.as_deref(), Some("Roygbiv"));
        assert!(plan.changes[0].rename_to.is_none());

        let batch = app.apply(&plan).unwrap();
        let tags = TagEngine::read(&track).unwrap().tags;
        assert_eq!(
            tags.get(&TagField::Artist).map(String::as_str),
            Some("Boards of Canada")
        );
        assert_eq!(
            tags.get(&TagField::Title).map(String::as_str),
            Some("Roygbiv")
        );
        assert_eq!(
            tags.get(&TagField::TrackNumber).map(String::as_str),
            Some("5")
        );

        app.undo(batch.id).unwrap();
        let tags = TagEngine::read(&track).unwrap().tags;
        assert_eq!(
            tags.get(&TagField::Artist).map(String::as_str),
            Some("Unknown")
        );
        assert_eq!(
            tags.get(&TagField::Title).map(String::as_str),
            Some("Track 5")
        );
    }

    // A pattern with separators reaches up into the folders, which is where the
    // artist and album usually are. Either separator is accepted (#71).
    #[test]
    fn tags_from_name_reads_the_parent_folders() {
        let dir = TempDir::new("fromname-folders");
        let track = dir.tagged_flac_at(
            "Aphex Twin/Selected Ambient Works/02 - Ageispolis.flac",
            "Unknown",
            "Untitled",
        );
        let app = open_app(&dir);

        for pattern in [
            "%albumartist%/%album%/%track% - %title%",
            "%albumartist%\\%album%\\%track% - %title%",
        ] {
            let plan = app
                .preview_tags_from_name(pattern, std::slice::from_ref(&track))
                .unwrap();
            assert_eq!(plan.changes.len(), 1, "pattern {pattern}");
            let value = |field: &str| {
                plan.changes[0]
                    .tag_changes
                    .iter()
                    .find(|c| c.field == field)
                    .and_then(|c| c.new.clone())
            };
            assert_eq!(value("albumartist").as_deref(), Some("Aphex Twin"));
            assert_eq!(value("album").as_deref(), Some("Selected Ambient Works"));
            assert_eq!(value("title").as_deref(), Some("Ageispolis"));
            assert_eq!(value("track").as_deref(), Some("2"));
        }
    }

    // A selection is rarely uniform: a name that doesn't fit is skipped, and so
    // is one whose tags already say what the name says.
    #[test]
    fn tags_from_name_skips_non_matching_and_unchanged_files() {
        let dir = TempDir::new("fromname-skip");
        let matching = dir.tagged_flac("Autechre - Gantz Graf.flac", "Unknown", "Unknown");
        let other = dir.tagged_flac("track01.flac", "Unknown", "Unknown");
        let already = dir.tagged_flac("Autechre - Vletrmx.flac", "Autechre", "Vletrmx");
        let app = open_app(&dir);

        let plan = app
            .preview_tags_from_name("%artist% - %title%", &[matching.clone(), other, already])
            .unwrap();
        assert_eq!(plan.changes.len(), 1);
        assert_eq!(plan.changes[0].path, matching.to_string_lossy());
    }

    // A pattern the extract direction refuses is an error up front, not an
    // empty plan: %side% is computed at render time and has no tag to go back
    // into, and adjacent placeholders have no boundary to split on.
    #[test]
    fn tags_from_name_refuses_a_pattern_that_cannot_extract() {
        let dir = TempDir::new("fromname-refuse");
        let track = dir.tagged_flac("A1 - Title.flac", "Unknown", "Unknown");
        let app = open_app(&dir);

        for pattern in ["%side%%track% - %title%", "%disc%%track% - %title%"] {
            assert!(app
                .preview_tags_from_name(pattern, std::slice::from_ref(&track))
                .is_err());
        }
    }

    // A capture landing in a typed field it can't hold is flagged in the
    // preview rather than written — same rule the editor uses.
    #[test]
    fn tags_from_name_flags_a_value_the_field_cannot_hold() {
        let dir = TempDir::new("fromname-invalid");
        let track = dir.tagged_flac("Live - Encore.flac", "Unknown", "Unknown");
        let app = open_app(&dir);

        let plan = app
            .preview_tags_from_name("%year% - %title%", std::slice::from_ref(&track))
            .unwrap();
        let year = plan.changes[0]
            .tag_changes
            .iter()
            .find(|c| c.field == "year")
            .unwrap();
        assert_eq!(year.new.as_deref(), Some("Live"));
        assert!(year.invalid);
    }

    // The live probe reports what the pattern sees, current tags irrelevant —
    // including the subject string, which is the whole point when it misses.
    #[test]
    fn probe_reports_what_the_pattern_sees_including_a_miss() {
        let dir = TempDir::new("fromname-probe");
        let track = dir.tagged_flac("Autechre - Gantz Graf.flac", "Autechre", "Gantz Graf");
        let app = open_app(&dir);

        let hit = app
            .probe_tags_from_name("%artist% - %title%", &track)
            .unwrap();
        assert!(hit.matched);
        assert_eq!(hit.subject, "Autechre - Gantz Graf");
        // Reported even though the file's tags already say exactly this.
        assert_eq!(
            hit.fields,
            vec![
                ("artist".to_string(), "Autechre".to_string()),
                ("title".to_string(), "Gantz Graf".to_string()),
            ]
        );

        let miss = app
            .probe_tags_from_name("%track%. %title%", &track)
            .unwrap();
        assert!(!miss.matched);
        assert!(miss.fields.is_empty());
        // The subject still comes back: seeing it is how you fix the pattern.
        assert_eq!(miss.subject, "Autechre - Gantz Graf");

        // A folder pattern shows how much of the path is in play.
        let deep = app
            .probe_tags_from_name("%album%/%artist% - %title%", &track)
            .unwrap();
        assert!(deep.subject.ends_with("/Autechre - Gantz Graf"));

        // A broken pattern is still an error.
        assert!(app
            .probe_tags_from_name("%artist% - [%title%", &track)
            .is_err());
    }

    #[test]
    fn name_subject_takes_as_many_folders_as_the_pattern_asks_for() {
        let path = Path::new("/lib/Artist/Album/01 - Title.flac");
        assert_eq!(name_subject(path, 0).as_deref(), Some("01 - Title"));
        assert_eq!(name_subject(path, 1).as_deref(), Some("Album/01 - Title"));
        assert_eq!(
            name_subject(path, 2).as_deref(),
            Some("Artist/Album/01 - Title")
        );
        // Deeper than the path goes: no subject rather than a partial match.
        assert_eq!(name_subject(Path::new("/a/b.flac"), 3), None);
    }

    // #58: a rename detects same-stem sidecars in the configured set, retargets
    // them to the new stem, and moves/restores them with the track — while
    // leaving wrong-extension and wrong-stem neighbours alone.
    #[test]
    fn rename_carries_sidecar_files() {
        let dir = TempDir::new("sidecar-app");
        let track = dir.tagged_flac("original.flac", "Boards of Canada", "Roygbiv");
        let lrc = dir.0.join("original.lrc");
        let txt = dir.0.join("original.txt");
        let other = dir.0.join("original.zzz"); // stem matches, extension not in the set
        let elsewhere = dir.0.join("different.lrc"); // extension in set, wrong stem
        for p in [&lrc, &txt, &other, &elsewhere] {
            std::fs::write(p, b"x").unwrap();
        }
        let mut app = open_app(&dir);

        let plan = app
            .preview_rename("%artist% - %title%", std::slice::from_ref(&track))
            .unwrap();
        let stem = "Boards of Canada - Roygbiv";
        let sidecars = &plan.changes[0].sidecar_renames;
        assert_eq!(sidecars.len(), 2, "lrc + txt only");
        let froms: Vec<&str> = sidecars.iter().map(|(f, _)| f.as_str()).collect();
        assert!(froms.contains(&lrc.to_string_lossy().as_ref()));
        assert!(froms.contains(&txt.to_string_lossy().as_ref()));
        assert!(sidecars.iter().all(|(_, to)| to.contains(stem)));

        let batch = app.apply(&plan).unwrap();
        assert!(dir.0.join(format!("{stem}.lrc")).exists());
        assert!(dir.0.join(format!("{stem}.txt")).exists());
        assert!(!lrc.exists() && !txt.exists());
        assert!(
            other.exists() && elsewhere.exists(),
            "non-sidecars untouched"
        );

        app.undo(batch.id).unwrap();
        assert!(lrc.exists() && txt.exists(), "sidecars restored");
        assert!(!dir.0.join(format!("{stem}.lrc")).exists());
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
    fn clear_tags_wipes_every_text_field_keeps_cover_and_undoes() {
        let dir = TempDir::new("clear");
        let track = dir.tagged_flac("x.flac", "Some Artist", "Some Title");
        // Give it a cover so we can prove clearing text tags leaves it intact.
        let art = CoverArt {
            mime: "image/png".to_string(),
            data: vec![1, 2, 3, 4],
            ..CoverArt::default()
        };
        TagEngine::write_covers(&track, std::slice::from_ref(&art)).unwrap();
        let mut app = open_app(&dir);

        let plan = app
            .preview_clear_tags(std::slice::from_ref(&track))
            .unwrap();
        assert_eq!(plan.changes.len(), 1);
        // Every modeled field the file carries is cleared (new == None) and keeps
        // its old value for undo; at minimum the artist and title we wrote.
        assert!(plan.changes[0].tag_changes.len() >= 2);
        assert!(plan.changes[0]
            .tag_changes
            .iter()
            .all(|c| c.new.is_none() && c.old.is_some()));
        assert!(plan.changes[0].cover_change.is_none());

        let batch = app.apply(&plan).unwrap();
        let cleared = TagEngine::read(&track).unwrap();
        // No field carries a value any more (lofty may re-emit an empty encoder
        // key on a FLAC write; that empty remnant is harmless).
        assert!(
            cleared.tags.values().all(|v| v.is_empty()),
            "no text tag should keep a value, got {:?}",
            cleared.tags
        );
        // The cover is a separate change kind and must survive the clear.
        assert_eq!(TagEngine::read_cover(&track).unwrap(), Some(art.clone()));

        app.undo(batch.id).unwrap();
        let restored = TagEngine::read(&track).unwrap();
        assert_eq!(
            restored.tags.get(&TagField::Artist).map(String::as_str),
            Some("Some Artist")
        );
        assert_eq!(
            restored.tags.get(&TagField::Title).map(String::as_str),
            Some("Some Title")
        );
    }

    #[test]
    fn clear_tags_skips_files_with_no_modeled_tags() {
        let dir = TempDir::new("clear-empty");
        let path = dir.0.join("bare.flac");
        std::fs::write(&path, MINIMAL_FLAC).unwrap();
        let app = open_app(&dir);
        let plan = app.preview_clear_tags(&[path]).unwrap();
        assert!(plan.changes.is_empty());
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
                    disc: None,
                    position: "1".into(),
                    artist: String::new(),
                    title: "First".into(),
                    duration_secs: None,
                    isrc: None,
                },
                ImportTrackDto {
                    disc: None,
                    position: "5".into(),
                    artist: "Guest".into(),
                    title: "Fifth".into(),
                    duration_secs: None,
                    isrc: None,
                },
            ],
            release_id: Some("249504".into()),
            source: Some("discogs".into()),
            label: Some("Antler-Subway".into()),
            catalog_number: Some("AS 5606".into()),
            disc_total: None,
            country: Some("Belgium".into()),
            track_total: Some("15".into()),
            url: Some("https://www.discogs.com/release/249504".into()),
            media_type: Some("Vinyl".into()),
        };

        let plan = app.preview_import(&[a, b], &selection, false).unwrap();
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
        // The Discogs release id is written under DISCOGS_RELEASE_ID for grouping
        // by release (#20), on every mapped file.
        assert_eq!(
            first.get("custom:DISCOGS_RELEASE_ID").map(String::as_str),
            Some("249504")
        );
        // The chosen label + catalogue number (#90): Publisher + CatalogNumber.
        assert_eq!(
            first.get("publisher").map(String::as_str),
            Some("Antler-Subway")
        );
        assert_eq!(
            first.get("catalognumber").map(String::as_str),
            Some("AS 5606")
        );
        assert_eq!(
            fields(&plan.changes[1])
                .get("custom:DISCOGS_RELEASE_ID")
                .map(String::as_str),
            Some("249504")
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
        // Album-level total track count and release country.
        assert_eq!(first.get("tracktotal").map(String::as_str), Some("15"));
        assert_eq!(
            first.get("custom:RELEASECOUNTRY").map(String::as_str),
            Some("Belgium")
        );
        assert_eq!(
            first.get("url").map(String::as_str),
            Some("https://www.discogs.com/release/249504")
        );

        let second = fields(&plan.changes[1]);
        assert_eq!(second.get("artist").map(String::as_str), Some("Guest"));
        // Position 5, not selection index 2.
        assert_eq!(second.get("track").map(String::as_str), Some("5"));
    }

    #[test]
    fn preview_import_stores_musicbrainz_release_id_under_its_own_key() {
        let dir = TempDir::new("import-mbid");
        let path = dir.tagged_flac("a.flac", "Old", "Old Title");
        let app = open_app(&dir);

        let selection = ImportSelectionDto {
            album: Some("Album".into()),
            release_id: Some("aeb1c1c0-mbid".into()),
            source: Some("musicbrainz".into()),
            ..ImportSelectionDto::default()
        };
        let plan = app.preview_import(&[path], &selection, false).unwrap();
        let fields: std::collections::BTreeMap<_, _> = plan.changes[0]
            .tag_changes
            .iter()
            .map(|fc| (fc.field.clone(), fc.new.clone().unwrap()))
            .collect();
        // MusicBrainz uses MUSICBRAINZ_ALBUMID, not the Discogs key.
        assert_eq!(
            fields.get("custom:MUSICBRAINZ_ALBUMID").map(String::as_str),
            Some("aeb1c1c0-mbid")
        );
        assert!(!fields.contains_key("custom:DISCOGS_RELEASE_ID"));
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
                disc: None,
                position: "5".into(),
                artist: "Artist".into(),
                title: "Title".into(),
                duration_secs: None,
                isrc: None,
            }],
            ..ImportSelectionDto::default()
        };
        let plan = app.preview_import(&[path], &selection, false).unwrap();
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
    fn side_disc_from_position_maps_the_side_letter() {
        assert_eq!(side_disc_from_position("A1"), Some(1));
        assert_eq!(side_disc_from_position("B2"), Some(2));
        assert_eq!(side_disc_from_position("C15"), Some(3)); // C = 3rd side
        assert_eq!(side_disc_from_position("1A"), Some(1)); // reverse notation
        assert_eq!(side_disc_from_position("12B"), Some(2)); // reverse, multi-digit
        assert_eq!(side_disc_from_position("2b"), Some(2)); // lower-case side
        assert_eq!(side_disc_from_position("A"), Some(1)); // bare side, whole side is one track
        assert_eq!(side_disc_from_position("B"), Some(2));
        // Not a vinyl side:
        assert_eq!(side_disc_from_position("5"), None); // plain number
        assert_eq!(side_disc_from_position("1-05"), None); // disc-track pair
        assert_eq!(side_disc_from_position("AB"), None); // not a side + track
        assert_eq!(side_disc_from_position(""), None);
    }

    #[test]
    fn import_maps_vinyl_sides_to_discs_with_per_side_track_numbers() {
        let dir = TempDir::new("import-vinyl");
        // A vinyl release: side A tracks A1..A2, side B is a single bare "B".
        let a1 = dir.tagged_flac("f1.flac", "Old", "Old");
        let a2 = dir.tagged_flac("f2.flac", "Old", "Old");
        let b = dir.tagged_flac("f3.flac", "Old", "Old");
        // Every file already carries the ubiquitous default disc 1 -- which must
        // not stop side B from becoming disc 2.
        for path in [&a1, &a2, &b] {
            let mut file = TagEngine::read(path).unwrap();
            file.tags.insert(TagField::DiscNumber, "1".into());
            TagEngine::write(&file).unwrap();
        }
        let app = open_app(&dir);
        let track = |position: &str| ImportTrackDto {
            disc: None,
            position: position.into(),
            artist: "X".into(),
            title: "T".into(),
            duration_secs: None,
            isrc: None,
        };
        let selection = ImportSelectionDto {
            album: Some("Album".into()),
            tracks: vec![track("A1"), track("A2"), track("B")],
            ..ImportSelectionDto::default()
        };
        let td = |change: &FileChangeDto| {
            let m = change
                .tag_changes
                .iter()
                .map(|fc| (fc.field.clone(), fc.new.clone().unwrap()))
                .collect::<std::collections::BTreeMap<_, _>>();
            (
                m.get("track").cloned(),
                m.get("disc").cloned().or_else(|| Some("<none>".into())),
            )
        };

        let on = app
            .preview_import(&[a1.clone(), a2.clone(), b.clone()], &selection, true)
            .unwrap();
        // A1/A2 -> disc 1 (already set, so no disc change) / tracks 1,2; the bare
        // "B" -> disc 2 (overwrites the default disc 1) / track 1 (per-side restart).
        assert_eq!(
            td(&on.changes[0]),
            (Some("1".into()), Some("<none>".into()))
        );
        assert_eq!(
            td(&on.changes[1]),
            (Some("2".into()), Some("<none>".into()))
        );
        assert_eq!(td(&on.changes[2]), (Some("1".into()), Some("2".into())));

        // Toggle off: sides are not mapped; bare "B" falls back to the row index.
        let off = app.preview_import(&[a1, a2, b], &selection, false).unwrap();
        assert_eq!(td(&off.changes[2]).1, Some("<none>".into())); // no disc change
        assert_eq!(td(&off.changes[2]).0, Some("3".into())); // index-based fallback
    }

    /// The disc the release itself states (#146): a Discogs `1-05` position or a
    /// MusicBrainz medium, both of which arrive here as `ImportTrackDto::disc`.
    /// Before this, only a vinyl side could ever set a disc, so a 2xCD imported
    /// with no disc numbers at all.
    #[test]
    fn import_writes_the_disc_the_release_states() {
        let dir = TempDir::new("import-disc");
        let one = dir.tagged_flac("f1.flac", "Old", "Old");
        let two = dir.tagged_flac("f2.flac", "Old", "Old");
        let app = open_app(&dir);
        let track = |disc: u32, position: &str| ImportTrackDto {
            disc: Some(disc),
            position: position.into(),
            artist: "X".into(),
            title: "T".into(),
            duration_secs: None,
            isrc: None,
        };
        let selection = ImportSelectionDto {
            album: Some("Two Discs".into()),
            tracks: vec![track(1, "1-05"), track(2, "2-01")],
            disc_total: Some("2".into()),
            ..ImportSelectionDto::default()
        };

        let plan = app.preview_import(&[one, two], &selection, false).unwrap();
        let field = |change: &FileChangeDto, name: &str| {
            change
                .tag_changes
                .iter()
                .find(|fc| fc.field == name)
                .and_then(|fc| fc.new.clone())
        };
        assert_eq!(field(&plan.changes[0], "disc").as_deref(), Some("1"));
        assert_eq!(field(&plan.changes[1], "disc").as_deref(), Some("2"));
        // The track number still comes out of the position's tail, unchanged by
        // the disc prefix.
        assert_eq!(field(&plan.changes[0], "track").as_deref(), Some("5"));
        assert_eq!(field(&plan.changes[1], "track").as_deref(), Some("1"));
        // "of N" for both files (album-level, like tracktotal).
        assert_eq!(field(&plan.changes[0], "disctotal").as_deref(), Some("2"));
        assert_eq!(field(&plan.changes[1], "disctotal").as_deref(), Some("2"));
    }

    /// A single-disc release says nothing about a disc, and nothing is what gets
    /// written. Defaulting to "1" would touch every file of every ordinary album
    /// for no reason.
    #[test]
    fn import_writes_no_disc_when_the_release_states_none() {
        let dir = TempDir::new("import-nodisc");
        let path = dir.tagged_flac("f1.flac", "Old", "Old");
        let app = open_app(&dir);
        let selection = ImportSelectionDto {
            album: Some("One Disc".into()),
            tracks: vec![ImportTrackDto {
                disc: None,
                position: "3".into(),
                artist: "X".into(),
                title: "T".into(),
                duration_secs: None,
                isrc: None,
            }],
            ..ImportSelectionDto::default()
        };

        let plan = app.preview_import(&[path], &selection, false).unwrap();
        assert!(plan.changes[0]
            .tag_changes
            .iter()
            .all(|fc| fc.field != "disc"));
        // And no lone "of N" either: a disc total with no disc number to
        // complete says nothing, and would land on every ordinary album.
        assert!(plan.changes[0]
            .tag_changes
            .iter()
            .all(|fc| fc.field != "disctotal"));
    }

    /// A release stating it holds one disc puts every track on disc 1 of 1
    /// (#157). That is the provider's own statement, not a default -- and it is
    /// what stops an ordinary single-CD album reading blank next to files that
    /// do carry a disc.
    #[test]
    fn a_stated_single_disc_release_writes_disc_one_of_one() {
        let dir = TempDir::new("import-solo-release");
        let path = dir.tagged_flac("f1.flac", "Old", "Old");
        let app = open_app(&dir);
        let selection = ImportSelectionDto {
            album: Some("One Disc".into()),
            disc_total: Some("1".into()),
            tracks: vec![ImportTrackDto {
                disc: None,
                position: "3".into(),
                artist: "X".into(),
                title: "T".into(),
                duration_secs: None,
                isrc: None,
            }],
            ..ImportSelectionDto::default()
        };

        let plan = app.preview_import(&[path], &selection, false).unwrap();
        let field = |name: &str| {
            plan.changes[0]
                .tag_changes
                .iter()
                .find(|fc| fc.field == name)
                .and_then(|fc| fc.new.clone())
        };
        assert_eq!(field("disc").as_deref(), Some("1"));
        assert_eq!(field("disctotal").as_deref(), Some("1"));
    }

    /// The count is what licenses the default. On a release stating two discs, a
    /// track whose position names no disc is genuinely unplaced -- 1 would be a
    /// guess, so nothing is written.
    #[test]
    fn a_multi_disc_release_does_not_default_an_unplaced_track_to_disc_one() {
        let dir = TempDir::new("import-multi-unplaced");
        let path = dir.tagged_flac("f1.flac", "Old", "Old");
        let app = open_app(&dir);
        let selection = ImportSelectionDto {
            album: Some("Two Discs".into()),
            disc_total: Some("2".into()),
            tracks: vec![ImportTrackDto {
                disc: None,
                position: "3".into(),
                artist: "X".into(),
                title: "T".into(),
                duration_secs: None,
                isrc: None,
            }],
            ..ImportSelectionDto::default()
        };

        let plan = app.preview_import(&[path], &selection, false).unwrap();
        assert!(plan.changes[0]
            .tag_changes
            .iter()
            .all(|fc| fc.field != "disc"));
    }

    /// Silence is not a statement. A provider that reports no count at all still
    /// produces no disc -- which is what the release from #146 does, and why
    /// `import_writes_no_disc_when_the_release_states_none` still passes.
    #[test]
    fn no_stated_count_still_writes_no_disc() {
        let dir = TempDir::new("import-count-silent");
        let path = dir.tagged_flac("f1.flac", "Old", "Old");
        let app = open_app(&dir);
        let selection = ImportSelectionDto {
            album: Some("Unknown".into()),
            disc_total: None,
            tracks: vec![ImportTrackDto {
                disc: None,
                position: "3".into(),
                artist: "X".into(),
                title: "T".into(),
                duration_secs: None,
                isrc: None,
            }],
            ..ImportSelectionDto::default()
        };

        let plan = app.preview_import(&[path], &selection, false).unwrap();
        assert!(plan.changes[0]
            .tag_changes
            .iter()
            .all(|fc| fc.field != "disc" && fc.field != "disctotal"));
    }

    /// Last resort only: the folder names the disc, the release doesn't, and the
    /// file has none yet.
    #[test]
    fn import_falls_back_to_the_folder_name_for_a_disc() {
        let dir = TempDir::new("import-folder-disc");
        let inside = dir.tagged_flac_at("Album/CD2/f1.flac", "Old", "Old");
        let plain = dir.tagged_flac_at("Album/Bonus/f2.flac", "Old", "Old");
        // A folder that merely ends in a number is NOT a disc: the series that
        // raised #146 is filed as "... (1996) 2", where the 2 is the volume.
        let volume = dir.tagged_flac_at("Compilation (1996) 2/f3.flac", "Old", "Old");
        let app = open_app(&dir);
        let track = ImportTrackDto {
            disc: None,
            position: "1".into(),
            artist: "X".into(),
            title: "T".into(),
            duration_secs: None,
            isrc: None,
        };
        let selection = ImportSelectionDto {
            album: Some("Album".into()),
            tracks: vec![track.clone(), track.clone(), track],
            ..ImportSelectionDto::default()
        };

        let plan = app
            .preview_import(&[inside, plain, volume], &selection, false)
            .unwrap();
        let disc = |change: &FileChangeDto| {
            change
                .tag_changes
                .iter()
                .find(|fc| fc.field == "disc")
                .and_then(|fc| fc.new.clone())
        };
        assert_eq!(disc(&plan.changes[0]).as_deref(), Some("2"));
        assert_eq!(disc(&plan.changes[1]), None);
        assert_eq!(disc(&plan.changes[2]), None);
    }

    #[test]
    fn the_folder_disc_keyword_has_to_start_a_word() {
        let disc = |folder: &str| {
            disc_from_folder_name(&PathBuf::from(format!("/lib/{folder}/track.flac")), None)
        };
        assert_eq!(disc("CD2"), Some(2));
        assert_eq!(disc("CD 2"), Some(2));
        assert_eq!(disc("Disc 3"), Some(3));
        assert_eq!(disc("disk_1"), Some(1));
        assert_eq!(disc("Album - CD2"), Some(2));
        // Inside another word it is not a keyword. These bit for real: a temp
        // directory named "...-import-nodisc-<pid>-..." read the pid as a disc.
        assert_eq!(disc("nodisc-2"), None);
        assert_eq!(disc("Discography 2"), None);
        assert_eq!(disc("abcd 2"), None);
        // A keyword with no number, and a number with no keyword.
        assert_eq!(disc("CD"), None);
        // A number no disc could be: the keyword was followed by digits that
        // mean something else. Caught by a temp directory named
        // "...-single-disc-<pid>-..." reading as disc 99831.
        assert_eq!(disc("single-disc-99831"), None);
        assert_eq!(disc("CD 12"), Some(12));
        assert_eq!(disc("Compilation (1996) 2"), None);
        // Never overrides a disc the file already carries.
        assert_eq!(
            disc_from_folder_name(
                &PathBuf::from("/lib/CD2/track.flac"),
                Some(&"1".to_string())
            ),
            None
        );
    }

    // ---- mask-defined table columns (#150) ----

    #[test]
    fn render_column_renders_a_pattern_over_a_batch() {
        let dir = TempDir::new("render-column");
        let a = dir.tagged_flac("a.flac", "Massive Attack", "Safe From Harm");
        let b = dir.tagged_flac("b.flac", "Boards of Canada", "Roygbiv");
        let app = open_app(&dir);

        let rendered = app
            .render_column("%artist% — %title%", &[a.clone(), b.clone()])
            .unwrap();
        assert_eq!(
            rendered,
            vec![
                "Massive Attack — Safe From Harm".to_string(),
                "Boards of Canada — Roygbiv".to_string(),
            ]
        );
        // Order follows the paths given, since the frontend keys the cache on it.
        let reversed = app.render_column("%title%", &[b, a]).unwrap();
        assert_eq!(
            reversed,
            vec!["Roygbiv".to_string(), "Safe From Harm".to_string()]
        );
    }

    /// A column must show something for every row. A placeholder with nothing
    /// behind it renders empty instead of dropping the file -- otherwise the
    /// column would silently disagree with the table about how many rows exist.
    #[test]
    fn render_column_is_lenient_about_missing_tags() {
        let dir = TempDir::new("render-column-lenient");
        let track = dir.tagged_flac("a.flac", "Massive Attack", "Safe From Harm");
        let app = open_app(&dir);

        // Neither year nor media is set on this file. The bracketed part is a
        // conditional section, so it drops whole; the bare %media% renders empty
        // rather than failing the row.
        let rendered = app
            .render_column("%title% [(%year%)] %media%", std::slice::from_ref(&track))
            .unwrap();
        assert_eq!(rendered, vec!["Safe From Harm  ".to_string()]);
        assert_eq!(rendered.len(), 1);
    }

    /// File and technical placeholders (#147) work here too -- a column showing
    /// the bitrate or the containing folder is much of the point.
    #[test]
    fn render_column_resolves_file_placeholders() {
        let dir = TempDir::new("render-column-file");
        let track = dir.tagged_flac_at("Blue Lines/a.flac", "Massive Attack", "Safe");
        let app = open_app(&dir);

        let rendered = app
            .render_column("%foldername% · %_codec%", std::slice::from_ref(&track))
            .unwrap();
        assert_eq!(rendered, vec!["Blue Lines · FLAC".to_string()]);
    }

    /// A pattern that cannot render at all is an error before anything is shown,
    /// but a file that cannot be READ is just an empty cell -- the rest of the
    /// column still renders.
    #[test]
    fn render_column_reports_a_bad_pattern_and_tolerates_a_bad_file() {
        let dir = TempDir::new("render-column-bad");
        let good = dir.tagged_flac("a.flac", "Massive Attack", "Safe");
        let broken = dir.0.join("broken.flac");
        std::fs::write(&broken, b"not audio").unwrap();
        let app = open_app(&dir);

        assert!(app
            .render_column("%bogus%", std::slice::from_ref(&good))
            .is_err());
        // %skip% is extract-only, so it can never render.
        let extract_only = app
            .render_column("%skip%", std::slice::from_ref(&good))
            .unwrap();
        assert_eq!(extract_only, vec![String::new()]);

        let mixed = app.render_column("%title%", &[good, broken]).unwrap();
        assert_eq!(mixed, vec!["Safe".to_string(), String::new()]);
    }

    // ---- which fields an import may write (#152) ----

    /// A selection with every album- and track-level value populated, so an
    /// import off it emits the widest set of fields it ever can.
    fn maximal_selection() -> ImportSelectionDto {
        ImportSelectionDto {
            album: Some("Album".into()),
            album_artist: Some("Album Artist".into()),
            year: Some("1996".into()),
            genre: Some("House".into()),
            release_id: Some("316795".into()),
            source: Some("discogs".into()),
            label: Some("Antler-Subway".into()),
            catalog_number: Some("AS 5606".into()),
            country: Some("Belgium".into()),
            track_total: Some("15".into()),
            url: Some("https://www.discogs.com/release/316795".into()),
            media_type: Some("CD".into()),
            disc_total: Some("2".into()),
            tracks: vec![ImportTrackDto {
                disc: Some(2),
                position: "2-05".into(),
                artist: "The X Factor".into(),
                title: "Desert Rain".into(),
                duration_secs: Some(278),
                isrc: Some("GBAYE9800011".into()),
            }],
        }
    }

    /// The setting can only offer what the import actually writes, and the
    /// import must not write anything the setting can't reach. `import_fields`
    /// is a hand-kept list because `preview_import` builds its fields inline;
    /// this is what stops the two drifting.
    #[test]
    fn import_field_catalogue_covers_everything_an_import_writes() {
        let dir = TempDir::new("import-catalogue");
        let path = dir.tagged_flac("f1.flac", "Old", "Old");
        let app = open_app(&dir);

        let plan = app
            .preview_import(&[path], &maximal_selection(), false)
            .unwrap();
        let written: std::collections::BTreeSet<String> = plan.changes[0]
            .tag_changes
            .iter()
            .map(|fc| fc.field.clone())
            .collect();
        let listed: std::collections::BTreeSet<String> =
            import_fields().into_iter().flat_map(|f| f.keys).collect();

        // Everything written is listed, so every field can be switched off.
        let unlistable: Vec<&String> = written.difference(&listed).collect();
        assert!(
            unlistable.is_empty(),
            "the import writes fields the setting cannot reach: {unlistable:?}"
        );
        // And nothing is listed that an import can never produce, except the
        // MusicBrainz release-id key -- the same row covers both providers, and
        // this selection is a Discogs one.
        let stale: Vec<&String> = listed
            .difference(&written)
            .filter(|key| key.as_str() != "custom:MUSICBRAINZ_ALBUMID")
            .collect();
        assert!(stale.is_empty(), "the setting lists dead fields: {stale:?}");
    }

    #[test]
    fn an_import_skips_the_fields_the_setting_denies() {
        let dir = TempDir::new("import-skip");
        let path = dir.tagged_flac("f1.flac", "Old", "Old");
        let app = open_app(&dir);
        app.apply_settings(&SettingsDto {
            import_skip_fields: vec!["genre".into(), "url".into(), "custom:RELEASECOUNTRY".into()],
            ..SettingsDto::default()
        });

        let plan = app
            .preview_import(&[path], &maximal_selection(), false)
            .unwrap();
        let written: Vec<&str> = plan.changes[0]
            .tag_changes
            .iter()
            .map(|fc| fc.field.as_str())
            .collect();
        for denied in ["genre", "url", "custom:RELEASECOUNTRY"] {
            assert!(!written.contains(&denied), "{denied} was written anyway");
        }
        // The rest still arrives -- a denial is per field, not a switch on the
        // whole import.
        assert!(written.contains(&"album"));
        assert!(written.contains(&"title"));
    }

    /// The historical behaviour, and what an older settings.json deserializes
    /// to: an empty deny list writes everything.
    #[test]
    fn an_empty_deny_list_writes_every_field() {
        let dir = TempDir::new("import-nodeny");
        let path = dir.tagged_flac("f1.flac", "Old", "Old");
        let app = open_app(&dir);
        app.apply_settings(&SettingsDto::default());

        let plan = app
            .preview_import(&[path], &maximal_selection(), false)
            .unwrap();
        assert!(plan.changes[0].tag_changes.len() > 10);
        assert_eq!(
            serde_json::from_str::<SettingsDto>("{}")
                .unwrap()
                .import_skip_fields,
            Vec::<String>::new()
        );
    }

    /// A disc the file already carries is never overwritten by a folder guess --
    /// only by something the release or the user actually stated.
    #[test]
    fn the_folder_fallback_never_overwrites_an_existing_disc() {
        let dir = TempDir::new("import-folder-keep");
        let path = dir.tagged_flac_at("Album/CD2/f1.flac", "Old", "Old");
        let mut file = TagEngine::read(&path).unwrap();
        file.tags.insert(TagField::DiscNumber, "1".into());
        TagEngine::write(&file).unwrap();
        let app = open_app(&dir);
        let selection = ImportSelectionDto {
            album: Some("Album".into()),
            tracks: vec![ImportTrackDto {
                disc: None,
                position: "1".into(),
                artist: "X".into(),
                title: "T".into(),
                duration_secs: None,
                isrc: None,
            }],
            ..ImportSelectionDto::default()
        };

        let plan = app.preview_import(&[path], &selection, false).unwrap();
        assert!(plan.changes[0]
            .tag_changes
            .iter()
            .all(|fc| fc.field != "disc"));
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
            enabled: true,
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
    fn preview_transform_fileext_scope_touches_only_the_extension() {
        let dir = TempDir::new("transform-fileext");
        let shouty = dir.tagged_flac("Desert_Rain.FLAC", "The X Factor", "Desert Rain");
        let app = open_app(&dir);
        let rules = vec![case_rule("lower")];

        // Lower-casing the extension leaves the stem's capitals alone -- the whole
        // point of a scope separate from `filename`.
        let plan = app
            .preview_transform(std::slice::from_ref(&shouty), &rules, "fileext")
            .unwrap();
        assert_eq!(
            plan.changes[0].rename_to.as_deref(),
            Some(dir.0.join("Desert_Rain.flac").to_string_lossy().as_ref())
        );
        assert!(plan.changes[0].tag_changes.is_empty());

        // A rule that would move the file rather than retype the extension is not
        // a rename this scope offers, so the file is left out of the plan.
        let escaping = vec![replace_rule("flac", "../flac")];
        let refused = app
            .preview_transform(std::slice::from_ref(&shouty), &escaping, "fileext")
            .unwrap();
        assert!(refused.changes.is_empty());
    }

    #[test]
    fn transform_groups_compose_instead_of_overwriting_each_other() {
        let dir = TempDir::new("transform-groups");
        let track = dir.tagged_flac("Desert_Rain.FLAC", "The_X_Factor", "Desert_Rain");
        let app = open_app(&dir);

        let group = |name: &str, scope: &str, rules: Vec<TransformRuleDto>| ActionGroupDto {
            name: name.into(),
            scope: scope.into(),
            rules,
            note: String::new(),
        };

        // Two renaming groups over one file: the second must see the first's
        // result, not the name on disk, or one of them is silently lost.
        let plan = app
            .preview_transform_groups(
                std::slice::from_ref(&track),
                &[
                    group("underscores", "filename", vec![replace_rule("_", " ")]),
                    group("lower ext", "fileext", vec![case_rule("lower")]),
                ],
            )
            .unwrap();
        assert_eq!(plan.changes.len(), 1);
        assert_eq!(
            plan.changes[0].rename_to.as_deref(),
            Some(dir.0.join("Desert Rain.flac").to_string_lossy().as_ref())
        );

        // Same for two groups over the same field: one change carrying the net
        // result, not two changes racing each other.
        let plan = app
            .preview_transform_groups(
                std::slice::from_ref(&track),
                &[
                    group("underscores", "tags", vec![replace_rule("_", " ")]),
                    group("upper", "artist", vec![case_rule("upper")]),
                ],
            )
            .unwrap();
        let changed: std::collections::HashMap<_, _> = plan.changes[0]
            .tag_changes
            .iter()
            .map(|c| (c.field.clone(), c.new.clone().unwrap_or_default()))
            .collect();
        assert_eq!(
            changed.get("artist").map(String::as_str),
            Some("THE X FACTOR")
        );
        assert_eq!(
            changed.get("title").map(String::as_str),
            Some("Desert Rain")
        );
    }

    #[test]
    fn transform_groups_report_a_bad_rule_before_previewing_anything() {
        let dir = TempDir::new("transform-groups-bad");
        let track = dir.tagged_flac("x.flac", "Artist", "Title");
        let app = open_app(&dir);

        // The failing group is second, so a per-file loop would already have
        // produced changes for the first before noticing.
        let groups = vec![
            ActionGroupDto {
                name: "fine".into(),
                scope: "tags".into(),
                rules: vec![replace_rule("a", "b")],
                note: String::new(),
            },
            ActionGroupDto {
                name: "broken".into(),
                scope: "tags".into(),
                rules: vec![TransformRuleDto {
                    kind: "nonsense".into(),
                    ..replace_rule("a", "b")
                }],
                note: String::new(),
            },
        ];
        assert!(matches!(
            app.preview_transform_groups(std::slice::from_ref(&track), &groups),
            Err(AppError::UnknownTransform(_))
        ));
    }

    #[test]
    fn builtin_presets_all_build_into_a_chain() {
        // A preset is data, so a typo in a pattern or a style would otherwise
        // reach the user as an error toast on the day they click it.
        for group in builtin_action_groups() {
            assert!(!group.rules.is_empty(), "{} has no steps", group.name);
            assert!(!group.note.is_empty(), "{} has no note", group.name);
            build_chain(&group.rules)
                .unwrap_or_else(|err| panic!("preset {} does not build: {err}", group.name));
        }
    }

    #[test]
    fn builtin_presets_do_what_their_names_say() {
        let run = |name: &str, input: &str| {
            let group = builtin_action_groups()
                .into_iter()
                .find(|g| g.name == name)
                .unwrap_or_else(|| panic!("no preset named {name}"));
            build_chain(&group.rules).unwrap().apply(input)
        };

        assert_eq!(run("Standard values", "  Desert   Rain "), "Desert Rain");
        assert_eq!(run("Discogs cleanup", "Sunbeam (2)"), "Sunbeam");
        assert_eq!(
            run("No dash", "The X Factor - Desert Rain"),
            "The X Factor Desert Rain"
        );
        assert_eq!(run("General Latin", "Пётр Ильич"), "Pyotr Ilich");
        assert_eq!(
            run("FTP format", "Björk — Jóga (12\" Mix)"),
            "Bjork_Joga_12_Mix"
        );
        assert_eq!(run("File extension", "FLAC"), "flac");
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
                None,
                false,
                false,
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
                None,
                false,
                false,
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
                .preview_move(pattern, std::slice::from_ref(&track), None, false, false)
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
            .preview_move(
                "%album%/%title%",
                std::slice::from_ref(&track),
                None,
                false,
                false,
            )
            .unwrap();
        assert!(
            empty.changes.is_empty(),
            "empty folder component is skipped"
        );

        // A literal `..` in the pattern must never produce a plan, with either
        // separator.
        for pattern in ["../%title%", "..\\%title%"] {
            let escaping = app
                .preview_move(pattern, std::slice::from_ref(&track), None, false, false)
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
            ..CoverArt::default()
        };
        TagEngine::write_covers(&with_cover, std::slice::from_ref(&art)).unwrap();
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
    fn read_external_cover_finds_sibling_and_prefers_cover_over_folder() {
        let dir = TempDir::new("ext-cover");
        let a = dir.tagged_flac("a.flac", "Artist", "Title");
        // Both a folder.png and a cover.jpg sit next to the track (#41).
        std::fs::write(dir.0.join("folder.png"), [9u8, 9, 9]).unwrap();
        std::fs::write(dir.0.join("cover.jpg"), [1u8, 2, 3]).unwrap();
        let app = open_app(&dir);

        let found = app
            .read_external_cover(&[a])
            .unwrap()
            .expect("a sibling cover");
        // `cover.jpg` wins over `folder.png`.
        assert_eq!(found.mime, "image/jpeg");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&found.data_base64)
            .unwrap();
        assert_eq!(bytes, vec![1, 2, 3]);
    }

    #[test]
    fn read_external_cover_returns_none_without_a_sibling() {
        let dir = TempDir::new("ext-cover-none");
        let a = dir.tagged_flac("a.flac", "Artist", "Title");
        let app = open_app(&dir);
        assert!(app.read_external_cover(&[a]).unwrap().is_none());
    }

    #[test]
    fn find_duplicates_groups_by_normalized_artist_and_title() {
        let dir = TempDir::new("dups");
        dir.tagged_flac("a.flac", "The Field", "Over the Ice");
        // Same track after case/whitespace normalization -> a duplicate.
        dir.tagged_flac("b.flac", "the  field", "over the ice");
        // A unique track is never a group on its own.
        dir.tagged_flac("c.flac", "Someone Else", "Another Song");
        let app = open_app(&dir);

        let groups = app.find_duplicates("artist_title").unwrap();
        assert_eq!(groups.len(), 1, "only the shared track is a duplicate");
        assert_eq!(groups[0].files.len(), 2);
        // Members carry the columns needed to tell copies apart (#40).
        assert!(groups[0].files.iter().all(|f| f.size_bytes > 0));

        // A criterion the files can't satisfy (no album/track) yields nothing.
        assert!(app.find_duplicates("album_track").unwrap().is_empty());
    }

    #[test]
    fn export_cover_collapses_same_folder_targets_to_one_file() {
        let dir = TempDir::new("export-dedup");
        let a = dir.tagged_flac("a.flac", "Artist", "A");
        let b = dir.tagged_flac("b.flac", "Artist", "B");
        let art = CoverArt {
            mime: "image/jpeg".to_string(),
            data: vec![9, 8, 7],
            ..CoverArt::default()
        };
        TagEngine::write_covers(&a, std::slice::from_ref(&art)).unwrap();
        TagEngine::write_covers(&b, std::slice::from_ref(&art)).unwrap();
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
                disc: None,
                position: "11".into(),
                artist: "B.B.E.".into(),
                // Punctuation/decoration differs from the local tag.
                title: "Seven Days & One Week (Original Mix)".into(),
                duration_secs: None,
                isrc: None,
            },
            ImportTrackDto {
                disc: None,
                position: "14".into(),
                artist: "Plastic".into(),
                title: "Sexy Groove".into(),
                duration_secs: None,
                isrc: None,
            },
        ];

        // Each file finds its own track despite the order and the decoration.
        let aligned = app.auto_align(&[a, b], &tracks).unwrap();
        assert_eq!(aligned[0].map(|m| m.track), Some(1));
        assert_eq!(aligned[1].map(|m| m.track), Some(0));
        // These matched on title, not ISRC.
        assert!(aligned.iter().flatten().all(|m| !m.by_isrc));
    }

    #[test]
    fn auto_align_uses_isrc_as_an_exact_key() {
        let dir = TempDir::new("align-isrc");
        let path = dir.tagged_flac("a.flac", "Someone", "Totally Different Local Title");
        // Give the file an ISRC the release track also carries.
        let mut file = TagEngine::read(&path).unwrap();
        file.tags.insert(TagField::Isrc, "GB-AYE-12-34567".into());
        TagEngine::write(&file).unwrap();
        let app = open_app(&dir);

        let tracks = vec![
            ImportTrackDto {
                disc: None,
                position: "1".into(),
                artist: "Nobody".into(),
                title: "Unrelated Release Title".into(),
                duration_secs: None,
                isrc: Some("gbaye1234567".into()), // same ISRC, different formatting
            },
            ImportTrackDto {
                disc: None,
                position: "2".into(),
                artist: "Nobody".into(),
                title: "Totally Different Local Title".into(),
                duration_secs: None,
                isrc: None,
            },
        ];

        // The ISRC wins over the (closer) title of track 1, and is flagged.
        let aligned = app.auto_align(&[path], &tracks).unwrap();
        assert_eq!(aligned[0].map(|m| m.track), Some(0));
        assert!(aligned[0].unwrap().by_isrc);
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

    #[test]
    fn image_basename_names_primary_folder_then_cover_series() {
        // The user's convention (#102): primary -> folder, then cover, cover-1…
        assert_eq!(image_basename(0), "folder");
        assert_eq!(image_basename(1), "cover");
        assert_eq!(image_basename(2), "cover-1");
        assert_eq!(image_basename(3), "cover-2");
    }

    // ---- drag-and-drop resolution (#127) ----

    #[test]
    fn common_ancestor_is_the_shared_directory() {
        let paths = [
            PathBuf::from("/music/a/01.mp3"),
            PathBuf::from("/music/a/02.mp3"),
            PathBuf::from("/music/b/03.mp3"),
        ];
        assert_eq!(common_ancestor(&paths), Some(PathBuf::from("/music")));
        // A single file contributes its own directory.
        assert_eq!(
            common_ancestor(&[PathBuf::from("/music/a/01.mp3")]),
            Some(PathBuf::from("/music/a"))
        );
        assert_eq!(common_ancestor(&[]), None);
    }

    #[test]
    fn resolve_drop_single_folder_opens_as_library() {
        let dir = TempDir::new("drop-lib");
        dir.tagged_flac_at("album/01.flac", "A", "One");
        match resolve_drop(&[dir.0.join("album")]) {
            DropPlan::Library { root } => assert_eq!(root, dir.0.join("album")),
            other => panic!("expected Library, got {other:?}"),
        }
    }

    #[test]
    fn resolve_drop_folder_plus_loose_file_is_a_fileset() {
        let dir = TempDir::new("drop-set");
        let f1 = dir.tagged_flac_at("folderA/01.flac", "A", "One");
        let f2 = dir.tagged_flac_at("folderA/02.flac", "A", "Two");
        let loose = dir.tagged_flac_at("loose.flac", "L", "Loose");
        match resolve_drop(&[dir.0.join("folderA"), loose.clone()]) {
            DropPlan::FileSet {
                root,
                files,
                folders,
            } => {
                // Root is the common ancestor of the folder's files and the loose one.
                assert_eq!(root, dir.0);
                assert_eq!(folders, vec![dir.0.join("folderA")]);
                let mut got = files;
                got.sort();
                let mut want = vec![f1, f2, loose];
                want.sort();
                assert_eq!(got, want);
            }
            other => panic!("expected FileSet, got {other:?}"),
        }
    }

    #[test]
    fn resolve_drop_ignores_non_audio_and_missing() {
        let dir = TempDir::new("drop-junk");
        std::fs::write(dir.0.join("notes.txt"), b"hi").unwrap();
        let plan = resolve_drop(&[dir.0.join("notes.txt"), dir.0.join("ghost.mp3")]);
        assert!(matches!(plan, DropPlan::Empty));
    }

    #[test]
    fn file_set_lists_only_its_files() {
        let dir = TempDir::new("fileset-list");
        let a = dir.tagged_flac_at("01.flac", "A", "One");
        let _b = dir.tagged_flac_at("02.flac", "B", "Two");
        let c = dir.tagged_flac_at("03.flac", "C", "Three");
        let app = App::open_file_set(
            dir.0.clone(),
            vec![a.clone(), c.clone()],
            &dir.0.join("j.sqlite"),
        )
        .unwrap();
        let paths: Vec<String> = app.list_tracks().into_iter().map(|t| t.path).collect();
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&a.to_string_lossy().into_owned()));
        assert!(paths.contains(&c.to_string_lossy().into_owned()));
        // The un-listed file is on disk — filtered out of the session, not gone.
        assert!(dir.0.join("02.flac").exists());
    }

    #[test]
    fn read_cover_image_reads_by_extension_and_rejects_non_images() {
        let dir = TempDir::new("cover-read");
        let img = dir.0.join("art.png");
        std::fs::write(&img, [0x89, 0x50, 0x4e, 0x47]).unwrap(); // "\x89PNG" magic
        let dto = read_cover_image(&img).unwrap();
        assert_eq!(dto.mime, "image/png");
        assert!(!dto.data_base64.is_empty());
        // A .jpeg maps to image/jpeg; a non-image extension is refused.
        let jpg = dir.0.join("art.jpeg");
        std::fs::write(&jpg, [0xff, 0xd8]).unwrap();
        assert_eq!(read_cover_image(&jpg).unwrap().mime, "image/jpeg");
        let txt = dir.0.join("notes.txt");
        std::fs::write(&txt, b"nope").unwrap();
        assert!(matches!(
            read_cover_image(&txt),
            Err(AppError::NotAnImage(_))
        ));
    }
}
