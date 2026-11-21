//! TagRex desktop shell (Tauri).
//!
//! Deliberately thin: each command locks the shared [`App`] and forwards one
//! call into it. All logic lives in `tagrex-core` behind the `App` command
//! layer (see `lib.rs` and docs/architecture.md). The frontend is static
//! HTML/JS under `ui/`.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

mod blocking;
mod mcp;
mod media_session;
mod update;
mod window_controls;

use tauri::{Manager, State};

use tagrex_commands::{
    ActionGroupDto, AlignMatchDto, App, AppError, BatchDto, BlockTargetsDto, CandidateDto,
    CoverArtDto, CoverExportDto, CoverSummaryDto, DropResultDto, DuplicateFileDto,
    DuplicateGroupDto, ErrorDto, ImportFieldDto, ImportSelectionDto, ImportTrackDto, NameProbeDto,
    NameProbesDto, PlaceholderDto, PlanDto, ProviderHub, ReleaseDto, SaveImagesDto, SearchQueryDto,
    SettingsDto, TagEditDto, TextLinesDto, TextProbeDto, TrackDto, TransformRuleDto, UndoReportDto,
};
use tagrex_core::plan::UndoMode;
use tagrex_player::{Player, PlayerStatus};

/// No library is open until the user opens one, hence `Option`. `Mutex` makes
/// the non-`Sync` journal usable as shared Tauri state.
type AppState = Arc<Mutex<Option<App>>>;

static SAMPLE_QUEUE: blocking::Queue = blocking::Queue::new();
static IMAGE_READ_QUEUE: blocking::Queue = blocking::Queue::new();
static BROWSER_QUEUE: blocking::Queue = blocking::Queue::new();
static SETTINGS_QUEUE: blocking::Queue = blocking::Queue::new();
static SESSION_QUEUE: blocking::Queue = blocking::Queue::new();
static LIBRARY_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SETTINGS_REVISION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The provider side (#166): one per process, alive whether or not a library is
/// open. Each source has one persistent hub and queue, preserving its throttle
/// cadence without making unrelated sources wait for each other's network I/O.
type ProviderState = Arc<ProviderQueues>;

struct SourceQueue {
    hub: Mutex<ProviderHub>,
    jobs: blocking::Queue,
    settings: Arc<Mutex<SettingsDto>>,
}

struct ProviderQueues {
    sources: std::collections::HashMap<&'static str, Arc<SourceQueue>>,
    settings: Arc<Mutex<SettingsDto>>,
}

impl Default for ProviderQueues {
    fn default() -> Self {
        let settings = Arc::new(Mutex::new(SettingsDto::default()));
        Self {
            sources: ["discogs", "musicbrainz", "beatport", "bandcamp", "soundeo"]
                .into_iter()
                .map(|source| {
                    (
                        source,
                        Arc::new(SourceQueue {
                            hub: Mutex::new(ProviderHub::default()),
                            jobs: blocking::Queue::new(),
                            settings: settings.clone(),
                        }),
                    )
                })
                .collect(),
            settings,
        }
    }
}

impl ProviderQueues {
    fn source(&self, source: &str) -> Result<Arc<SourceQueue>, ErrorDto> {
        self.sources
            .get(source)
            .cloned()
            .ok_or_else(|| shell_error(format!("unknown source: {source}")))
    }

    fn apply_settings(&self, settings: &SettingsDto) -> Result<(), ErrorDto> {
        // Do not wait for in-flight HTTP while saving preferences. Each next
        // source job adopts this snapshot without resetting request cadence.
        *self.settings.lock().map_err(shell_error)? = settings.clone();
        Ok(())
    }
}

/// Paths the OS handed the app, waiting for the frontend to come and take them
/// (#51). Opening one is the frontend's job — it is the same "open these paths"
/// the drop handler already does — but the hand-over can arrive before there is
/// a webview to tell, so it queues here instead.
type PendingOpen = Mutex<Vec<PathBuf>>;

/// The nudge that says [`PendingOpen`] is not empty. Deliberately payload-free:
/// the paths travel in the queue and the frontend drains it, so an event that
/// fires before the listener exists costs nothing — the startup drain picks the
/// same paths up.
const OPEN_PATHS_EVENT: &str = "tagrex://open-paths";
/// Emitted while an apply runs (#437): `{ done, total, path }`.
const APPLY_PROGRESS_EVENT: &str = "tagrex://apply-progress";
/// The apply is reported at most this often; the files of a fast local apply go
/// by faster than a window can repaint.
const APPLY_PROGRESS_INTERVAL: std::time::Duration = std::time::Duration::from_millis(60);
/// Emitted while BPM detection runs (#536): `{ done, total, path }`, before each
/// file and once at the end.
const BPM_PROGRESS_EVENT: &str = "tagrex://bpm-progress";
/// Emitted while the identical-audio scan decodes files (#821):
/// `{ stage, done, total }`, `stage` being `"quick"` or `"full"`.
const DUPLICATE_PROGRESS_EVENT: &str = "tagrex://duplicate-progress";
/// Emitted while an update downloads (#602): `{ downloaded, total }` in bytes,
/// `total` null when the server does not say.
const UPDATE_PROGRESS_EVENT: &str = "tagrex://update-progress";

/// The paths in a command line. Windows and Linux hand a folder over as an
/// argument; macOS does not, and passes `-psn_…` of its own when Finder starts
/// the app, so anything that is not a path that exists is not ours.
fn paths_from_args<I>(args: I) -> Vec<PathBuf>
where
    I: IntoIterator<Item = std::ffi::OsString>,
{
    args.into_iter()
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .collect()
}

/// Queue `paths` for the frontend and tell it they are there.
fn hand_over_paths(app: &tauri::AppHandle, paths: Vec<PathBuf>) {
    if paths.is_empty() {
        return;
    }
    app.state::<PendingOpen>().lock().unwrap().extend(paths);
    // A window that isn't listening yet is not a problem: what matters is in
    // the queue, and the frontend drains it on startup as well.
    let _ = tauri::Emitter::emit(app, OPEN_PATHS_EVENT, ());
}

/// A failure raised by the shell rather than by the app — Tauri itself, a
/// background task that panicked, an argument that never reaches the command
/// layer. There is no error type of ours behind it and so no code to give it
/// (#268); the interface shows the text.
fn shell_error(error: impl std::fmt::Display) -> ErrorDto {
    ErrorDto::plain(error.to_string())
}

fn with_app<T>(
    state: &AppState,
    f: impl FnOnce(&App) -> Result<T, ErrorDto>,
) -> Result<T, ErrorDto> {
    let guard = state.lock().map_err(shell_error)?;
    let app = guard
        .as_ref()
        .ok_or_else(|| ErrorDto::from(AppError::NoLibraryOpen))?;
    f(app)
}

fn with_providers<T>(
    state: &SourceQueue,
    f: impl FnOnce(&ProviderHub) -> Result<T, ErrorDto>,
) -> Result<T, ErrorDto> {
    let hub = state.hub.lock().map_err(shell_error)?;
    let settings = state.settings.lock().map_err(shell_error)?.clone();
    hub.apply_settings(&settings);
    f(&hub)
}

fn with_app_mut<T>(
    state: &AppState,
    f: impl FnOnce(&mut App) -> Result<T, ErrorDto>,
) -> Result<T, ErrorDto> {
    let mut guard = state.lock().map_err(shell_error)?;
    let app = guard
        .as_mut()
        .ok_or_else(|| ErrorDto::from(AppError::NoLibraryOpen))?;
    f(app)
}

#[tauri::command]
async fn open_library(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    root: String,
) -> Result<(), ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let config_dir = app.path().app_config_dir().map_err(shell_error)?;
            std::fs::create_dir_all(&config_dir).map_err(ErrorDto::from)?;
            let journal_path = config_dir.join("journal.sqlite");
            let opened = App::open(root, &journal_path).map_err(ErrorDto::from)?;
            // Apply saved settings (proxy / rate-limit / ID3 version) to the new session.
            opened.apply_settings(&read_settings(&app));
            let mut session = state.lock().map_err(shell_error)?;
            *session = Some(opened);
            LIBRARY_GENERATION.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .await
}

/// Open a drag-and-drop of `paths` (#127): a lone folder opens as a library,
/// anything else (files, several folders, a mix) as a file-set. Returns the
/// resolved mode + grouping info for the frontend.
#[tauri::command]
async fn open_drop(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    paths: Vec<String>,
) -> Result<DropResultDto, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let config_dir = app.path().app_config_dir().map_err(shell_error)?;
            std::fs::create_dir_all(&config_dir).map_err(ErrorDto::from)?;
            let journal_path = config_dir.join("journal.sqlite");
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            let (opened, result) = App::open_drop(paths, &journal_path).map_err(ErrorDto::from)?;
            opened.apply_settings(&read_settings(&app));
            let mut session = state.lock().map_err(shell_error)?;
            *session = Some(opened);
            LIBRARY_GENERATION.fetch_add(1, Ordering::SeqCst);
            Ok(result)
        })
        .await
}

/// Take the paths the OS handed the app, if any (#51).
///
/// Draining rather than reading is what keeps the two sides from opening the
/// same folder twice: the frontend asks once at startup and again on every
/// nudge, and whichever call arrives first is the one that gets them.
#[tauri::command]
fn take_launch_paths(pending: State<PendingOpen>) -> Vec<String> {
    std::mem::take(&mut *pending.lock().unwrap())
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

// Session jobs acquire their queue asynchronously, then run blocking work on
// the blocking pool. No mutex guard crosses an await or blocks the event thread.
#[tauri::command]
async fn list_tracks(state: State<'_, AppState>) -> Result<Vec<TrackDto>, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || with_app(&state, |app| Ok(app.list_tracks())))
        .await
}

/// Move files to the system Trash (#213). Returns what actually went, so the
/// table drops exactly those rows. Refuses the whole call if any path is not a
/// file this session lists inside the open library — nothing is moved then.
#[tauri::command]
async fn trash_files(
    state: State<'_, AppState>,
    paths: Vec<PathBuf>,
) -> Result<Vec<String>, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            with_app_mut(&state, |app| {
                app.trash_files(&paths).map_err(ErrorDto::from)
            })
        })
        .await
}

