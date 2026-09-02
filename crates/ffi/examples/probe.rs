//! Manual check of the C ABI the spike shells read through (#271).
//!
//! Exercises the bridge the way a shell does — scan, write a batch, take it
//! back — without a UI in the way.
//!
//!   cargo run -q -p tagrex-ffi --example probe -- scan  <folder>
//!   cargo run -q -p tagrex-ffi --example probe -- apply <folder> <file> artist=New title=Other
//!   cargo run -q -p tagrex-ffi --example probe -- undo  <folder>
//!
//! Run it on a disposable copy. It writes.

use std::ffi::{CStr, CString};

fn call(
    f: unsafe extern "C" fn(*const std::ffi::c_char) -> *mut std::ffi::c_char,
    arg: &str,
) -> String {
    let input = CString::new(arg).expect("no interior NUL");
    unsafe {
        let raw = f(input.as_ptr());
        let text = CStr::from_ptr(raw).to_string_lossy().into_owned();
        tagrex_ffi::tagrex_string_free(raw);
        text
    }
}

fn journal_for(folder: &str) -> String {
    format!("{folder}/.tagrex-spike-journal.sqlite")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        eprintln!("usage: probe <scan|apply|undo> <folder> [...]");
        std::process::exit(2);
    };
    let folder = args.get(1).cloned().unwrap_or_default();

    match command {
        "scan" => println!("{}", call(tagrex_ffi::tagrex_scan_json, &folder)),

        "apply" => {
            let file = args.get(2).cloned().unwrap_or_default();
            let mut fields = serde_json::Map::new();
            for pair in args.iter().skip(3) {
                let (key, value) = pair.split_once('=').expect("field=value");
                fields.insert(
                    key.to_string(),
                    serde_json::Value::String(value.to_string()),
                );
            }

            let request = serde_json::json!({
                "root": folder,
                "journal": journal_for(&folder),
                "description": "Edit tags (probe)",
                "edits": [{ "path": file, "fields": fields }],
            });
            println!(
                "{}",
                call(tagrex_ffi::tagrex_apply_json, &request.to_string())
            );
        }

        "undo" => {
            let request = serde_json::json!({
                "root": folder,
                "journal": journal_for(&folder),
            });
            println!(
                "{}",
                call(tagrex_ffi::tagrex_undo_json, &request.to_string())
            );
        }

        other => {
            eprintln!("unknown command: {other}");
            std::process::exit(2);
        }
    }
}
