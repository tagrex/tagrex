//! Manual check: scan a real directory and report what was found.
//!
//! Usage: `cargo run -p tagrex-core --example scan -- /path/to/library`

use std::path::PathBuf;
use std::time::Instant;

use tagrex_core::scanner::{scan, ScanOptions};

fn main() {
    let Some(root) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: scan <path-to-directory>");
        std::process::exit(1);
    };

    let started = Instant::now();
    let mut count = 0usize;
    let mut errors = 0usize;

    for entry in scan(&root, &ScanOptions::default()) {
        match entry {
            Ok(path) => {
                count += 1;
                if count <= 10 {
                    println!("{}", path.display());
                }
            }
            Err(err) => {
                errors += 1;
                eprintln!("scan error: {err}");
            }
        }
    }

    if count > 10 {
        println!("... and {} more", count - 10);
    }
    println!(
        "found {count} supported files, {errors} errors, in {:?}",
        started.elapsed()
    );
}
