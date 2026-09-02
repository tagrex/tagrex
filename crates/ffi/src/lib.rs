//! A C ABI over the core, for the native shell spikes (#271).
//!
//! The spikes are read-only stands: they scan a folder, read tags, and draw a
//! table. Nothing here writes, so the whole surface is two calls — one that
//! answers with JSON, one that gives the string back.
//!
//! JSON rather than a struct layout on purpose: three shells in three languages
//! have to agree about the shape, and a listing of a few thousand rows is not
//! where this app spends its time. When a spike grows into something real, this
//! is the file that gets replaced by a typed bridge.

use std::ffi::{c_char, CStr, CString};
use std::path::{Path, PathBuf};

use serde::Serialize;
use tagrex_core::model::{TagEngine, TagField};
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
