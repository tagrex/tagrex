//! A C ABI over the command layer, for the native shell spikes (#271, #293).
//!
//! The bridge holds one [`App`] per open library and dispatches into it by name,
//! rather than reimplementing a slice of the core the way the first cut did. A
//! shell here reaches the same surface the desktop app does — every `preview_*`,
//! the change plan, Apply and Undo, the exporters, duplicates, field locks — and
//! the write path is the core's own gated, journaled `ChangePlan`, so a write is
//! undoable here exactly as it is there.
//!
//! JSON in and JSON out on purpose: several shells in several languages have to
//! agree about the shape, and a listing of a few thousand rows is not where this
//! app spends its time. Each dispatch arm is the same thin forward the desktop
//! handler is, over the same DTOs from `tagrex-commands` — no logic is restated.
//!
//! Threading: a `Session` is not synchronised. A shell drives one from a single
//! thread, or serialises its own access; calling into the same handle from two
//! threads at once is undefined, the same contract the desktop side keeps by
//! holding the `App` behind a mutex.

use std::ffi::{c_char, CStr, CString};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Value};

use tagrex_commands::{
    ActionGroupDto, App, CoverArtDto, ErrorDto, ImportSelectionDto, ImportTrackDto, PlanDto,
    ProviderHub, SearchQueryDto, SettingsDto, TagEditDto, TransformRuleDto,
};
use tagrex_player::Player;

/// One open library: the `App` a shell drives across calls, the `ProviderHub`
/// the online-source commands go through, and the `Player` for preview
/// playback. The hub is synchronous (the provider crates use blocking HTTP) and
/// the player opens its audio device lazily on the first play, so neither needs
/// a runtime and opening a session touches no device.
pub struct Session {
    app: App,
    providers: ProviderHub,
    player: Player,
    /// Where the journal, settings and the token live — one dir, as on the
    /// desktop. Kept so the settings commands can find their files.
    config_dir: PathBuf,
}

// ------------------------------------------------------------- string plumbing

/// Free a string handed out by this library.
///
/// # Safety
///
/// `ptr` must be a pointer returned by one of this library's functions, and must
/// not be used afterwards. Null is accepted and ignored.
#[no_mangle]
pub unsafe extern "C" fn tagrex_string_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}

