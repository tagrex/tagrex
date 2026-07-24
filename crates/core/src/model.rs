//! Tag data model and the tag I/O engine facade.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

use lofty::aac::AacFile;
use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::{AudioFile, FileType, TaggedFileExt};
use lofty::id3::v2::{Frame, Id3v2Tag};
use lofty::iff::aiff::AiffFile;
use lofty::iff::wav::WavFile;
use lofty::mpeg::MpegFile;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::probe::Probe;
use lofty::tag::{ItemKey, ItemValue, Tag, TagExt, TagItem, TagType};
use thiserror::Error;

/// Whether ID3v2 tags are written as v2.3 (`true`) or v2.4 (`false`, the
/// default). App-wide preference (Settings › Tag defaults, #79). Kept as a
/// process global rather than threaded through every `write` call because it is
/// a rarely-changed, app-wide default; the app sets it from saved settings via
/// [`set_write_id3v23`].
static WRITE_ID3V23: AtomicBool = AtomicBool::new(false);

/// Set the ID3v2 version future writes use (`true` = v2.3, `false` = v2.4).
pub fn set_write_id3v23(v23: bool) {
    WRITE_ID3V23.store(v23, Ordering::Relaxed);
}

/// The write options for an ID3v2 save, honoring the version preference.
fn id3_write_options() -> WriteOptions {
    WriteOptions::default().use_id3v23(WRITE_ID3V23.load(Ordering::Relaxed))
}

/// User-preferred order for choosing which tag block a multi-tag file is read
/// from (Settings › Tag defaults › Read priority, #84). Empty (the default) =
/// follow lofty's order: the primary tag, then the first present. App-wide,
/// rarely changed, set from saved settings via [`set_read_priority`] — the same
/// process-global model as [`WRITE_ID3V23`]. Held behind a lock rather than an
/// atomic because it is an ordered list, not a flag.
static READ_PRIORITY: RwLock<Vec<TagType>> = RwLock::new(Vec::new());

/// Map a settings key to a lofty [`TagType`]. Unknown keys are ignored so a
/// stale or partial setting can never break reading.
fn tag_type_from_key(key: &str) -> Option<TagType> {
    match key {
        "id3v2" => Some(TagType::Id3v2),
        "id3v1" => Some(TagType::Id3v1),
        "vorbis" => Some(TagType::VorbisComments),
        "ape" => Some(TagType::Ape),
        "mp4" => Some(TagType::Mp4Ilst),
        _ => None,
    }
}

/// Set the app-wide tag-read priority from an ordered list of settings keys
/// (e.g. `["id3v2", "vorbis", "ape"]`). Later reads pick their values from the
/// first listed tag block that is present in the file. An empty list restores
/// the backend's default order.
pub fn set_read_priority(order: &[String]) {
    let mapped: Vec<TagType> = order.iter().filter_map(|k| tag_type_from_key(k)).collect();
    if let Ok(mut guard) = READ_PRIORITY.write() {
        *guard = mapped;
    }
}

/// Choose which tag block to read from, given the tag types `present` in the
/// file and the configured `priority` order. Returns the first prioritized type
/// that is present, or `None` (the caller then falls back to lofty's primary /
/// first tag) when the priority is empty or none of its types are present.
fn choose_priority_type(present: &[TagType], priority: &[TagType]) -> Option<TagType> {
    priority.iter().copied().find(|tt| present.contains(tt))
}

/// Audio container formats we read and write. Covers everything the tag backend
/// (lofty) supports; the preview player already decodes all of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Mp3,
    Flac,
    OggVorbis,
    M4a,
    Aac,
    Aiff,
    Wav,
    Opus,
    Speex,
    Musepack,
    MonkeysAudio,
    WavPack,
}