/// Lock a set of fields against change for the rest of the session (#48).
/// Replaces the whole set, so the UI sends what is locked rather than a delta.
#[tauri::command]
async fn set_locked_fields(
    state: State<'_, AppState>,
    fields: Vec<String>,
) -> Result<(), ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            with_app(&state, |app| {
                app.set_locked_fields(&fields);
                Ok(())
            })
        })
        .await
}

/// What is locked right now (#48). The UI reads it back on start rather than
/// trusting its own copy — the lock lives in the session, and the window can be
/// reloaded without the session ending.
#[tauri::command]
async fn locked_fields(state: State<'_, AppState>) -> Result<Vec<String>, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || with_app(&state, |app| Ok(app.locked_fields())))
        .await
}

#[tauri::command]
async fn find_duplicates(
    state: State<'_, AppState>,
    criterion: String,
) -> Result<Vec<DuplicateGroupDto>, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            with_app(&state, |app| {
                app.find_duplicates(&criterion).map_err(ErrorDto::from)
            })
        })
        .await
}

/// Asks a running identical-audio scan to stop (#821). Shared with the
/// threads the scan decodes on, hence the `Arc`.
#[derive(Default)]
struct DuplicateControl(Arc<AtomicBool>);

/// Progress of the identical-audio scan (#821).
#[derive(Clone, serde::Serialize)]
struct DuplicateProgressDto {
    /// `"quick"` while the start of each file is compared, `"full"` while the
    /// likely copies are decoded in full.
    stage: &'static str,
    done: usize,
    total: usize,
}

/// What the identical-audio scan found (#821).
#[derive(serde::Serialize)]
struct AudioDuplicatesDto {
    groups: Vec<DuplicateGroupDto>,
    /// Files that could not be decoded, by path.
    failed: Vec<String>,
    /// The scan was stopped; `groups` holds what was confirmed before that.
    cancelled: bool,
    /// Digests taken from the cache instead of decoding (#823).
    reused: usize,
}

/// Seconds of audio the quick pass of the identical-audio scan compares.
const QUICK_DIGEST_SECS: u32 = 10;

/// Audio digests kept between identical-audio scans for the session (#823),
/// by path and length decoded (`None` for the whole file), each with the
/// fingerprint the file had when it was decoded. A file whose fingerprint
/// changed since is decoded again. Failures are not kept.
type DigestStore =
    std::collections::HashMap<(String, Option<u32>), (tagrex_commands::FileFingerprint, [u8; 32])>;

#[derive(Default)]
struct DigestCache(Arc<Mutex<DigestStore>>);

/// One digest result per file, `None` for a file not reached before a cancel.
type Digests = Vec<Option<Result<[u8; 32], tagrex_player::digest::DigestError>>>;

/// Entries the digest cache holds before it starts over: two per file, so a
/// library of this many files' worth of candidates, a few megabytes.
const DIGEST_CACHE_LIMIT: usize = 200_000;

/// The audio digest of each file, on several threads, in the order given.
/// `None` for a file not reached before a cancel. A digest `cache` holds for
/// the file as it is now is used instead of decoding it (#823); the count of
/// those comes back with the digests.
fn digest_files(
    files: &[DuplicateFileDto],
    seconds: Option<u32>,
    cancel: &AtomicBool,
    cache: &Mutex<DigestStore>,
    report: &(dyn Fn(usize, usize) + Sync),
) -> (Digests, usize) {
    use std::sync::atomic::AtomicUsize;
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let reused = AtomicUsize::new(0);
    let results: Vec<Mutex<Option<_>>> = files.iter().map(|_| Mutex::new(None)).collect();
    // One core is left to the window; decoding is all CPU.
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .saturating_sub(1)
        .clamp(1, 8)
        .min(files.len().max(1));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(file) = files.get(index) else {
                    break;
                };
                let path = Path::new(&file.path);
                let key = (file.path.clone(), seconds);
                let stamp = tagrex_commands::FileFingerprint::of(path);
                let cached = stamp.as_ref().and_then(|stamp| {
                    let cache = cache.lock().unwrap();
                    let (kept, digest) = cache.get(&key)?;
                    (kept == stamp).then_some(*digest)
                });
                let digest = match cached {
                    Some(digest) => {
                        reused.fetch_add(1, Ordering::Relaxed);
                        Ok(digest)
                    }
                    None => {
                        let digest = tagrex_player::digest::audio_digest(path, seconds, cancel);
                        if let (Ok(digest), Some(stamp)) = (&digest, stamp) {
                            let mut cache = cache.lock().unwrap();
                            if cache.len() >= DIGEST_CACHE_LIMIT {
                                cache.clear();
                            }
                            cache.insert(key, (stamp, *digest));
                        }
                        digest
                    }
                };
                *results[index].lock().unwrap() = Some(digest);
                report(done.fetch_add(1, Ordering::Relaxed) + 1, files.len());
            });
        }
    });
    let digests = results
        .into_iter()
        .map(|slot| slot.into_inner().unwrap())
        .collect();
    (digests, reused.into_inner())
}

/// The identical-audio scan over `candidates` (#821): the opening seconds of
/// each first, then in full only the files whose openings match another's.
/// `report` hears `(stage, done, total)` as files are decoded.
fn audio_duplicates(
    candidates: Vec<DuplicateFileDto>,
    cancel: &AtomicBool,
    cache: &Mutex<DigestStore>,
    report: &(dyn Fn(&'static str, usize, usize) + Sync),
) -> AudioDuplicatesDto {
    use tagrex_player::digest::DigestError;
    let mut failed = Vec::new();
    let mut sort = |files: Vec<DuplicateFileDto>, digests: Digests| {
        let mut decoded = Vec::new();
        for (file, digest) in files.into_iter().zip(digests) {
            match digest {
                Some(Ok(digest)) => decoded.push((file, digest)),
                Some(Err(DigestError::Failed(_))) => failed.push(file.path),
                Some(Err(DigestError::Cancelled)) | None => {}
            }
        }
        decoded
    };

    let (quick, mut reused) = digest_files(
        &candidates,
        Some(QUICK_DIGEST_SECS),
        cancel,
        cache,
        &|done, total| report("quick", done, total),
    );
    let opened = sort(candidates, quick);
    let mut groups = Vec::new();
    if !cancel.load(Ordering::SeqCst) {
        let likely: Vec<DuplicateFileDto> = tagrex_commands::group_by_digest(opened, "")
            .into_iter()
            .flat_map(|group| group.files)
            .collect();
        let (full, reused_full) = digest_files(&likely, None, cancel, cache, &|done, total| {
            report("full", done, total)
        });
        reused += reused_full;
        groups = tagrex_commands::group_by_digest(sort(likely, full), "identical audio");
    }
    failed.sort();
    AudioDuplicatesDto {
        groups,
        failed,
        cancelled: cancel.load(Ordering::SeqCst),
        reused,
    }
}

/// Find files holding identical decoded audio, whatever their tags and covers
/// (#821). Read-only, like [`find_duplicates`].
///
/// Only the files that could have a copy are decoded at all (same sample rate,
/// channel count and length as another file), first just their opening
/// seconds, and only the files whose openings match are decoded in full. The
/// decoding runs outside the session lock on several threads; a cancel keeps
/// the groups already confirmed. Digests of files unchanged since an earlier
/// scan in the session are reused (#823).
#[tauri::command]
async fn find_audio_duplicates(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    control: State<'_, DuplicateControl>,
    cache: State<'_, DigestCache>,
) -> Result<AudioDuplicatesDto, ErrorDto> {
    control.0.store(false, Ordering::SeqCst);
    let cancel = control.0.clone();
    let cache = cache.0.clone();
    let session = state.inner().clone();
    let candidates = SESSION_QUEUE
        .run(move || with_app(&session, |app| Ok(app.audio_duplicate_candidates())))
        .await?;
    tauri::async_runtime::spawn_blocking(move || {
        let report = |stage: &'static str, done: usize, total: usize| {
            let _ = tauri::Emitter::emit(
                &app_handle,
                DUPLICATE_PROGRESS_EVENT,
                DuplicateProgressDto { stage, done, total },
            );
        };
        audio_duplicates(candidates, &cancel, &cache, &report)
    })
    .await
    .map_err(shell_error)
}

#[tauri::command]
fn cancel_find_duplicates(control: State<DuplicateControl>) {
    control.0.store(true, Ordering::SeqCst);
}

/// Build a provider *release* page URL from a hard-coded host plus a
/// charset-validated id (#92). Kept separate from the command so the frontend can
/// only ever reach a Discogs/MusicBrainz release page — never an arbitrary URL —
/// and so the construction is unit-testable without touching the system browser.
fn release_url(source: &str, id: &str) -> Result<String, ErrorDto> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(shell_error(format!("invalid release id: {id:?}")));
    }
    match source {
        "discogs" => Ok(format!("https://www.discogs.com/release/{id}")),
        "musicbrainz" => Ok(format!("https://musicbrainz.org/release/{id}")),
        // The slug is cosmetic; the store resolves a release by its id alone
        // (#162), so nothing user-supplied has to go into the path.
        "beatport" => Ok(format!("https://www.beatport.com/release/-/{id}")),
        other => Err(shell_error(format!("unknown source: {other}"))),
    }
}

/// Open a provider release page in the system browser (#92).
#[tauri::command]
async fn open_release_page(source: String, id: String) -> Result<(), ErrorDto> {
    BROWSER_QUEUE
        .run(move || {
            let url = release_url(&source, &id)?;
            open::that(url).map_err(ErrorDto::from)
        })
        .await
}

#[tauri::command]
async fn preview_rename(
    state: State<'_, AppState>,
    mask: String,
    paths: Vec<String>,
) -> Result<PlanDto, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| {
                app.preview_rename(&mask, &paths).map_err(ErrorDto::from)
            })
        })
        .await
}

#[tauri::command]
async fn preview_tags_from_name(
    state: State<'_, AppState>,
    mask: String,
    paths: Vec<String>,
) -> Result<PlanDto, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| {
                app.preview_tags_from_name(&mask, &paths)
                    .map_err(ErrorDto::from)
            })
        })
        .await
}

#[tauri::command]
async fn probe_tags_from_name(
    state: State<'_, AppState>,
    mask: String,
    path: String,
) -> Result<NameProbeDto, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            with_app(&state, |app| {
                app.probe_tags_from_name(&mask, &PathBuf::from(path))
                    .map_err(ErrorDto::from)
            })
        })
        .await
}

