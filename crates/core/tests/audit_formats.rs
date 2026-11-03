//! Audit harness over real files of every format (made by an outside encoder):
//! foreign tags read, edge values round-trip with every other field kept, a
//! no-op save, and heavily edited and cleared copies written out so the decoded
//! audio can be compared outside (ffmpeg -f md5).
//! Ignored by default. Run:
//! TAGREX_AUDIT_IN=<dir> TAGREX_AUDIT_OUT=<dir> cargo test -p tagrex-core --test audit_formats -- --ignored --nocapture

use std::path::{Path, PathBuf};

use tagrex_core::model::{TagEngine, TagField, TagMap};

fn values() -> Vec<(TagField, &'static str)> {
    vec![
        (TagField::Title, "Ünïcödé 日本語 🎵 Группа крови"),
        (TagField::Artist, "A; B; C"),
        (TagField::Album, "line1\nline2"),
        (TagField::Comment, " padded "),
        (TagField::Genre, "Electronic; IDM"),
        (TagField::TrackNumber, "7"),
        (TagField::TrackTotal, "12"),
        (TagField::DiscNumber, "2"),
        (TagField::Year, "1996-05-01"),
        (TagField::Bpm, "128"),
        (TagField::InitialKey, "Am"),
        (TagField::Isrc, "GBAYE0601498"),
        (TagField::Publisher, "Label ✓"),
        (TagField::Composer, "Композитор"),
        (TagField::Custom("TAGREX_AUDIT".to_string()), "custom ü"),
        (
            TagField::Title,
            &*Box::leak("x".repeat(5000).into_boxed_str()),
        ),
    ]
}

fn copy_to(src: &Path, dir: &Path, label: &str) -> PathBuf {
    let name = src.file_name().unwrap().to_string_lossy();
    let (stem, ext) = name.split_once('.').unwrap();
    let dst = dir.join(format!("{stem}.{label}.{ext}"));
    std::fs::copy(src, &dst).unwrap();
    dst
}

#[test]
#[ignore = "audit harness: prints findings, run with --ignored --nocapture"]
fn audit_formats() {
    let input = PathBuf::from(std::env::var("TAGREX_AUDIT_IN").expect("TAGREX_AUDIT_IN"));
    let out = PathBuf::from(std::env::var("TAGREX_AUDIT_OUT").expect("TAGREX_AUDIT_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&input)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    let work = out.join("work");
    std::fs::create_dir_all(&work).unwrap();

    for src in files {
        let name = src.file_name().unwrap().to_string_lossy().to_string();
        let mut problems: Vec<String> = Vec::new();
        let original = match TagEngine::read(&src) {
            Ok(file) => file,
            Err(e) => {
                println!("\n== {name}: READ ERROR {e}");
                continue;
            }
        };
        println!(
            "\n== {name} ({:?}) read: {:?}",
            original.format, original.tags
        );

        // A save with nothing changed.
        let noop = copy_to(&src, &work, "noop");
        let mut file = TagEngine::read(&noop).unwrap();
        file.path = noop.clone();
        match TagEngine::write(&file) {
            Ok(()) => {
                if std::fs::read(&noop).unwrap() != std::fs::read(&src).unwrap() {
                    problems.push("no-op save changed the file's bytes".into());
                }
                let again = TagEngine::read(&noop).unwrap();
                if again.tags != original.tags {
                    problems.push(format!("no-op save changed tags: {:?}", again.tags));
                }
            }
            Err(e) => problems.push(format!("no-op save failed: {e}")),
        }

        // Each value on its own, every other field kept.
        for (index, (field, value)) in values().into_iter().enumerate() {
            let path = copy_to(&src, &work, &format!("v{index}"));
            let mut file = TagEngine::read(&path).unwrap();
            file.tags.insert(field.clone(), value.to_string());
            let expected: TagMap = file.tags.clone();
            if let Err(e) = TagEngine::write(&file) {
                problems.push(format!("write {field:?}={:.30?} failed: {e}", value));
                continue;
            }
            let back = match TagEngine::read(&path) {
                Ok(back) => back.tags,
                Err(e) => {
                    problems.push(format!("UNREADABLE after {field:?}: {e}"));
                    continue;
                }
            };
            for (k, v) in &expected {
                let got = back.get(k);
                if got != Some(v) {
                    let what = if k == &field { "VALUE" } else { "OTHER FIELD" };
                    problems.push(format!(
                        "{what} {k:?}: wrote {:.40?}, read {:.40?} (while setting {field:?})",
                        v, got
                    ));
                }
            }
            for k in back.keys() {
                if !expected.contains_key(k) {
                    problems.push(format!(
                        "APPEARED {k:?}={:.40?} (while setting {field:?})",
                        back[k]
                    ));
                }
            }
            std::fs::remove_file(&path).ok();
        }

        // Heavy edit, kept for the audio comparison.
        let edited = copy_to(&src, &out, "edited");
        let mut file = TagEngine::read(&edited).unwrap();
        for (field, value) in values().into_iter().take(15) {
            file.tags.insert(field, value.to_string());
        }
        if let Err(e) = TagEngine::write(&file) {
            problems.push(format!("heavy edit failed: {e}"));
        }

        // Every field cleared, kept for the audio comparison.
        let cleared = copy_to(&src, &out, "cleared");
        let mut file = TagEngine::read(&cleared).unwrap();
        file.tags.clear();
        match TagEngine::write(&file) {
            Ok(()) => {
                let left = TagEngine::read(&cleared).map(|f| f.tags);
                match left {
                    Ok(tags) if !tags.is_empty() => {
                        problems.push(format!("clearing left fields: {tags:?}"))
                    }
                    Err(e) => problems.push(format!("UNREADABLE after clearing: {e}")),
                    _ => {}
                }
            }
            Err(e) => problems.push(format!("clearing failed: {e}")),
        }

        problems.dedup();
        if problems.is_empty() {
            println!("   OK");
        }
        for p in problems {
            println!("   - {p}");
        }
        std::fs::remove_file(&noop).ok();
    }
}
