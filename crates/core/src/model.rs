//! Tag data model and the tag I/O engine facade.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lofty::config::WriteOptions;
use lofty::file::{FileType, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::{ItemKey, ItemValue, Tag, TagExt, TagItem, TagType};
use thiserror::Error;

/// Audio container formats supported at launch (see architecture.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Mp3,
    Flac,
    OggVorbis,
    M4a,
}

impl AudioFormat {
    fn from_lofty(file_type: FileType) -> Result<Self, TagIoError> {
        match file_type {
            FileType::Mpeg => Ok(Self::Mp3),
            FileType::Flac => Ok(Self::Flac),
            FileType::Vorbis => Ok(Self::OggVorbis),
            FileType::Mp4 => Ok(Self::M4a),
            other => Err(TagIoError::UnsupportedFormat(format!("{other:?}"))),
        }
    }

    /// The tag type each format is written and read through, matching
    /// [`lofty::file::FileType::primary_tag_type`] for the formats above.
    fn primary_tag_type(self) -> TagType {
        match self {
            Self::Mp3 => TagType::Id3v2,
            Self::Flac | Self::OggVorbis => TagType::VorbisComments,
            Self::M4a => TagType::Mp4Ilst,
        }
    }
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

impl TagField {
    /// Lossless string encoding for persistence (the journal). Distinct from
    /// the `lofty` `ItemKey` mapping and the mask placeholder names: those can
    /// be lossy or format-specific, this must round-trip exactly. `Custom`
    /// values are prefixed so a `Custom("artist")` can never collide with the
    /// first-class `Artist`.
    pub fn to_storage_key(&self) -> String {
        match self {
            Self::Artist => "artist".to_string(),
            Self::Title => "title".to_string(),
            Self::Album => "album".to_string(),
            Self::AlbumArtist => "albumartist".to_string(),
            Self::TrackNumber => "track".to_string(),
            Self::TrackTotal => "tracktotal".to_string(),
            Self::DiscNumber => "disc".to_string(),
            Self::Year => "year".to_string(),
            Self::Genre => "genre".to_string(),
            Self::Comment => "comment".to_string(),
            Self::Custom(name) => format!("custom:{name}"),
        }
    }

    /// Inverse of [`to_storage_key`](Self::to_storage_key).
    pub fn from_storage_key(key: &str) -> Self {
        if let Some(name) = key.strip_prefix("custom:") {
            return Self::Custom(name.to_string());
        }
        match key {
            "artist" => Self::Artist,
            "title" => Self::Title,
            "album" => Self::Album,
            "albumartist" => Self::AlbumArtist,
            "track" => Self::TrackNumber,
            "tracktotal" => Self::TrackTotal,
            "disc" => Self::DiscNumber,
            "year" => Self::Year,
            "genre" => Self::Genre,
            "comment" => Self::Comment,
            // Only reachable if the database holds a key this build didn't
            // write; preserve it verbatim rather than losing it.
            other => Self::Custom(other.to_string()),
        }
    }
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
    pub fn read(path: &Path) -> Result<TrackFile, TagIoError> {
        let tagged_file = Probe::open(path)?.guess_file_type()?.read()?;
        let format = AudioFormat::from_lofty(tagged_file.file_type())?;

        let mut tags = TagMap::new();
        if let Some(tag) = tagged_file
            .primary_tag()
            .or_else(|| tagged_file.first_tag())
        {
            for item in tag.items() {
                if let Some(text) = item.value().text() {
                    tags.insert(item_key_to_tag_field(item.key()), text.to_string());
                }
            }
        }

        Ok(TrackFile {
            path: path.to_path_buf(),
            format,
            tags,
        })
    }

