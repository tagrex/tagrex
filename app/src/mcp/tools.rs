//! The tools an assistant can call (#828). Read-only, and limited to the files
//! the open library lists: a path that is not one of them is reported back,
//! never read.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{json, Value};

use tagrex_commands::{App, ErrorDto, PlanDto, ReleaseDto, TagEditDto, TrackDto};
use tagrex_core::model::TagField;

use super::lookup::{FORMATS, MAX_PER_PAGE, SOURCES};
use crate::{shell_error, AppState};

/// The most tracks one `list_tracks` page holds.
const MAX_PAGE: usize = 500;
const DEFAULT_PAGE: usize = 100;
/// The most files one `read_tracks` or `render_mask` call takes.
const MAX_READ: usize = 200;
const MAX_RENDER: usize = 500;
/// The duplicate criteria offered. Identical audio is left out: it decodes the
/// whole library and reports progress in the window.
const DUPLICATE_CRITERIA: [&str; 5] = ["artist_title", "album_track", "duration", "size", "hash"];
/// The most edits one proposal carries, and the longest summary and value.
const MAX_EDITS: usize = 5000;
const MAX_SUMMARY: usize = 200;
const MAX_VALUE: usize = 10_000;
/// The longest rename mask, and how many renames a rename proposal's answer
/// lists as examples.
const MAX_MASK: usize = 1000;
const RENAME_EXAMPLES: usize = 20;