impl AudioFormat {
    fn from_lofty(file_type: FileType) -> Result<Self, TagIoError> {
        match file_type {
            FileType::Mpeg => Ok(Self::Mp3),
            FileType::Flac => Ok(Self::Flac),
            FileType::Vorbis => Ok(Self::OggVorbis),
            FileType::Mp4 => Ok(Self::M4a),
            FileType::Aac => Ok(Self::Aac),
            FileType::Aiff => Ok(Self::Aiff),
            FileType::Wav => Ok(Self::Wav),
            FileType::Opus => Ok(Self::Opus),
            FileType::Speex => Ok(Self::Speex),
            FileType::Mpc => Ok(Self::Musepack),
            FileType::Ape => Ok(Self::MonkeysAudio),
            FileType::WavPack => Ok(Self::WavPack),
            other => Err(TagIoError::UnsupportedFormat(format!("{other:?}"))),
        }
    }

    /// The tag type each format is read and written through, matching
    /// [`lofty::file::FileType::primary_tag_type`].
    fn primary_tag_type(self) -> TagType {
        match self {
            Self::Mp3 | Self::Aac | Self::Aiff | Self::Wav => TagType::Id3v2,
            Self::Flac | Self::OggVorbis | Self::Opus | Self::Speex => TagType::VorbisComments,
            Self::Musepack | Self::MonkeysAudio | Self::WavPack => TagType::Ape,
            Self::M4a => TagType::Mp4Ilst,
        }
    }

    /// Whether this format keeps its tags in an ID3v2 tag. Those need the
    /// concrete-`Id3v2Tag` write path so binary frames (DJ cue points, ratings,
    /// ReplayGain) survive — lofty's generic tag doesn't even surface them on
    /// read (see [`write_id3v2`] and #52).
    fn uses_id3v2(self) -> bool {
        matches!(self, Self::Mp3 | Self::Aac | Self::Aiff | Self::Wav)
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
    Composer,
    Publisher,
    /// Beats per minute, written to the canonical integer BPM frame so DJ
    /// software reads it.
    Bpm,
    /// International Standard Recording Code — a per-recording identifier, and
    /// the highest-precision key available for matching.
    Isrc,
    /// Musical key of the recording (harmonic mixing).
    InitialKey,
    /// Label catalogue number — also the highest-precision Discogs search key.
    CatalogNumber,
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
            Self::Composer => "composer".to_string(),
            Self::Publisher => "publisher".to_string(),
            Self::Bpm => "bpm".to_string(),
            Self::Isrc => "isrc".to_string(),
            Self::InitialKey => "key".to_string(),
            Self::CatalogNumber => "catalognumber".to_string(),
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
            "composer" => Self::Composer,
            "publisher" => Self::Publisher,
            "bpm" => Self::Bpm,
            "isrc" => Self::Isrc,
            "key" => Self::InitialKey,
            "catalognumber" => Self::CatalogNumber,
            // Only reachable if the database holds a key this build didn't
            // write; preserve it verbatim rather than losing it.
            other => Self::Custom(other.to_string()),
        }
    }
}

/// Field -> value map. BTreeMap keeps a stable, predictable field order for
/// previews and diffs.
pub type TagMap = BTreeMap<TagField, String>;