/// Every line of a text list under a FROM TEXT mask and the file each track
/// lands on (#550), for the mask builder's list.
#[tauri::command]
async fn probe_text_lines(
    state: State<'_, AppState>,
    mask: String,
    text: String,
    paths: Vec<String>,
) -> Result<TextLinesDto, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| {
                app.probe_text_lines(&mask, &text, &paths)
                    .map_err(ErrorDto::from)
            })
        })
        .await
}

/// What a FROM NAME mask reads from each of several files (#548), for the mask
/// builder's list.
#[tauri::command]
async fn probe_tags_from_names(
    state: State<'_, AppState>,
    mask: String,
    paths: Vec<String>,
) -> Result<NameProbesDto, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| {
                app.probe_tags_from_names(&mask, &paths)
                    .map_err(ErrorDto::from)
            })
        })
        .await
}

/// Tags from a text list (#61): one line per selected file, in order.
#[tauri::command]
async fn preview_tags_from_text(
    state: State<'_, AppState>,
    mask: String,
    text: String,
    paths: Vec<String>,
) -> Result<PlanDto, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| {
                app.preview_tags_from_text(&mask, &text, &paths)
                    .map_err(ErrorDto::from)
            })
        })
        .await
}

#[tauri::command]
async fn probe_tags_from_text(
    state: State<'_, AppState>,
    mask: String,
    text: String,
) -> Result<TextProbeDto, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            with_app(&state, |app| {
                app.probe_tags_from_text(&mask, &text)
                    .map_err(ErrorDto::from)
            })
        })
        .await
}

/// A FROM TEXT mask read off the list's own shape (#522), or none when no
/// reading of it fits a line. Stateless: it only looks at the text.
#[tauri::command]
async fn suggest_text_list_mask(text: String) -> Result<Option<String>, ErrorDto> {
    SAMPLE_QUEUE
        .run(move || Ok(tagrex_commands::suggest_text_list_mask(&text)))
        .await
}

/// Read a text list the user picked for the FROM TEXT panel (#61). Stateless,
/// like [`read_cover_image`]: the file is the user's explicit choice.
#[tauri::command]
async fn read_text_list(path: String) -> Result<String, ErrorDto> {
    IMAGE_READ_QUEUE
        .run(move || tagrex_commands::read_text_list(&PathBuf::from(path)).map_err(ErrorDto::from))
        .await
}

#[tauri::command]
async fn preview_transform(
    state: State<'_, AppState>,
    paths: Vec<String>,
    rules: Vec<TransformRuleDto>,
    scope: String,
) -> Result<PlanDto, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| {
                app.preview_transform(&paths, &rules, &scope)
                    .map_err(ErrorDto::from)
            })
        })
        .await
}

#[tauri::command]
async fn preview_move(
    state: State<'_, AppState>,
    mask: String,
    paths: Vec<String>,
    destination: Option<String>,
    copy: bool,
    prune_empty_dirs: bool,
) -> Result<PlanDto, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
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
                .map_err(ErrorDto::from)
            })
        })
        .await
}

#[tauri::command]
async fn preview_tag_edits(
    state: State<'_, AppState>,
    edits: Vec<TagEditDto>,
    // The release cover an import brought with it (#207), when there is one.
    cover: Option<CoverArtDto>,
    // The files that cover is for (#773): the ones the import paired with a
    // track. Absent, it goes to every file with edits, as it always did.
    cover_paths: Option<Vec<String>>,
) -> Result<PlanDto, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            with_app(&state, |app| {
                app.preview_tag_edits_with_cover_for(&edits, cover.as_ref(), cover_paths.as_deref())
                    .map_err(ErrorDto::from)
            })
        })
        .await
}

/// Preview replacing every selected file's whole image set (#56) — the one
/// command behind add, remove, reorder and set-the-type.
#[tauri::command]
async fn preview_cover_set(
    state: State<'_, AppState>,
    paths: Vec<String>,
    covers: Vec<CoverArtDto>,
) -> Result<PlanDto, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| {
                app.preview_cover_set(&paths, &covers)
                    .map_err(ErrorDto::from)
            })
        })
        .await
}

#[tauri::command]
async fn preview_cover_embed(
    state: State<'_, AppState>,
    paths: Vec<String>,
    cover: CoverArtDto,
) -> Result<PlanDto, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| {
                app.preview_cover_embed(&paths, &cover)
                    .map_err(ErrorDto::from)
            })
        })
        .await
}

#[tauri::command]
async fn export_cover(
    state: State<'_, AppState>,
    paths: Vec<String>,
    basename: String,
) -> Result<CoverExportDto, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| {
                app.export_cover(&paths, &basename).map_err(ErrorDto::from)
            })
        })
        .await
}

#[tauri::command]
async fn read_cover_summary(
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<CoverSummaryDto, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| {
                app.read_cover_summary(&paths).map_err(ErrorDto::from)
            })
        })
        .await
}

/// Each file's own folder image as its front cover (#824).
#[tauri::command]
async fn preview_folder_covers(
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<PlanDto, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| {
                app.preview_folder_covers(&paths).map_err(ErrorDto::from)
            })
        })
        .await
}

/// Whether any of the files has an image in its folder to embed (#824).
#[tauri::command]
async fn has_folder_cover(
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<bool, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| Ok(app.has_folder_cover(&paths)))
        })
        .await
}

/// Read an image file (dropped onto the cover well, #133) into a cover DTO for
/// [`preview_cover_embed`]. Stateless — the source image isn't confined to the
/// library, matching the file picker (the user chose it explicitly).
#[tauri::command]
async fn read_cover_image(path: String) -> Result<CoverArtDto, ErrorDto> {
    IMAGE_READ_QUEUE
        .run(move || {
            tagrex_commands::read_cover_image(&PathBuf::from(path)).map_err(ErrorDto::from)
        })
        .await
}

#[tauri::command]
async fn preview_cover_remove(
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<PlanDto, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| {
                app.preview_cover_remove(&paths).map_err(ErrorDto::from)
            })
        })
        .await
}

#[tauri::command]
async fn preview_remove_tag_block(
    state: State<'_, AppState>,
    paths: Vec<String>,
    kind: String,
) -> Result<PlanDto, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| {
                app.preview_remove_tag_block(&paths, &kind)
                    .map_err(ErrorDto::from)
            })
        })
        .await
}

#[tauri::command]
async fn preview_fix_encoding(
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<PlanDto, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| {
                app.preview_fix_encoding(&paths).map_err(ErrorDto::from)
            })
        })
        .await
}

#[tauri::command]
async fn tag_block_targets(
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<BlockTargetsDto, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| {
                app.tag_block_targets(&paths).map_err(ErrorDto::from)
            })
        })
        .await
}

#[tauri::command]
async fn preview_convert_tag_block(
    state: State<'_, AppState>,
    paths: Vec<String>,
    from: String,
    to: String,
    revision: Option<String>,
) -> Result<PlanDto, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| {
                app.preview_convert_tag_block(&paths, &from, &to, revision.as_deref())
                    .map_err(ErrorDto::from)
            })
        })
        .await
}

#[tauri::command]
async fn preview_clear_tags(
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<PlanDto, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| {
                app.preview_clear_tags(&paths).map_err(ErrorDto::from)
            })
        })
        .await
}

#[tauri::command]
async fn export_playlist(
    state: State<'_, AppState>,
    paths: Vec<String>,
    file_name: String,
) -> Result<String, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| {
                app.export_playlist(&paths, &file_name)
                    .map_err(ErrorDto::from)
            })
        })
        .await
}

/// One playlist per folder or per album (#62). Returns every file written.
#[tauri::command]
async fn export_playlists(
    state: State<'_, AppState>,
    paths: Vec<String>,
    grouping: String,
    name_mask: String,
) -> Result<Vec<String>, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| {
                app.export_playlists(&paths, &grouping, &name_mask)
                    .map_err(ErrorDto::from)
            })
        })
        .await
}

#[tauri::command]
async fn export_cue(
    state: State<'_, AppState>,
    paths: Vec<String>,
    file_name: String,
) -> Result<String, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| {
                app.export_cue(&paths, &file_name).map_err(ErrorDto::from)
            })
        })
        .await
}

#[tauri::command]
async fn export_csv(
    state: State<'_, AppState>,
    paths: Vec<String>,
    file_name: String,
) -> Result<String, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| {
                app.export_csv(&paths, &file_name).map_err(ErrorDto::from)
            })
        })
        .await
}

#[tauri::command]
async fn export_report(
    state: State<'_, AppState>,
    paths: Vec<String>,
    mask: String,
    file_name: String,
) -> Result<String, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| {
                app.export_report(&paths, &mask, &file_name)
                    .map_err(ErrorDto::from)
            })
        })
        .await
}

#[tauri::command]
async fn export_html(
    state: State<'_, AppState>,
    paths: Vec<String>,
    file_name: String,
) -> Result<String, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| {
                app.export_html(&paths, &file_name).map_err(ErrorDto::from)
            })
        })
        .await
}

#[tauri::command]
async fn export_xml(
    state: State<'_, AppState>,
    paths: Vec<String>,
    file_name: String,
) -> Result<String, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| {
                app.export_xml(&paths, &file_name).map_err(ErrorDto::from)
            })
        })
        .await
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

/// Waveforms already computed, newest last (#101): the file's path, the mtime
/// it was decoded at, and the buckets.
///
/// Keyed by mtime as well as path because a file whose tags were just rewritten
/// is a different file on disk, and a stale picture beside a track that has
/// changed is exactly the kind of small lie that makes a display untrustworthy.
/// Bounded, and a kilobyte an entry — the cache exists so replaying a track
/// costs nothing, not so a whole library can be held.
type WaveformState = Mutex<Vec<(String, u64, Vec<u8>)>>;

/// How many waveforms are kept. Sixty-four kilobytes of pictures, which is more
/// than anybody plays through in a sitting.
const WAVEFORM_CACHE_ENTRIES: usize = 64;

