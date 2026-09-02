//! A C ABI over the core, for the native shell spikes (#271).
//!
//! The spike shell reads a folder, edits tags, and writes them back through the
//! core's own change plan — the same `ChangePlan` / `plan::apply` / journal path
//! the app uses, so a write here is gated and undoable exactly as it is there.
//!
//! JSON rather than a struct layout on purpose: three shells in three languages
//! have to agree about the shape, and a listing of a few thousand rows is not
//! where this app spends its time. When a spike grows into something real, this
//! is the file that gets replaced by a typed bridge.

use std::collections::BTreeMap;
use std::ffi::{c_char, CStr, CString};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tagrex_core::journal::{SqliteJournal, UndoJournal};
use tagrex_core::model::{TagEngine, TagField};
use tagrex_core::plan::{ChangePlan, Executor, FieldChange, FileChange};
use tagrex_core::scanner::{scan, ScanOptions};

/// One table row, named the way the columns are named in the UI.
#[derive(Serialize)]
struct Row {
    path: String,
    file: String,
    format: String,
    artist: String,
    title: String,
    album: String,
    albumartist: String,
    year: String,
    genre: String,
    track: String,
    duration_secs: u64,
    bitrate_kbps: Option<u32>,
}

#[derive(Serialize)]
struct Library {
    root: String,
    rows: Vec<Row>,
    /// Files the scanner found but the reader could not open, with the reason.
    errors: Vec<String>,
}

fn field(track: &tagrex_core::model::TrackFile, field: TagField) -> String {
    track.tags.get(&field).cloned().unwrap_or_default()
}

fn read_library(root: &Path) -> Library {
    let mut rows = Vec::new();
    let mut errors = Vec::new();

    for entry in scan(root, &ScanOptions::default()) {
        let path: PathBuf = match entry {
            Ok(path) => path,
            Err(err) => {
                errors.push(format!("{err}"));
                continue;
            }
        };

        match TagEngine::read_with_props(&path) {
            Ok(read) => {
                let file = &read.file;
                rows.push(Row {
                    path: file.path.display().to_string(),
                    file: file
                        .path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    format: format!("{:?}", file.format),
                    artist: field(file, TagField::Artist),
                    title: field(file, TagField::Title),
                    album: field(file, TagField::Album),
                    albumartist: field(file, TagField::AlbumArtist),
                    year: field(file, TagField::Year),
                    genre: field(file, TagField::Genre),
                    track: field(file, TagField::TrackNumber),
                    duration_secs: read.props.duration_secs,
                    bitrate_kbps: read.props.bitrate_kbps,
                });
            }
            Err(err) => errors.push(format!("{}: {err}", path.display())),
        }
    }

    Library {
        root: root.display().to_string(),
        rows,
        errors,
    }
}

/// Scan `root` and return the library as a JSON string.
///
/// The caller owns the result and must hand it back to `tagrex_string_free`.
/// A null or non-UTF-8 path, or a serialization failure, answers with a JSON
/// object carrying an `error` key rather than a null pointer, so every shell has
/// exactly one thing to parse.
///
/// # Safety
///
/// `root` must be a valid, NUL-terminated C string, or null.
#[no_mangle]
pub unsafe extern "C" fn tagrex_scan_json(root: *const c_char) -> *mut c_char {
    let out = match cstr_to_path(root) {
        Ok(path) => serde_json::to_string(&read_library(&path))
            .unwrap_or_else(|err| error_json(&format!("serialize: {err}"))),
        Err(message) => error_json(&message),
    };

    // The string is built here and freed by tagrex_string_free; a NUL inside it
    // is impossible, since serde_json never emits one unescaped.
    CString::new(out)
        .unwrap_or_else(|_| CString::new(error_json("interior NUL")).expect("static json"))
        .into_raw()
}

/// Free a string handed out by this library.
///
/// # Safety
///
/// `ptr` must be a pointer returned by `tagrex_scan_json`, and must not be used
/// afterwards. Null is accepted and ignored.
#[no_mangle]
pub unsafe extern "C" fn tagrex_string_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}

unsafe fn cstr_to_path(root: *const c_char) -> Result<PathBuf, String> {
    if root.is_null() {
        return Err("null path".to_string());
    }
    CStr::from_ptr(root)
        .to_str()
        .map(PathBuf::from)
        .map_err(|_| "path is not UTF-8".to_string())
}

fn error_json(message: &str) -> String {
    serde_json::json!({ "root": "", "rows": [], "errors": [message] }).to_string()
}

// ---------------------------------------------------------------- write path

/// One file's edits, as the shell states them: storage key to new value.
#[derive(Deserialize)]
struct EditRequest {
    path: String,
    fields: BTreeMap<String, String>,
}

/// Everything a write needs. `root` is the authorization — `plan::apply`
/// refuses to touch anything outside it — and `journal` is where the batch is
/// recorded so it can be undone.
#[derive(Deserialize)]
struct ApplyRequest {
    root: String,
    journal: String,
    #[serde(default)]
    description: String,
    edits: Vec<EditRequest>,
}

#[derive(Deserialize)]
struct UndoRequest {
    root: String,
    journal: String,
}

#[derive(Serialize, Default)]
struct WriteResult {
    /// Files the batch actually touched.
    applied: usize,
    /// The journal id, for the undo that follows.
    batch: Option<i64>,
    description: String,
    errors: Vec<String>,
}