/// An embedded front-cover image: raw bytes plus its MIME type. Kept out of
/// [`TagMap`] (which is text-only) so binary art doesn't have to pretend to be
/// a string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverArt {
    pub mime: String,
    pub data: Vec<u8>,
}

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

        // Honor the user's read priority (#84) when the file carries more than
        // one tag block: pick the first prioritized block that is present, then
        // fall back to lofty's primary/first tag. Resolve to a `TagType` while
        // the lock is held, then re-fetch the borrow so no guard outlives it.
        let present: Vec<TagType> = tagged_file.tags().iter().map(|t| t.tag_type()).collect();
        let priority_type = READ_PRIORITY
            .read()
            .ok()
            .and_then(|order| choose_priority_type(&present, &order));

        let mut tags = TagMap::new();
        if let Some(tag) = priority_type
            .and_then(|tt| tagged_file.tag(tt))
            .or_else(|| tagged_file.primary_tag())
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
    /// [`TagMap`] is text-only, so a naive "rebuild the tag from the map" write
    /// destroys everything it cannot express: embedded artwork, DJ cue points
    /// and loops, ratings, ReplayGain. Both paths below are written to preserve
    /// that data.
    ///
    /// ID3v2-carrying formats (MP3, AAC, AIFF, WAV) get special handling because
    /// lofty's *generic* [`Tag`] does not even surface frames like `PRIV`/`GEOB`
    /// when reading, so round-tripping through it silently drops them — and
    /// AIFF/WAV are exactly where Serato writes DJ cue points. For those we edit
    /// the concrete tag; other formats round-trip through the generic one.
    ///
    /// Only [`plan::Executor`](crate::plan::Executor) is allowed to call this —
    /// see the crate-level invariant.
    pub fn write(file: &TrackFile) -> Result<(), TagIoError> {
        // Guard the year before touching the file. It is written as a timestamp
        // (ID3v2.4 TDRC / Vorbis DATE / MP4 ©day) whose year MUST be exactly 4
        // digits; lofty accepts a shorter one like "222" on write but then
        // rejects it on the next READ — poisoning the file so it can no longer
        // be listed or edited. Reject it here, leaving the file untouched, so no
        // plan source (edits, transforms, import) can ever corrupt a file.
        if let Some(year) = file.tags.get(&TagField::Year) {
            if !is_writable_year(year) {
                return Err(TagIoError::Malformed(format!(
                    "invalid year {year:?}: a year must be 4 digits (e.g. 1996)"
                )));
            }
        }
        if file.format.uses_id3v2() {
            write_id3v2(&file.path, &file.tags)
        } else {
            write_generic(file)
        }
    }

    /// Read the track's playback duration from its audio properties. Returns
    /// `Duration::ZERO` when the backend can't determine it. Used by the preview
    /// player for the seek bar / total time.
    pub fn read_duration(path: &Path) -> Result<std::time::Duration, TagIoError> {
        let tagged = Probe::open(path)?.guess_file_type()?.read()?;
        Ok(tagged.properties().duration())
    }

    /// Read the file's front cover (or the first embedded image if there's no
    /// explicit front cover), if any.
    pub fn read_cover(path: &Path) -> Result<Option<CoverArt>, TagIoError> {
        let tagged = Probe::open(path)?.guess_file_type()?.read()?;
        let cover = tagged
            .primary_tag()
            .or_else(|| tagged.first_tag())
            .and_then(|tag| {
                tag.get_picture_type(PictureType::CoverFront)
                    .or_else(|| tag.pictures().first())
            })
            .map(|picture| CoverArt {
                mime: picture
                    .mime_type()
                    .map(|mime| mime.as_str().to_string())
                    .unwrap_or_default(),
                data: picture.data().to_vec(),
            });
        Ok(cover)
    }

    /// Embed `cover` as the file's front cover, replacing any existing front
    /// cover and preserving all text tags. Only [`Executor`](crate::plan::Executor)
    /// should call this.
    pub fn embed_cover(path: &Path, cover: &CoverArt) -> Result<(), TagIoError> {
        let picture = Picture::new_unchecked(
            PictureType::CoverFront,
            Some(MimeType::from_str(&cover.mime)),
            None,
            cover.data.clone(),
        );
        // Same reason as `write`: going through the generic tag would drop an
        // MP3's non-representable frames, so edit the concrete tag instead.
        if is_id3v2_container(path) {
            let mut tag = read_id3v2(path)?.unwrap_or_default();
            tag.remove_picture_type(PictureType::CoverFront);
            tag.insert_picture(picture);
            tag.save_to_path(path, id3_write_options())?;
            return Ok(());
        }
        let mut tag = load_or_new_tag(path)?;
        tag.remove_picture_type(PictureType::CoverFront);
        tag.push_picture(picture);
        tag.save_to_path(path, WriteOptions::default())?;
        Ok(())
    }

    /// Remove the file's front cover, preserving all text tags.
    pub fn remove_cover(path: &Path) -> Result<(), TagIoError> {
        if is_id3v2_container(path) {
            let mut tag = read_id3v2(path)?.unwrap_or_default();
            tag.remove_picture_type(PictureType::CoverFront);
            tag.save_to_path(path, id3_write_options())?;
            return Ok(());
        }
        let mut tag = load_or_new_tag(path)?;
        tag.remove_picture_type(PictureType::CoverFront);
        tag.save_to_path(path, WriteOptions::default())?;
        Ok(())
    }
}