/// The amplitude envelope of a file, for the player's seek bar (#101).
///
/// `async` and off on a blocking thread, both deliberately: decoding an
/// hour-long mix takes seconds, a *synchronous* command would run it on the main
/// thread and freeze the whole webview, and an async one that blocked would sit
/// on a runtime worker that provider requests also want.
#[tauri::command]
async fn waveform(cache: State<'_, WaveformState>, path: String) -> Result<Vec<u8>, ErrorDto> {
    let target = path.clone();
    let stamp = IMAGE_READ_QUEUE
        .run(move || {
            Ok({
                std::fs::metadata(&target)
                    .and_then(|meta| meta.modified())
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|since| since.as_secs())
                    .unwrap_or(0)
            })
        })
        .await?;

    // Scoped so no guard is alive across the await below.
    {
        let entries = cache.lock().unwrap();
        if let Some((_, _, peaks)) = entries
            .iter()
            .find(|(cached, at, _)| cached == &path && *at == stamp)
        {
            return Ok(peaks.clone());
        }
    }
    let target = PathBuf::from(&path);
    let peaks = tauri::async_runtime::spawn_blocking(move || tagrex_player::waveform(&target))
        .await
        .map_err(shell_error)?
        .map_err(shell_error)?;
    let mut entries = cache.lock().unwrap();
    entries.retain(|(cached, _, _)| cached != &path);
    entries.push((path, stamp, peaks.clone()));
    if entries.len() > WAVEFORM_CACHE_ENTRIES {
        entries.remove(0);
    }
    Ok(peaks)
}

#[tauri::command]
fn player_set_volume(player: State<Player>, level: f64) {
    player.set_volume(level as f32);
}

#[tauri::command]
fn player_status(player: State<Player>) -> PlayerStatus {
    player.status()
}

/// What the preview player is doing, for the system's Now Playing (#218).
#[tauri::command]
async fn media_session_update(app: tauri::AppHandle, now: media_session::NowPlaying) {
    media_session::update(app, now).await;
}

/// The callback that turns the executor's progress into events (#437, #448),
/// throttled so a fast run does not flood the window.
fn progress_reporter(
    app_handle: tauri::AppHandle,
) -> impl FnMut(&tagrex_core::plan::ApplyProgress) {
    let mut last = std::time::Instant::now() - APPLY_PROGRESS_INTERVAL;
    move |progress| {
        if progress.done != progress.total && last.elapsed() < APPLY_PROGRESS_INTERVAL {
            return;
        }
        last = std::time::Instant::now();
        let _ = tauri::Emitter::emit(
            &app_handle,
            APPLY_PROGRESS_EVENT,
            ApplyProgressDto {
                done: progress.done,
                total: progress.total,
                path: progress.path.to_string_lossy().into_owned(),
            },
        );
    }
}

/// Asks the running apply to stop (#437). Its own state rather than part of
/// [`AppState`]: the apply holds that lock for its whole run, and the request has
/// to get through while it does.
#[derive(Default)]
struct ApplyControl(Arc<AtomicBool>);

#[derive(serde::Serialize, Clone)]
struct ApplyProgressDto {
    done: usize,
    total: usize,
    path: String,
}

// Both session lock waiting and Apply work stay off the event/runtime threads.
// Cancellation uses independent atomic state and bypasses the session queue.
#[tauri::command]
async fn apply_plan(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    control: State<'_, ApplyControl>,
    plan: PlanDto,
) -> Result<BatchDto, ErrorDto> {
    let state = state.inner().clone();
    let control = control.0.clone();
    control.store(false, Ordering::SeqCst);
    SESSION_QUEUE
        .run(move || {
            let mut report = progress_reporter(app_handle);
            with_app_mut(&state, |app| {
                app.apply_with(&plan, &mut report, &control)
                    .map_err(ErrorDto::from)
            })
        })
        .await
}

#[tauri::command]
fn cancel_apply(control: State<ApplyControl>) {
    control.0.store(true, Ordering::SeqCst);
}

/// Asks a running BPM detection to stop (#536). Shared with the blocking thread
/// the detection runs on, hence the `Arc`.
#[derive(Default)]
struct BpmControl(Arc<AtomicBool>);

/// What a BPM detection found (#536): the plan for the tempos it measured, and
/// what it could not measure, for the toast.
#[derive(serde::Serialize)]
struct BpmDetectionDto {
    plan: PlanDto,
    /// Files gone through, whether measured, skipped or failed.
    analysed: usize,
    /// Files with no steady beat to measure.
    no_tempo: usize,
    /// Files passed over as longer than the limit in settings (#538).
    too_long: usize,
    /// Files that could not be decoded, by path.
    failed: Vec<String>,
    /// The run was cancelled; the plan holds the files measured before that.
    cancelled: bool,
}

/// Measure the tempo of each file and stage the BPM field (#536).
///
/// The measuring runs on a blocking thread and outside the session lock: it
/// takes seconds a file, and the window has to stay usable while it does. Only
/// building the plan from the results takes the lock, for as long as reading
/// the current values takes. A cancel stops at the current file and stages what
/// was measured before it.
#[tauri::command]
async fn detect_bpm(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    control: State<'_, BpmControl>,
    paths: Vec<String>,
) -> Result<BpmDetectionDto, ErrorDto> {
    control.0.store(false, Ordering::SeqCst);
    let cancel = control.0.clone();
    let session = state.inner().clone();
    let limit = SESSION_QUEUE
        .run(move || with_app(&session, |app| Ok(app.bpm_max_length())))
        .await?;
    let measured = tauri::async_runtime::spawn_blocking(move || {
        let mut detector = tagrex_player::tempo::TempoDetector::new().map_err(shell_error)?;
        let total = paths.len();
        let report = |done: usize, path: &str| {
            let _ = tauri::Emitter::emit(
                &app_handle,
                BPM_PROGRESS_EVENT,
                ApplyProgressDto {
                    done,
                    total,
                    path: path.to_string(),
                },
            );
        };
        let (mut detected, mut no_tempo, mut failed) = (Vec::new(), 0, Vec::new());
        let (mut too_long, mut cancelled) = (0, false);
        for (done, path) in paths.iter().enumerate() {
            report(done, path);
            let target = PathBuf::from(path);
            if tagrex_commands::too_long_for_bpm(&target, limit) {
                too_long += 1;
                continue;
            }
            match detector.detect(&target, &cancel) {
                Ok(Some(bpm)) => detected.push((target, bpm)),
                Ok(None) => no_tempo += 1,
                Err(tagrex_player::tempo::TempoError::Failed(_)) => failed.push(path.clone()),
                Err(tagrex_player::tempo::TempoError::Cancelled) => {
                    cancelled = true;
                    break;
                }
            }
        }
        let analysed = detected.len() + no_tempo + failed.len() + too_long;
        report(analysed, "");
        Ok::<_, ErrorDto>((detected, analysed, no_tempo, too_long, failed, cancelled))
    })
    .await
    .map_err(shell_error)??;
    let (detected, analysed, no_tempo, too_long, failed, cancelled) = measured;
    let session = state.inner().clone();
    let plan = SESSION_QUEUE
        .run(move || {
            with_app(&session, |app| {
                app.preview_bpm(&detected).map_err(ErrorDto::from)
            })
        })
        .await?;
    Ok(BpmDetectionDto {
        plan,
        analysed,
        no_tempo,
        too_long,
        failed,
        cancelled,
    })
}

#[tauri::command]
fn cancel_detect_bpm(control: State<BpmControl>) {
    control.0.store(true, Ordering::SeqCst);
}

// `async` for the same reason as `apply_plan` (#448): a synchronous command runs
// on the main thread, and rolling back a big batch took seconds with the window
// frozen. Progress goes out as the same events; there is no cancel, because a
// rollback stopped half-way would leave the batch partly undone.
#[tauri::command]
async fn undo(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    batch_id: i64,
    mode: Option<String>,
) -> Result<UndoReportDto, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let mode = match mode.as_deref() {
                Some("keep_changed") => UndoMode::KeepChanged,
                Some("overwrite") => UndoMode::Overwrite,
                _ => UndoMode::Strict,
            };
            let mut report = progress_reporter(app_handle);
            with_app_mut(&state, |app| {
                app.undo_with_mode(batch_id, mode, &mut report)
                    .map_err(ErrorDto::from)
            })
        })
        .await
}

// Apply the batch undone last again (#775), off the main thread and with the
// same progress events as an undo; `None` when nothing is undone.
#[tauri::command]
async fn redo(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<BatchDto>, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let mut report = progress_reporter(app_handle);
            with_app_mut(&state, |app| {
                app.redo_with(&mut report).map_err(ErrorDto::from)
            })
        })
        .await
}

#[tauri::command]
async fn redo_count(state: State<'_, AppState>) -> Result<usize, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || with_app(&state, |app| Ok(app.redo_count())))
        .await
}

// `async` so it runs off the main thread (#449): the interface asks for the
// history before and after every apply and undo, and a synchronous command
// would hold the window for as long as the journal takes to read.
#[tauri::command]
async fn take_journal_notice(state: State<'_, AppState>) -> Result<Option<String>, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || with_app(&state, |app| Ok(app.take_journal_notice())))
        .await
}

/// Where a damaged settings file was set aside at startup (#808), the first
/// time it is asked, and `None` after that and whenever the file was sound.
#[tauri::command]
fn take_settings_notice(notice: State<'_, SettingsNotice>) -> Option<String> {
    notice
        .0
        .lock()
        .unwrap()
        .take()
        .map(|path| path.to_string_lossy().into_owned())
}

#[tauri::command]
async fn history(state: State<'_, AppState>) -> Result<Vec<BatchDto>, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || with_app(&state, |app| app.history().map_err(ErrorDto::from)))
        .await
}

// Provider requests queue per source before entering the blocking pool.
// Blocking HTTP/throttle waits cannot occupy the event or async runtime thread,
// and a slow source cannot prevent another source from answering. The token
// is ignored by token-less providers.
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
) -> Result<Vec<CandidateDto>, ErrorDto> {
    let providers = providers.source(&source)?;
    let hub = providers.clone();
    providers
        .jobs
        .run(move || {
            with_providers(&hub, |hub| {
                hub.provider_search(&source, &token, &query)
                    .map_err(ErrorDto::from)
            })
        })
        .await
}

#[tauri::command]
async fn provider_fetch_release(
    providers: State<'_, ProviderState>,
    source: String,
    token: String,
    release_id: String,
) -> Result<ReleaseDto, ErrorDto> {
    let providers = providers.source(&source)?;
    let hub = providers.clone();
    providers
        .jobs
        .run(move || {
            with_providers(&hub, |hub| {
                hub.provider_fetch_release(&source, &token, &release_id)
                    .map_err(ErrorDto::from)
            })
        })
        .await
}