/// Turn the shell's edits into a plan, reading each file for the old side.
///
/// The old value has to come from disk rather than from the table the shell is
/// showing: between the scan and the Apply the file may have changed, and the
/// core's staleness check is what catches that — but only if the plan says what
/// the shell believed the file held.
fn build_plan(request: &ApplyRequest) -> Result<ChangePlan, String> {
    let mut changes = Vec::new();

    for edit in &request.edits {
        let path = PathBuf::from(&edit.path);
        let current = TagEngine::read(&path).map_err(|err| format!("{}: {err}", path.display()))?;

        let mut tag_changes = Vec::new();
        for (key, value) in &edit.fields {
            let field = TagField::from_storage_key(key);
            let old = current.tags.get(&field).cloned();
            let new = (!value.is_empty()).then(|| value.clone());

            // A field the edit did not actually change is not a change.
            if old.as_deref().unwrap_or_default() == new.as_deref().unwrap_or_default() {
                continue;
            }
            tag_changes.push(FieldChange { field, old, new });
        }

        if !tag_changes.is_empty() {
            changes.push(FileChange {
                path,
                tag_changes,
                ..Default::default()
            });
        }
    }

    Ok(ChangePlan {
        description: if request.description.is_empty() {
            "Edit tags".to_string()
        } else {
            request.description.clone()
        },
        changes,
        ..Default::default()
    })
}

fn apply(request: &ApplyRequest) -> WriteResult {
    let plan = match build_plan(request) {
        Ok(plan) => plan,
        Err(message) => {
            return WriteResult {
                errors: vec![message],
                ..Default::default()
            }
        }
    };

    if plan.is_empty() {
        return WriteResult {
            description: "nothing to write".to_string(),
            ..Default::default()
        };
    }

    let mut journal = match SqliteJournal::open(Path::new(&request.journal)) {
        Ok(journal) => journal,
        Err(err) => {
            return WriteResult {
                errors: vec![format!("journal: {err}")],
                ..Default::default()
            }
        }
    };

    let roots = vec![PathBuf::from(&request.root)];
    match Executor::apply(&plan, &mut journal, &roots) {
        Ok(batch) => WriteResult {
            applied: plan.file_count(),
            batch: Some(batch.id.0),
            description: plan.description,
            errors: Vec::new(),
        },
        Err(err) => WriteResult {
            errors: vec![format!("{err}")],
            ..Default::default()
        },
    }
}

fn undo(request: &UndoRequest) -> WriteResult {
    let mut journal = match SqliteJournal::open(Path::new(&request.journal)) {
        Ok(journal) => journal,
        Err(err) => {
            return WriteResult {
                errors: vec![format!("journal: {err}")],
                ..Default::default()
            }
        }
    };

    // Newest first, so the head of the list is the batch to take back.
    let batches = match journal.batches() {
        Ok(batches) => batches,
        Err(err) => {
            return WriteResult {
                errors: vec![format!("journal: {err}")],
                ..Default::default()
            }
        }
    };

    let Some(batch) = batches.into_iter().next() else {
        return WriteResult {
            description: "nothing to undo".to_string(),
            ..Default::default()
        };
    };

    let roots = vec![PathBuf::from(&request.root)];
    match Executor::undo(&mut journal, batch.id, &roots) {
        Ok(()) => WriteResult {
            applied: batch.plan.changes.len(),
            batch: Some(batch.id.0),
            description: batch.description,
            errors: Vec::new(),
        },
        Err(err) => WriteResult {
            errors: vec![format!("{err}")],
            ..Default::default()
        },
    }
}

/// Write a set of tag edits, as one journaled batch.
///
/// # Safety
///
/// `request` must be a valid, NUL-terminated C string holding the JSON above.
#[no_mangle]
pub unsafe extern "C" fn tagrex_apply_json(request: *const c_char) -> *mut c_char {
    into_c_string(match parse_request::<ApplyRequest>(request) {
        Ok(request) => serde_json::to_string(&apply(&request))
            .unwrap_or_else(|err| error_json(&format!("serialize: {err}"))),
        Err(message) => error_json(&message),
    })
}

/// Take back the most recent batch in the journal.
///
/// # Safety
///
/// `request` must be a valid, NUL-terminated C string holding the JSON above.
#[no_mangle]
pub unsafe extern "C" fn tagrex_undo_json(request: *const c_char) -> *mut c_char {
    into_c_string(match parse_request::<UndoRequest>(request) {
        Ok(request) => serde_json::to_string(&undo(&request))
            .unwrap_or_else(|err| error_json(&format!("serialize: {err}"))),
        Err(message) => error_json(&message),
    })
}

unsafe fn parse_request<T: serde::de::DeserializeOwned>(raw: *const c_char) -> Result<T, String> {
    if raw.is_null() {
        return Err("null request".to_string());
    }
    let text = CStr::from_ptr(raw)
        .to_str()
        .map_err(|_| "request is not UTF-8".to_string())?;
    serde_json::from_str(text).map_err(|err| format!("request: {err}"))
}

fn into_c_string(text: String) -> *mut c_char {
    CString::new(text)
        .unwrap_or_else(|_| CString::new(error_json("interior NUL")).expect("static json"))
        .into_raw()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_folder_reads_as_an_empty_library_rather_than_a_failure() {
        let library = read_library(Path::new("/definitely/not/here"));
        assert!(library.rows.is_empty());
    }

    #[test]
    fn a_null_path_answers_with_json_carrying_the_reason() {
        let ptr = unsafe { tagrex_scan_json(std::ptr::null()) };
        let text = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
        unsafe { tagrex_string_free(ptr) };
        assert!(text.contains("null path"), "{text}");
    }
}
