//! Tag data model and the tag I/O engine facade.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// Audio container formats supported at launch (see architecture.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Mp3,
    Flac,
    OggVorbis,
    M4a,
}

/// A tag field. Well-known fields are first-class variants so the table UI,
/// masks and providers can address them without string matching; anything
/// else round-trips through `Custom`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TagField {
    Artist,
    Title,
    Album,
    AlbumArtist,
    TrackNumber,
    TrackTotal,
    DiscNumber,
    Year,
    Genre,
    Comment,
    Custom(String),
}

/// Field -> value map. BTreeMap keeps a stable, predictable field order for
/// previews and diffs.
pub type TagMap = BTreeMap<TagField, String>;

/// A single audio file as seen by the table model.
#[derive(Debug, Clone)]
pub struct TrackFile {
    pub path: PathBuf,
    pub format: AudioFormat,
    pub tags: TagMap,
}

/// Facade over the tag I/O backend (lofty). The rest of the codebase must
/// never touch the backend directly — this is the only place format-specific
/// knowledge is allowed to live.
pub struct TagEngine;

impl TagEngine {
    /// Read tags from a file on disk.
    pub fn read(_path: &Path) -> Result<TrackFile, TagIoError> {
        todo!("wire up lofty: detect format, map primary tag into TagMap")
    }

    /// Write the tags of `file` back to disk.
    ///
    /// Only [`plan::Executor`](crate::plan::Executor) is allowed to call this —
    /// see the crate-level invariant.
    pub fn write(_file: &TrackFile) -> Result<(), TagIoError> {
        todo!("wire up lofty: map TagMap back into the format-specific tag")
    }
}

#[derive(Debug, Error)]
pub enum TagIoError {
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("malformed tag data: {0}")]
    Malformed(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