/// Write text tags into an MP3's ID3v2 tag, preserving every frame the model
/// cannot express.
///
/// The text frames are produced by converting a generic [`Tag`] built from the
/// map, so the field -> frame mapping stays identical to every other format.
/// Everything else — pictures, `PRIV`/`GEOB` blobs (where DJ software keeps cue
/// points and loops), popularimeter ratings, unsynchronised lyrics — is copied
/// over from the file's existing tag. Text frames are deliberately *not* copied
/// over, so clearing a field still clears it.
fn write_id3v2(path: &Path, tags: &TagMap) -> Result<(), TagIoError> {
    let mut generic = Tag::new(TagType::Id3v2);
    for (field, value) in tags {
        generic.insert_unchecked(TagItem::new(
            tag_field_to_item_key(field),
            ItemValue::Text(value.clone()),
        ));
    }
    let mut updated = Id3v2Tag::from(generic);

    if let Some(original) = read_id3v2(path)? {
        for frame in &original {
            if !is_model_text_frame(frame) {
                updated.insert(frame.clone());
            }
        }
    }
    updated.save_to_path(path, id3_write_options())?;
    Ok(())
}

/// Whether a frame is one the text-only [`TagMap`] already represents, and so
/// must come from the model rather than being carried over from the old tag.
fn is_model_text_frame(frame: &Frame<'_>) -> bool {
    matches!(
        frame,
        Frame::Text(_) | Frame::UserText(_) | Frame::Comment(_)
    )
}

/// The file's concrete ID3v2 tag, if it has one. Dispatches on the container so
/// it works for every ID3v2-carrying format (MP3, AAC, AIFF, WAV), reading the
/// concrete tag lofty's generic representation can't fully reproduce.
fn read_id3v2(path: &Path) -> Result<Option<Id3v2Tag>, TagIoError> {
    let file_type = Probe::open(path)?.guess_file_type()?.file_type();
    let mut file = std::fs::File::open(path)?;
    let options = ParseOptions::new();
    let tag = match file_type {
        Some(FileType::Mpeg) => MpegFile::read_from(&mut file, options)?.id3v2().cloned(),
        Some(FileType::Aac) => AacFile::read_from(&mut file, options)?.id3v2().cloned(),
        Some(FileType::Aiff) => AiffFile::read_from(&mut file, options)?.id3v2().cloned(),
        Some(FileType::Wav) => WavFile::read_from(&mut file, options)?.id3v2().cloned(),
        _ => None,
    };
    Ok(tag)
}

/// Whether `path` is an ID3v2-carrying container, for the cover paths that only
/// have a path to work from. `Id3v2Tag::save_to_path` is container-aware, so
/// the same concrete path serves all of them.
fn is_id3v2_container(path: &Path) -> bool {
    let probed = || -> Result<bool, TagIoError> {
        Ok(matches!(
            Probe::open(path)?.guess_file_type()?.file_type(),
            Some(FileType::Mpeg | FileType::Aac | FileType::Aiff | FileType::Wav)
        ))
    };
    probed().unwrap_or(false)
}

/// Write text tags through the generic tag representation, used by every format
/// other than MP3. Starts from the existing tag so non-text items and pictures
/// survive, and drops only the text items the model no longer carries.
fn write_generic(file: &TrackFile) -> Result<(), TagIoError> {
    let tagged = Probe::open(&file.path)?.guess_file_type()?.read()?;
    let mut tag = tagged
        .primary_tag()
        .or_else(|| tagged.first_tag())
        .cloned()
        .unwrap_or_else(|| Tag::new(file.format.primary_tag_type()));

    let desired: HashSet<ItemKey> = file.tags.keys().map(tag_field_to_item_key).collect();
    tag.retain(|item| item.value().text().is_none() || desired.contains(item.key()));

    for (field, value) in &file.tags {
        let key = tag_field_to_item_key(field);
        // Replace rather than append, so repeated writes can't accumulate
        // duplicate entries for the same field.
        tag.remove_key(&key);
        // `insert_text`/`insert` silently drop `ItemKey::Unknown` (Custom
        // fields) since they refuse to write keys without a known mapping for
        // the tag type. `insert_unchecked` is the documented escape hatch.
        tag.insert_unchecked(TagItem::new(key, ItemValue::Text(value.clone())));
    }
    tag.save_to_path(&file.path, WriteOptions::default())?;
    Ok(())
}