#[tauri::command]
async fn provider_fetch_image(
    providers: State<'_, ProviderState>,
    source: String,
    token: String,
    url: String,
) -> Result<CoverArtDto, ErrorDto> {
    let providers = providers.source(&source)?;
    let hub = providers.clone();
    let fetch = providers
        .jobs
        .run(move || {
            with_providers(&hub, |hub| {
                hub.image_fetch(&source, &token).map_err(ErrorDto::from)
            })
        })
        .await?;
    // Only fetcher preparation uses the source queue/lock. The download is
    // bounded by the shared image slots and cannot block a release lookup.
    tauri::async_runtime::spawn_blocking(move || fetch.fetch_cover(&url).map_err(ErrorDto::from))
        .await
        .map_err(shell_error)?
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
) -> Result<SaveImagesDto, ErrorDto> {
    let state = state.inner().clone();
    let path = PathBuf::from(path);
    let target = path.clone();
    let session = state.clone();
    let generation = SESSION_QUEUE
        .run(move || {
            with_app(&session, |app| {
                app.validate_image_target(&target).map_err(ErrorDto::from)?;
                Ok(LIBRARY_GENERATION.load(Ordering::SeqCst))
            })
        })
        .await?;
    let provider = providers.source(&source)?;
    let hub = provider.clone();
    let images = provider
        .jobs
        .run(move || {
            with_providers(&hub, |hub| {
                urls.iter()
                    .map(|url| {
                        hub.fetch_image_bytes(&source, &token, url)
                            .map_err(ErrorDto::from)
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
        })
        .await?;
    SESSION_QUEUE
        .run(move || {
            with_app(&state, |app| {
                if LIBRARY_GENERATION.load(Ordering::SeqCst) != generation {
                    return Err(shell_error(
                        "The open library changed. Save the artwork again.",
                    ));
                }
                app.save_fetched_release_images(&path, &images, overwrite)
                    .map_err(ErrorDto::from)
            })
        })
        .await
}

#[tauri::command]
async fn auto_align(
    state: State<'_, AppState>,
    paths: Vec<String>,
    tracks: Vec<ImportTrackDto>,
) -> Result<Vec<Option<AlignMatchDto>>, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| {
                app.auto_align(&paths, &tracks).map_err(ErrorDto::from)
            })
        })
        .await
}

#[tauri::command]
async fn preview_import(
    state: State<'_, AppState>,
    paths: Vec<String>,
    selection: ImportSelectionDto,
) -> Result<PlanDto, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| {
                app.preview_import(&paths, &selection)
                    .map_err(ErrorDto::from)
            })
        })
        .await
}

/// Path to the locally saved Discogs token (in the OS app-config dir, never in
/// the repo). Convenience only, so the token isn't retyped each session.
fn token_path(app: &tauri::AppHandle) -> Result<PathBuf, ErrorDto> {
    let dir = app.path().app_config_dir().map_err(shell_error)?;
    Ok(dir.join("discogs_token"))
}

#[tauri::command]
async fn saved_discogs_token(app: tauri::AppHandle) -> Result<String, ErrorDto> {
    SETTINGS_QUEUE
        .run(move || {
            let path = token_path(&app)?;
            tighten_secret(&path);
            Ok(std::fs::read_to_string(path)
                .unwrap_or_default()
                .trim()
                .to_string())
        })
        .await
}

#[tauri::command]
async fn save_discogs_token(app: tauri::AppHandle, token: String) -> Result<(), ErrorDto> {
    SETTINGS_QUEUE
        .run(move || {
            let path = token_path(&app)?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(ErrorDto::from)?;
            }
            write_secret(&path, token.trim()).map_err(ErrorDto::from)
        })
        .await
}

/// Write `contents` to `path` as a file only its owner can read: the Discogs
/// token and the Beatport session are credentials, and a plain write left them
/// readable by every account on the machine under the usual umask (0644).
/// The mode is set on the open handle as well, so a file an earlier version
/// wrote is tightened on the next save. On Windows the per-user app-config
/// folder is already private to its owner.
fn write_secret(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(contents.as_bytes())?;
    file.flush()
}

/// Make a credential file an earlier version left readable by others readable
/// by its owner alone (#675), when it is read: [`write_secret`] only reaches a
/// file that is saved again, and a token is saved once and read every session.
/// Best effort; a file that is missing or cannot be changed is left as it is.
fn tighten_secret(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = std::fs::metadata(path) {
            if metadata.permissions().mode() & 0o077 != 0 {
                let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
            }
        }
    }
    #[cfg(not(unix))]
    let _ = path;
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

fn beatport_session_path(app: &tauri::AppHandle) -> Result<PathBuf, ErrorDto> {
    let dir = app.path().app_config_dir().map_err(shell_error)?;
    Ok(dir.join("beatport_session.json"))
}

fn read_beatport_session(app: &tauri::AppHandle) -> Option<BeatportSession> {
    let path = beatport_session_path(app).ok()?;
    tighten_secret(&path);
    let json = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&json).ok()
}

fn write_beatport_session(
    app: &tauri::AppHandle,
    session: &BeatportSession,
) -> Result<(), ErrorDto> {
    let path = beatport_session_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(ErrorDto::from)?;
    }
    let json = serde_json::to_string(session).map_err(ErrorDto::from)?;
    write_secret(&path, &json).map_err(ErrorDto::from)
}

/// The proxy every provider request goes through, from settings.
fn beatport_proxy(app: &tauri::AppHandle) -> Option<String> {
    let proxy = read_settings(app).proxy.trim().to_string();
    (!proxy.is_empty()).then_some(proxy)
}

#[tauri::command]
async fn beatport_status(app: tauri::AppHandle) -> Result<BeatportStatusDto, ErrorDto> {
    SETTINGS_QUEUE
        .run(move || {
            Ok({
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
            })
        })
        .await
}

#[tauri::command]
async fn beatport_logout(app: tauri::AppHandle) -> Result<(), ErrorDto> {
    SETTINGS_QUEUE
        .run(move || {
            let path = beatport_session_path(&app)?;
            match std::fs::remove_file(path) {
                Ok(()) => Ok(()),
                // Signing out of a session that isn't there is a no-op, not a failure.
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(ErrorDto::from(err)),
            }
        })
        .await
}

/// Sign in: read the public client id, open Beatport's login page in its own
/// window, wait for the redirect that carries the authorization code, trade it
/// for tokens. Returns the account name.
///
/// `async` for the same reason the provider commands are (#95): every step here
/// blocks on the network or on the user, and a synchronous command would freeze
/// the whole webview while it did.
#[tauri::command]
async fn beatport_login(app: tauri::AppHandle) -> Result<String, ErrorDto> {
    use tagrex_providers_beatport::auth;

    let proxy = beatport_proxy(&app);
    let agent = auth::agent(proxy.as_deref()).map_err(ErrorDto::from)?;

    let scraper = agent.clone();
    let client_id = tauri::async_runtime::spawn_blocking(move || auth::fetch_client_id(&scraper))
        .await
        .map_err(shell_error)?
        .map_err(ErrorDto::from)?;

    // The window reports back either a code or, if the user gives up and closes
    // it, nothing — hence a channel rather than a return value.
    let (tx, rx) = std::sync::mpsc::channel::<Option<String>>();
    let on_close = tx.clone();
    let url = auth::authorize_url(&client_id)
        .parse()
        .map_err(|_| shell_error("could not build the Beatport sign-in URL"))?;
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
    .map_err(shell_error)?;
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
            let _ = on_close.send(None);
        }
    });

    let received = tauri::async_runtime::spawn_blocking(move || {
        rx.recv_timeout(std::time::Duration::from_secs(BEATPORT_LOGIN_TIMEOUT_SECS))
    })
    .await
    .map_err(shell_error)?;
    // The window has done its job either way; leaving it open would strand a
    // second window with no owner.
    let _ = window.close();

    let code = match received {
        Ok(Some(code)) => code,
        Ok(None) => return Err(shell_error("Sign-in cancelled")),
        Err(_) => return Err(shell_error("Sign-in timed out")),
    };

    let exchange_id = client_id.clone();
    let token = tauri::async_runtime::spawn_blocking(move || {
        auth::exchange_code(&agent, &exchange_id, &code)
    })
    .await
    .map_err(shell_error)?
    .map_err(ErrorDto::from)?;

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
async fn beatport_token(app: tauri::AppHandle) -> Result<String, ErrorDto> {
    use tagrex_providers_beatport::auth;

    let mut session =
        read_beatport_session(&app).ok_or_else(|| shell_error("Not signed in to Beatport"))?;
    if !session.token.is_expired() {
        return Ok(session.token.access_token);
    }

    let proxy = beatport_proxy(&app);
    let agent = auth::agent(proxy.as_deref()).map_err(ErrorDto::from)?;
    let client_id = session.client_id.clone();
    let refresh_token = session.token.refresh_token.clone();
    let token = tauri::async_runtime::spawn_blocking(move || {
        auth::refresh(&agent, &client_id, &refresh_token)
    })
    .await
    .map_err(shell_error)?
    // A refresh token can be revoked from the account page, and then the
    // only way back is a fresh sign-in — say so rather than reporting a
    // bare HTTP status.
    .map_err(|err| ErrorDto::from(AppError::BeatportReauth(err.to_string())))?;

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
fn settings_path(app: &tauri::AppHandle) -> Result<PathBuf, ErrorDto> {
    let dir = app.path().app_config_dir().map_err(shell_error)?;
    Ok(dir.join("settings.json"))
}

/// Where a damaged settings file was set aside at startup (#808), until the
/// interface has said so.
#[derive(Default)]
struct SettingsNotice(Mutex<Option<PathBuf>>);

/// Set a settings file that exists but does not parse aside, as
/// `settings.json.corrupt-<seconds>`, and return its new path (#808).
///
/// Left in place it was read as the defaults and then overwritten by the next
/// save, so a half-written file silently cost every preference. Renamed, the
/// next save writes a fresh file and the damaged one can still be recovered. A
/// file that cannot be read at all (permissions) is not called damaged and is
/// left alone, like a missing one.
fn set_aside_damaged_settings(path: &Path) -> Option<PathBuf> {
    let text = std::fs::read(path).ok()?;
    if serde_json::from_slice::<SettingsDto>(&text).is_ok() {
        return None;
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    let mut aside = path.as_os_str().to_owned();
    aside.push(format!(".corrupt-{stamp}"));
    let aside = PathBuf::from(aside);
    std::fs::rename(path, &aside).ok()?;
    Some(aside)
}

/// Write `json` to `path` so that a crash part-way leaves the old file whole
/// (#808): into a temporary file beside it, synced, then renamed over it.
fn write_settings_file(path: &Path, json: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    let written = (|| {
        let mut file = std::fs::File::create(&temporary)?;
        // The replacement keeps the permissions the old file had, so a file
        // the user made private stays private.
        if let Ok(metadata) = std::fs::metadata(path) {
            file.set_permissions(metadata.permissions())?;
        }
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)
    })();
    if written.is_err() {
        std::fs::remove_file(&temporary).ok();
    }
    written
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

/// Ask GitHub whether a newer release exists (#435). `automatic` is the
/// start-up check: it does nothing at all when the setting is off, so switching
/// it off is enough to keep the app from making the request. The button in
/// Settings asks regardless. `async` because the request blocks.
///
/// A newer release is also looked up in its `latest.json` (#602); when it can be
/// installed from here, it is kept for [`install_update`] and the answer says so.
#[tauri::command]
async fn check_for_update(
    app: tauri::AppHandle,
    pending: State<'_, update::PendingUpdate>,
    automatic: bool,
) -> Result<Option<update::UpdateCheckDto>, ErrorDto> {
    let handle = app.clone();
    let settings = SETTINGS_QUEUE
        .run(move || Ok(read_settings(&handle)))
        .await?;
    if automatic && !settings.check_updates {
        return Ok(None);
    }
    if !update::self_updating(&app) {
        if automatic {
            return Ok(None);
        }
        return Err(ErrorDto::plain("This build does not update itself."));
    }
    let proxy = settings.proxy.clone();
    let mut found = tauri::async_runtime::spawn_blocking(move || update::check(&proxy))
        .await
        .map_err(shell_error)?
        .map_err(|detail| ErrorDto::plain(format!("Could not check for updates: {detail}")))?;
    let ready = if found.newer {
        update::installable(&app, &settings.proxy, &found.latest).await
    } else {
        None
    };
    found.installable = ready.is_some();
    *pending.0.lock().unwrap() = ready;
    Ok(Some(found))
}

/// Download the update the last check found, check its signature against the
/// key built into the app, install it and restart (#602). On Windows the
/// installer takes over and the app quits for it, so this returns there only
/// with an error; elsewhere too, the update kept for another try. In a folder
/// the user cannot write to, Windows is asked for administrator rights first
/// (#699). An rpm install is installed here, through polkit, because rpm 6
/// refuses the unsigned package the plugin hands it (#787). The frontend
/// refuses to start it while there is anything unapplied.
#[tauri::command]
async fn install_update(
    app: tauri::AppHandle,
    pending: State<'_, update::PendingUpdate>,
) -> Result<(), ErrorDto> {
    let update = pending
        .0
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| ErrorDto::plain("There is no update to install. Check again."))?;
    let mut downloaded: u64 = 0;
    let mut reported: Option<std::time::Instant> = None;
    let bytes = update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                if reported.is_some_and(|at| at.elapsed() < APPLY_PROGRESS_INTERVAL) {
                    return;
                }
                reported = Some(std::time::Instant::now());
                let _ = tauri::Emitter::emit(
                    &app,
                    UPDATE_PROGRESS_EVENT,
                    serde_json::json!({ "downloaded": downloaded, "total": total }),
                );
            },
            || {},
        )
        .await
        .map_err(|error| ErrorDto::plain(format!("Could not install the update: {error}")))?;
    #[cfg(windows)]
    if let Some(dir) = update::protected_install_dir() {
        update::run_installer_elevated(&bytes, &update.version, &dir)
            .map_err(|detail| ErrorDto::plain(format!("Could not install the update: {detail}")))?;
        std::process::exit(0);
    }
    #[cfg(target_os = "linux")]
    if update::installed_from_rpm() {
        let installed = update::install_rpm(&bytes)
            .map_err(|detail| ErrorDto::plain(format!("Could not install the update: {detail}")))?;
        if matches!(installed, update::RpmInstall::Installed) {
            app.restart()
        }
    }
    update
        .install(&bytes)
        .map_err(|error| ErrorDto::plain(format!("Could not install the update: {error}")))?;
    app.restart()
}

