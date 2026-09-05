//! Manual check of the C ABI the spike shells read through (#271, #293).
//!
//! Exercises the bridge the way a shell does — open a session, then invoke
//! commands by name — without a UI in the way.
//!
//!   cargo run -q -p tagrex-ffi --example probe -- scan  <folder>
//!   cargo run -q -p tagrex-ffi --example probe -- edit  <folder> <file> artist=New title=Other
//!   cargo run -q -p tagrex-ffi --example probe -- undo  <folder>
//!
//! Run `edit`/`undo` on a disposable copy. They write.

use std::ffi::{c_char, CStr, CString};

use tagrex_ffi::{tagrex_close, tagrex_invoke, tagrex_open, tagrex_string_free, Session};

/// The config dir (and so the journal) lives beside the folder under test, so a
/// probe run leaves nothing in the real app's config.
fn config_for(folder: &str) -> String {
    format!("{folder}/.tagrex-probe")
}

fn take(raw: *mut c_char) -> serde_json::Value {
    unsafe {
        let text = CStr::from_ptr(raw).to_string_lossy().into_owned();
        tagrex_string_free(raw);
        serde_json::from_str(&text).expect("the ABI answers with JSON")
    }
}

fn open(folder: &str) -> *mut Session {
    let root = CString::new(folder).expect("no interior NUL");
    let config = CString::new(config_for(folder)).expect("no interior NUL");
    let mut handle: *mut Session = std::ptr::null_mut();
    let reply = take(unsafe { tagrex_open(root.as_ptr(), config.as_ptr(), &mut handle) });
    if let Some(error) = reply.get("error") {
        eprintln!("open: {error}");
        std::process::exit(1);
    }
    handle
}

fn invoke(handle: *mut Session, cmd: &str, args: &serde_json::Value) -> serde_json::Value {
    let cmd_c = CString::new(cmd).expect("no interior NUL");
    let args_c = CString::new(args.to_string()).expect("no interior NUL");
    take(unsafe { tagrex_invoke(handle, cmd_c.as_ptr(), args_c.as_ptr()) })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        eprintln!("usage: probe <scan|edit|undo> <folder> [...]");
        std::process::exit(2);
    };
    let folder = args.get(1).cloned().unwrap_or_default();
    let handle = open(&folder);

    match command {
        "scan" => {
            let listed = invoke(handle, "list_tracks", &serde_json::json!({}));
            println!("{listed}");
        }

        // Stage the edits into a plan, then apply that plan — the same two steps
        // the interface takes, so the write goes through the gate.
        "edit" => {
            let file = args.get(2).cloned().unwrap_or_default();
            let edits: Vec<serde_json::Value> = args
                .iter()
                .skip(3)
                .map(|pair| {
                    let (key, value) = pair.split_once('=').expect("field=value");
                    serde_json::json!({ "path": file, "field": key, "value": value })
                })
                .collect();

            let plan = invoke(
                handle,
                "preview_tag_edits",
                &serde_json::json!({ "edits": edits }),
            );
            match plan.get("ok") {
                Some(plan) => {
                    let applied =
                        invoke(handle, "apply_plan", &serde_json::json!({ "plan": plan }));
                    println!("{applied}");
                }
                None => println!("{plan}"),
            }
        }

        // Take back the newest batch in the journal.
        "undo" => {
            let history = invoke(handle, "history", &serde_json::json!({}));
            let newest = history
                .get("ok")
                .and_then(|h| h.as_array())
                .and_then(|batches| batches.first())
                .and_then(|batch| batch.get("id"))
                .and_then(serde_json::Value::as_i64);
            match newest {
                Some(id) => {
                    let undone = invoke(handle, "undo", &serde_json::json!({ "batch_id": id }));
                    println!("{undone}");
                }
                None => println!("nothing to undo"),
            }
        }

        other => {
            eprintln!("unknown command: {other}");
            unsafe { tagrex_close(handle) };
            std::process::exit(2);
        }
    }

    unsafe { tagrex_close(handle) };
}
