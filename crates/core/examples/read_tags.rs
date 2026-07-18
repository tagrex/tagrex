//! Read-only manual check: print the tags TagEngine sees for a real file.
//!
//! Usage: `cargo run -p tagrex-core --example read_tags -- /path/to/track.mp3`

use std::path::PathBuf;

use tagrex_core::model::TagEngine;

fn main() {
    let Some(path) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: read_tags <path-to-audio-file>");
        std::process::exit(1);
    };

    match TagEngine::read(&path) {
        Ok(track) => {
            println!("{}", track.path.display());
            println!("format: {:?}", track.format);
            if track.tags.is_empty() {
                println!("(no tags found)");
            }
            for (field, value) in &track.tags {
                println!("{field:?}: {value}");
            }
        }
        Err(err) => {
            eprintln!("failed to read {}: {err}", path.display());
            std::process::exit(1);
        }
    }
}