/// Open the releases page in the system browser (#435). No argument: the address
/// is fixed here, so nothing the frontend sends can make this open anything else.
#[tauri::command]
async fn open_releases_page() -> Result<(), ErrorDto> {
    BROWSER_QUEUE
        .run(move || open::that(update::RELEASES_PAGE).map_err(ErrorDto::from))
        .await
}

/// The mask guide, linked from the foot of the placeholder reference (#557).
const MASK_GUIDE: &str = "https://github.com/tagrex/tagrex/blob/main/docs/guide/masks.md";

/// Open the mask guide in the system browser (#557). Fixed address, like the
/// releases page: nothing the frontend sends can make this open anything else.
#[tauri::command]
async fn open_mask_guide() -> Result<(), ErrorDto> {
    BROWSER_QUEUE
        .run(move || open::that(MASK_GUIDE).map_err(ErrorDto::from))
        .await
}

/// The places the About dialog links to (#805). The frontend names one of these
/// and the address is chosen here, like the mask guide and the releases page,
/// so nothing it sends can make the app open anything else.
///
/// The project has no website yet. When it has one, add a `Website` variant with
/// its address and a link for it in the dialog (index.html, `#about-modal`).
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
enum AboutLink {
    Repository,
    Guide,
    Releases,
    License,
    Acknowledgements,
}

impl AboutLink {
    fn address(&self) -> &'static str {
        match self {
            AboutLink::Repository => "https://github.com/tagrex/tagrex",
            AboutLink::Guide => "https://github.com/tagrex/tagrex/blob/main/docs/guide/README.md",
            AboutLink::Releases => update::RELEASES_PAGE,
            AboutLink::License => "https://github.com/tagrex/tagrex/blob/main/LICENSE",
            AboutLink::Acknowledgements => {
                "https://github.com/tagrex/tagrex/blob/main/README.md#acknowledgements"
            }
        }
    }
}

/// The menu item and the event that open the About dialog from the macOS app
/// menu (#805), in place of the stock About window that showed only the version.
#[cfg(target_os = "macos")]
const ABOUT_MENU_ID: &str = "about";
#[cfg(target_os = "macos")]
const ABOUT_EVENT: &str = "tagrex://about";

/// The macOS menu bar (#805): the stock one cut down to what the app uses. The
/// app menu's About opens the app's own dialog; Edit stays whole, since the
/// webview's text fields only get Cut, Copy, Paste and Select All through it;
/// Window takes Full Screen from View. File (a second Close Window), View and
/// the empty Help are gone. Linux and Windows have no menu bar, as before.
#[cfg(target_os = "macos")]
fn app_menu(app: &tauri::AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu, WINDOW_SUBMENU_ID};
    let name = app.package_info().name.clone();
    let about = MenuItem::with_id(
        app,
        ABOUT_MENU_ID,
        format!("About {name}"),
        true,
        None::<&str>,
    )?;
    let app_submenu = Submenu::with_items(
        app,
        &name,
        true,
        &[
            &about,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::services(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::show_all(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, None)?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    let window = Submenu::with_id_and_items(
        app,
        WINDOW_SUBMENU_ID,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::maximize(app, None)?,
            &PredefinedMenuItem::fullscreen(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?;
    Menu::with_items(app, &[&app_submenu, &edit, &window])
}

/// Open one of the About dialog's links in the system browser (#805).
#[tauri::command]
async fn open_about_link(link: AboutLink) -> Result<(), ErrorDto> {
    let address = link.address();
    BROWSER_QUEUE
        .run(move || open::that(address).map_err(ErrorDto::from))
        .await
}

/// The version the About dialog shows (#805): the one the app was built as.
#[tauri::command]
fn app_version(app: tauri::AppHandle) -> String {
    app.package_info().version.to_string()
}

/// The name the app was built as (#819): "TagRex", or "TagRex Next" for a
/// build made with the side-by-side overlay (#818), which marks its window.
#[tauri::command]
fn app_name(app: tauri::AppHandle) -> String {
    app.package_info().name.clone()
}

/// The small-word lists a title-case rule can keep lowercase (#537), as
/// `[language, words]` pairs in the core's order, so the rule card offers the
/// same lists the transform applies.
#[tauri::command]
fn small_word_lists() -> Vec<(String, Vec<String>)> {
    tagrex_core::transform::SMALL_WORDS
        .iter()
        .map(|(lang, words)| {
            (
                (*lang).to_string(),
                words.iter().map(|word| (*word).to_string()).collect(),
            )
        })
        .collect()
}

/// Which end of an in-app window's head the close button goes at on this
/// desktop (#565): "start" or "end".
#[tauri::command]
async fn close_button_side() -> Result<&'static str, ErrorDto> {
    BROWSER_QUEUE
        .run(move || Ok(window_controls::close_button_side()))
        .await
}

#[tauri::command]
async fn load_settings(app: tauri::AppHandle) -> Result<SettingsDto, ErrorDto> {
    SETTINGS_QUEUE.run(move || Ok(read_settings(&app))).await
}

/// The shipped preset library (#137). Read-only and not part of settings: the
/// frontend lists these below the user's own saved groups.
#[tauri::command]
fn builtin_action_groups() -> Vec<ActionGroupDto> {
    tagrex_commands::builtin_action_groups()
}

/// Run a preset's rules over one sample string, for the preset editor's live
/// example (#394). Needs no library: it is the rules and the text alone.
#[tauri::command]
async fn transform_sample(
    group: ActionGroupDto,
    text: String,
    target: String,
) -> Result<String, ErrorDto> {
    SAMPLE_QUEUE
        .run(move || {
            tagrex_commands::transform_sample(&group, &text, &target).map_err(ErrorDto::from)
        })
        .await
}

/// Every placeholder a mask accepts (#148), for the in-app reference. Needs no
/// open library — the grammar is the same whether or not one is.
#[tauri::command]
fn mask_placeholders() -> Vec<PlaceholderDto> {
    tagrex_commands::mask_placeholders()
}

/// Every field an online import can write (#152), for the setting that picks
/// which of them it may. Read-only, like the placeholder reference.
#[tauri::command]
fn import_fields() -> Vec<ImportFieldDto> {
    tagrex_commands::import_fields()
}

/// Render a mask-defined column over a batch of files (#150).
#[tauri::command]
async fn render_column(
    state: State<'_, AppState>,
    pattern: String,
    paths: Vec<String>,
) -> Result<Vec<String>, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| {
                app.render_column(&pattern, &paths).map_err(ErrorDto::from)
            })
        })
        .await
}

