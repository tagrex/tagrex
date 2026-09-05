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

        // The non-interactive Beatport pieces: status, begin (scrapes the
        // client id and builds the authorize URL), logout.
        //   probe beatport <folder>
        "beatport" => {
            let before = invoke(handle, "beatport_status", &serde_json::json!({}));
            println!("status before: {}", before["ok"]);
            let begin = invoke(handle, "beatport_begin", &serde_json::json!({}));
            match begin.get("ok") {
                Some(ok) => {
                    let url = ok["authorize_url"].as_str().unwrap_or("");
                    println!(
                        "authorize_url has client_id: {}",
                        url.contains("client_id=")
                    );
                }
                None => println!("begin error: {}", begin["error"]["text"]),
            }
            let out = invoke(handle, "beatport_logout", &serde_json::json!({}));
            println!("logout: {out}");
        }

        // Play a track, read the status back, and compute its waveform.
        //   probe play <folder> <file>
        "play" => {
            let file = args.get(2).cloned().unwrap_or_default();
            invoke(handle, "player_play", &serde_json::json!({ "path": file }));
            std::thread::sleep(std::time::Duration::from_millis(700));
            let status = invoke(handle, "player_status", &serde_json::json!({}));
            println!("status: {}", status["ok"]);
            let wave = invoke(handle, "waveform", &serde_json::json!({ "path": file }));
            let buckets = wave["ok"].as_array().map(|a| a.len()).unwrap_or(0);
            println!("waveform buckets: {buckets}");
            invoke(handle, "player_stop", &serde_json::json!({}));
        }

        // A live provider search. MusicBrainz needs no token.
        //   probe search <folder> <source> <artist> <album>
        "search" => {
            let source = args.get(2).cloned().unwrap_or_else(|| "musicbrainz".into());
            let artist = args.get(3).cloned().unwrap_or_default();
            let album = args.get(4).cloned().unwrap_or_default();
            let result = invoke(
                handle,
                "provider_search",
                &serde_json::json!({
                    "source": source,
                    "token": "",
                    "query": { "artist": artist, "album": album },
                }),
            );
            println!("{result}");
        }

        // Round-trip settings and the token, then confirm a search still runs
        // with the settings applied to the hub.
        //   probe settings <folder>
        "settings" => {
            let saved = invoke(
                handle,
                "save_settings",
                &serde_json::json!({ "settings": { "rate_limit_per_min": 30, "id3_v23": true } }),
            );
            println!("save_settings: {saved}");
            let loaded = invoke(handle, "load_settings", &serde_json::json!({}));
            println!(
                "load_settings rate_limit_per_min: {}",
                loaded["ok"]["rate_limit_per_min"]
            );

            let token_set = invoke(
                handle,
                "save_discogs_token",
                &serde_json::json!({ "token": "  probe-token-123  " }),
            );
            println!("save_discogs_token: {token_set}");
            let token = invoke(handle, "saved_discogs_token", &serde_json::json!({}));
            println!("saved_discogs_token: {}", token["ok"]);

            let search = invoke(
                handle,
                "provider_search",
                &serde_json::json!({
                    "source": "musicbrainz", "token": "",
                    "query": { "artist": "Air", "album": "Moon Safari" },
                }),
            );
            let count = search["ok"].as_array().map(|a| a.len()).unwrap_or(0);
            println!("search after settings: {count} candidates");
        }

        // Pull a full release by id.
        //   probe fetch <folder> <source> <release_id>
        "fetch" => {
            let source = args.get(2).cloned().unwrap_or_else(|| "musicbrainz".into());
            let release_id = args.get(3).cloned().unwrap_or_default();
            let result = invoke(
                handle,
                "provider_fetch_release",
                &serde_json::json!({ "source": source, "token": "", "release_id": release_id }),
            );
            println!("{result}");
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
