//! Audit harness for the transactional pipeline: failure injection, rollback,
//! rename edge cases, symlink containment, undo after outside changes.
//! Ignored by default. Run: cargo test -p tagrex-core --test audit_executor -- --nocapture --test-threads=1

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use tagrex_core::journal::{UndoJournal, VecJournal};
use tagrex_core::model::{TagEngine, TagField};
use tagrex_core::plan::{ChangePlan, Executor, FieldChange, FileChange};

fn flac_bytes() -> Vec<u8> {
    let mut v = vec![0x66, 0x4c, 0x61, 0x43, 0x00, 0x00, 0x00, 0x22];
    v.extend_from_slice(&[0x10, 0x00, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    v.extend_from_slice(&[0x0a, 0xc4, 0x42, 0xf0, 0x00, 0x00, 0x00, 0x00]);
    v.extend_from_slice(&[0u8; 16]);
    v.extend_from_slice(&[0x81, 0x00, 0x00, 0x10]);
    v.extend_from_slice(&[0u8; 16]);
    v.extend_from_slice(&[
        0xff, 0xf8, 0xc9, 0x18, 0x00, 0xc2, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xb8, 0xee,
    ]);
    v
}

struct Dir(PathBuf);
impl Dir {
    fn new(tag: &str) -> Self {
        let d =
            std::env::temp_dir().join(format!("tagrex-audit-exec-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        std::fs::create_dir_all(&d).unwrap();
        Self(d.canonicalize().unwrap())
    }
    fn flac(&self, name: &str) -> PathBuf {
        let p = self.0.join(name);
        std::fs::write(&p, flac_bytes()).unwrap();
        p
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// name (relative, recursive) -> bytes; symlinks recorded as their target.
fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    for e in walkdir(root) {
        let rel = e.strip_prefix(root).unwrap().to_string_lossy().to_string();
        let meta = std::fs::symlink_metadata(&e).unwrap();
        if meta.file_type().is_symlink() {
            out.insert(
                rel,
                format!("->{}", std::fs::read_link(&e).unwrap().display()).into_bytes(),
            );
        } else if meta.is_file() {
            out.insert(rel, std::fs::read(&e).unwrap());
        } else {
            out.insert(format!("{rel}/"), Vec::new());
        }
    }
    out
}
fn walkdir(root: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    for e in std::fs::read_dir(root).unwrap().flatten() {
        let p = e.path();
        v.push(p.clone());
        let m = std::fs::symlink_metadata(&p).unwrap();
        if m.is_dir() {
            v.extend(walkdir(&p));
        }
    }
    v
}

fn tag(path: &Path, value: &str) -> FileChange {
    FileChange {
        path: path.to_path_buf(),
        tag_changes: vec![FieldChange {
            field: TagField::Artist,
            old: None,
            new: Some(value.to_string()),
        }],
        ..FileChange::default()
    }
}
fn rename(path: &Path, to: PathBuf) -> FileChange {
    FileChange {
        path: path.to_path_buf(),
        rename_to: Some(to),
        ..FileChange::default()
    }
}
fn plan(changes: Vec<FileChange>) -> ChangePlan {
    ChangePlan {
        description: "audit".into(),
        changes,
        ..ChangePlan::default()
    }
}

fn report(name: &str, ok: bool, detail: impl AsRef<str>) {
    println!(
        "{} {name}: {}",
        if ok { "PASS" } else { "FAIL" },
        detail.as_ref()
    );
}

#[test]
#[ignore = "audit harness: prints findings, run with --ignored --nocapture"]
fn audit_executor() {
    // S1: second file fails at run time (header + padding, no audio) -> first must be restored.
    {
        let d = Dir::new("s1");
        let a = d.flac("a.flac");
        let b = d.0.join("b.flac");
        std::fs::write(&b, &flac_bytes()[..62]).unwrap();
        let c = d.flac("c.flac");
        let before = snapshot(&d.0);
        let mut j = VecJournal::new();
        let r = Executor::apply(
            &plan(vec![tag(&a, "A"), tag(&b, "B"), tag(&c, "C")]),
            &mut j,
            std::slice::from_ref(&d.0),
        );
        let after = snapshot(&d.0);
        report(
            "S1 mid-batch write failure rolls back",
            after == before && j.batches().unwrap().is_empty(),
            format!(
                "result={:?} dir_identical={} journal_len={}",
                r.as_ref().map(|_| "ok").map_err(|e| e.to_string()),
                after == before,
                j.batches().unwrap().len()
            ),
        );
    }
    // S2: rename with an over-long target name in the middle of a batch.
    {
        let d = Dir::new("s2");
        let a = d.flac("a.flac");
        let b = d.flac("b.flac");
        let c = d.flac("c.flac");
        let before = snapshot(&d.0);
        let mut j = VecJournal::new();
        let long = d.0.join(format!("{}.flac", "x".repeat(300)));
        let r = Executor::apply(
            &plan(vec![
                rename(&a, d.0.join("a2.flac")),
                rename(&b, long),
                rename(&c, d.0.join("c2.flac")),
            ]),
            &mut j,
            std::slice::from_ref(&d.0),
        );
        let after = snapshot(&d.0);
        report(
            "S2 over-long rename rolls back whole batch",
            after == before,
            format!(
                "result={:?} dir_identical={}",
                r.as_ref().map(|_| "ok").map_err(|e| e.to_string()),
                after == before
            ),
        );
    }
    // S3: swap a<->b and chain a->b, b->c.
    {
        let d = Dir::new("s3");
        let a = d.flac("a.flac");
        let b = d.flac("b.flac");
        // make them distinguishable
        let mut j = VecJournal::new();
        Executor::apply(
            &plan(vec![tag(&a, "IS-A"), tag(&b, "IS-B")]),
            &mut j,
            std::slice::from_ref(&d.0),
        )
        .unwrap();
        let before = snapshot(&d.0);
        let r = Executor::apply(
            &plan(vec![
                rename(&a, d.0.join("b.flac")),
                rename(&b, d.0.join("a.flac")),
            ]),
            &mut j,
            std::slice::from_ref(&d.0),
        );
        let after = snapshot(&d.0);
        let names: Vec<_> = after.keys().cloned().collect();
        let lost = after.len() != before.len();
        report(
            "S3a swap rename never loses a file",
            !lost,
            format!(
                "result={:?} files={names:?}",
                r.as_ref().map(|_| "ok").map_err(|e| e.to_string())
            ),
        );
        let r2 = Executor::apply(
            &plan(vec![
                rename(&a, d.0.join("b.flac")),
                rename(&b, d.0.join("c.flac")),
            ]),
            &mut j,
            std::slice::from_ref(&d.0),
        );
        let after2 = snapshot(&d.0);
        report(
            "S3b chain rename never loses a file",
            after2.len() == after.len(),
            format!(
                "result={:?} files={:?}",
                r2.as_ref().map(|_| "ok").map_err(|e| e.to_string()),
                after2.keys().collect::<Vec<_>>()
            ),
        );
    }
    // S4: undo after outside modification of the file.
    {
        let d = Dir::new("s4");
        let a = d.flac("a.flac");
        let mut j = VecJournal::new();
        let batch = Executor::apply(
            &plan(vec![tag(&a, "Mine")]),
            &mut j,
            std::slice::from_ref(&d.0),
        )
        .unwrap();
        // someone else edits the artist afterwards
        let mut t = TagEngine::read(&a).unwrap();
        t.tags.insert(TagField::Artist, "Edited elsewhere".into());
        TagEngine::write(&t).unwrap();
        let r = Executor::undo(&mut j, batch.id, std::slice::from_ref(&d.0));
        let now = TagEngine::read(&a)
            .unwrap()
            .tags
            .get(&TagField::Artist)
            .cloned();
        report(
            "S4 undo after outside edit does not silently clobber",
            r.is_err() || now.as_deref() == Some("Edited elsewhere"),
            format!(
                "undo={:?} artist_now={now:?}",
                r.as_ref().map(|_| "ok").map_err(|e| e.to_string())
            ),
        );
    }
    // S5: file symlinked from inside the root to a file outside it.
    #[cfg(unix)]
    {
        let d = Dir::new("s5");
        let outside = Dir::new("s5-outside");
        let target = outside.flac("secret.flac");
        let link = d.0.join("link.flac");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let before = std::fs::read(&target).unwrap();
        let mut j = VecJournal::new();
        let r = Executor::apply(
            &plan(vec![tag(&link, "ESCAPED")]),
            &mut j,
            std::slice::from_ref(&d.0),
        );
        let after = std::fs::read(&target).unwrap();
        report(
            "S5 tag write through symlink cannot touch file outside root",
            after == before,
            format!(
                "result={:?} outside_modified={}",
                r.as_ref().map(|_| "ok").map_err(|e| e.to_string()),
                after != before
            ),
        );
    }
    // S6: rename target under a symlinked directory pointing outside.
    #[cfg(unix)]
    {
        let d = Dir::new("s6");
        let outside = Dir::new("s6-outside");
        let a = d.flac("a.flac");
        std::os::unix::fs::symlink(&outside.0, d.0.join("sub")).unwrap();
        let mut j = VecJournal::new();
        let r = Executor::apply(
            &plan(vec![rename(&a, d.0.join("sub").join("moved.flac"))]),
            &mut j,
            std::slice::from_ref(&d.0),
        );
        let escaped = outside.0.join("moved.flac").exists();
        report(
            "S6 rename through symlinked dir cannot leave root",
            !escaped,
            format!(
                "result={:?} escaped={escaped}",
                r.as_ref().map(|_| "ok").map_err(|e| e.to_string())
            ),
        );
    }
    // S7: rename onto a name differing only by case / unicode normalization.
    {
        let d = Dir::new("s7");
        let a = d.flac("a.flac");
        let _upper = d.flac("B.flac");
        let before = snapshot(&d.0);
        let mut j = VecJournal::new();
        let r = Executor::apply(
            &plan(vec![rename(&a, d.0.join("b.flac"))]),
            &mut j,
            std::slice::from_ref(&d.0),
        );
        let after = snapshot(&d.0);
        report(
            "S7 case-variant target keeps both files on case-sensitive fs",
            after.len() == before.len(),
            format!(
                "result={:?} files={:?}",
                r.as_ref().map(|_| "ok").map_err(|e| e.to_string()),
                after.keys().collect::<Vec<_>>()
            ),
        );
    }
    // S8: cancel mid-batch leaves nothing changed.
    {
        let d = Dir::new("s8");
        let a = d.flac("a.flac");
        let b = d.flac("b.flac");
        let before = snapshot(&d.0);
        let mut j = VecJournal::new();
        let cancel = AtomicBool::new(false);
        let mut n = 0;
        let r = Executor::apply_with(
            &plan(vec![tag(&a, "A"), tag(&b, "B")]),
            &mut j,
            std::slice::from_ref(&d.0),
            &mut |_| {
                n += 1;
                if n == 1 {
                    cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                }
            },
            &cancel,
        );
        let after = snapshot(&d.0);
        report(
            "S8 cancel rolls back",
            after == before && j.batches().unwrap().is_empty(),
            format!(
                "result={:?} dir_identical={}",
                r.as_ref().map(|_| "ok").map_err(|e| e.to_string()),
                after == before
            ),
        );
    }
    // S9: rename to a path with a trailing dot/space and reserved-looking names (Windows hazards).
    {
        let d = Dir::new("s9");
        for (i, name) in [
            "trail.", "trail ", "CON.flac", "a:b.flac", "q?.flac", "nul.flac", "x|y.flac",
        ]
        .iter()
        .enumerate()
        {
            let a = d.flac(&format!("f{i}.flac"));
            let mut j = VecJournal::new();
            let r = Executor::apply(
                &plan(vec![rename(&a, d.0.join(name))]),
                &mut j,
                std::slice::from_ref(&d.0),
            );
            println!(
                "INFO S9 rename -> {name:?}: {:?}",
                r.as_ref().map(|_| "ok").map_err(|e| e.to_string())
            );
        }
    }
}
