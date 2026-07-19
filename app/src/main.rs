//! TagRex desktop shell (Tauri).
//!
//! Deliberately thin: each command locks the shared [`App`] and forwards one
//! call into it. All logic lives in `tagrex-core` behind the `App` command
//! layer (see `lib.rs` and docs/architecture.md). The frontend is static
//! HTML/JS under `ui/`.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::{Manager, State};

use tagrex::{
    App, BatchDto, CandidateDto, ImportSelectionDto, PlanDto, ReleaseDto, SearchQueryDto,
    TagEditDto, TrackDto,
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
fn preview_tag_edits(state: State<AppState>, edits: Vec<TagEditDto>) -> Result<PlanDto, String> {
    with_app(&state, |app| {
        app.preview_tag_edits(&edits).map_err(|e| e.to_string())
    })
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

#[tauri::command]
fn search_discogs(
    state: State<AppState>,
    token: String,
    query: SearchQueryDto,
) -> Result<Vec<CandidateDto>, String> {
    with_app(&state, |app| {
        app.search_discogs(&token, &query)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn fetch_discogs_release(
    state: State<AppState>,
    token: String,
    release_id: String,
) -> Result<ReleaseDto, String> {
    with_app(&state, |app| {
        app.fetch_discogs_release(&token, &release_id)
            .map_err(|e| e.to_string())
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

fn main() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            open_library,
            list_tracks,
            preview_rename,
            preview_tag_edits,
            apply_plan,
            undo,
            history,
            search_discogs,
            fetch_discogs_release,
            preview_import
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