/// Load the file's primary tag (cloned, owned) or a fresh empty one, so the
/// caller can modify pictures and save it back without disturbing text tags.
fn load_or_new_tag(path: &Path) -> Result<Tag, TagIoError> {
    let tagged = Probe::open(path)?.guess_file_type()?.read()?;
    let tag_type = tagged.primary_tag_type();
    Ok(tagged
        .primary_tag()
        .or_else(|| tagged.first_tag())
        .cloned()
        .unwrap_or_else(|| Tag::new(tag_type)))
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
        TagField::Composer => ItemKey::Composer,
        TagField::Publisher => ItemKey::Publisher,
        // `IntegerBpm` is ID3v2's TBPM, which is what DJ software reads;
        // `ItemKey::Bpm` would land in a TXXX frame instead.
        TagField::Bpm => ItemKey::IntegerBpm,
        TagField::Isrc => ItemKey::Isrc,
        TagField::InitialKey => ItemKey::InitialKey,
        TagField::CatalogNumber => ItemKey::CatalogNumber,
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
        ItemKey::Composer => TagField::Composer,
        ItemKey::Publisher => TagField::Publisher,
        // Accept either BPM spelling on read, the same way Year accepts both
        // the legacy and modern frame; writing always picks `IntegerBpm`.
        ItemKey::Bpm | ItemKey::IntegerBpm => TagField::Bpm,
        ItemKey::Isrc => TagField::Isrc,
        ItemKey::InitialKey => TagField::InitialKey,
        ItemKey::CatalogNumber => TagField::CatalogNumber,
        ItemKey::Unknown(key) => TagField::Custom(key.clone()),
        other => TagField::Custom(format!("{other:?}")),
    }
}

/// Whether a `year` value is safe to write. Empty clears the field (allowed).
/// Otherwise the year segment (before any `-MM-DD` date suffix) must be exactly
/// 4 ASCII digits — the one timestamp rule lofty enforces as a hard error on
/// read regardless of parsing mode, so violating it makes the file unreadable.
fn is_writable_year(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    let year = value.split('-').next().unwrap_or(value);
    year.len() == 4 && year.bytes().all(|b| b.is_ascii_digit())
}

/// A non-empty run of ASCII digits (a plain non-negative integer).
fn is_ascii_digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit())
}

/// A non-negative decimal: digits, with at most one `.` separating more digits
/// (e.g. `128` or `128.5`). Used for BPM, which DJ software often stores
/// fractionally.
fn is_decimal(value: &str) -> bool {
    let mut parts = value.split('.');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(int), None, _) => is_ascii_digits(int),
        (Some(int), Some(frac), None) => is_ascii_digits(int) && is_ascii_digits(frac),
        _ => false,
    }
}

