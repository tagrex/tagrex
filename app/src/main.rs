//! TagRex desktop shell (Tauri).
//!
//! Deliberately thin: each command locks the shared [`App`] and forwards one
//! call into it. All logic lives in `tagrex-core` behind the `App` command
//! layer (see `lib.rs` and docs/architecture.md). The frontend is static
//! HTML/JS under `ui/`.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod player;

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::{Manager, State};

use player::{Player, PlayerStatus};
use tagrex::{
    ActionGroupDto, AlignMatchDto, App, BatchDto, BlockTargetsDto, CandidateDto, CoverArtDto,
    CoverExportDto, CoverSummaryDto, DropResultDto, DuplicateGroupDto, ImportFieldDto,
    ImportSelectionDto, ImportTrackDto, NameProbeDto, PlaceholderDto, PlanDto, ProviderHub,
    ReleaseDto, SaveImagesDto, SearchQueryDto, SettingsDto, TagEditDto, TrackDto, TransformRuleDto,
};

/// No library is open until the user opens one, hence `Option`. `Mutex` makes
/// the non-`Sync` journal usable as shared Tauri state.
type AppState = Mutex<Option<App>>;

/// The provider side (#166): one per process, alive whether or not a library is
/// open. `Mutex` for the same reason as `AppState` — its interior mutability is
/// not `Sync` — and it doubles as what serializes provider requests, which is
/// what makes the throttle a single cadence per source.
type ProviderState = Mutex<ProviderHub>;

fn with_app<T>(
    state: &State<AppState>,
    f: impl FnOnce(&App) -> Result<T, String>,
) -> Result<T, String> {
    let guard = state.lock().unwrap();
    let app = guard.as_ref().ok_or("no library open")?;
    f(app)
}

fn with_providers<T>(
    state: &State<ProviderState>,
    f: impl FnOnce(&ProviderHub) -> Result<T, String>,
) -> Result<T, String> {
    f(&state.lock().unwrap())
}

fn with_app_mut<T>(
    state: &State<AppState>,
    f: impl FnOnce(&mut App) -> Result<T, String>,
) -> Result<T, String> {
    let mut guard = state.lock().unwrap();
    let app = guard.as_mut().ok_or("no library open")?;
    f(app)
}

#[tauri::command]
fn open_library(state: State<AppState>, app: tauri::AppHandle, root: String) -> Result<(), String> {
    let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&config_dir).map_err(|e| e.to_string())?;
    let journal_path = config_dir.join("journal.sqlite");
    let opened = App::open(root, &journal_path).map_err(|e| e.to_string())?;
    // Apply saved settings (proxy / rate-limit / ID3 version) to the new session.
    opened.apply_settings(&read_settings(&app));
    *state.lock().unwrap() = Some(opened);
    Ok(())
}

/// Open a drag-and-drop of `paths` (#127): a lone folder opens as a library,
/// anything else (files, several folders, a mix) as a file-set. Returns the
/// resolved mode + grouping info for the frontend.
#[tauri::command]
fn open_drop(
    state: State<AppState>,
    app: tauri::AppHandle,
    paths: Vec<String>,
) -> Result<DropResultDto, String> {
    let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&config_dir).map_err(|e| e.to_string())?;
    let journal_path = config_dir.join("journal.sqlite");
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let (opened, result) = App::open_drop(paths, &journal_path).map_err(|e| e.to_string())?;
    opened.apply_settings(&read_settings(&app));
    *state.lock().unwrap() = Some(opened);
    Ok(result)
}

#[tauri::command]
fn list_tracks(state: State<AppState>) -> Result<Vec<TrackDto>, String> {
    with_app(&state, |app| Ok(app.list_tracks()))
}

#[tauri::command]
fn find_duplicates(
    state: State<AppState>,
    criterion: String,
) -> Result<Vec<DuplicateGroupDto>, String> {
    with_app(&state, |app| {
        app.find_duplicates(&criterion).map_err(|e| e.to_string())
    })
}