/// Every tool, in the shape MCP's `tools/list` answers with.
pub fn definitions() -> Value {
    let read_only = json!({
        "readOnlyHint": true,
        "destructiveHint": false,
        "idempotentHint": true,
        "openWorldHint": false
    });
    // Asks a website, so the world it reads is open; still changes nothing.
    let online = json!({
        "readOnlyHint": true,
        "destructiveHint": false,
        "idempotentHint": true,
        "openWorldHint": true
    });
    json!([
        {
            "name": "library_status",
            "title": "Library status",
            "description": "Whether a library is open in TagRex, its folder, and how many audio files it lists. Call this first: every other tool works on the open library only.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false },
            "annotations": read_only
        },
        {
            "name": "list_tracks",
            "title": "List tracks",
            "description": "A page of the open library's tracks in TagRex's order, each with its path, format, length, whether it has a cover, and its tags as field -> value. Narrow it with `search` (case-insensitive text in the path or any tag value) and `missing` (a field that is empty or absent, or \"cover\" for files without a picture). `total` is the number of matching tracks; page through with `offset`.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "offset": { "type": "integer", "minimum": 0, "default": 0 },
                    "limit": { "type": "integer", "minimum": 1, "maximum": MAX_PAGE, "default": DEFAULT_PAGE },
                    "search": { "type": "string", "description": "Case-insensitive text to find in the path or any tag value." },
                    "missing": { "type": "string", "description": "A field key (see list_fields), or \"cover\"." },
                    "fields": { "type": "array", "items": { "type": "string" }, "description": "Only return these tag fields." }
                },
                "additionalProperties": false
            },
            "annotations": read_only
        },
        {
            "name": "read_tracks",
            "title": "Read tracks",
            "description": "Full details of the given files: every tag, the tag blocks each file carries (ID3v2.4, Vorbis Comments, ...), length, cover. Paths must be ones list_tracks returned; others are listed under `not_in_library`.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "paths": { "type": "array", "items": { "type": "string" }, "minItems": 1, "maxItems": MAX_READ }
                },
                "required": ["paths"],
                "additionalProperties": false
            },
            "annotations": read_only
        },
        {
            "name": "list_fields",
            "title": "List fields and mask placeholders",
            "description": "The tag fields TagRex knows and the placeholders and functions a mask can use, with what each means. Field keys are what list_tracks shows in `tags`.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false },
            "annotations": read_only
        },
        {
            "name": "render_mask",
            "title": "Render a mask",
            "description": "What a TagRex mask (for example \"%artist% - %title%\" or \"$upper(%album%)\") renders to for each given file, the same way a rename or a mask column would. Nothing is renamed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "mask": { "type": "string" },
                    "paths": { "type": "array", "items": { "type": "string" }, "minItems": 1, "maxItems": MAX_RENDER }
                },
                "required": ["mask", "paths"],
                "additionalProperties": false
            },
            "annotations": read_only
        },
        {
            "name": "find_duplicates",
            "title": "Find duplicates",
            "description": "Groups of files in the open library that look like copies of each other: same artist and title, same album and track number, same length, same size, or identical bytes (hash, slower).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "criterion": { "type": "string", "enum": DUPLICATE_CRITERIA }
                },
                "required": ["criterion"],
                "additionalProperties": false
            },
            "annotations": read_only
        },
        {
            "name": "propose_tag_edits",
            "title": "Propose tag edits",
            "description": "Stage tag changes in TagRex's preview for the user to review. Nothing is written: the user sees the changes in the file table, can untick files or adjust values, and clicks Apply or Discard. `field` is a key as list_tracks shows it (artist, title, album, track, year, genre, ..., or custom:NAME); a null or empty `value` clears the field. Refused while the user has other unapplied changes. The answer lists anything left out and a `proposal` id for proposal_status.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "summary": { "type": "string", "maxLength": MAX_SUMMARY, "description": "One line the user sees next to the changes, e.g. \"Fill in missing track numbers from the file names\"." },
                    "edits": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": MAX_EDITS,
                        "items": {
                            "type": "object",
                            "properties": {
                                "path": { "type": "string" },
                                "field": { "type": "string" },
                                "value": { "type": ["string", "null"] }
                            },
                            "required": ["path", "field"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["summary", "edits"],
                "additionalProperties": false
            },
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": false,
                "openWorldHint": false
            }
        },
        {
            "name": "propose_rename",
            "title": "Propose renaming files",
            "description": "Stage a rename of files in TagRex's preview for the user to review, built by TagRex's own renamer from a mask (see list_fields for placeholders; try it with render_mask first). Nothing moves until the user clicks Apply, and Undo takes it back. mode \"rename\" (default) renames each file in its folder; a / in the mask makes subfolders under the file's current folder. mode \"reorganize\" builds the path from the library root, e.g. \"%albumartist%/[%catalognumber%] %album%/%track% - %title%\", and removes folders it leaves empty; a folder outside the library cannot be targeted, and the folder that is the library root itself cannot be renamed (ask the user to open its parent folder in TagRex first). Refused while other changes are staged. The answer shows example renames, placeholders that were empty, and a `proposal` id for proposal_status.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "summary": { "type": "string", "maxLength": MAX_SUMMARY, "description": "One line the user sees next to the changes." },
                    "paths": { "type": "array", "minItems": 1, "maxItems": MAX_EDITS, "items": { "type": "string" } },
                    "mask": { "type": "string", "maxLength": MAX_MASK, "description": "The new name or path, without the file extension." },
                    "mode": { "type": "string", "enum": ["rename", "reorganize"], "default": "rename" }
                },
                "required": ["summary", "paths", "mask"],
                "additionalProperties": false
            },
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": false,
                "openWorldHint": false
            }
        },
        {
            "name": "propose_import",
            "title": "Propose importing a release",
            "description": "Stage an import of a release onto files, exactly as TagRex's own Import button does: the same fields (titles, artists, album, year, genre, label, catalogue number, country, media, track and disc numbers, release id, URL), the user's import settings and ONLINE rules, and the release cover per the import-cover setting, as one plan in the preview. Prefer this over propose_tag_edits when tagging from a release. Take source and release_id from search_releases, and pair each file with a release track by `position` as read_release spells it or by `track`, its 1-based number in the tracklist. When the release lists several labels, `catalog_number` picks one; otherwise the first is used. Refused while other changes are staged. The answer reports files or tracks it could not pair and a `proposal` id for proposal_status.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "summary": { "type": "string", "maxLength": MAX_SUMMARY },
                    "source": { "type": "string", "enum": SOURCES },
                    "release_id": { "type": "string" },
                    "pairs": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": MAX_EDITS,
                        "items": {
                            "type": "object",
                            "properties": {
                                "path": { "type": "string" },
                                "position": { "type": "string" },
                                "track": { "type": "integer", "minimum": 1 }
                            },
                            "required": ["path"],
                            "additionalProperties": false
                        }
                    },
                    "catalog_number": { "type": "string" }
                },
                "required": ["summary", "source", "release_id", "pairs"],
                "additionalProperties": false
            },
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": false,
                "openWorldHint": true
            }
        },
        {
            "name": "proposal_status",
            "title": "Proposal status",
            "description": "What happened to a proposal from propose_tag_edits, propose_rename or propose_import: still staged, applied (to how many files) or discarded.",
            "inputSchema": {
                "type": "object",
                "properties": { "proposal": { "type": "string" } },
                "required": ["proposal"],
                "additionalProperties": false
            },
            "annotations": read_only
        },
        {
            "name": "search_releases",
            "title": "Search releases",
            "description": "Search an online source for releases through TagRex, with its saved Discogs token or Beatport sign-in (neither is ever returned). Works without an open library. Give at least one of artist, title, album or catalog_number. Results are scored against the query; use read_release for the tracklist. read_settings says which sources are ready.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "source": { "type": "string", "enum": SOURCES },
                    "artist": { "type": "string" },
                    "title": { "type": "string", "description": "A track title." },
                    "album": { "type": "string", "description": "The release title." },
                    "catalog_number": { "type": "string" },
                    "format": { "type": "string", "enum": FORMATS },
                    "page": { "type": "integer", "minimum": 1, "default": 1 },
                    "per_page": { "type": "integer", "minimum": 1, "maximum": MAX_PER_PAGE }
                },
                "required": ["source"],
                "additionalProperties": false
            },
            "annotations": online
        },
        {
            "name": "read_release",
            "title": "Read release",
            "description": "One release from an online source, by the id search_releases gave: tracklist with positions, discs, titles, artists, lengths and, where the source has them, ISRC, BPM and key; labels with catalogue numbers, genres and styles, country, format, page URL and images.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "source": { "type": "string", "enum": SOURCES },
                    "release_id": { "type": "string" }
                },
                "required": ["source", "release_id"],
                "additionalProperties": false
            },
            "annotations": online
        },
        {
            "name": "read_settings",
            "title": "Read settings",
            "description": "The TagRex settings that decide what it writes: ID3v2 version, how an online import fills tags (cover, vinyl numbering, skipped fields, where the genre comes from), the multi-value separator, portable file names, what travels with a renamed file, the saved mask presets, and which online sources are ready. Use it to tag the way the user's imports do.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false },
            "annotations": read_only
        }
    ])
}

/// Whether `name` is a tool [`definitions`] lists.
pub fn exists(name: &str) -> bool {
    definitions()
        .as_array()
        .is_some_and(|tools| tools.iter().any(|tool| tool["name"] == name))
}

/// Run one tool against the window's session. Blocking: called on the session
/// queue, like every command that reads the library.
pub fn call(state: &AppState, name: &str, arguments: Value) -> Result<Value, ErrorDto> {
    let guard = state.lock().map_err(shell_error)?;
    if name == "library_status" {
        return Ok(library_status(guard.as_ref()));
    }
    let app = guard
        .as_ref()
        .ok_or_else(|| ErrorDto::from(tagrex_commands::AppError::NoLibraryOpen))?;
    run(app, name, arguments)
}