/// Whether `value` is a sensible thing to write into `field` — the rule behind
/// the preview's per-field rejection ("don't-be-a-fool" input validation).
///
/// Empty always clears the field. Beyond that only the *typed* fields are
/// constrained, to match what the tag backend does with them:
/// - **year** must be a valid 4-digit timestamp — an invalid one is written as
///   a raw text frame the reader then rejects, corrupting the file (see the
///   hard guard in [`TagEngine::write`]).
/// - **track / track-total / disc** are parsed as integers; a non-numeric value
///   is silently dropped, so require a plain non-negative integer.
/// - **bpm** must be numeric (integer or decimal).
///
/// Every free-text field (artist, title, album, genre, comment, ISRC, key,
/// catalog number, custom …) accepts anything.
pub fn is_writable_value(field: &TagField, value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    match field {
        TagField::Year => is_writable_year(value),
        TagField::TrackNumber | TagField::TrackTotal | TagField::DiscNumber => {
            is_ascii_digits(value)
        }
        TagField::Bpm => is_decimal(value),
        _ => true,
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
            TagField::Composer,
            TagField::Publisher,
            TagField::Bpm,
            TagField::Isrc,
            TagField::InitialKey,
            TagField::CatalogNumber,
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
    fn is_writable_year_requires_exactly_four_digits() {
        assert!(is_writable_year("1996"));
        assert!(is_writable_year("1996-05-01")); // date suffix allowed
        assert!(is_writable_year("")); // clearing the field
        assert!(!is_writable_year("222")); // too short — poisons the file
        assert!(!is_writable_year("96"));
        assert!(!is_writable_year("12345")); // too long
        assert!(!is_writable_year("19x6")); // non-digit
    }

    #[test]
    fn is_writable_value_constrains_typed_fields_only() {
        // Empty clears any field.
        assert!(is_writable_value(&TagField::Year, ""));
        assert!(is_writable_value(&TagField::TrackNumber, ""));

        // Numeric fields require plain integers; BPM also allows a decimal.
        assert!(is_writable_value(&TagField::TrackNumber, "7"));
        assert!(is_writable_value(&TagField::TrackTotal, "12"));
        assert!(is_writable_value(&TagField::DiscNumber, "1"));
        assert!(!is_writable_value(&TagField::TrackNumber, "A1"));
        assert!(!is_writable_value(&TagField::DiscNumber, "one"));
        assert!(is_writable_value(&TagField::Bpm, "128"));
        assert!(is_writable_value(&TagField::Bpm, "128.5"));
        assert!(!is_writable_value(&TagField::Bpm, "128.5.1"));
        assert!(!is_writable_value(&TagField::Bpm, "fast"));

        // Year keeps its 4-digit rule.
        assert!(is_writable_value(&TagField::Year, "1996"));
        assert!(!is_writable_value(&TagField::Year, "222"));

        // Free-text fields accept anything, including things that look numeric
        // or vinyl-ish.
        assert!(is_writable_value(&TagField::Artist, "19x6"));
        assert!(is_writable_value(&TagField::Title, "A1"));
        assert!(is_writable_value(&TagField::Isrc, "GBAYE0601498"));
        assert!(is_writable_value(&TagField::InitialKey, "8A"));
    }

    #[test]
    fn custom_field_round_trips_through_unknown_item_key() {
        let field = TagField::Custom("MOOD".to_string());
        let key = tag_field_to_item_key(&field);
        assert_eq!(key, ItemKey::Unknown("MOOD".to_string()));
        assert_eq!(item_key_to_tag_field(&key), field);
    }

    #[test]
    fn read_priority_keys_map_to_tag_types() {
        assert_eq!(tag_type_from_key("id3v2"), Some(TagType::Id3v2));
        assert_eq!(tag_type_from_key("id3v1"), Some(TagType::Id3v1));
        assert_eq!(tag_type_from_key("vorbis"), Some(TagType::VorbisComments));
        assert_eq!(tag_type_from_key("ape"), Some(TagType::Ape));
        assert_eq!(tag_type_from_key("mp4"), Some(TagType::Mp4Ilst));
        // Unknown / stale keys are ignored, never a hard error.
        assert_eq!(tag_type_from_key("flac"), None);
        assert_eq!(tag_type_from_key(""), None);
    }

    #[test]
    fn choose_priority_type_picks_first_present_in_order() {
        let present = [TagType::Id3v2, TagType::Ape];

        // First listed present type wins, regardless of the file's block order.
        assert_eq!(
            choose_priority_type(&present, &[TagType::Ape, TagType::Id3v2]),
            Some(TagType::Ape)
        );
        assert_eq!(
            choose_priority_type(&present, &[TagType::Id3v2, TagType::Ape]),
            Some(TagType::Id3v2)
        );

        // A prioritized-but-absent type is skipped to the next present one.
        assert_eq!(
            choose_priority_type(&present, &[TagType::VorbisComments, TagType::Id3v2]),
            Some(TagType::Id3v2)
        );

        // No overlap, or an empty priority, means "fall back to lofty's default".
        assert_eq!(
            choose_priority_type(&present, &[TagType::VorbisComments]),
            None
        );
        assert_eq!(choose_priority_type(&present, &[]), None);
    }
}
