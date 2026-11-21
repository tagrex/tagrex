//! Bounded, fingerprint-validated display/preview snapshots. Executor staleness
//! checks never use this cache. Failed reads are never cached.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::SystemTime,
};

use tagrex_core::model::{TagEngine, TagIoError, TrackRead};

/// What a file looks like on disk, to tell whether it changed since it was
/// last read. Also keys the duplicate scan's audio digests (#823).
#[derive(Clone, PartialEq, Eq)]
pub struct Fingerprint {
    len: u64,
    modified: SystemTime,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
    /// NTFS change time (#813): every write moves it, it is read from the open
    /// file rather than its directory entry, and no program can set it back
    /// the way a tagger that keeps the file's date sets the modification time.
    #[cfg(windows)]
    changed: Option<i64>,
}

impl Fingerprint {
    /// The fingerprint of `path` now, or None when its metadata cannot be read.
    pub fn of(path: &Path) -> Option<Self> {
        let m = std::fs::metadata(path).ok()?;
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Some(Self {
            len: m.len(),
            modified: m.modified().ok()?,
            #[cfg(unix)]
            identity: (m.dev(), m.ino(), m.ctime(), m.ctime_nsec()),
            #[cfg(windows)]
            changed: windows_change_time(path),
        })
    }
}

/// The file's NTFS change time, or None where the volume does not keep one.
/// A metadata-only open takes no part in sharing checks, so this works while
/// another program holds the file open for writing.
#[cfg(windows)]
fn windows_change_time(path: &Path) -> Option<i64> {
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        FileBasicInfo, GetFileInformationByHandleEx, FILE_BASIC_INFO,
    };
    const FILE_READ_ATTRIBUTES: u32 = 0x80;
    const FILE_SHARE_ALL: u32 = 0x1 | 0x2 | 0x4;
    let file = std::fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_ALL)
        .open(path)
        .ok()?;
    let mut info = FILE_BASIC_INFO {
        CreationTime: 0,
        LastAccessTime: 0,
        LastWriteTime: 0,
        ChangeTime: 0,
        FileAttributes: 0,
    };
    // SAFETY: the handle stays open for the call; info is writable for exactly
    // its declared size and the API keeps neither pointer.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileBasicInfo,
            (&mut info as *mut FILE_BASIC_INFO).cast(),
            std::mem::size_of::<FILE_BASIC_INFO>() as u32,
        )
    };
    (ok != 0 && info.ChangeTime != 0).then_some(info.ChangeTime)
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
        // Windows volumes without a change time (FAT, exFAT), length + mtime can
        // collide on their coarse timestamps.
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