fn run(app: &App, name: &str, arguments: Value) -> Result<Value, ErrorDto> {
    match name {
        "list_tracks" => list_tracks(app, parse(arguments)?),
        "read_tracks" => read_tracks(app, parse(arguments)?),
        "list_fields" => Ok(list_fields()),
        "render_mask" => render_mask(app, parse(arguments)?),
        "find_duplicates" => find_duplicates(app, parse(arguments)?),
        _ => Err(ErrorDto::plain(format!("unknown tool: {name}"))),
    }
}

fn parse<T: for<'de> Deserialize<'de>>(arguments: Value) -> Result<T, ErrorDto> {
    let arguments = if arguments.is_null() {
        json!({})
    } else {
        arguments
    };
    serde_json::from_value(arguments)
        .map_err(|error| ErrorDto::plain(format!("invalid arguments: {error}")))
}

fn library_status(app: Option<&App>) -> Value {
    match app {
        None => json!({ "open": false }),
        Some(app) if app.library_root_missing() => json!({
            "open": true,
            "root": app.library_root().to_string_lossy(),
            "file_set": false,
            "root_missing": true,
            "track_count": 0,
            "note": "This folder no longer exists: it was renamed, moved or deleted outside TagRex. Ask the user to open it again in TagRex from its new place.",
        }),
        Some(app) => json!({
            "open": true,
            "root": app.library_root().to_string_lossy(),
            "file_set": app.is_file_set(),
            "track_count": app.list_tracks().len(),
        }),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListArgs {
    #[serde(default)]
    offset: usize,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    search: Option<String>,
    #[serde(default)]
    missing: Option<String>,
    #[serde(default)]
    fields: Option<Vec<String>>,
}

fn is_missing(track: &TrackDto, field: &str) -> bool {
    if field == "cover" {
        return !track.has_cover;
    }
    track
        .tags
        .get(field)
        .is_none_or(|value| value.trim().is_empty())
}

fn matches_search(track: &TrackDto, needle: &str) -> bool {
    track.path.to_lowercase().contains(needle)
        || track
            .tags
            .values()
            .any(|value| value.to_lowercase().contains(needle))
}

fn list_tracks(app: &App, args: ListArgs) -> Result<Value, ErrorDto> {
    let limit = args.limit.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE);
    let needle = args
        .search
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_lowercase);
    let missing = args
        .missing
        .as_deref()
        .map(str::trim)
        .filter(|field| !field.is_empty());
    let matching: Vec<TrackDto> = app
        .list_tracks()
        .into_iter()
        .filter(|track| needle.as_deref().is_none_or(|n| matches_search(track, n)))
        .filter(|track| missing.is_none_or(|field| is_missing(track, field)))
        .collect();
    let total = matching.len();
    let tracks: Vec<Value> = matching
        .into_iter()
        .skip(args.offset)
        .take(limit)
        .map(|mut track| {
            if let Some(fields) = &args.fields {
                track.tags.retain(|key, _| fields.iter().any(|f| f == key));
            }
            json!({
                "path": track.path,
                "format": track.format,
                "duration_secs": track.duration_secs,
                "has_cover": track.has_cover,
                "unreadable": track.unreadable,
                "tags": track.tags,
            })
        })
        .collect();
    Ok(json!({
        "total": total,
        "offset": args.offset,
        "returned": tracks.len(),
        "tracks": tracks,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadArgs {
    paths: Vec<String>,
}

fn read_tracks(app: &App, args: ReadArgs) -> Result<Value, ErrorDto> {
    if args.paths.len() > MAX_READ {
        return Err(ErrorDto::plain(format!(
            "at most {MAX_READ} paths per call; got {}",
            args.paths.len()
        )));
    }
    let mut listed: HashMap<String, TrackDto> = app
        .list_tracks()
        .into_iter()
        .map(|track| (track.path.clone(), track))
        .collect();
    let mut tracks = Vec::new();
    let mut outside = Vec::new();
    for path in args.paths {
        match listed.remove(&path) {
            Some(track) => tracks.push(track),
            None => outside.push(path),
        }
    }
    Ok(json!({ "tracks": tracks, "not_in_library": outside }))
}

fn list_fields() -> Value {
    let placeholders: Vec<Value> = tagrex_commands::mask_placeholders()
        .into_iter()
        .map(|p| json!({ "token": p.token, "name": p.name, "description": p.description }))
        .collect();
    json!({ "placeholders": placeholders })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RenderArgs {
    mask: String,
    paths: Vec<String>,
}

fn render_mask(app: &App, args: RenderArgs) -> Result<Value, ErrorDto> {
    if args.paths.len() > MAX_RENDER {
        return Err(ErrorDto::plain(format!(
            "at most {MAX_RENDER} paths per call; got {}",
            args.paths.len()
        )));
    }
    let listed: HashSet<String> = app
        .list_tracks()
        .into_iter()
        .map(|track| track.path)
        .collect();
    let (inside, outside): (Vec<String>, Vec<String>) = args
        .paths
        .into_iter()
        .partition(|path| listed.contains(path));
    let paths: Vec<PathBuf> = inside.iter().map(PathBuf::from).collect();
    let rendered = app
        .render_column(&args.mask, &paths)
        .map_err(ErrorDto::from)?;
    let values: Vec<Value> = inside
        .into_iter()
        .zip(rendered)
        .map(|(path, value)| json!({ "path": path, "value": value }))
        .collect();
    Ok(json!({ "values": values, "not_in_library": outside }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DuplicateArgs {
    criterion: String,
}

fn find_duplicates(app: &App, args: DuplicateArgs) -> Result<Value, ErrorDto> {
    if !DUPLICATE_CRITERIA.contains(&args.criterion.as_str()) {
        return Err(ErrorDto::plain(format!(
            "criterion must be one of {}",
            DUPLICATE_CRITERIA.join(", ")
        )));
    }
    let groups = app
        .find_duplicates(&args.criterion)
        .map_err(ErrorDto::from)?;
    Ok(json!({ "groups": groups }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposeArgs {
    summary: String,
    edits: Vec<EditArg>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditArg {
    path: String,
    field: String,
    #[serde(default)]
    value: Option<String>,
}

/// A proposal checked against the open library (#830): the edits that would
/// change something, ready for the window to stage, and what was left out.
pub struct Prepared {
    pub summary: String,
    pub edits: Vec<TagEditDto>,
    /// What the assistant is told besides the counts: everything left out.
    pub report: Value,
}

/// A key the app writes as given. Anything else would be read as a custom
/// field of that name, so a typo such as `titel` must not get through.
fn is_known_field(key: &str) -> bool {
    TagField::from_storage_key(key).to_storage_key() == key
        && key
            .strip_prefix("custom:")
            .is_none_or(|name| !name.trim().is_empty())
}

/// Check a proposal (#830). Blocking: run on the session queue.
pub fn prepare_proposal(state: &AppState, arguments: Value) -> Result<Prepared, ErrorDto> {
    let guard = state.lock().map_err(shell_error)?;
    let app = guard
        .as_ref()
        .ok_or_else(|| ErrorDto::from(tagrex_commands::AppError::NoLibraryOpen))?;
    prepare(app, parse(arguments)?)
}

fn prepare(app: &App, args: ProposeArgs) -> Result<Prepared, ErrorDto> {
    let summary: String = args.summary.trim().chars().take(MAX_SUMMARY).collect();
    if summary.is_empty() {
        return Err(ErrorDto::plain("a summary is required"));
    }
    if args.edits.is_empty() || args.edits.len() > MAX_EDITS {
        return Err(ErrorDto::plain(format!(
            "between 1 and {MAX_EDITS} edits per proposal; got {}",
            args.edits.len()
        )));
    }
    let listed: HashMap<String, bool> = app
        .list_tracks()
        .into_iter()
        .map(|track| (track.path, track.unreadable))
        .collect();
    let mut outside = Vec::new();
    let mut unreadable = Vec::new();
    let mut unknown = Vec::new();
    let mut too_long = Vec::new();
    // The last edit of a field wins, in the order the files were named.
    let mut order: Vec<(String, String)> = Vec::new();
    let mut values: HashMap<(String, String), Option<String>> = HashMap::new();
    for edit in args.edits {
        match listed.get(&edit.path) {
            None => {
                if !outside.contains(&edit.path) {
                    outside.push(edit.path);
                }
                continue;
            }
            Some(true) => {
                if !unreadable.contains(&edit.path) {
                    unreadable.push(edit.path);
                }
                continue;
            }
            Some(false) => {}
        }
        if !is_known_field(&edit.field) {
            if !unknown.contains(&edit.field) {
                unknown.push(edit.field);
            }
            continue;
        }
        if edit
            .value
            .as_deref()
            .is_some_and(|value| value.chars().count() > MAX_VALUE)
        {
            too_long.push(json!({ "path": edit.path, "field": edit.field }));
            continue;
        }
        let key = (edit.path, edit.field);
        if !values.contains_key(&key) {
            order.push(key.clone());
        }
        values.insert(key, edit.value);
    }
    let candidates: Vec<TagEditDto> = order
        .into_iter()
        .map(|key| {
            let value = values.remove(&key).flatten();
            TagEditDto {
                path: key.0,
                field: key.1,
                value,
            }
        })
        .collect();
    // The same plan the window will build: it drops what would not change,
    // keeps locked fields out and flags values the format cannot hold.
    let plan = if candidates.is_empty() {
        None
    } else {
        Some(app.preview_tag_edits(&candidates).map_err(ErrorDto::from)?)
    };
    let mut edits = Vec::new();
    let mut invalid = Vec::new();
    let mut locked = Vec::new();
    if let Some(plan) = &plan {
        for change in &plan.changes {
            for field_change in &change.tag_changes {
                if field_change.invalid {
                    invalid.push(json!({ "path": change.path, "field": field_change.field }));
                    continue;
                }
                edits.push(TagEditDto {
                    path: change.path.clone(),
                    field: field_change.field.clone(),
                    value: field_change.new.clone(),
                });
            }
        }
        locked = plan
            .locked_skipped
            .iter()
            .map(|skip| json!({ "field": skip.field, "files": skip.files }))
            .collect();
    }
    // Already that value, or kept out by a locked field (named below).
    let unchanged = candidates.len().saturating_sub(edits.len() + invalid.len());
    Ok(Prepared {
        summary,
        edits,
        report: json!({
            "unchanged": unchanged,
            "not_in_library": outside,
            "unreadable": unreadable,
            "unknown_fields": unknown,
            "too_long": too_long,
            "invalid_values": invalid,
            "locked_fields": locked,
        }),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportArgs {
    summary: String,
    pub source: String,
    pub release_id: String,
    pairs: Vec<PairArg>,
    #[serde(default)]
    catalog_number: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PairArg {
    path: String,
    #[serde(default)]
    position: Option<String>,
    #[serde(default)]
    track: Option<usize>,
}

/// An import proposal's arguments (#839), checked before the release is
/// fetched.
pub fn parse_import(arguments: Value) -> Result<ImportArgs, ErrorDto> {
    let args: ImportArgs = parse(arguments)?;
    if args.summary.trim().is_empty() {
        return Err(ErrorDto::plain("a summary is required"));
    }
    if args.release_id.trim().is_empty() {
        return Err(ErrorDto::plain("a release_id is required"));
    }
    if args.pairs.is_empty() || args.pairs.len() > MAX_EDITS {
        return Err(ErrorDto::plain(format!(
            "between 1 and {MAX_EDITS} file and track pairs per proposal; got {}",
            args.pairs.len()
        )));
    }
    Ok(args)
}

/// An import proposal resolved against its release and the library: each
/// file with the index of its release track, in the order given.
pub struct ResolvedImport {
    pub summary: String,
    pub pairs: Vec<(String, usize)>,
    pub catalog_number: Option<String>,
    pub report: Value,
}

/// The paths the open library lists, and whether each is unreadable.
/// Blocking: run on the session queue.
pub fn listed_paths(state: &AppState) -> Result<HashMap<String, bool>, ErrorDto> {
    let guard = state.lock().map_err(shell_error)?;
    let app = guard
        .as_ref()
        .ok_or_else(|| ErrorDto::from(tagrex_commands::AppError::NoLibraryOpen))?;
    Ok(app
        .list_tracks()
        .into_iter()
        .map(|track| (track.path, track.unreadable))
        .collect())
}

/// Pair files with release tracks (#839): by `position` as the release spells
/// it, or by `track`, the 1-based number in its tracklist.
pub fn resolve_import(
    release: &ReleaseDto,
    listed: &HashMap<String, bool>,
    args: ImportArgs,
) -> Result<ResolvedImport, ErrorDto> {
    let catalog_number = match args
        .catalog_number
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
    {
        None => None,
        Some(wanted) => {
            let found = release
                .labels
                .iter()
                .any(|label| label.catalog_number.as_deref() == Some(wanted.as_str()));
            if !found {
                let listed: Vec<&str> = release
                    .labels
                    .iter()
                    .filter_map(|label| label.catalog_number.as_deref())
                    .collect();
                return Err(ErrorDto::plain(format!(
                    "catalogue number {wanted:?} is not on this release; it lists: {}",
                    if listed.is_empty() {
                        "none".to_string()
                    } else {
                        listed.join(", ")
                    }
                )));
            }
            Some(wanted)
        }
    };
    let mut outside = Vec::new();
    let mut unreadable = Vec::new();
    let mut unknown = Vec::new();
    let mut repeated = Vec::new();
    let mut pairs: Vec<(String, usize)> = Vec::new();
    for pair in args.pairs {
        match listed.get(&pair.path) {
            None => {
                outside.push(pair.path);
                continue;
            }
            Some(true) => {
                unreadable.push(pair.path);
                continue;
            }
            Some(false) => {}
        }
        let index = match (&pair.position, pair.track) {
            (Some(position), None) => {
                let matching: Vec<usize> = release
                    .tracks
                    .iter()
                    .enumerate()
                    .filter(|(_, track)| track.position == *position)
                    .map(|(index, _)| index)
                    .collect();
                // An ambiguous position (some releases repeat or omit them) has
                // to be named by number instead.
                (matching.len() == 1).then(|| matching[0])
            }
            (None, Some(number)) => (1..=release.tracks.len())
                .contains(&number)
                .then(|| number - 1),
            _ => None,
        };
        let Some(index) = index else {
            unknown
                .push(json!({ "path": pair.path, "position": pair.position, "track": pair.track }));
            continue;
        };
        if pairs
            .iter()
            .any(|(path, used)| *path == pair.path || *used == index)
        {
            repeated.push(json!({ "path": pair.path, "track": index + 1 }));
            continue;
        }
        pairs.push((pair.path, index));
    }
    Ok(ResolvedImport {
        summary: args.summary.trim().chars().take(MAX_SUMMARY).collect(),
        pairs,
        catalog_number,
        report: json!({
            "release": { "id": release.id, "artist": release.artist, "title": release.title },
            "not_in_library": outside,
            "unreadable": unreadable,
            "unknown_tracks": unknown,
            "repeated": repeated,
        }),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RenameArgs {
    summary: String,
    paths: Vec<String>,
    mask: String,
    #[serde(default)]
    mode: Option<String>,
}

/// A rename proposal checked against the open library (#835): the plan the
/// RENAMER would stage for the same mask, and what was left out.
pub struct PreparedRename {
    pub summary: String,
    pub plan: PlanDto,
    pub report: Value,
}

/// Check a rename proposal (#835). Blocking: run on the session queue.
pub fn prepare_rename_proposal(
    state: &AppState,
    arguments: Value,
) -> Result<PreparedRename, ErrorDto> {
    let guard = state.lock().map_err(shell_error)?;
    let app = guard
        .as_ref()
        .ok_or_else(|| ErrorDto::from(tagrex_commands::AppError::NoLibraryOpen))?;
    prepare_rename(app, parse(arguments)?)
}

fn prepare_rename(app: &App, args: RenameArgs) -> Result<PreparedRename, ErrorDto> {
    let summary: String = args.summary.trim().chars().take(MAX_SUMMARY).collect();
    if summary.is_empty() {
        return Err(ErrorDto::plain("a summary is required"));
    }
    let mask = args.mask.trim();
    if mask.is_empty() || mask.chars().count() > MAX_MASK {
        return Err(ErrorDto::plain(format!(
            "a mask of 1 to {MAX_MASK} characters is required"
        )));
    }
    let reorganize = match args.mode.as_deref().unwrap_or("rename") {
        "rename" => false,
        "reorganize" => true,
        other => {
            return Err(ErrorDto::plain(format!(
                "unknown mode {other:?}; use rename or reorganize"
            )))
        }
    };
    if args.paths.is_empty() || args.paths.len() > MAX_EDITS {
        return Err(ErrorDto::plain(format!(
            "between 1 and {MAX_EDITS} files per proposal; got {}",
            args.paths.len()
        )));
    }
    let listed: HashMap<String, bool> = app
        .list_tracks()
        .into_iter()
        .map(|track| (track.path, track.unreadable))
        .collect();
    let mut outside = Vec::new();
    let mut unreadable = Vec::new();
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut seen = HashSet::new();
    for path in args.paths {
        match listed.get(&path) {
            None => outside.push(path),
            Some(true) => unreadable.push(path),
            Some(false) => {
                if seen.insert(path.clone()) {
                    paths.push(PathBuf::from(path));
                }
            }
        }
    }
    // The RENAMER's own plans. A reorganize stays inside the library and
    // removes the folders it empties; a destination elsewhere is the user's
    // own pick in the panel, never an assistant's.
    let plan = if paths.is_empty() {
        None
    } else if reorganize {
        Some(
            app.preview_move(mask, &paths, None, false, true)
                .map_err(ErrorDto::from)?,
        )
    } else {
        Some(app.preview_rename(mask, &paths).map_err(ErrorDto::from)?)
    };
    let examples: Vec<Value> = plan
        .iter()
        .flat_map(|plan| plan.changes.iter())
        .filter_map(|change| {
            let to = change.rename_to.as_ref()?;
            Some(json!({ "from": change.path, "to": to }))
        })
        .take(RENAME_EXAMPLES)
        .collect();
    let unrendered: Vec<Value> = plan
        .iter()
        .flat_map(|plan| plan.unrendered.iter())
        .map(|missing| json!({ "placeholder": format!("%{}%", missing.field), "files": missing.files }))
        .collect();
    let renamed = plan.as_ref().map_or(0, |plan| {
        plan.changes
            .iter()
            .filter(|c| c.rename_to.is_some())
            .count()
    });
    let unrendered_files: usize = plan
        .iter()
        .flat_map(|plan| plan.unrendered.iter())
        .map(|missing| missing.files)
        .sum();
    let unchanged = paths.len().saturating_sub(renamed + unrendered_files);
    let plan = plan.unwrap_or_else(|| PlanDto {
        description: String::new(),
        message: None,
        notes: Vec::new(),
        changes: Vec::new(),
        prune_empty_dirs: false,
        locked_skipped: Vec::new(),
        unrendered: Vec::new(),
    });
    Ok(PreparedRename {
        summary,
        plan,
        report: json!({
            "mode": if reorganize { "reorganize" } else { "rename" },
            "renames": examples,
            "unchanged": unchanged,
            "empty_placeholders": unrendered,
            "not_in_library": outside,
            "unreadable": unreadable,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_tool_writes_and_every_one_has_an_object_schema() {
        for tool in definitions().as_array().unwrap() {
            // A proposal changes what the window has staged, never a file.
            let read_only = !tool["name"].as_str().unwrap().starts_with("propose_");
            assert_eq!(tool["annotations"]["readOnlyHint"], read_only, "{tool}");
            assert_eq!(tool["annotations"]["destructiveHint"], false, "{tool}");
            assert_eq!(tool["inputSchema"]["type"], "object", "{tool}");
            assert!(exists(tool["name"].as_str().unwrap()));
        }
        assert!(!exists("write_tags"));
    }

    #[test]
    fn arguments_of_the_wrong_shape_are_refused() {
        assert!(parse::<ReadArgs>(json!({})).is_err());
        assert!(parse::<ListArgs>(json!({ "limit": "ten" })).is_err());
        assert!(parse::<ListArgs>(json!({ "unknown": 1 })).is_err());
        assert!(parse::<ListArgs>(Value::Null).is_ok());
    }

    #[test]
    fn library_status_without_a_library_says_so() {
        assert_eq!(library_status(None), json!({ "open": false }));
    }

    // #839: files pair with release tracks by position or by number.
    #[test]
    fn an_import_pairs_files_with_tracks_and_reports_the_rest() {
        use tagrex_commands::{ReleaseLabelDto, ReleaseTrackDto};
        let track = |position: &str, title: &str| ReleaseTrackDto {
            position: position.into(),
            disc: None,
            artist: None,
            title: title.into(),
            duration_secs: None,
            isrc: None,
            bpm: None,
            key: None,
            section: None,
        };
        let release = ReleaseDto {
            id: "1".into(),
            artist: "Artist".into(),
            title: "Release".into(),
            year: None,
            genres: vec![],
            styles: vec![],
            tracks: vec![
                track("A1", "One"),
                track("A2", "Two"),
                track("", "Three"),
                track("", "Four"),
            ],
            labels: vec![ReleaseLabelDto {
                name: "Label".into(),
                catalog_number: Some("CAT 1".into()),
            }],
            country: None,
            format: None,
            disc_total: None,
            url: None,
            cover_image_url: None,
            images: vec![],
        };
        let listed: HashMap<String, bool> = [
            ("/l/a", false),
            ("/l/b", false),
            ("/l/c", false),
            ("/l/d", false),
            ("/l/bad", true),
        ]
        .into_iter()
        .map(|(path, unreadable)| (path.to_string(), unreadable))
        .collect();
        let args = |value: Value| parse_import(value).unwrap();
        let resolved = resolve_import(
            &release,
            &listed,
            args(json!({
                "summary": " Tag it ",
                "source": "discogs",
                "release_id": "1",
                "catalog_number": "CAT 1",
                "pairs": [
                    { "path": "/l/a", "position": "A2" },
                    { "path": "/l/b", "track": 1 },
                    { "path": "/l/c", "position": "" },
                    { "path": "/l/d", "track": 3 },
                    { "path": "/l/c", "track": 2 },
                    { "path": "/l/bad", "track": 4 },
                    { "path": "/elsewhere", "track": 4 },
                    { "path": "/l/a", "track": 9 }
                ]
            })),
        )
        .unwrap();
        assert_eq!(resolved.summary, "Tag it");
        assert_eq!(
            resolved.pairs,
            vec![
                ("/l/a".to_string(), 1),
                ("/l/b".to_string(), 0),
                ("/l/d".to_string(), 2)
            ]
        );
        assert_eq!(resolved.catalog_number.as_deref(), Some("CAT 1"));
        assert_eq!(resolved.report["not_in_library"], json!(["/elsewhere"]));
        assert_eq!(resolved.report["unreadable"], json!(["/l/bad"]));
        // An ambiguous empty position and a number past the tracklist.
        assert_eq!(
            resolved.report["unknown_tracks"].as_array().unwrap().len(),
            2
        );
        // The second pair for /l/c names a track already used.
        assert_eq!(
            resolved.report["repeated"],
            json!([{ "path": "/l/c", "track": 2 }])
        );

        let wrong_label = args(json!({
            "summary": "x", "source": "discogs", "release_id": "1",
            "catalog_number": "NOPE", "pairs": [{ "path": "/l/a", "track": 1 }]
        }));
        assert!(resolve_import(&release, &listed, wrong_label).is_err());
        assert!(parse_import(json!({ "summary": " ", "source": "discogs", "release_id": "1", "pairs": [{ "path": "/l/a" }] })).is_err());
        assert!(parse_import(
            json!({ "summary": "x", "source": "discogs", "release_id": "1", "pairs": [] })
        )
        .is_err());
    }

    // #838: a library folder renamed behind the app's back is reported as gone.
    #[test]
    fn library_status_names_a_folder_that_has_gone() {
        let base = std::env::temp_dir().join(format!("tagrex-mcp-gone-{}", std::process::id()));
        let root = base.join("library");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.wav"), tagged_wav(&[(b"INAM", "First")])).unwrap();
        let app = App::open(&root, &base.join("journal.sqlite")).unwrap();
        assert_eq!(library_status(Some(&app))["track_count"], 1);
        assert!(library_status(Some(&app))["root_missing"].is_null());

        std::fs::rename(&root, base.join("renamed")).unwrap();
        let status = library_status(Some(&app));
        assert_eq!(status["root_missing"], true);
        assert_eq!(status["track_count"], 0);
        assert!(status["note"].as_str().unwrap().contains("open it again"));
        drop(app);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A short silent WAV whose RIFF INFO chunk holds `tags` (four-letter id,
    /// value).
    fn tagged_wav(tags: &[(&[u8; 4], &str)]) -> Vec<u8> {
        let mut info = b"INFO".to_vec();
        for (id, value) in tags {
            let mut data = value.as_bytes().to_vec();
            data.push(0);
            info.extend_from_slice(*id);
            info.extend_from_slice(&(data.len() as u32).to_le_bytes());
            if data.len() % 2 == 1 {
                data.push(0);
            }
            info.extend_from_slice(&data);
        }
        let samples = vec![0u8; 16_000];
        let mut out = b"RIFF".to_vec();
        out.extend_from_slice(&((36 + samples.len() + 8 + info.len()) as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        for field in [16u32, 1 | (1 << 16), 8_000, 16_000, 2 | (16 << 16)] {
            out.extend_from_slice(&field.to_le_bytes());
        }
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(samples.len() as u32).to_le_bytes());
        out.extend_from_slice(&samples);
        out.extend_from_slice(b"LIST");
        out.extend_from_slice(&(info.len() as u32).to_le_bytes());
        out.extend_from_slice(&info);
        out
    }

    // #828: the tools see the open library and nothing else.
    #[test]
    fn the_tools_read_the_open_library_only() {
        let base = std::env::temp_dir().join(format!("tagrex-mcp-tools-{}", std::process::id()));
        let root = base.join("library");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("a.wav"),
            tagged_wav(&[(b"IART", "Alpha"), (b"INAM", "First")]),
        )
        .unwrap();
        std::fs::write(root.join("b.wav"), tagged_wav(&[(b"IART", "Beta")])).unwrap();
        let outside = base.join("outside.wav");
        std::fs::write(&outside, tagged_wav(&[(b"IART", "Gamma")])).unwrap();
        let app = App::open(&root, &base.join("journal.sqlite")).unwrap();
        let a = root.join("a.wav").to_string_lossy().into_owned();
        let b = root.join("b.wav").to_string_lossy().into_owned();
        let outside = outside.to_string_lossy().into_owned();

        let status = library_status(Some(&app));
        assert_eq!(status["open"], true);
        assert_eq!(status["track_count"], 2);

        let all = run(&app, "list_tracks", json!({})).unwrap();
        assert_eq!(all["total"], 2);
        assert_eq!(all["tracks"][0]["tags"]["artist"], "Alpha");
        let untitled = run(&app, "list_tracks", json!({ "missing": "title" })).unwrap();
        assert_eq!(untitled["total"], 1);
        assert_eq!(untitled["tracks"][0]["path"], b);
        let found = run(&app, "list_tracks", json!({ "search": "alpha" })).unwrap();
        assert_eq!(found["total"], 1);
        let page = run(
            &app,
            "list_tracks",
            json!({ "offset": 1, "limit": 1, "fields": ["title"] }),
        )
        .unwrap();
        assert_eq!(page["returned"], 1);
        assert_eq!(page["tracks"][0]["tags"], json!({}));

        let read = run(&app, "read_tracks", json!({ "paths": [a, outside] })).unwrap();
        assert_eq!(read["tracks"].as_array().unwrap().len(), 1);
        assert_eq!(read["not_in_library"], json!([outside]));

        let rendered = run(
            &app,
            "render_mask",
            json!({ "mask": "%artist% - %title%", "paths": [a, outside] }),
        )
        .unwrap();
        assert_eq!(rendered["values"][0]["value"], "Alpha - First");
        assert_eq!(rendered["not_in_library"], json!([outside]));

        assert!(run(&app, "find_duplicates", json!({ "criterion": "audio" })).is_err());
        let groups = run(&app, "find_duplicates", json!({ "criterion": "duration" })).unwrap();
        assert_eq!(groups["groups"][0]["files"].as_array().unwrap().len(), 2);
        assert!(run(&app, "list_fields", json!({})).unwrap()["placeholders"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "artist"));
        drop(app);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn only_fields_the_app_writes_as_given_are_known() {
        for key in ["artist", "title", "track", "custom:MOOD"] {
            assert!(is_known_field(key), "{key}");
        }
        for key in ["titel", "Artist", "custom:", "custom: ", ""] {
            assert!(!is_known_field(key), "{key}");
        }
    }

    // #830: a proposal keeps what would change and reports the rest.
    #[test]
    fn a_proposal_keeps_real_changes_and_reports_the_rest() {
        let base = std::env::temp_dir().join(format!("tagrex-mcp-propose-{}", std::process::id()));
        let root = base.join("library");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("a.wav"),
            tagged_wav(&[(b"IART", "Alpha"), (b"INAM", "First")]),
        )
        .unwrap();
        let app = App::open(&root, &base.join("journal.sqlite")).unwrap();
        let a = root.join("a.wav").to_string_lossy().into_owned();

        let prepared = prepare(
            &app,
            parse(json!({
                "summary": "  Fix the album  ",
                "edits": [
                    { "path": a, "field": "album", "value": "Demo" },
                    { "path": a, "field": "artist", "value": "Alpha" },
                    { "path": a, "field": "title", "value": null },
                    { "path": a, "field": "titel", "value": "x" },
                    { "path": "/elsewhere/b.wav", "field": "album", "value": "x" },
                    { "path": a, "field": "album", "value": "Demo (final)" }
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(prepared.summary, "Fix the album");
        let staged: Vec<(&str, Option<&str>)> = prepared
            .edits
            .iter()
            .map(|e| (e.field.as_str(), e.value.as_deref()))
            .collect();
        assert_eq!(
            staged,
            vec![("album", Some("Demo (final)")), ("title", None)]
        );
        assert_eq!(prepared.report["unchanged"], 1);
        assert_eq!(prepared.report["unknown_fields"], json!(["titel"]));
        assert_eq!(
            prepared.report["not_in_library"],
            json!(["/elsewhere/b.wav"])
        );

        let nothing = parse::<ProposeArgs>(json!({ "summary": " ", "edits": [] })).unwrap();
        assert!(prepare(&app, nothing).is_err());
        drop(app);
        let _ = std::fs::remove_dir_all(&base);
    }

    // #835: a rename proposal is the RENAMER's plan, scoped to the library.
    #[test]
    fn a_rename_proposal_builds_the_renamers_plan() {
        let base = std::env::temp_dir().join(format!("tagrex-mcp-rename-{}", std::process::id()));
        let root = base.join("library");
        std::fs::create_dir_all(root.join("in")).unwrap();
        std::fs::write(
            root.join("in").join("a.wav"),
            tagged_wav(&[(b"IART", "Alpha"), (b"INAM", "First"), (b"IPRD", "Demo")]),
        )
        .unwrap();
        std::fs::write(
            root.join("in").join("b.wav"),
            tagged_wav(&[(b"INAM", "Second")]),
        )
        .unwrap();
        let app = App::open(&root, &base.join("journal.sqlite")).unwrap();
        let a = root.join("in").join("a.wav").to_string_lossy().into_owned();
        let b = root.join("in").join("b.wav").to_string_lossy().into_owned();
        let args = |value: Value| parse::<RenameArgs>(value).unwrap();

        let renamed = prepare_rename(
            &app,
            args(json!({
                "summary": "Name by artist",
                "paths": [a, b, a, "/elsewhere/c.wav"],
                "mask": "%artist% - %title%"
            })),
        )
        .unwrap();
        let targets: Vec<&str> = renamed
            .plan
            .changes
            .iter()
            .filter_map(|c| c.rename_to.as_deref())
            .collect();
        assert_eq!(
            targets,
            vec![root.join("in").join("Alpha - First.wav").to_string_lossy()]
        );
        assert_eq!(renamed.report["mode"], "rename");
        assert_eq!(
            renamed.report["not_in_library"],
            json!(["/elsewhere/c.wav"])
        );
        assert_eq!(
            renamed.report["empty_placeholders"],
            json!([{ "placeholder": "%artist%", "files": 1 }])
        );
        assert_eq!(renamed.report["unchanged"], 0);

        let moved = prepare_rename(
            &app,
            args(json!({
                "summary": "File by album",
                "paths": [a],
                "mask": "%album%/%title%",
                "mode": "reorganize"
            })),
        )
        .unwrap();
        assert!(moved.plan.prune_empty_dirs);
        assert_eq!(
            moved.plan.changes[0].rename_to.as_deref(),
            Some(
                root.join("Demo")
                    .join("First.wav")
                    .to_string_lossy()
                    .as_ref()
            )
        );

        for bad in [
            json!({ "summary": "x", "paths": [a], "mask": "  " }),
            json!({ "summary": "x", "paths": [a], "mask": "%title%", "mode": "copy" }),
            json!({ "summary": " ", "paths": [a], "mask": "%title%" }),
            json!({ "summary": "x", "paths": [], "mask": "%title%" }),
        ] {
            assert!(prepare_rename(&app, args(bad)).is_err());
        }
        assert!(parse::<RenameArgs>(
            json!({ "summary": "x", "paths": [a], "mask": "m", "destination": "/tmp" })
        )
        .is_err());
        drop(app);
        let _ = std::fs::remove_dir_all(&base);
    }
}