/// Build a provider *release* page URL from a hard-coded host plus a
/// charset-validated id (#92). Kept separate from the command so the frontend can
/// only ever reach a Discogs/MusicBrainz release page — never an arbitrary URL —
/// and so the construction is unit-testable without touching the system browser.
fn release_url(source: &str, id: &str) -> Result<String, String> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(format!("invalid release id: {id:?}"));
    }
    match source {
        "discogs" => Ok(format!("https://www.discogs.com/release/{id}")),
        "musicbrainz" => Ok(format!("https://musicbrainz.org/release/{id}")),
        // The slug is cosmetic; the store resolves a release by its id alone
        // (#162), so nothing user-supplied has to go into the path.
        "beatport" => Ok(format!("https://www.beatport.com/release/-/{id}")),
        other => Err(format!("unknown source: {other}")),
    }
}

/// Open a provider release page in the system browser (#92).
#[tauri::command]
fn open_release_page(source: String, id: String) -> Result<(), String> {
    let url = release_url(&source, &id)?;
    open::that(url).map_err(|e| e.to_string())
}

#[tauri::command]
fn preview_rename(
    state: State<AppState>,
    mask: String,
    paths: Vec<String>,
) -> Result<PlanDto, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    with_app(&state, |app| {
        app.preview_rename(&mask, &paths).map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn preview_tags_from_name(
    state: State<AppState>,
    mask: String,
    paths: Vec<String>,
) -> Result<PlanDto, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    with_app(&state, |app| {
        app.preview_tags_from_name(&mask, &paths)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn probe_tags_from_name(
    state: State<AppState>,
    mask: String,
    path: String,
) -> Result<NameProbeDto, String> {
    with_app(&state, |app| {
        app.probe_tags_from_name(&mask, &PathBuf::from(path))
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn preview_transform(
    state: State<AppState>,
    paths: Vec<String>,
    rules: Vec<TransformRuleDto>,
    scope: String,
) -> Result<PlanDto, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    with_app(&state, |app| {
        app.preview_transform(&paths, &rules, &scope)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn preview_move(
    state: State<AppState>,
    mask: String,
    paths: Vec<String>,
    destination: Option<String>,
    copy: bool,
    prune_empty_dirs: bool,
) -> Result<PlanDto, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let destination = destination
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty())
        .map(PathBuf::from);
    with_app(&state, |app| {
        app.preview_move(
            &mask,
            &paths,
            destination.as_deref(),
            copy,
            prune_empty_dirs,
        )
        .map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn preview_tag_edits(
    state: State<AppState>,
    edits: Vec<TagEditDto>,
    // The release cover an import brought with it (#207), when there is one.
    cover: Option<CoverArtDto>,
) -> Result<PlanDto, String> {
    with_app(&state, |app| {
        app.preview_tag_edits_with_cover(&edits, cover.as_ref())
            .map_err(|e| e.to_string())
    })
}

/// Preview replacing every selected file's whole image set (#56) — the one
/// command behind add, remove, reorder and set-the-type.
#[tauri::command]
fn preview_cover_set(
    state: State<AppState>,
    paths: Vec<String>,
    covers: Vec<CoverArtDto>,
) -> Result<PlanDto, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    with_app(&state, |app| {
        app.preview_cover_set(&paths, &covers)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn preview_cover_embed(
    state: State<AppState>,
    paths: Vec<String>,
    cover: CoverArtDto,
) -> Result<PlanDto, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    with_app(&state, |app| {
        app.preview_cover_embed(&paths, &cover)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn export_cover(
    state: State<AppState>,
    paths: Vec<String>,
    basename: String,
) -> Result<CoverExportDto, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    with_app(&state, |app| {
        app.export_cover(&paths, &basename)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn read_cover_summary(
    state: State<AppState>,
    paths: Vec<String>,
) -> Result<CoverSummaryDto, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    with_app(&state, |app| {
        app.read_cover_summary(&paths).map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn read_external_cover(
    state: State<AppState>,
    paths: Vec<String>,
) -> Result<Option<CoverArtDto>, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    with_app(&state, |app| {
        app.read_external_cover(&paths).map_err(|e| e.to_string())
    })
}

/// Read an image file (dropped onto the cover well, #133) into a cover DTO for
/// [`preview_cover_embed`]. Stateless — the source image isn't confined to the
/// library, matching the file picker (the user chose it explicitly).
#[tauri::command]
fn read_cover_image(path: String) -> Result<CoverArtDto, String> {
    tagrex::read_cover_image(&PathBuf::from(path)).map_err(|e| e.to_string())
}

#[tauri::command]
fn preview_cover_remove(state: State<AppState>, paths: Vec<String>) -> Result<PlanDto, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    with_app(&state, |app| {
        app.preview_cover_remove(&paths).map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn preview_remove_tag_block(
    state: State<AppState>,
    paths: Vec<String>,
    kind: String,
) -> Result<PlanDto, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    with_app(&state, |app| {
        app.preview_remove_tag_block(&paths, &kind)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn tag_block_targets(
    state: State<AppState>,
    paths: Vec<String>,
) -> Result<BlockTargetsDto, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    with_app(&state, |app| {
        app.tag_block_targets(&paths).map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn preview_convert_tag_block(
    state: State<AppState>,
    paths: Vec<String>,
    from: String,
    to: String,
    revision: Option<String>,
) -> Result<PlanDto, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    with_app(&state, |app| {
        app.preview_convert_tag_block(&paths, &from, &to, revision.as_deref())
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn preview_clear_tags(state: State<AppState>, paths: Vec<String>) -> Result<PlanDto, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    with_app(&state, |app| {
        app.preview_clear_tags(&paths).map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn export_playlist(
    state: State<AppState>,
    paths: Vec<String>,
    file_name: String,
) -> Result<String, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    with_app(&state, |app| {
        app.export_playlist(&paths, &file_name)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn export_csv(
    state: State<AppState>,
    paths: Vec<String>,
    file_name: String,
) -> Result<String, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    with_app(&state, |app| {
        app.export_csv(&paths, &file_name)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn export_report(
    state: State<AppState>,
    paths: Vec<String>,
    mask: String,
    file_name: String,
) -> Result<String, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    with_app(&state, |app| {
        app.export_report(&paths, &mask, &file_name)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn export_html(
    state: State<AppState>,
    paths: Vec<String>,
    file_name: String,
) -> Result<String, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    with_app(&state, |app| {
        app.export_html(&paths, &file_name)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn export_xml(
    state: State<AppState>,
    paths: Vec<String>,
    file_name: String,
) -> Result<String, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    with_app(&state, |app| {
        app.export_xml(&paths, &file_name)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn player_play(player: State<Player>, path: String) {
    player.play(PathBuf::from(path));
}

#[tauri::command]
fn player_set_next(player: State<Player>, path: String) {
    player.set_next(PathBuf::from(path));
}

#[tauri::command]
fn player_pause(player: State<Player>) {
    player.pause();
}

#[tauri::command]
fn player_resume(player: State<Player>) {
    player.resume();
}

#[tauri::command]
fn player_stop(player: State<Player>) {
    player.stop();
}

#[tauri::command]
fn player_seek(player: State<Player>, secs: f64) {
    player.seek(secs);
}

#[tauri::command]
fn player_set_volume(player: State<Player>, level: f64) {
    player.set_volume(level as f32);
}

#[tauri::command]
fn player_status(player: State<Player>) -> PlayerStatus {
    player.status()
}

#[tauri::command]
fn apply_plan(state: State<AppState>, plan: PlanDto) -> Result<BatchDto, String> {
    with_app_mut(&state, |app| app.apply(&plan).map_err(|e| e.to_string()))
}

#[tauri::command]
fn undo(state: State<AppState>, batch_id: i64) -> Result<(), String> {
    with_app_mut(&state, |app| app.undo(batch_id).map_err(|e| e.to_string()))
}

#[tauri::command]
fn history(state: State<AppState>) -> Result<Vec<BatchDto>, String> {
    with_app(&state, |app| app.history().map_err(|e| e.to_string()))
}

// The three provider commands are `async` so Tauri runs them off the main
// thread: their bodies do blocking HTTP (ureq), and a synchronous command would
// freeze the webview for the whole request — very visible when the picker
// prefetches a release per candidate. No `.await` inside, so no MutexGuard
// crosses one. `source` selects the provider ("discogs" | "musicbrainz" |
// "beatport"); the token is ignored by token-less providers.
//
// They take the provider state, NOT the library (#166): looking a release up is
// something the user does before choosing files, so requiring an open folder
// only produced a "no library open" refusal for a search that needs no folder
// at all.
#[tauri::command]
async fn provider_search(
    providers: State<'_, ProviderState>,
    source: String,
    token: String,
    query: SearchQueryDto,
) -> Result<Vec<CandidateDto>, String> {
    with_providers(&providers, |hub| {
        hub.provider_search(&source, &token, &query)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
async fn provider_fetch_release(
    providers: State<'_, ProviderState>,
    source: String,
    token: String,
    release_id: String,
) -> Result<ReleaseDto, String> {
    with_providers(&providers, |hub| {
        hub.provider_fetch_release(&source, &token, &release_id)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
async fn provider_fetch_image(
    providers: State<'_, ProviderState>,
    source: String,
    token: String,
    url: String,
) -> Result<CoverArtDto, String> {
    with_providers(&providers, |hub| {
        hub.provider_fetch_image(&source, &token, &url)
            .map_err(|e| e.to_string())
    })
}

/// Save a release's images to disk next to the selected tracks (#102). Async:
/// it downloads each image with the provider's auth/User-Agent headers.
#[tauri::command]
async fn save_release_images(
    state: State<'_, AppState>,
    providers: State<'_, ProviderState>,
    source: String,
    token: String,
    path: String,
    urls: Vec<String>,
    overwrite: bool,
) -> Result<SaveImagesDto, String> {
    let path = PathBuf::from(path);
    // This one does need the library: it writes the images next to the tracks,
    // inside the opened root.
    let hub = providers.lock().unwrap();
    with_app(&state, |app| {
        app.save_release_images(&hub, &source, &token, &path, &urls, overwrite)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn auto_align(
    state: State<AppState>,
    paths: Vec<String>,
    tracks: Vec<ImportTrackDto>,
) -> Result<Vec<Option<AlignMatchDto>>, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    with_app(&state, |app| {
        app.auto_align(&paths, &tracks).map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn preview_import(
    state: State<AppState>,
    paths: Vec<String>,
    selection: ImportSelectionDto,
    vinyl_sides_to_disc: bool,
) -> Result<PlanDto, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    with_app(&state, |app| {
        app.preview_import(&paths, &selection, vinyl_sides_to_disc)
            .map_err(|e| e.to_string())
    })
}

/// Path to the locally saved Discogs token (in the OS app-config dir, never in
/// the repo). Convenience only, so the token isn't retyped each session.
fn token_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    Ok(dir.join("discogs_token"))
}

#[tauri::command]
fn saved_discogs_token(app: tauri::AppHandle) -> Result<String, String> {
    let path = token_path(&app)?;
    Ok(std::fs::read_to_string(path)
        .unwrap_or_default()
        .trim()
        .to_string())
}

#[tauri::command]
fn save_discogs_token(app: tauri::AppHandle, token: String) -> Result<(), String> {
    let path = token_path(&app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, token.trim()).map_err(|e| e.to_string())
}

// ---- Beatport sign-in (#162) ----------------------------------------------
//
// The store has no self-serve developer tier, so the only way in is OAuth
// against the user's own account, through the public client its documentation
// page uses. TagRex never sees the password: the sign-in happens in a window
// pointed at Beatport's own login page, and all that comes back is the
// authorization code in the redirect URL.
//
// What is stored is what a session needs and nothing more: the tokens, when the
// access one dies, the account name (so settings can say *who* is signed in),
// and the client id (so a refresh doesn't have to re-read it). It lives in the
// OS app-config dir next to the Discogs token — never in the repository.

/// The saved sign-in, as it sits on disk.
#[derive(serde::Serialize, serde::Deserialize)]
struct BeatportSession {
    #[serde(flatten)]
    token: tagrex_providers_beatport::auth::BeatportToken,
    username: String,
    client_id: String,
}

/// What the UI shows in the source row and in settings.
#[derive(serde::Serialize)]
struct BeatportStatusDto {
    authorized: bool,
    username: String,
}

fn beatport_session_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    Ok(dir.join("beatport_session.json"))
}

fn read_beatport_session(app: &tauri::AppHandle) -> Option<BeatportSession> {
    let path = beatport_session_path(app).ok()?;
    let json = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&json).ok()
}

fn write_beatport_session(app: &tauri::AppHandle, session: &BeatportSession) -> Result<(), String> {
    let path = beatport_session_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string(session).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

/// The proxy every provider request goes through, from settings.
fn beatport_proxy(app: &tauri::AppHandle) -> Option<String> {
    let proxy = read_settings(app).proxy.trim().to_string();
    (!proxy.is_empty()).then_some(proxy)
}

#[tauri::command]
fn beatport_status(app: tauri::AppHandle) -> BeatportStatusDto {
    match read_beatport_session(&app) {
        Some(session) => BeatportStatusDto {
            authorized: true,
            username: session.username,
        },
        None => BeatportStatusDto {
            authorized: false,
            username: String::new(),
        },
    }
}

#[tauri::command]
fn beatport_logout(app: tauri::AppHandle) -> Result<(), String> {
    let path = beatport_session_path(&app)?;
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        // Signing out of a session that isn't there is a no-op, not a failure.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.to_string()),
    }
}

/// Sign in: read the public client id, open Beatport's login page in its own
/// window, wait for the redirect that carries the authorization code, trade it
/// for tokens. Returns the account name.
///
/// `async` for the same reason the provider commands are (#95): every step here
/// blocks on the network or on the user, and a synchronous command would freeze
/// the whole webview while it did.
#[tauri::command]
async fn beatport_login(app: tauri::AppHandle) -> Result<String, String> {
    use tagrex_providers_beatport::auth;

    let proxy = beatport_proxy(&app);
    let agent = auth::agent(proxy.as_deref()).map_err(|e| e.to_string())?;

    let scraper = agent.clone();
    let client_id = tauri::async_runtime::spawn_blocking(move || auth::fetch_client_id(&scraper))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

    // The window reports back either a code or, if the user gives up and closes
    // it, nothing — hence a channel rather than a return value.
    let (tx, rx) = std::sync::mpsc::channel::<Option<String>>();
    let on_close = tx.clone();
    let url = auth::authorize_url(&client_id)
        .parse()
        .map_err(|_| "could not build the Beatport sign-in URL".to_string())?;
    let window = tauri::WebviewWindowBuilder::new(
        &app,
        BEATPORT_LOGIN_WINDOW,
        tauri::WebviewUrl::External(url),
    )
    .title("Sign in to Beatport")
    .inner_size(520.0, 760.0)
    .on_navigation(move |url| {
        if let Some(code) = auth::code_from_redirect(url.as_str()) {
            let _ = tx.send(Some(code));
        }
        true
    })
    .build()
    .map_err(|e| e.to_string())?;
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
            let _ = on_close.send(None);
        }
    });

    let received = tauri::async_runtime::spawn_blocking(move || {
        rx.recv_timeout(std::time::Duration::from_secs(BEATPORT_LOGIN_TIMEOUT_SECS))
    })
    .await
    .map_err(|e| e.to_string())?;
    // The window has done its job either way; leaving it open would strand a
    // second window with no owner.
    let _ = window.close();

    let code = match received {
        Ok(Some(code)) => code,
        Ok(None) => return Err("Sign-in cancelled".to_string()),
        Err(_) => return Err("Sign-in timed out".to_string()),
    };

    let exchange_id = client_id.clone();
    let token = tauri::async_runtime::spawn_blocking(move || {
        auth::exchange_code(&agent, &exchange_id, &code)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    let username = beatport_account_name(&proxy, &token.access_token).await;
    write_beatport_session(
        &app,
        &BeatportSession {
            token,
            username: username.clone(),
            client_id,
        },
    )?;
    Ok(username)
}

/// A valid access token for the provider commands, renewed first if the stored
/// one has expired. The frontend passes what this returns straight into
/// `provider_search` and friends, exactly as it passes the Discogs token.
#[tauri::command]
async fn beatport_token(app: tauri::AppHandle) -> Result<String, String> {
    use tagrex_providers_beatport::auth;

    let mut session = read_beatport_session(&app).ok_or("Not signed in to Beatport")?;
    if !session.token.is_expired() {
        return Ok(session.token.access_token);
    }

    let proxy = beatport_proxy(&app);
    let agent = auth::agent(proxy.as_deref()).map_err(|e| e.to_string())?;
    let client_id = session.client_id.clone();
    let refresh_token = session.token.refresh_token.clone();
    let token = tauri::async_runtime::spawn_blocking(move || {
        auth::refresh(&agent, &client_id, &refresh_token)
    })
    .await
    .map_err(|e| e.to_string())?
    // A refresh token can be revoked from the account page, and then the
    // only way back is a fresh sign-in — say so rather than reporting a
    // bare HTTP status.
    .map_err(|err| format!("{err} — sign in to Beatport again"))?;

    let access_token = token.access_token.clone();
    session.token = token;
    write_beatport_session(&app, &session)?;
    Ok(access_token)
}

/// The account name behind a token. A failure here is not worth failing the
/// whole sign-in over — the token is what matters, the name is a label — so it
/// degrades to an empty string.
async fn beatport_account_name(proxy: &Option<String>, access_token: &str) -> String {
    use tagrex_providers_beatport::auth;

    let Ok(agent) = auth::agent(proxy.as_deref()) else {
        return String::new();
    };
    let token = access_token.to_string();
    tauri::async_runtime::spawn_blocking(move || auth::account_username(&agent, &token))
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default()
}

/// Label of the sign-in window, so a second Sign in click can't open a duplicate.
const BEATPORT_LOGIN_WINDOW: &str = "beatport-login";
/// How long the sign-in window waits for the user before giving up.
const BEATPORT_LOGIN_TIMEOUT_SECS: u64 = 300;

/// Path to the persisted settings JSON (#79), in the OS app-config dir.
fn settings_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    Ok(dir.join("settings.json"))
}

/// Read saved settings, falling back to defaults if the file is missing or
/// unreadable (so a corrupt file never blocks startup).
fn read_settings(app: &tauri::AppHandle) -> SettingsDto {
    settings_path(app)
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

#[tauri::command]
fn load_settings(app: tauri::AppHandle) -> Result<SettingsDto, String> {
    Ok(read_settings(&app))
}

/// The shipped preset library (#137). Read-only and not part of settings: the
/// frontend lists these below the user's own saved groups.
#[tauri::command]
fn builtin_action_groups() -> Vec<ActionGroupDto> {
    tagrex::builtin_action_groups()
}

/// Every placeholder a mask accepts (#148), for the in-app reference. Needs no
/// open library — the grammar is the same whether or not one is.
#[tauri::command]
fn mask_placeholders() -> Vec<PlaceholderDto> {
    tagrex::mask_placeholders()
}

/// Every field an online import can write (#152), for the setting that picks
/// which of them it may. Read-only, like the placeholder reference.
#[tauri::command]
fn import_fields() -> Vec<ImportFieldDto> {
    tagrex::import_fields()
}

/// Render a mask-defined column over a batch of files (#150).
#[tauri::command]
fn render_column(
    state: State<AppState>,
    pattern: String,
    paths: Vec<String>,
) -> Result<Vec<String>, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    with_app(&state, |app| {
        app.render_column(&pattern, &paths)
            .map_err(|e| e.to_string())
    })
}

/// Preview the ticked action groups run in order as one plan (#137).
#[tauri::command]
fn preview_transform_groups(
    state: State<AppState>,
    paths: Vec<String>,
    groups: Vec<ActionGroupDto>,
) -> Result<PlanDto, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    with_app(&state, |app| {
        app.preview_transform_groups(&paths, &groups)
            .map_err(|e| e.to_string())
    })
}

/// Run the ticked action groups over a staged plan rather than over the files
/// (#142) — cleanup composed into the operation that produced the values.
#[tauri::command]
fn preview_transform_over_plan(
    state: State<AppState>,
    plan: PlanDto,
    groups: Vec<ActionGroupDto>,
) -> Result<PlanDto, String> {
    with_app(&state, |app| {
        app.preview_transform_over_plan(&plan, &groups)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn save_settings(
    app: tauri::AppHandle,
    state: State<AppState>,
    providers: State<ProviderState>,
    settings: SettingsDto,
) -> Result<(), String> {
    let path = settings_path(&app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())?;
    // Apply immediately: the network half always (the hub is always there), the
    // library half only when a library is open — so a change takes effect
    // without reopening either way.
    providers.lock().unwrap().apply_settings(&settings);
    if let Some(app) = state.lock().unwrap().as_ref() {
        app.apply_settings(&settings);
    }
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(AppState::default())
        .manage(ProviderState::default())
        .setup(|app| {
            // The provider hub exists from startup, before any library is
            // opened (#166), so it takes the saved network settings here rather
            // than waiting for a library or a trip through Settings › Save.
            let settings = read_settings(&app.handle().clone());
            app.state::<ProviderState>()
                .lock()
                .unwrap()
                .apply_settings(&settings);
            Ok(())
        })
        .manage(Player::new())
        .invoke_handler(tauri::generate_handler![
            open_library,
            open_drop,
            list_tracks,
            find_duplicates,
            open_release_page,
            preview_rename,
            preview_tags_from_name,
            probe_tags_from_name,
            preview_move,
            preview_transform,
            preview_tag_edits,
            preview_cover_embed,
            preview_cover_set,
            export_cover,
            read_cover_summary,
            read_external_cover,
            read_cover_image,
            preview_cover_remove,
            preview_remove_tag_block,
            preview_convert_tag_block,
            tag_block_targets,
            preview_clear_tags,
            export_playlist,
            export_csv,
            export_report,
            export_html,
            export_xml,
            apply_plan,
            undo,
            history,
            provider_search,
            provider_fetch_release,
            provider_fetch_image,
            save_release_images,
            preview_import,
            auto_align,
            saved_discogs_token,
            save_discogs_token,
            beatport_status,
            beatport_login,
            beatport_logout,
            beatport_token,
            load_settings,
            save_settings,
            player_play,
            player_set_next,
            player_pause,
            player_resume,
            player_stop,
            player_seek,
            player_set_volume,
            builtin_action_groups,
            mask_placeholders,
            import_fields,
            render_column,
            preview_transform_groups,
            preview_transform_over_plan,
            player_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::release_url;

    #[test]
    fn builds_provider_release_urls() {
        assert_eq!(
            release_url("discogs", "5606").unwrap(),
            "https://www.discogs.com/release/5606"
        );
        assert_eq!(
            release_url("musicbrainz", "1a2b3c4d-0000-0000-0000-000000000000").unwrap(),
            "https://musicbrainz.org/release/1a2b3c4d-0000-0000-0000-000000000000"
        );
        // The store's own URLs carry a slug, but it is cosmetic (#162) — a
        // placeholder keeps anything user-supplied out of the path.
        assert_eq!(
            release_url("beatport", "4321").unwrap(),
            "https://www.beatport.com/release/-/4321"
        );
    }

    #[test]
    fn rejects_unknown_source_and_unsafe_ids() {
        assert!(release_url("bandcamp", "5606").is_err());
        assert!(release_url("beatport", "").is_err());
        assert!(release_url("beatport", "4321/evil").is_err());
        assert!(release_url("discogs", "").is_err());
        // A path-traversal / injection attempt in the id must never build a URL.
        assert!(release_url("discogs", "5606/../../evil").is_err());
        assert!(release_url("discogs", "5606?q=1").is_err());
    }
}