    /// Write the tags of `file` back to disk.
    ///
    /// Only [`plan::Executor`](crate::plan::Executor) is allowed to call this —
    /// see the crate-level invariant.
    pub fn write(file: &TrackFile) -> Result<(), TagIoError> {
        let mut tag = Tag::new(file.format.primary_tag_type());
        for (field, value) in &file.tags {
            // `insert_text`/`insert` silently drop `ItemKey::Unknown` (Custom
            // fields) since they refuse to write keys without a known mapping
            // for the tag type. `insert_unchecked` is the documented escape
            // hatch for exactly this case.
            tag.insert_unchecked(TagItem::new(
                tag_field_to_item_key(field),
                ItemValue::Text(value.clone()),
            ));
        }
        tag.save_to_path(&file.path, WriteOptions::default())?;
        Ok(())
    }
}

fn tag_field_to_item_key(field: &TagField) -> ItemKey {
    match field {
        TagField::Artist => ItemKey::TrackArtist,
        TagField::Title => ItemKey::TrackTitle,
        TagField::Album => ItemKey::AlbumTitle,
        TagField::AlbumArtist => ItemKey::AlbumArtist,
        TagField::TrackNumber => ItemKey::TrackNumber,
        TagField::TrackTotal => ItemKey::TrackTotal,
        TagField::DiscNumber => ItemKey::DiscNumber,
        // Write the year through `RecordingDate` (ID3v2.4 TDRC, Vorbis DATE,
        // MP4 ©day), not `ItemKey::Year`: ID3v2.4 has no plain "year" frame, so
        // lofty silently drops `ItemKey::Year` there — which lost the year on
        // every write. `read` maps both back to `Year`, so this round-trips.
        TagField::Year => ItemKey::RecordingDate,
        TagField::Genre => ItemKey::Genre,
        TagField::Comment => ItemKey::Comment,
        TagField::Custom(key) => ItemKey::Unknown(key.clone()),
    }
}

// Only `ItemKey::Unknown` round-trips as the literal string a caller put
// into `TagField::Custom`. Any other recognized-but-unmapped `ItemKey`
// variant (e.g. `Composer`, `Mood` — lofty recognizes far more keys than the
// ten modeled here) falls back to its Rust `Debug` name instead, since we
// have no per-format key text to recover once lofty has already parsed it
// into a variant.
fn item_key_to_tag_field(key: &ItemKey) -> TagField {
    match key {
        ItemKey::TrackArtist => TagField::Artist,
        ItemKey::TrackTitle => TagField::Title,
        ItemKey::AlbumTitle => TagField::Album,
        ItemKey::AlbumArtist => TagField::AlbumArtist,
        ItemKey::TrackNumber => TagField::TrackNumber,
        ItemKey::TrackTotal => TagField::TrackTotal,
        ItemKey::DiscNumber => TagField::DiscNumber,
        // `Year` (ID3v2.3 TYER) is legacy; real-world taggers (verified
        // against TagScanner-tagged files) overwhelmingly write the year
        // into `RecordingDate` (ID3v2.4 TDRC) instead. Writing still targets
        // `Year` alone — see `tag_field_to_item_key`.
        ItemKey::Year | ItemKey::RecordingDate => TagField::Year,
        ItemKey::Genre => TagField::Genre,
        ItemKey::Comment => TagField::Comment,
        ItemKey::Unknown(key) => TagField::Custom(key.clone()),
        other => TagField::Custom(format!("{other:?}")),
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
    #[error("tag backend error: {0}")]
    Backend(#[from] lofty::error::LoftyError),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known_fields() -> Vec<TagField> {
        vec![
            TagField::Artist,
            TagField::Title,
            TagField::Album,
            TagField::AlbumArtist,
            TagField::TrackNumber,
            TagField::TrackTotal,
            TagField::DiscNumber,
            TagField::Year,
            TagField::Genre,
            TagField::Comment,
        ]
    }

    #[test]
    fn known_fields_round_trip_through_item_key() {
        for field in known_fields() {
            let key = tag_field_to_item_key(&field);
            assert_eq!(item_key_to_tag_field(&key), field);
        }
    }

    #[test]
    fn custom_field_round_trips_through_unknown_item_key() {
        let field = TagField::Custom("MOOD".to_string());
        let key = tag_field_to_item_key(&field);
        assert_eq!(key, ItemKey::Unknown("MOOD".to_string()));
        assert_eq!(item_key_to_tag_field(&key), field);
    }
}
