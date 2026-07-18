//! Directory scanner.
//!
//! Must stay responsive at 50k+ files (architecture.md): the real
//! implementation streams results to the table model instead of collecting
//! everything first. The signature below is the synchronous placeholder to
//! be replaced by a streaming/channel-based API together with the table
//! model.

use std::path::{Path, PathBuf};

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

/// Collect paths of supported audio files under `root`.
pub fn scan(_root: &Path, _options: &ScanOptions) -> Result<Vec<PathBuf>, std::io::Error> {
    todo!("walk the tree, filter by supported extensions, stream to the table model")
}
