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
    App, BatchDto, CandidateDto, CoverArtDto, CoverExportDto, CoverSummaryDto, ImportSelectionDto,
    ImportTrackDto, PlanDto, ReleaseDto, SearchQueryDto, SettingsDto, TagEditDto, TrackDto,
    TransformRuleDto,
};

/// No library is open until the user opens one, hence `Option`. `Mutex` makes
/// the non-`Sync` journal usable as shared Tauri state.
type AppState = Mutex<Option<App>>;

fn with_app<T>(
    state: &State<AppState>,
    f: impl FnOnce(&App) -> Result<T, String>,
) -> Result<T, String> {
    let guard = state.lock().unwrap();
    let app = guard.as_ref().ok_or("no library open")?;
    f(app)
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

#[tauri::command]
fn list_tracks(state: State<AppState>) -> Result<Vec<TrackDto>, String> {
    with_app(&state, |app| Ok(app.list_tracks()))
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
) -> Result<PlanDto, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    with_app(&state, |app| {
        app.preview_move(&mask, &paths).map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn preview_tag_edits(state: State<AppState>, edits: Vec<TagEditDto>) -> Result<PlanDto, String> {
    with_app(&state, |app| {
        app.preview_tag_edits(&edits).map_err(|e| e.to_string())
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
fn preview_cover_remove(state: State<AppState>, paths: Vec<String>) -> Result<PlanDto, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    with_app(&state, |app| {
        app.preview_cover_remove(&paths).map_err(|e| e.to_string())
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

// The three Discogs commands are `async` so Tauri runs them off the main thread:
// their bodies do blocking HTTP (ureq), and a synchronous command would freeze
// the webview for the whole request — very visible when the picker prefetches a
// release per candidate. No `.await` inside, so no MutexGuard crosses one.
#[tauri::command]
async fn search_discogs(
    state: State<'_, AppState>,
    token: String,
    query: SearchQueryDto,
) -> Result<Vec<CandidateDto>, String> {
    with_app(&state, |app| {
        app.search_discogs(&token, &query)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
async fn fetch_discogs_release(
    state: State<'_, AppState>,
    token: String,
    release_id: String,
) -> Result<ReleaseDto, String> {
    with_app(&state, |app| {
        app.fetch_discogs_release(&token, &release_id)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
async fn fetch_discogs_image(
    state: State<'_, AppState>,
    token: String,
    url: String,
) -> Result<CoverArtDto, String> {
    with_app(&state, |app| {
        app.fetch_discogs_image(&token, &url)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn auto_align(
    state: State<AppState>,
    paths: Vec<String>,
    tracks: Vec<ImportTrackDto>,
) -> Result<Vec<Option<usize>>, String> {
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
) -> Result<PlanDto, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    with_app(&state, |app| {
        app.preview_import(&paths, &selection)
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

#[tauri::command]
fn save_settings(
    app: tauri::AppHandle,
    state: State<AppState>,
    settings: SettingsDto,
) -> Result<(), String> {
    let path = settings_path(&app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())?;
    // Apply immediately if a library is open, so the change takes effect without
    // reopening.
    if let Some(app) = state.lock().unwrap().as_ref() {
        app.apply_settings(&settings);
    }
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .manage(Player::new())
        .invoke_handler(tauri::generate_handler![
            open_library,
            list_tracks,
            preview_rename,
            preview_move,
            preview_transform,
            preview_tag_edits,
            preview_cover_embed,
            export_cover,
            read_cover_summary,
            preview_cover_remove,
            export_playlist,
            export_csv,
            export_report,
            apply_plan,
            undo,
            history,
            search_discogs,
            fetch_discogs_release,
            fetch_discogs_image,
            preview_import,
            auto_align,
            saved_discogs_token,
            save_discogs_token,
            load_settings,
            save_settings,
            player_play,
            player_set_next,
            player_pause,
            player_resume,
            player_stop,
            player_seek,
            player_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