/// Preview the ticked action groups run in order as one plan (#137).
/// Selection-driven previews may read many files, including remote files.
/// Dispatch off the webview event thread, as with preview_tag_edits.
#[tauri::command]
async fn preview_transform_groups(
    state: State<'_, AppState>,
    paths: Vec<String>,
    groups: Vec<ActionGroupDto>,
    base: Option<Vec<TagEditDto>>,
) -> Result<PlanDto, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            with_app(&state, |app| {
                app.preview_transform_groups_over(&paths, &groups, base.as_deref().unwrap_or(&[]))
                    .map_err(ErrorDto::from)
            })
        })
        .await
}

/// Run the ticked action groups over a staged plan rather than over the files
/// (#142) — cleanup composed into the operation that produced the values.
#[tauri::command]
async fn preview_transform_over_plan(
    state: State<'_, AppState>,
    plan: PlanDto,
    groups: Vec<ActionGroupDto>,
) -> Result<PlanDto, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            with_app(&state, |app| {
                app.preview_transform_over_plan(&plan, &groups)
                    .map_err(ErrorDto::from)
            })
        })
        .await
}

/// Run EDITOR's ticked action groups over the values typed into the field
/// editor, before they are staged (#789).
#[tauri::command]
async fn transform_field_values(
    state: State<'_, AppState>,
    groups: Vec<ActionGroupDto>,
    values: std::collections::BTreeMap<String, String>,
) -> Result<std::collections::BTreeMap<String, String>, ErrorDto> {
    let state = state.inner().clone();
    SESSION_QUEUE
        .run(move || {
            with_app(&state, |app| {
                app.transform_field_values(&groups, &values)
                    .map_err(ErrorDto::from)
            })
        })
        .await
}

#[tauri::command]
async fn save_settings(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    providers: State<'_, ProviderState>,
    settings: SettingsDto,
) -> Result<(), ErrorDto> {
    save_settings_at(
        settings_path(&app)?,
        state.inner().clone(),
        providers.inner().clone(),
        settings,
    )
    .await
}

async fn save_settings_at(
    path: PathBuf,
    state: AppState,
    providers: ProviderState,
    settings: SettingsDto,
) -> Result<(), ErrorDto> {
    let (settings, revision) = SETTINGS_QUEUE
        .run(move || {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(ErrorDto::from)?;
            }
            let json = serde_json::to_string_pretty(&settings).map_err(ErrorDto::from)?;
            write_settings_file(&path, &json).map_err(ErrorDto::from)?;
            // Apply immediately: the network half always (the hub is always there), the
            // library half only when a library is open — so a change takes effect
            // without reopening either way.
            providers.apply_settings(&settings)?;
            let revision = SETTINGS_REVISION.fetch_add(1, Ordering::SeqCst) + 1;
            Ok((settings, revision))
        })
        .await?;
    // Opening a library and adopting parser/write preferences must have one
    // order. Persist/network snapshots can proceed independently, but must not
    // apply to the old session while a new one is still being constructed.
    SESSION_QUEUE
        .run(move || {
            if SETTINGS_REVISION.load(Ordering::SeqCst) == revision {
                if let Some(app) = state.lock().map_err(shell_error)?.as_ref() {
                    app.apply_settings(&settings);
                }
            }
            Ok(())
        })
        .await
}

