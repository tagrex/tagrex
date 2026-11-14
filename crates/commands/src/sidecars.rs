//! Request-local sidecar scanning for rename/move previews (#58).
//!
//! A preview used to list every renamed file's own folder once *per file* —
//! renaming 2 000 files spread over 20 folders meant 2 000 `read_dir` walks of
//! 20 folders that never changed, and one `is_file` per matching candidate on
//! every walk. What the walk answers is a property of the folder, not of the
//! rename, so one preview only has to ask once per folder.
//!
//! [`SidecarScan`] is that one-ask scope, and it is deliberately per request:
//! every public preview builds a fresh one and drops it on return, so the next
//! preview re-reads the disk and sees a sidecar that was added, renamed or
//! removed in between, and a settings change (the feature switched off, another
//! extension set) is applied by the caller's own guards before a scan is even
//! consulted. Nothing here lives on [`App`](crate::App) or outlives a request.
//!
//! A folder that cannot be read is *not* remembered: the next change in the
//! same preview asks again, exactly as the per-file listing did, so a transient
//! failure cannot silently empty the rest of one preview.
//!
//! Names are snapshotted once per folder; newly created names appear on the
//! next preview. Matching names still receive a fresh `is_file` check before
//! each pair is accepted, so removal or a file/type/symlink change is not cached.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// The source folders one preview has already listed, by their spelling.
#[derive(Default)]
pub(crate) struct SidecarScan {
    dirs: HashMap<PathBuf, Directory>,
}

impl SidecarScan {
    /// The pairs travelling from `src` to `dst`: every file in `src`'s own
    /// folder whose stem is `src`'s, whose extension is in `exts`
    /// (case-insensitively, spelled as the file on disk spells it) and which is
    /// a file — `Path::is_file`, so a symlink to a file counts and a dangling
    /// one does not. Sorted, so preview and journal read consistently.
    ///
    /// `None` when a path has no parent/UTF-8 stem or the folder cannot be read,
    /// preserving the old caller's no-op. `Some(empty)` means a successful scan
    /// found no matches. The caller keeps the guards that are not
    /// about the folder at all: the feature switch, an empty extension set, and
    /// a rename that does not actually rename.
    pub(crate) fn pairs(
        &mut self,
        src: &Path,
        dst: &Path,
        exts: &[String],
    ) -> Option<Vec<(String, String)>> {
        let (Some(src_dir), Some(src_stem)) =
            (src.parent(), src.file_stem().and_then(OsStr::to_str))
        else {
            return None;
        };
        let (Some(dst_dir), Some(dst_stem)) =
            (dst.parent(), dst.file_stem().and_then(OsStr::to_str))
        else {
            return None;
        };
        let dir = self.directory(src_dir)?;
        let Some(entries) = dir.by_stem.get(OsStr::new(src_stem)) else {
            return Some(Vec::new());
        };
        let mut pairs = Vec::new();
        for entry in entries {
            let path = dir.path.join(&entry.name);
            // Never the audio file itself.
            if path == *src {
                continue;
            }
            let Some(ext) = Path::new(&entry.name).extension().and_then(OsStr::to_str) else {
                continue; // an extension-less file can't match the set
            };
            if !exts.iter().any(|e| e.eq_ignore_ascii_case(ext)) {
                continue;
            }
            // Cache names only; recheck file/symlink state at acceptance.
            if !path.is_file() {
                continue;
            }
            let from = path.to_string_lossy().into_owned();
            let to = dst_dir
                .join(format!("{dst_stem}.{ext}"))
                .to_string_lossy()
                .into_owned();
            pairs.push((from, to));
        }
        pairs.sort();
        Some(pairs)
    }

    /// `path`'s listing, read once per preview. A failed listing is retried by
    /// the next caller rather than cached as "empty".
    fn directory(&mut self, path: &Path) -> Option<&Directory> {
        if !self.dirs.contains_key(path) {
            let dir = Directory::scan(path)?;
            self.dirs.insert(path.to_path_buf(), dir);
        }
        self.dirs.get(path)
    }
}

/// One source folder as [`SidecarScan::pairs`] needs it.
struct Directory {
    path: PathBuf,
    /// Entries grouped by file stem, so a change is a lookup rather than a pass
    /// over the whole folder. The stem keys the comparison is made with — the
    /// same `file_stem` the per-file walk took from each entry's path.
    by_stem: HashMap<OsString, Vec<Entry>>,
}

