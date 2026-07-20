//! Directory scanner.
//!
//! Must stay responsive at 50k+ files (architecture.md): [`scan`] returns a
//! lazy iterator instead of collecting the whole tree first, so a caller (or
//! a future table model) can start consuming entries as they're found.

use std::path::{Path, PathBuf};

use thiserror::Error;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub recursive: bool,
    pub follow_symlinks: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            recursive: true,
            follow_symlinks: false,
        }
    }
}

/// Extensions of the formats [`model::AudioFormat`](crate::model::AudioFormat)
/// supports. A file that slips through by extension but isn't actually one of
/// these is rejected later when the tag backend fails to parse it, so this only
/// has to be permissive enough not to hide real audio.
const SUPPORTED_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "ogg", "oga", "m4a", "m4b", "mp4", "aac", "aif", "aiff", "aifc", "wav", "opus",
    "spx", "mpc", "mp+", "mpp", "ape", "wv",
];

/// Lazily walk `root`, yielding the paths of supported audio files as
/// they're found.
pub fn scan(
    root: &Path,
    options: &ScanOptions,
) -> impl Iterator<Item = Result<PathBuf, ScanError>> {
    let mut walker = WalkDir::new(root).follow_links(options.follow_symlinks);
    if !options.recursive {
        // WalkDir counts `root` itself as depth 0, so depth 1 is its direct
        // children -- exactly "don't recurse into subdirectories".
        walker = walker.max_depth(1);
    }

    walker.into_iter().filter_map(|entry| match entry {
        Ok(entry) => (entry.file_type().is_file() && is_supported(entry.path()))
            .then(|| Ok(entry.into_path())),
        Err(err) => Some(Err(ScanError::from(err))),
    })
}

fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            SUPPORTED_EXTENSIONS
                .iter()
                .any(|supported| supported.eq_ignore_ascii_case(ext))
        })
}

#[derive(Debug, Error)]
pub enum ScanError {
    #[error(transparent)]
    Walk(#[from] walkdir::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, rel: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, b"").unwrap();
    }

    fn scanned_names(root: &Path, options: &ScanOptions) -> Vec<String> {
        let mut names: Vec<String> = scan(root, options)
            .map(|entry| {
                entry
                    .unwrap()
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    #[test]
    fn finds_supported_files_recursively_and_skips_others() {
        let dir = std::env::temp_dir().join(format!("tagrex-scanner-test-{}", std::process::id()));
        write(&dir, "track.mp3");
        write(&dir, "cover.jpg");
        write(&dir, "sub/track.flac");
        write(&dir, "sub/notes.txt");

        let names = scanned_names(&dir, &ScanOptions::default());

        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(names, vec!["track.flac", "track.mp3"]);
    }

    #[test]
    fn non_recursive_only_scans_the_top_level() {
        let dir =
            std::env::temp_dir().join(format!("tagrex-scanner-test-flat-{}", std::process::id()));
        write(&dir, "track.mp3");
        write(&dir, "sub/track.flac");

        let options = ScanOptions {
            recursive: false,
            ..ScanOptions::default()
        };
        let names = scanned_names(&dir, &options);

        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(names, vec!["track.mp3"]);
    }

    #[test]
    fn picks_up_every_supported_extension_and_ignores_the_rest() {
        let dir = std::env::temp_dir().join(format!(
            "tagrex-scanner-test-formats-{}",
            std::process::id()
        ));
        // One file per supported container, plus files that must be ignored.
        for name in [
            "a.mp3", "b.flac", "c.ogg", "d.m4a", "e.aac", "f.aiff", "g.wav", "h.opus", "i.wv",
            "j.ape", "k.mpc", "l.spx",
        ] {
            write(&dir, name);
        }
        for ignored in ["cover.jpg", "notes.txt", "playlist.m3u", "readme"] {
            write(&dir, ignored);
        }

        let names = scanned_names(&dir, &ScanOptions::default());
        std::fs::remove_dir_all(&dir).ok();

        // `scanned_names` returns them sorted, and the fixture names are
        // already in alphabetical order.
        let expected: Vec<String> = [
            "a.mp3", "b.flac", "c.ogg", "d.m4a", "e.aac", "f.aiff", "g.wav", "h.opus", "i.wv",
            "j.ape", "k.mpc", "l.spx",
        ]
        .iter()
        .map(|name| (*name).to_string())
        .collect();
        assert_eq!(names, expected);
    }

    #[test]
    fn extension_matching_is_case_insensitive() {
        let dir =
            std::env::temp_dir().join(format!("tagrex-scanner-test-case-{}", std::process::id()));
        write(&dir, "SHOUTY.MP3");
        write(&dir, "Mixed.FlAc");

        let names = scanned_names(&dir, &ScanOptions::default());
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(names, vec!["Mixed.FlAc", "SHOUTY.MP3"]);
    }
}