fn main() {
    // Started by an AI assistant as its MCP server (#828): answer on stdio and
    // exit, before anything of the window (or the single-instance hand-over)
    // starts.
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    if mcp::is_connector(&args) {
        std::process::exit(mcp::connector::run(args.into_iter()));
    }

    let builder = tauri::Builder::default();

    // Windows and Linux start a second process for a second launch; the folder
    // it was given belongs in the window that is already open (#51). macOS
    // routes it to the running instance itself, as the Apple Event below.
    #[cfg(any(windows, target_os = "linux"))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
        // `argv` is the whole command line, the executable included.
        hand_over_paths(
            app,
            paths_from_args(argv.into_iter().skip(1).map(Into::into)),
        );
    }));

    #[cfg(target_os = "macos")]
    let builder = builder.menu(app_menu).on_menu_event(|app, event| {
        if event.id() == ABOUT_MENU_ID {
            let _ = tauri::Emitter::emit(app, ABOUT_EVENT, ());
        }
    });

    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        // Installing a newer release in place (#602); the key and the address of
        // latest.json are under `plugins.updater` in tauri.conf.json.
        .plugin(tauri_plugin_updater::Builder::new().build())
        // Size, position and maximized/fullscreen of the main window (#434).
        // Not DECORATIONS or VISIBLE: the overlay title bar and the first show
        // are the app's to decide. Only "main": the Beatport sign-in window is a
        // one-off and must open at its own size every time.
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::SIZE
                        | tauri_plugin_window_state::StateFlags::POSITION
                        | tauri_plugin_window_state::StateFlags::MAXIMIZED
                        | tauri_plugin_window_state::StateFlags::FULLSCREEN,
                )
                .with_filter(|label| label == "main")
                .build(),
        )
        .manage(AppState::default())
        .manage(ProviderState::default())
        .manage(ApplyControl::default())
        .manage(BpmControl::default())
        .manage(DuplicateControl::default())
        .manage(DigestCache::default())
        .manage(update::PendingUpdate::default())
        .manage(SettingsNotice::default())
        .manage(mcp::McpState::default())
        .manage(mcp::Proposals::default())
        // What the app was started with, waiting for the frontend to ask (#51).
        .manage(PendingOpen::new(paths_from_args(
            std::env::args_os().skip(1),
        )))
        .setup(|app| {
            // The provider hub exists from startup, before any library is
            // opened (#166), so it takes the saved network settings here rather
            // than waiting for a library or a trip through Settings › Save.
            // A damaged file is set aside first (#808), before anything reads
            // it as the defaults and a save overwrites it.
            if let Ok(path) = settings_path(app.handle()) {
                *app.state::<SettingsNotice>().0.lock().unwrap() =
                    set_aside_damaged_settings(&path);
            }
            let settings = read_settings(&app.handle().clone());
            app.state::<ProviderState>()
                .apply_settings(&settings)
                .map_err(|error| std::io::Error::other(error.text))?;
            media_session::install(app.handle());
            mcp::install(app.handle());
            // The title follows the product name, so a TagRex Next build
            // (#818) says which app it is without repeating the window config.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_title(&app.package_info().name);
            }
            Ok(())
        })
        .manage(Player::new())
        .manage(WaveformState::default())
        .invoke_handler(tauri::generate_handler![
            open_library,
            open_drop,
            list_tracks,
            trash_files,
            set_locked_fields,
            locked_fields,
            find_duplicates,
            find_audio_duplicates,
            cancel_find_duplicates,
            open_release_page,
            open_mask_guide,
            open_about_link,
            app_version,
            app_name,
            close_button_side,
            small_word_lists,
            preview_rename,
            preview_tags_from_name,
            probe_tags_from_name,
            probe_tags_from_names,
            probe_text_lines,
            preview_tags_from_text,
            probe_tags_from_text,
            read_text_list,
            suggest_text_list_mask,
            preview_move,
            preview_transform,
            preview_tag_edits,
            preview_cover_embed,
            preview_cover_set,
            export_cover,
            read_cover_summary,
            preview_folder_covers,
            has_folder_cover,
            read_cover_image,
            preview_cover_remove,
            preview_remove_tag_block,
            preview_convert_tag_block,
            tag_block_targets,
            preview_fix_encoding,
            preview_clear_tags,
            export_playlist,
            export_playlists,
            export_cue,
            export_csv,
            export_report,
            export_html,
            export_xml,
            apply_plan,
            cancel_apply,
            detect_bpm,
            cancel_detect_bpm,
            undo,
            redo,
            redo_count,
            history,
            take_journal_notice,
            take_settings_notice,
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
            check_for_update,
            install_update,
            open_releases_page,
            save_settings,
            player_play,
            player_set_next,
            player_pause,
            player_resume,
            player_stop,
            player_seek,
            player_set_volume,
            waveform,
            builtin_action_groups,
            transform_sample,
            mask_placeholders,
            import_fields,
            render_column,
            preview_transform_groups,
            preview_transform_over_plan,
            transform_field_values,
            player_status,
            media_session_update,
            take_launch_paths,
            mcp::mcp_clients,
            mcp::mcp_pair,
            mcp::mcp_revoke,
            mcp::proposals::mcp_proposal_reply,
            mcp::proposals::mcp_proposal_outcome
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, _event| {
            // macOS does not pass an opened folder as an argument. A double
            // click, an "Open With", a folder dropped on the Dock icon and a
            // second `open` of the bundle all arrive here, as one Apple Event
            // delivered to the running instance (#51).
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Opened { urls } = _event {
                let paths = urls
                    .iter()
                    .filter_map(|url| url.to_file_path().ok())
                    .collect();
                hand_over_paths(_app, paths);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::{paths_from_args, release_url};

    #[test]
    fn saved_preferences_reach_a_library_that_is_still_opening() {
        use std::sync::{mpsc, Arc, Mutex};
        use std::time::{Duration, Instant, SystemTime};
        let root = std::env::temp_dir().join(format!(
            "tagrex-settings-open-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let state: super::AppState = Arc::new(Mutex::new(None));
        let providers: super::ProviderState = Arc::new(super::ProviderQueues::default());
        let (started, ready) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let opening = tauri::async_runtime::spawn({
            let state = state.clone();
            let root = root.clone();
            async move {
                super::SESSION_QUEUE
                    .run(move || {
                        started.send(()).unwrap();
                        wait.recv_timeout(Duration::from_secs(10)).unwrap();
                        let app = super::App::open(&root, &root.join("journal.sqlite")).unwrap();
                        app.apply_settings(&super::SettingsDto::default());
                        *state.lock().unwrap() = Some(app);
                        Ok(())
                    })
                    .await
            }
        });
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        let saving = tauri::async_runtime::spawn(super::save_settings_at(
            root.join("settings.json"),
            state.clone(),
            providers.clone(),
            super::SettingsDto {
                bpm_max_minutes: 7,
                ..Default::default()
            },
        ));
        let deadline = Instant::now() + Duration::from_secs(5);
        while providers.settings.lock().unwrap().bpm_max_minutes != 7 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let persisted = providers.settings.lock().unwrap().bpm_max_minutes == 7;
        release.send(()).unwrap();
        tauri::async_runtime::block_on(opening).unwrap().unwrap();
        tauri::async_runtime::block_on(saving).unwrap().unwrap();
        assert!(persisted);
        let actual = state.lock().unwrap().as_ref().unwrap().bpm_max_length();
        drop(state);
        std::fs::remove_dir_all(root).unwrap();
        assert_eq!(actual, Some(Duration::from_secs(7 * 60)));
    }

    fn scratch_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tagrex-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_damaged_settings_file_is_set_aside_not_overwritten() {
        let dir = scratch_dir("settings-damaged");
        let path = dir.join("settings.json");
        // A save cut short: half a JSON object.
        std::fs::write(&path, "{\"proxy\": \"socks5://h").unwrap();
        let aside = super::set_aside_damaged_settings(&path).expect("set aside");
        assert!(!path.exists(), "the damaged file is moved away");
        let name = aside.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("settings.json.corrupt-"), "{name}");
        assert_eq!(
            std::fs::read_to_string(&aside).unwrap(),
            "{\"proxy\": \"socks5://h"
        );
        // An empty file, the other shape a crash leaves, is damaged too.
        std::fs::write(&path, "").unwrap();
        assert!(super::set_aside_damaged_settings(&path).is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sound_or_missing_settings_are_left_alone() {
        let dir = scratch_dir("settings-sound");
        let path = dir.join("settings.json");
        assert_eq!(super::set_aside_damaged_settings(&path), None);
        // An older file without most fields still parses: it is not damaged.
        std::fs::write(&path, "{\"proxy\": \"\"}").unwrap();
        assert_eq!(super::set_aside_damaged_settings(&path), None);
        assert!(path.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn settings_are_replaced_whole_through_a_temporary_file() {
        let dir = scratch_dir("settings-write");
        let path = dir.join("settings.json");
        super::write_settings_file(&path, "{\"proxy\": \"first\"}").unwrap();
        super::write_settings_file(&path, "{\"proxy\": \"second\"}").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{\"proxy\": \"second\"}"
        );
        assert!(!dir.join("settings.json.tmp").exists(), "no temporary left");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn provider_sources_keep_independent_persistent_queues() {
        let providers = super::ProviderQueues::default();
        let first = providers.source("discogs").unwrap();
        assert!(std::sync::Arc::ptr_eq(
            &first,
            &providers.source("discogs").unwrap()
        ));
        assert!(!std::sync::Arc::ptr_eq(
            &first,
            &providers.source("musicbrainz").unwrap()
        ));
        assert!(providers.source("unknown").is_err());
    }

    #[test]
    fn saving_network_preferences_does_not_wait_for_inflight_http() {
        let providers = std::sync::Arc::new(super::ProviderQueues::default());
        let source = providers.source("discogs").unwrap();
        // Represents a provider request holding its hub during HTTP/throttling.
        let inflight = source.hub.lock().unwrap();
        let (finished, received) = std::sync::mpsc::channel();
        let worker = std::thread::spawn({
            let providers = providers.clone();
            move || {
                let settings = super::SettingsDto {
                    rate_limit_per_min: 15,
                    ..Default::default()
                };
                providers.apply_settings(&settings).unwrap();
                finished.send(()).unwrap();
            }
        });
        let ready = received
            .recv_timeout(std::time::Duration::from_secs(2))
            .is_ok();
        drop(inflight);
        worker.join().unwrap();
        assert!(
            ready,
            "saving settings waited for an in-flight provider request"
        );
        assert_eq!(source.settings.lock().unwrap().rate_limit_per_min, 15);
    }

    #[test]
    fn a_command_line_yields_the_paths_in_it_and_nothing_else() {
        let dir = std::env::temp_dir().join(format!("tagrex-launch-args-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // What Finder adds when it starts the app is not a folder, and neither
        // is a path that is not there.
        let argv = [
            "-psn_0_774163".to_string(),
            dir.to_string_lossy().into_owned(),
            dir.join("no-such-folder").to_string_lossy().into_owned(),
        ];
        assert_eq!(
            paths_from_args(argv.into_iter().map(Into::into)),
            vec![dir.clone()]
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

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

    /// A credential file is readable by its owner alone, and a file an older
    /// version wrote world-readable is tightened when it is saved again.
    #[cfg(unix)]
    #[test]
    fn a_secret_is_written_readable_by_its_owner_alone() {
        use super::write_secret;
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!("tagrex-secret-{}", std::process::id()));
        std::fs::write(&path, "old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        write_secret(&path, "token").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        let contents = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(mode, 0o600);
        assert_eq!(contents, "token");

        // A new file starts that way too.
        let fresh = std::env::temp_dir().join(format!("tagrex-secret-new-{}", std::process::id()));
        std::fs::remove_file(&fresh).ok();
        write_secret(&fresh, "t").unwrap();
        let mode = std::fs::metadata(&fresh).unwrap().permissions().mode() & 0o777;
        std::fs::remove_file(&fresh).ok();
        assert_eq!(mode, 0o600);
    }

    /// A credential file an older version left readable by others is
    /// tightened when it is read, and its contents are left alone (#675).
    #[cfg(unix)]
    #[test]
    fn a_secret_left_readable_is_tightened_on_read() {
        use super::tighten_secret;
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!("tagrex-secret-old-{}", std::process::id()));
        std::fs::write(&path, "token").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        tighten_secret(&path);
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        let contents = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(mode, 0o600);
        assert_eq!(contents, "token");
        // A missing file is no error.
        tighten_secret(&path);
    }

    /// A 16-bit mono PCM WAV of `samples` at 8 kHz, with `extra` chunks after
    /// the data, as a tagger appends a LIST chunk.
    fn wav(samples: &[i16], extra: &[u8]) -> Vec<u8> {
        let data_len = (samples.len() * 2) as u32;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len + extra.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        for field in [16u32, 1 | (1 << 16), 8_000, 16_000, 2 | (16 << 16)] {
            out.extend_from_slice(&field.to_le_bytes());
        }
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for sample in samples {
            out.extend_from_slice(&sample.to_le_bytes());
        }
        out.extend_from_slice(extra);
        out
    }

    // #821: the same samples under other tags make a group; a file that only
    // shares the opening seconds passes the quick pass and is dropped by the
    // full one; a file that will not decode is reported, not left out quietly.
    #[test]
    fn identical_audio_is_grouped_and_a_shared_opening_is_not() {
        use super::{audio_duplicates, DuplicateFileDto};
        use std::sync::atomic::AtomicBool;
        use std::sync::Mutex;

        let dir = std::env::temp_dir().join(format!("tagrex-audio-dups-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let tone = |step: i32| -> Vec<i16> {
            (0..8_000 * 12)
                .map(|i| ((i * step) % 16_000 - 8_000) as i16)
                .collect()
        };
        let base = tone(37);
        let mut opening = base.clone();
        opening[8_000 * 11..].copy_from_slice(&tone(41)[8_000 * 11..]);
        let list = b"LIST\x0c\x00\x00\x00INFOINAM\x00\x00\x00\x00";
        let files = [
            ("plain.wav", wav(&base, &[])),
            ("tagged.wav", wav(&base, list)),
            ("opening.wav", wav(&opening, &[])),
            ("other.wav", wav(&tone(43), &[])),
            ("broken.wav", b"RIFF\x04\x00\x00\x00WAVE".to_vec()),
        ];
        for (name, bytes) in &files {
            std::fs::write(dir.join(name), bytes).unwrap();
        }
        let candidates = || -> Vec<DuplicateFileDto> {
            files
                .iter()
                .map(|(name, bytes)| DuplicateFileDto {
                    path: dir.join(name).to_string_lossy().into_owned(),
                    artist: String::new(),
                    title: String::new(),
                    album: String::new(),
                    duration_secs: 12,
                    size_bytes: bytes.len() as u64,
                    bitrate_kbps: None,
                })
                .collect()
        };

        let heard = Mutex::new(Vec::new());
        let cache = Mutex::new(Default::default());
        let found = audio_duplicates(
            candidates(),
            &AtomicBool::new(false),
            &cache,
            &|stage, done, total| heard.lock().unwrap().push((stage, done, total)),
        );
        let names: Vec<Vec<String>> = found
            .groups
            .iter()
            .map(|group| {
                group
                    .files
                    .iter()
                    .map(|file| {
                        file.path
                            .rsplit(std::path::MAIN_SEPARATOR)
                            .next()
                            .unwrap()
                            .to_string()
                    })
                    .collect()
            })
            .collect();
        assert_eq!(names, [["plain.wav", "tagged.wav"]]);
        assert_eq!(found.failed.len(), 1);
        assert!(found.failed[0].ends_with("broken.wav"));
        assert!(!found.cancelled);
        let heard = heard.into_inner().unwrap();
        // All five opened, and only the three sharing an opening decoded in full.
        assert!(heard.contains(&("quick", 5, 5)));
        assert!(heard.contains(&("full", 3, 3)));
        assert!(!heard
            .iter()
            .any(|(stage, _, total)| *stage == "full" && *total != 3));

        assert_eq!(found.reused, 0);

        // #823: a second scan takes the digests from the cache (four quick,
        // as the broken file is tried again, and three full) and finds the same.
        let again = audio_duplicates(candidates(), &AtomicBool::new(false), &cache, &|_, _, _| {});
        assert_eq!(again.groups, found.groups);
        assert_eq!(again.reused, 7);
        // A file written since is decoded again, and still matches.
        std::fs::write(
            dir.join("tagged.wav"),
            wav(&base, b"LIST\x04\x00\x00\x00INFO"),
        )
        .unwrap();
        let changed =
            audio_duplicates(candidates(), &AtomicBool::new(false), &cache, &|_, _, _| {});
        assert_eq!(changed.groups.len(), 1);
        assert_eq!(changed.reused, 5);

        // A scan stopped before it starts confirms nothing and says so.
        let stopped = audio_duplicates(
            vec![found.groups[0].files[0].clone()],
            &AtomicBool::new(true),
            &Mutex::new(Default::default()),
            &|_, _, _| {},
        );
        assert!(stopped.cancelled && stopped.groups.is_empty() && stopped.failed.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