impl Directory {
    fn scan(path: &Path) -> Option<Self> {
        let entries = std::fs::read_dir(path).ok()?;
        let mut by_stem: HashMap<OsString, Vec<Entry>> = HashMap::new();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(stem) = Path::new(&name).file_stem().map(OsStr::to_os_string) else {
                continue;
            };
            by_stem.entry(stem).or_default().push(Entry { name });
        }
        Some(Self {
            path: path.to_path_buf(),
            by_stem,
        })
    }
}

struct Entry {
    /// The entry's own name, not the whole path: joining it back onto the
    /// folder gives the identical path the walk used to build per change.
    name: OsString,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("tagrex-sidecar-scan-{tag}-{}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).ok();
        }
    }

    fn exts() -> Vec<String> {
        ["lrc", "txt"].iter().map(|ext| ext.to_string()).collect()
    }

    #[test]
    fn one_scan_answers_from_one_listing_and_a_new_scan_rereads() {
        let dir = Scratch::new("snapshot");
        fs::write(dir.0.join("track.flac"), b"audio").unwrap();
        fs::write(dir.0.join("track.lrc"), b"lyrics").unwrap();
        let src = dir.0.join("track.flac");
        let dst = dir.0.join("renamed.flac");
        let mut scan = SidecarScan::default();
        assert_eq!(
            scan.pairs(&src, &dst, &exts()).unwrap(),
            vec![(
                dir.0.join("track.lrc").to_string_lossy().into_owned(),
                dir.0.join("renamed.lrc").to_string_lossy().into_owned(),
            )]
        );

        // Added after this preview listed the folder: the preview keeps the
        // snapshot it already took ...
        fs::write(dir.0.join("track.txt"), b"notes").unwrap();
        assert_eq!(scan.pairs(&src, &dst, &exts()).unwrap().len(), 1);

        // ... and the next preview sees the disk as it is now.
        let mut fresh = SidecarScan::default();
        assert_eq!(fresh.pairs(&src, &dst, &exts()).unwrap().len(), 2);
    }

    #[test]
    fn a_folder_that_cannot_be_read_is_retried_by_the_next_change() {
        let dir = Scratch::new("retry");
        let folder = dir.0.join("late");
        let src = folder.join("track.flac");
        let dst = folder.join("renamed.flac");
        let mut scan = SidecarScan::default();
        assert!(scan.pairs(&src, &dst, &exts()).is_none());

        // The folder appears during the same preview: the failure was not
        // remembered, so the next change in it asks again and finds the file.
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("track.lrc"), b"lyrics").unwrap();
        assert_eq!(scan.pairs(&src, &dst, &exts()).unwrap().len(), 1);
    }

    #[test]
    fn names_and_extensions_are_judged_as_the_walk_judged_them() {
        let dir = Scratch::new("names");
        let src = dir.0.join("track.flac");
        fs::write(&src, b"audio").unwrap();
        fs::write(dir.0.join("track.LRC"), b"upper").unwrap(); // carried, spelling kept
        fs::write(dir.0.join("other.lrc"), b"wrong stem").unwrap();
        fs::write(dir.0.join("track"), b"extensionless").unwrap();
        fs::write(dir.0.join("track.flac.lrc"), b"not this stem").unwrap();
        fs::create_dir(dir.0.join("track.txt")).unwrap(); // a directory, not a file
        let dst = dir.0.join("renamed.flac");

        let mut scan = SidecarScan::default();
        assert_eq!(
            scan.pairs(&src, &dst, &exts()).unwrap(),
            vec![(
                dir.0.join("track.LRC").to_string_lossy().into_owned(),
                dir.0.join("renamed.LRC").to_string_lossy().into_owned(),
            )],
            "the audio itself, wrong stems, extension-less names and directories are skipped"
        );
    }
    #[test]
    fn matching_names_recheck_file_type_within_the_same_preview() {
        let dir = Scratch::new("metadata-freshness");
        let audio = dir.0.join("track.flac");
        let sidecar = dir.0.join("track.lrc");
        let renamed = dir.0.join("renamed.flac");
        fs::write(&audio, b"audio").unwrap();
        fs::write(&sidecar, b"sidecar").unwrap();
        let mut scan = SidecarScan::default();
        assert_eq!(scan.pairs(&audio, &renamed, &exts()).unwrap().len(), 1);
        fs::remove_file(&sidecar).unwrap();
        fs::create_dir(&sidecar).unwrap();
        assert!(scan.pairs(&audio, &renamed, &exts()).unwrap().is_empty());
        fs::remove_dir(&sidecar).unwrap();
        fs::write(&sidecar, b"restored").unwrap();
        assert_eq!(scan.pairs(&audio, &renamed, &exts()).unwrap().len(), 1);
    }
}