fn into_c_string(text: String) -> *mut c_char {
    CString::new(text)
        .unwrap_or_else(|_| CString::new(r#"{"error":"interior NUL"}"#).expect("static json"))
        .into_raw()
}

/// Serialise an `ok`/`error` envelope to a heap C string the caller frees.
fn reply(result: Result<Value, ErrorDto>) -> *mut c_char {
    let value = match result {
        Ok(value) => json!({ "ok": value }),
        Err(error) => json!({ "error": error }),
    };
    into_c_string(value.to_string())
}

unsafe fn cstr<'a>(raw: *const c_char, what: &str) -> Result<&'a str, ErrorDto> {
    if raw.is_null() {
        return Err(ErrorDto::plain(format!("null {what}")));
    }
    CStr::from_ptr(raw)
        .to_str()
        .map_err(|_| ErrorDto::plain(format!("{what} is not UTF-8")))
}

fn journal_path(config_dir: &str) -> Result<PathBuf, ErrorDto> {
    let dir = PathBuf::from(config_dir);
    std::fs::create_dir_all(&dir).map_err(|err| ErrorDto::plain(format!("config dir: {err}")))?;
    Ok(dir.join("journal.sqlite"))
}

// ----------------------------------------------------------- session lifecycle

/// Open `root` as a library, storing the handle through `out`.
///
/// The journal lives at `config_dir/journal.sqlite`, the way the desktop shell
/// derives it under the app config dir. Answers with `{"ok":null}` and a live
/// handle, or `{"error":…}` and a null handle.
///
/// # Safety
///
/// `root` and `config_dir` must be valid, NUL-terminated C strings; `out` must
/// be a valid pointer to a `*mut Session`.
#[no_mangle]
pub unsafe extern "C" fn tagrex_open(
    root: *const c_char,
    config_dir: *const c_char,
    out: *mut *mut Session,
) -> *mut c_char {
    if !out.is_null() {
        *out = std::ptr::null_mut();
    }
    reply((|| {
        let root = cstr(root, "root")?;
        let config_dir = cstr(config_dir, "config dir")?;
        let journal = journal_path(config_dir)?;
        let app = App::open(root, &journal).map_err(ErrorDto::from)?;
        if out.is_null() {
            return Err(ErrorDto::plain("null out pointer"));
        }
        *out = Box::into_raw(Box::new(Session {
            app,
            providers: ProviderHub::default(),
            player: Player::new(),
            config_dir: PathBuf::from(config_dir),
        }));
        Ok(Value::Null)
    })())
}

/// Open a drag-and-drop of files and/or folders (#127), storing the handle
/// through `out`. `paths_json` is a JSON array of path strings. Answers with
/// `{"ok":<DropResultDto>}` and a live handle, or `{"error":…}` and a null one.
///
/// # Safety
///
/// `paths_json` and `config_dir` must be valid, NUL-terminated C strings; `out`
/// must be a valid pointer to a `*mut Session`.
#[no_mangle]
pub unsafe extern "C" fn tagrex_open_drop(
    paths_json: *const c_char,
    config_dir: *const c_char,
    out: *mut *mut Session,
) -> *mut c_char {
    if !out.is_null() {
        *out = std::ptr::null_mut();
    }
    reply((|| {
        let paths_json = cstr(paths_json, "paths")?;
        let config_dir = cstr(config_dir, "config dir")?;
        let paths: Vec<PathBuf> = serde_json::from_str(paths_json)
            .map_err(|err| ErrorDto::plain(format!("paths: {err}")))?;
        let journal = journal_path(config_dir)?;
        let (app, dto) = App::open_drop(paths, &journal).map_err(ErrorDto::from)?;
        if out.is_null() {
            return Err(ErrorDto::plain("null out pointer"));
        }
        *out = Box::into_raw(Box::new(Session {
            app,
            providers: ProviderHub::default(),
            player: Player::new(),
            config_dir: PathBuf::from(config_dir),
        }));
        to_value(dto)
    })())
}

/// Close a session opened by `tagrex_open` / `tagrex_open_drop`.
///
/// # Safety
///
/// `handle` must be a handle from this library that has not already been closed,
/// or null. It must not be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn tagrex_close(handle: *mut Session) {
    if !handle.is_null() {
        drop(Box::from_raw(handle));
    }
}

// -------------------------------------------------------------------- dispatch

/// Run a command against an open session.
///
/// `cmd` is the command name and `args_json` a JSON object keyed the way the
/// desktop handler names its parameters. Answers with `{"ok":<result>}` or
/// `{"error":<ErrorDto>}`.
///
/// # Safety
///
/// `handle` must be a live handle from this library; `cmd` and `args_json` must
/// be valid, NUL-terminated C strings.
#[no_mangle]
pub unsafe extern "C" fn tagrex_invoke(
    handle: *mut Session,
    cmd: *const c_char,
    args_json: *const c_char,
) -> *mut c_char {
    reply((|| {
        let Some(session) = handle.as_mut() else {
            return Err(ErrorDto::plain("null session"));
        };
        let cmd = cstr(cmd, "command")?;
        let args = cstr(args_json, "args")?;
        dispatch(session, cmd, args)
    })())
}

fn to_value<T: serde::Serialize>(value: T) -> Result<Value, ErrorDto> {
    serde_json::to_value(value).map_err(|err| ErrorDto::plain(format!("serialize: {err}")))
}

/// Deserialize a command's argument object.
macro_rules! args {
    ($args:expr, $t:ty) => {
        serde_json::from_str::<$t>($args).map_err(|err| ErrorDto::plain(format!("args: {err}")))?
    };
}

/// Commands that need the provider hub, or both it and the library. Everything
/// else falls through to [`dispatch_app`]. Split out because these borrow other
/// fields of the session alongside `app`.
fn dispatch(session: &mut Session, cmd: &str, raw: &str) -> Result<Value, ErrorDto> {
    match cmd {
        "provider_search" => {
            let a = args!(raw, ProviderSearch);
            to_value(
                session
                    .providers
                    .provider_search(&a.source, &a.token, &a.query)
                    .map_err(ErrorDto::from)?,
            )
        }
        "provider_fetch_release" => {
            let a = args!(raw, ProviderFetchRelease);
            to_value(
                session
                    .providers
                    .provider_fetch_release(&a.source, &a.token, &a.release_id)
                    .map_err(ErrorDto::from)?,
            )
        }
        "provider_fetch_image" => {
            let a = args!(raw, ProviderFetchImage);
            to_value(
                session
                    .providers
                    .provider_fetch_image(&a.source, &a.token, &a.url)
                    .map_err(ErrorDto::from)?,
            )
        }
        // Writes the images next to the tracks, inside the opened root — so it
        // needs the library as well as the hub.
        "save_release_images" => {
            let a = args!(raw, SaveReleaseImages);
            to_value(
                session
                    .app
                    .save_release_images(
                        &session.providers,
                        &a.source,
                        &a.token,
                        &a.path,
                        &a.urls,
                        a.overwrite,
                    )
                    .map_err(ErrorDto::from)?,
            )
        }
        // -- settings and the token, under the session's config dir
        "load_settings" => to_value(read_settings(&session.config_dir)),
        "save_settings" => {
            let a = args!(raw, SaveSettings);
            write_settings(&session.config_dir, &a.settings)?;
            // Apply live: the hub always, the library too (it is always open in
            // a session), so a change takes effect without reopening.
            session.providers.apply_settings(&a.settings);
            session.app.apply_settings(&a.settings);
            Ok(Value::Null)
        }
        "saved_discogs_token" => to_value(read_token(&session.config_dir)),
        "save_discogs_token" => {
            let a = args!(raw, SaveDiscogsToken);
            std::fs::write(token_path(&session.config_dir), a.token.trim())
                .map_err(|err| ErrorDto::plain(format!("token: {err}")))?;
            Ok(Value::Null)
        }

        // -- preview player. Each command is fire-and-forget over a channel;
        // the device opens lazily on the first play.
        "player_play" => {
            let a = args!(raw, PlayerPath);
            session.player.play(a.path);
            Ok(Value::Null)
        }
        "player_set_next" => {
            let a = args!(raw, PlayerPath);
            session.player.set_next(a.path);
            Ok(Value::Null)
        }
        "player_pause" => {
            session.player.pause();
            Ok(Value::Null)
        }
        "player_resume" => {
            session.player.resume();
            Ok(Value::Null)
        }
        "player_stop" => {
            session.player.stop();
            Ok(Value::Null)
        }
        "player_seek" => {
            let a = args!(raw, PlayerSeek);
            session.player.seek(a.secs);
            Ok(Value::Null)
        }
        "player_set_volume" => {
            let a = args!(raw, PlayerVolume);
            session.player.set_volume(a.level as f32);
            Ok(Value::Null)
        }
        "player_status" => to_value(session.player.status()),
        "waveform" => {
            let a = args!(raw, Waveform);
            to_value(tagrex_player::waveform(Path::new(&a.path)).map_err(ErrorDto::plain)?)
        }

        _ => dispatch_app(&mut session.app, cmd, raw),
    }
}

fn settings_path(config_dir: &Path) -> PathBuf {
    config_dir.join("settings.json")
}

fn token_path(config_dir: &Path) -> PathBuf {
    config_dir.join("discogs_token")
}

/// The stored settings, or the defaults when the file is absent or unreadable —
/// the same forgiving read the desktop does, so a fresh install starts clean.
fn read_settings(config_dir: &Path) -> SettingsDto {
    std::fs::read_to_string(settings_path(config_dir))
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

fn write_settings(config_dir: &Path, settings: &SettingsDto) -> Result<(), ErrorDto> {
    let json = serde_json::to_string_pretty(settings)
        .map_err(|err| ErrorDto::plain(format!("serialize: {err}")))?;
    std::fs::write(settings_path(config_dir), json)
        .map_err(|err| ErrorDto::plain(format!("settings: {err}")))
}

fn read_token(config_dir: &Path) -> String {
    std::fs::read_to_string(token_path(config_dir))
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// The command table over the library alone. Every arm is the forward the
/// matching desktop handler is, over the same DTOs; the argument structs below
/// mirror each handler's parameters so a shell calls them the same way the
/// frontend does.
fn dispatch_app(app: &mut App, cmd: &str, raw: &str) -> Result<Value, ErrorDto> {
    match cmd {
        // -- reading the open library
        "list_tracks" => to_value(app.list_tracks()),
        "history" => to_value(app.history().map_err(ErrorDto::from)?),
        "locked_fields" => to_value(app.locked_fields()),
        "render_column" => {
            let a = args!(raw, RenderColumn);
            to_value(
                app.render_column(&a.pattern, &a.paths)
                    .map_err(ErrorDto::from)?,
            )
        }
        "find_duplicates" => {
            let a = args!(raw, FindDuplicates);
            to_value(app.find_duplicates(&a.criterion).map_err(ErrorDto::from)?)
        }

        // -- covers
        "read_cover_summary" => {
            let a = args!(raw, Paths);
            to_value(app.read_cover_summary(&a.paths).map_err(ErrorDto::from)?)
        }
        "read_external_cover" => {
            let a = args!(raw, Paths);
            to_value(app.read_external_cover(&a.paths).map_err(ErrorDto::from)?)
        }
        "export_cover" => {
            let a = args!(raw, ExportCover);
            to_value(
                app.export_cover(&a.paths, &a.basename)
                    .map_err(ErrorDto::from)?,
            )
        }

        // -- tag-block reporting
        "tag_block_targets" => {
            let a = args!(raw, Paths);
            to_value(app.tag_block_targets(&a.paths).map_err(ErrorDto::from)?)
        }

        // -- field locks
        "set_locked_fields" => {
            let a = args!(raw, LockedFields);
            app.set_locked_fields(&a.fields);
            Ok(Value::Null)
        }

        // -- trash
        "trash_files" => {
            let a = args!(raw, TrashFiles);
            to_value(app.trash_files(&a.paths).map_err(ErrorDto::from)?)
        }

        // -- previews (staged plans, nothing written)
        "preview_rename" => {
            let a = args!(raw, MaskPaths);
            to_value(
                app.preview_rename(&a.mask, &a.paths)
                    .map_err(ErrorDto::from)?,
            )
        }
        "preview_tags_from_name" => {
            let a = args!(raw, MaskPaths);
            to_value(
                app.preview_tags_from_name(&a.mask, &a.paths)
                    .map_err(ErrorDto::from)?,
            )
        }
        "probe_tags_from_name" => {
            let a = args!(raw, ProbeTagsFromName);
            to_value(
                app.probe_tags_from_name(&a.mask, Path::new(&a.path))
                    .map_err(ErrorDto::from)?,
            )
        }
        "preview_transform" => {
            let a = args!(raw, PreviewTransform);
            to_value(
                app.preview_transform(&a.paths, &a.rules, &a.scope)
                    .map_err(ErrorDto::from)?,
            )
        }
        "preview_transform_groups" => {
            let a = args!(raw, PreviewTransformGroups);
            to_value(
                app.preview_transform_groups(&a.paths, &a.groups)
                    .map_err(ErrorDto::from)?,
            )
        }
        "preview_transform_over_plan" => {
            let a = args!(raw, PreviewTransformOverPlan);
            to_value(
                app.preview_transform_over_plan(&a.plan, &a.groups)
                    .map_err(ErrorDto::from)?,
            )
        }
        "preview_move" => {
            let a = args!(raw, PreviewMove);
            let destination = a
                .destination
                .map(|d| d.trim().to_string())
                .filter(|d| !d.is_empty())
                .map(PathBuf::from);
            to_value(
                app.preview_move(
                    &a.mask,
                    &a.paths,
                    destination.as_deref(),
                    a.copy,
                    a.prune_empty_dirs,
                )
                .map_err(ErrorDto::from)?,
            )
        }
        "preview_tag_edits" => {
            let a = args!(raw, PreviewTagEdits);
            to_value(
                app.preview_tag_edits_with_cover(&a.edits, a.cover.as_ref())
                    .map_err(ErrorDto::from)?,
            )
        }
        "preview_cover_set" => {
            let a = args!(raw, PreviewCoverSet);
            to_value(
                app.preview_cover_set(&a.paths, &a.covers)
                    .map_err(ErrorDto::from)?,
            )
        }
        "preview_cover_embed" => {
            let a = args!(raw, PreviewCoverEmbed);
            to_value(
                app.preview_cover_embed(&a.paths, &a.cover)
                    .map_err(ErrorDto::from)?,
            )
        }
        "preview_cover_remove" => {
            let a = args!(raw, Paths);
            to_value(app.preview_cover_remove(&a.paths).map_err(ErrorDto::from)?)
        }
        "preview_remove_tag_block" => {
            let a = args!(raw, PreviewRemoveTagBlock);
            to_value(
                app.preview_remove_tag_block(&a.paths, &a.kind)
                    .map_err(ErrorDto::from)?,
            )
        }
        "preview_convert_tag_block" => {
            let a = args!(raw, PreviewConvertTagBlock);
            to_value(
                app.preview_convert_tag_block(&a.paths, &a.from, &a.to, a.revision.as_deref())
                    .map_err(ErrorDto::from)?,
            )
        }
        "preview_clear_tags" => {
            let a = args!(raw, Paths);
            to_value(app.preview_clear_tags(&a.paths).map_err(ErrorDto::from)?)
        }
        "preview_import" => {
            let a = args!(raw, PreviewImport);
            to_value(
                app.preview_import(&a.paths, &a.selection, a.vinyl_sides_to_disc)
                    .map_err(ErrorDto::from)?,
            )
        }
        "auto_align" => {
            let a = args!(raw, AutoAlign);
            to_value(
                app.auto_align(&a.paths, &a.tracks)
                    .map_err(ErrorDto::from)?,
            )
        }

        // -- exporters
        "export_playlist" => {
            let a = args!(raw, PathsFileName);
            to_value(
                app.export_playlist(&a.paths, &a.file_name)
                    .map_err(ErrorDto::from)?,
            )
        }
        "export_playlists" => {
            let a = args!(raw, ExportPlaylists);
            to_value(
                app.export_playlists(&a.paths, &a.grouping, &a.name_mask)
                    .map_err(ErrorDto::from)?,
            )
        }
        "export_cue" => {
            let a = args!(raw, PathsFileName);
            to_value(
                app.export_cue(&a.paths, &a.file_name)
                    .map_err(ErrorDto::from)?,
            )
        }
        "export_csv" => {
            let a = args!(raw, PathsFileName);
            to_value(
                app.export_csv(&a.paths, &a.file_name)
                    .map_err(ErrorDto::from)?,
            )
        }
        "export_report" => {
            let a = args!(raw, ExportReport);
            to_value(
                app.export_report(&a.paths, &a.mask, &a.file_name)
                    .map_err(ErrorDto::from)?,
            )
        }
        "export_html" => {
            let a = args!(raw, PathsFileName);
            to_value(
                app.export_html(&a.paths, &a.file_name)
                    .map_err(ErrorDto::from)?,
            )
        }
        "export_xml" => {
            let a = args!(raw, PathsFileName);
            to_value(
                app.export_xml(&a.paths, &a.file_name)
                    .map_err(ErrorDto::from)?,
            )
        }

        // -- the gate: write and take back
        "apply_plan" => {
            let a = args!(raw, ApplyPlan);
            to_value(app.apply(&a.plan).map_err(ErrorDto::from)?)
        }
        "undo" => {
            let a = args!(raw, Undo);
            app.undo(a.batch_id).map_err(ErrorDto::from)?;
            Ok(Value::Null)
        }

        // -- static catalogues, independent of the open library
        "builtin_action_groups" => to_value(tagrex_commands::builtin_action_groups()),
        "mask_placeholders" => to_value(tagrex_commands::mask_placeholders()),
        "import_fields" => to_value(tagrex_commands::import_fields()),
        "read_cover_image" => {
            let a = args!(raw, ReadCoverImage);
            to_value(
                tagrex_commands::read_cover_image(&PathBuf::from(a.path))
                    .map_err(ErrorDto::from)?,
            )
        }

        other => Err(ErrorDto::plain(format!("unknown command: {other}"))),
    }
}

// ------------------------------------------------------------- argument structs
//
// One per handler shape, keyed the way the desktop handler names its parameters.
// `paths` deserialises straight into `PathBuf`, which serde builds from a JSON
// string, so the shell sends the same string arrays the frontend does.

#[derive(Deserialize)]
struct Paths {
    paths: Vec<PathBuf>,
}

#[derive(Deserialize)]
struct MaskPaths {
    mask: String,
    paths: Vec<PathBuf>,
}

#[derive(Deserialize)]
struct PathsFileName {
    paths: Vec<PathBuf>,
    file_name: String,
}

#[derive(Deserialize)]
struct RenderColumn {
    pattern: String,
    paths: Vec<PathBuf>,
}

#[derive(Deserialize)]
struct FindDuplicates {
    criterion: String,
}

#[derive(Deserialize)]
struct LockedFields {
    fields: Vec<String>,
}

#[derive(Deserialize)]
struct TrashFiles {
    paths: Vec<PathBuf>,
}

#[derive(Deserialize)]
struct ExportCover {
    paths: Vec<PathBuf>,
    basename: String,
}

#[derive(Deserialize)]
struct ProbeTagsFromName {
    mask: String,
    path: String,
}

#[derive(Deserialize)]
struct PreviewTransform {
    paths: Vec<PathBuf>,
    rules: Vec<TransformRuleDto>,
    scope: String,
}

#[derive(Deserialize)]
struct PreviewTransformGroups {
    paths: Vec<PathBuf>,
    groups: Vec<ActionGroupDto>,
}

#[derive(Deserialize)]
struct PreviewTransformOverPlan {
    plan: PlanDto,
    groups: Vec<ActionGroupDto>,
}

#[derive(Deserialize)]
struct PreviewMove {
    mask: String,
    paths: Vec<PathBuf>,
    #[serde(default)]
    destination: Option<String>,
    copy: bool,
    prune_empty_dirs: bool,
}

#[derive(Deserialize)]
struct PreviewTagEdits {
    edits: Vec<TagEditDto>,
    #[serde(default)]
    cover: Option<CoverArtDto>,
}

#[derive(Deserialize)]
struct PreviewCoverSet {
    paths: Vec<PathBuf>,
    covers: Vec<CoverArtDto>,
}

#[derive(Deserialize)]
struct PreviewCoverEmbed {
    paths: Vec<PathBuf>,
    cover: CoverArtDto,
}

#[derive(Deserialize)]
struct PreviewRemoveTagBlock {
    paths: Vec<PathBuf>,
    kind: String,
}

#[derive(Deserialize)]
struct PreviewConvertTagBlock {
    paths: Vec<PathBuf>,
    from: String,
    to: String,
    #[serde(default)]
    revision: Option<String>,
}

#[derive(Deserialize)]
struct PreviewImport {
    paths: Vec<PathBuf>,
    selection: ImportSelectionDto,
    vinyl_sides_to_disc: bool,
}

#[derive(Deserialize)]
struct AutoAlign {
    paths: Vec<PathBuf>,
    tracks: Vec<ImportTrackDto>,
}

#[derive(Deserialize)]
struct ExportPlaylists {
    paths: Vec<PathBuf>,
    grouping: String,
    name_mask: String,
}

#[derive(Deserialize)]
struct ExportReport {
    paths: Vec<PathBuf>,
    mask: String,
    file_name: String,
}

#[derive(Deserialize)]
struct ApplyPlan {
    plan: PlanDto,
}

#[derive(Deserialize)]
struct Undo {
    batch_id: i64,
}

#[derive(Deserialize)]
struct ReadCoverImage {
    path: String,
}

#[derive(Deserialize)]
struct ProviderSearch {
    source: String,
    token: String,
    query: SearchQueryDto,
}

#[derive(Deserialize)]
struct ProviderFetchRelease {
    source: String,
    token: String,
    release_id: String,
}

#[derive(Deserialize)]
struct ProviderFetchImage {
    source: String,
    token: String,
    url: String,
}

#[derive(Deserialize)]
struct SaveReleaseImages {
    source: String,
    token: String,
    path: PathBuf,
    urls: Vec<String>,
    overwrite: bool,
}

#[derive(Deserialize)]
struct SaveSettings {
    settings: SettingsDto,
}

#[derive(Deserialize)]
struct SaveDiscogsToken {
    token: String,
}

#[derive(Deserialize)]
struct PlayerPath {
    path: PathBuf,
}

#[derive(Deserialize)]
struct PlayerSeek {
    secs: f64,
}

#[derive(Deserialize)]
struct PlayerVolume {
    level: f64,
}

#[derive(Deserialize)]
struct Waveform {
    path: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    /// Drive the ABI the way a shell does: open a scratch library, invoke,
    /// free every string. Returns the parsed `ok`/`error` envelope.
    unsafe fn open(root: &Path, config: &Path) -> *mut Session {
        let root_c = CString::new(root.to_str().unwrap()).unwrap();
        let cfg_c = CString::new(config.to_str().unwrap()).unwrap();
        let mut handle: *mut Session = std::ptr::null_mut();
        let reply = tagrex_open(root_c.as_ptr(), cfg_c.as_ptr(), &mut handle);
        let text = CStr::from_ptr(reply).to_str().unwrap().to_owned();
        tagrex_string_free(reply);
        assert!(text.contains("\"ok\""), "open failed: {text}");
        assert!(!handle.is_null());
        handle
    }

    unsafe fn invoke(handle: *mut Session, cmd: &str, args: &str) -> Value {
        let cmd_c = CString::new(cmd).unwrap();
        let args_c = CString::new(args).unwrap();
        let reply = tagrex_invoke(handle, cmd_c.as_ptr(), args_c.as_ptr());
        let text = CStr::from_ptr(reply).to_str().unwrap().to_owned();
        tagrex_string_free(reply);
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn open_lists_the_library_and_a_bad_command_answers_with_an_error() {
        let dir = std::env::temp_dir().join(format!("tagrex-ffi-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let config = dir.join("config");

        unsafe {
            let handle = open(&dir, &config);

            let listed = invoke(handle, "list_tracks", "{}");
            assert!(
                listed.get("ok").and_then(Value::as_array).is_some(),
                "{listed}"
            );

            // A static catalogue needs no files on disk.
            let placeholders = invoke(handle, "mask_placeholders", "{}");
            assert!(
                placeholders.get("ok").and_then(Value::as_array).is_some(),
                "{placeholders}"
            );

            let unknown = invoke(handle, "no_such_command", "{}");
            assert!(
                unknown["error"]["text"]
                    .as_str()
                    .unwrap()
                    .contains("unknown command"),
                "{unknown}"
            );

            tagrex_close(handle);
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_null_path_answers_with_json_carrying_the_reason() {
        let cfg = CString::new("/tmp").unwrap();
        let mut handle: *mut Session = std::ptr::null_mut();
        let reply = unsafe { tagrex_open(std::ptr::null(), cfg.as_ptr(), &mut handle) };
        let text = unsafe { CStr::from_ptr(reply) }
            .to_str()
            .unwrap()
            .to_owned();
        unsafe { tagrex_string_free(reply) };
        assert!(text.contains("null root"), "{text}");
        assert!(handle.is_null());
    }
}
