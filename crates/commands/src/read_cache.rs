//! Bounded, fingerprint-validated display/preview snapshots. Executor staleness
//! checks never use this cache. Failed reads are never cached.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::SystemTime,
};

use tagrex_core::model::{TagEngine, TagIoError, TrackRead};

#[derive(Clone, PartialEq, Eq)]
struct Fingerprint {
    len: u64,
    modified: SystemTime,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}

impl Fingerprint {
    fn of(path: &Path) -> Option<Self> {
        let m = std::fs::metadata(path).ok()?;
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Some(Self {
            len: m.len(),
            modified: m.modified().ok()?,
            #[cfg(unix)]
            identity: (m.dev(), m.ino(), m.ctime(), m.ctime_nsec()),
        })
    }
}

struct Entry {
    stamp: Fingerprint,
    read: TrackRead,
    bytes: usize,
}

/// Request-independent display snapshots bounded by entry count and estimated
/// metadata bytes. Writes and Undo clear these; executor checks bypass them.
#[derive(Default)]
pub(crate) struct ReadCache {
    entries: HashMap<PathBuf, Entry>,
    bytes: usize,
}

impl ReadCache {
    /// Discard all snapshots after writes or a session change.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }

    /// Reuse a snapshot only while its filesystem fingerprint still matches.
    /// Failed or visibly concurrent reads are never added to the cache.
    pub fn read(&mut self, path: &Path) -> Result<TrackRead, TagIoError> {
        let before = Fingerprint::of(path);
        if let Some(entry) = self.entries.get(path) {
            if before.as_ref() == Some(&entry.stamp) {
                return Ok(entry.read.clone());
            }
        }
        if let Some(old) = self.entries.remove(path) {
            self.bytes -= old.bytes;
        }
        let read = TagEngine::read_with_props(path)?;
        // Cache only when the observed fingerprint is unchanged across the read.
        // This does not detect every concurrent or same-size external edit: on
        // Windows, length + mtime can collide on coarse timestamp filesystems.
        // Executor staleness checks always re-read the file independently.
        if let Some(stamp) = before.filter(|s| Some(s) == Fingerprint::of(path).as_ref()) {
            let bytes = 512
                + path.as_os_str().len()
                + read
                    .file
                    .tags
                    .iter()
                    .map(|(k, v)| k.to_storage_key().len() + v.len() + 64)
                    .sum::<usize>();
            const LIMIT: usize = 64 * 1024 * 1024;
            if bytes <= LIMIT {
                if self.bytes + bytes > LIMIT || self.entries.len() >= 65_536 {
                    self.clear();
                }
                self.bytes += bytes;
                self.entries.insert(
                    path.to_path_buf(),
                    Entry {
                        stamp,
                        read: read.clone(),
                        bytes,
                    },
                );
            }
        }
        Ok(read)
    }
}
