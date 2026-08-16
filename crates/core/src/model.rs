//! Tag data model and the tag I/O engine facade.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

use lofty::aac::AacFile;
use lofty::config::{ParseOptions, ParsingMode, WriteOptions};
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

/// The text that joins several values of one field into the single string
/// [`TagMap`] holds, and splits them apart again on write (#46).
pub const DEFAULT_MULTI_VALUE_SEPARATOR: &str = "; ";

/// The configured separator, empty until the app sets one. App-wide and rarely
/// changed, the same process-global model as [`WRITE_ID3V23`] and
/// [`READ_PRIORITY`], behind a lock because it is a string.
static MULTI_VALUE_SEPARATOR: RwLock<String> = RwLock::new(String::new());

/// Set the separator multi-value fields are joined with on read and split on
/// write (#46). An empty string restores [`DEFAULT_MULTI_VALUE_SEPARATOR`].
pub fn set_multi_value_separator(separator: &str) {
    if let Ok(mut guard) = MULTI_VALUE_SEPARATOR.write() {
        *guard = separator.to_string();
    }
}

/// The separator in force. Falls back to the default when unset, and — because
/// splitting on an empty string would explode every value into characters —
/// whenever the configured one is blank.
pub fn multi_value_separator() -> String {
    MULTI_VALUE_SEPARATOR
        .read()
        .ok()
        .filter(|s| !s.is_empty())
        .map(|s| s.clone())
        .unwrap_or_else(|| DEFAULT_MULTI_VALUE_SEPARATOR.to_string())
}

/// Split a stored value back into the individual values to write (#46).
/// Whitespace around a part is separator formatting, never part of the value;
/// an empty part is a stray separator, not a value to write. A value carrying
/// no separator comes back as itself, so the ordinary single-value case is
/// untouched.
///
/// Takes the separator rather than reading the global so it can be tested
/// without a process-wide setting the rest of the suite would race against.
fn split_multi_value_with(value: &str, separator: &str) -> Vec<String> {
    let parts: Vec<String> = value
        .split(separator)
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect();
    if parts.is_empty() {
        // The whole value was separators and space. Keep it verbatim rather
        // than writing nothing, so nothing is silently dropped here either.
        return vec![value.to_string()];
    }
    parts
}

fn split_multi_value(value: &str) -> Vec<String> {
    split_multi_value_with(value, &multi_value_separator())
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

    /// A short, display-ready name for the container (#147) — what `%_codec%`
    /// renders and what a UI shows in a format column. Deliberately the common
    /// spelling rather than the variant name (`APE`, not `MonkeysAudio`).
    pub fn name(self) -> &'static str {
        match self {
            Self::Mp3 => "MP3",
            Self::Flac => "FLAC",
            Self::OggVorbis => "Vorbis",
            Self::M4a => "M4A",
            Self::Aac => "AAC",
            Self::Aiff => "AIFF",
            Self::Wav => "WAV",
            Self::Opus => "Opus",
            Self::Speex => "Speex",
            Self::Musepack => "Musepack",
            Self::MonkeysAudio => "APE",
            Self::WavPack => "WavPack",
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
    /// Total number of discs in the set — the "of N" half of the disc pairing,
    /// written to the standard disc-total frame (ID3v2 `TPOS` total, Vorbis
    /// `DISCTOTAL`). Mirrors [`TrackTotal`](Self::TrackTotal).
    DiscTotal,
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
    /// A webpage for the release/recording, written to the ID3v2 `WOAF` URL
    /// frame (a proper URL link, not plain text) so players treat it as a link.
    Url,
    /// Physical/source medium of the release (Vinyl / CD / Cassette / File),
    /// written to the standard media frame (ID3v2 `TMED`, Vorbis `MEDIA`). Drives
    /// the vinyl side-notation view (#106).
    MediaType,
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
            Self::DiscTotal => "disctotal".to_string(),
            Self::Year => "year".to_string(),
            Self::Genre => "genre".to_string(),
            Self::Comment => "comment".to_string(),
            Self::Composer => "composer".to_string(),
            Self::Publisher => "publisher".to_string(),
            Self::Bpm => "bpm".to_string(),
            Self::Isrc => "isrc".to_string(),
            Self::InitialKey => "key".to_string(),
            Self::CatalogNumber => "catalognumber".to_string(),
            Self::Url => "url".to_string(),
            Self::MediaType => "media".to_string(),
            Self::Custom(name) => format!("custom:{name}"),
        }
    }

    /// Whether this field genuinely holds several values (#46).
    ///
    /// A track can have two artists and three genres, and both ID3v2.4 (several
    /// values in one frame) and Vorbis comments (a repeated key) can say so.
    /// Those values are joined into one string for [`TagMap`] and split apart
    /// again on write — see [`multi_value_separator`].
    ///
    /// The list is deliberately short. Splitting a field that is *not* on it
    /// would turn a title reading `Hello; Goodbye` into two titles, and the tags
    /// that are repeatable in practice are the credit and classification ones.
    /// Every other field keeps its previous behaviour exactly: the last value
    /// present wins on read, one value is written.
    pub fn is_multi_value(&self) -> bool {
        matches!(
            self,
            Self::Artist | Self::AlbumArtist | Self::Genre | Self::Composer
        )
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
            "disctotal" => Self::DiscTotal,
            "year" => Self::Year,
            "genre" => Self::Genre,
            "comment" => Self::Comment,
            "composer" => Self::Composer,
            "publisher" => Self::Publisher,
            "bpm" => Self::Bpm,
            "isrc" => Self::Isrc,
            "key" => Self::InitialKey,
            "catalognumber" => Self::CatalogNumber,
            "url" => Self::Url,
            "media" => Self::MediaType,
            // Only reachable if the database holds a key this build didn't
            // write; preserve it verbatim rather than losing it.
            other => Self::Custom(other.to_string()),
        }
    }
}

/// Field -> value map. BTreeMap keeps a stable, predictable field order for
/// previews and diffs.
pub type TagMap = BTreeMap<TagField, String>;

/// What an embedded image depicts (#56). Every tag format we write can hold
/// several pictures distinguished by exactly this, so it is part of the image
/// rather than something a caller decides on the way past.
///
/// A deliberately short list: the handful of types that appear on real releases
/// and that a person would pick from a menu. Everything else lofty knows about —
/// and anything a file happens to carry — reads back as [`Other`](Self::Other)
/// rather than being dropped, so a round trip never loses the image itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CoverKind {
    /// The front cover — what "the cover" means everywhere else in the app.
    #[default]
    Front,
    Back,
    /// The disc, cassette or record itself.
    Media,
    Leaflet,
    /// A picture of the artist or band.
    Artist,
    /// The label's or publisher's logo.
    Logo,
    Icon,
    Other,
}

impl CoverKind {
    /// Lossless string encoding, for the DTO boundary and the journal.
    pub fn to_storage_key(self) -> &'static str {
        match self {
            Self::Front => "front",
            Self::Back => "back",
            Self::Media => "media",
            Self::Leaflet => "leaflet",
            Self::Artist => "artist",
            Self::Logo => "logo",
            Self::Icon => "icon",
            Self::Other => "other",
        }
    }

    /// Inverse of [`to_storage_key`](Self::to_storage_key). An unknown key is
    /// `Other`, never an error: a journal or a frontend from another build must
    /// not be able to make an image unreadable.
    pub fn from_storage_key(key: &str) -> Self {
        match key {
            "front" => Self::Front,
            "back" => Self::Back,
            "media" => Self::Media,
            "leaflet" => Self::Leaflet,
            "artist" => Self::Artist,
            "logo" => Self::Logo,
            "icon" => Self::Icon,
            _ => Self::Other,
        }
    }

    fn to_picture_type(self) -> PictureType {
        match self {
            Self::Front => PictureType::CoverFront,
            Self::Back => PictureType::CoverBack,
            Self::Media => PictureType::Media,
            Self::Leaflet => PictureType::Leaflet,
            Self::Artist => PictureType::Artist,
            Self::Logo => PictureType::PublisherLogo,
            Self::Icon => PictureType::Icon,
            Self::Other => PictureType::Other,
        }
    }

    fn from_picture_type(picture_type: PictureType) -> Self {
        match picture_type {
            PictureType::CoverFront => Self::Front,
            PictureType::CoverBack => Self::Back,
            PictureType::Media => Self::Media,
            PictureType::Leaflet => Self::Leaflet,
            // Three ways of saying "a person on the release"; one menu entry.
            PictureType::Artist | PictureType::LeadArtist | PictureType::Band => Self::Artist,
            PictureType::PublisherLogo | PictureType::BandLogo => Self::Logo,
            PictureType::Icon | PictureType::OtherIcon => Self::Icon,
            _ => Self::Other,
        }
    }
}

/// An embedded image: raw bytes, its MIME type, what it depicts and its
/// description (#56). Kept out of [`TagMap`] (which is text-only) so binary art
/// doesn't have to pretend to be a string.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CoverArt {
    pub mime: String,
    pub data: Vec<u8>,
    /// What the image depicts. Defaults to the front cover, which is what a
    /// picked or fetched image is meant as unless said otherwise.
    pub kind: CoverKind,
    /// The description the format stores beside the picture. Usually empty;
    /// carried so a round trip doesn't quietly drop it.
    pub description: String,
}

/// A file's audio properties beyond its tags (#40).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioProps {
    pub duration_secs: u64,
    /// Audio bitrate in kbps, when the backend reports one.
    pub bitrate_kbps: Option<u32>,
    /// Sample rate in Hz, when the backend reports one (#147).
    pub sample_rate_hz: Option<u32>,
    /// Channel count, when the backend reports one (#147).
    pub channels: Option<u8>,
}

/// A single audio file as seen by the table model.
#[derive(Debug, Clone)]
pub struct TrackFile {
    pub path: PathBuf,
    pub format: AudioFormat,
    pub tags: TagMap,
}

/// Every read of a file goes through here (#183).
///
/// The backend's default parsing mode rejects a whole file over one frame it
/// cannot make sense of — a year of three digits is enough — and a tag editor
/// refusing to open exactly the files that need fixing is the wrong way round.
/// Relaxed mode skips the frame it cannot read and hands over the rest, so the
/// title, the artist, the key and the tempo are there to see and to repair. The
/// malformed frame is simply absent from the model, and any ordinary save
/// rewrites the tag's text frames from the model — which fixes the file.
///
/// It has to be ONE place: tags, properties, covers, the concrete ID3v2 tag and
/// the container check all parse the same file, and if they disagreed a file
/// would read but then fail to save.
fn parse_options() -> ParseOptions {
    ParseOptions::new().parsing_mode(ParsingMode::Relaxed)
}

fn probe_file(path: &Path) -> Result<lofty::file::TaggedFile, TagIoError> {
    Ok(Probe::open(path)?
        .guess_file_type()?
        .options(parse_options())
        .read()?)
}

/// Facade over the tag I/O backend (lofty). The rest of the codebase must
/// never touch the backend directly — this is the only place format-specific
/// knowledge is allowed to live.
pub struct TagEngine;

impl TagEngine {
    /// Read tags from a file on disk.
    pub fn read(path: &Path) -> Result<TrackFile, TagIoError> {
        Ok(Self::read_with_props(path)?.0)
    }

    /// Read the tags **and** the technical properties from one probe (#172).
    ///
    /// The file listing needs both — the tags for the columns, the duration for
    /// the Length column — and parsing it twice would double the cost of opening
    /// a library of thousands of files for a value the first parse already had.
    /// [`read`](Self::read) is this without the second half.
    pub fn read_with_props(path: &Path) -> Result<(TrackFile, AudioProps), TagIoError> {
        let tagged_file = probe_file(path)?;
        let format = AudioFormat::from_lofty(tagged_file.file_type())?;
        let properties = tagged_file.properties();
        let props = AudioProps {
            duration_secs: properties.duration().as_secs(),
            bitrate_kbps: properties.audio_bitrate(),
            sample_rate_hz: properties.sample_rate(),
            channels: properties.channels(),
        };

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
                // Text items (most fields), plus the release URL — its value is a
                // URL *locator*, not text. Only the one URL frame the model owns
                // (WOAF / AudioFileUrl) is read; other URL frames are left alone
                // so they survive as non-model frames instead of becoming text.
                let value = match item.value() {
                    ItemValue::Text(text) => Some(text.clone()),
                    ItemValue::Locator(url) if matches!(item.key(), ItemKey::AudioFileUrl) => {
                        Some(url.clone())
                    }
                    _ => None,
                };
                if let Some(value) = value {
                    let (field, from_alias) = field_for_item_key(item.key());
                    // A legacy spelling never overrides the standard frame
                    // (#171). Both can be in one file — another tagger's
                    // `TXXX:Label` next to the `TPUB` an import just wrote — and
                    // the standard one is the current value.
                    if from_alias && tags.contains_key(&field) {
                        continue;
                    }
                    // A file really can carry several artists or genres, as
                    // separate Vorbis comments or as one multi-value ID3v2.4
                    // frame — lofty surfaces both as repeated items here. Joining
                    // them is what keeps the extras (#46); overwriting, as this
                    // did, kept only the last and any edit then wrote that one
                    // value back over all of them. Fields that are not
                    // multi-valued keep the old last-wins behaviour.
                    match tags.entry(field) {
                        std::collections::btree_map::Entry::Vacant(slot) => {
                            slot.insert(value);
                        }
                        std::collections::btree_map::Entry::Occupied(mut slot) => {
                            if slot.key().is_multi_value() {
                                let joined =
                                    format!("{}{}{}", slot.get(), multi_value_separator(), value);
                                slot.insert(joined);
                            } else {
                                slot.insert(value);
                            }
                        }
                    }
                }
            }
        }

        Ok((
            TrackFile {
                path: path.to_path_buf(),
                format,
                tags,
            },
            props,
        ))
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
        let tagged = probe_file(path)?;
        Ok(tagged.properties().duration())
    }

    /// Read the technical audio properties from one probe (#40): duration and
    /// bitrate, the two the duplicate view needs beyond file size, plus sample
    /// rate and channel count for the `%_samplerate%` / `%_channels%` mask
    /// placeholders (#147). One probe, so a mask asking for several of them
    /// costs no more than asking for one.
    pub fn read_audio_props(path: &Path) -> Result<AudioProps, TagIoError> {
        let tagged = probe_file(path)?;
        let props = tagged.properties();
        Ok(AudioProps {
            duration_secs: props.duration().as_secs(),
            bitrate_kbps: props.audio_bitrate(),
            sample_rate_hz: props.sample_rate(),
            channels: props.channels(),
        })
    }

    /// Every image the file carries (#56), in the order the tag holds them.
    pub fn read_covers(path: &Path) -> Result<Vec<CoverArt>, TagIoError> {
        let tagged = probe_file(path)?;
        let covers = tagged
            .primary_tag()
            .or_else(|| tagged.first_tag())
            .map(|tag| tag.pictures().iter().map(cover_from_picture).collect())
            .unwrap_or_default();
        Ok(covers)
    }

    /// The file's front cover (or the first embedded image if there's no
    /// explicit front cover), if any — "the cover" as the rest of the app means
    /// it, over the set [`read_covers`](Self::read_covers) returns.
    pub fn read_cover(path: &Path) -> Result<Option<CoverArt>, TagIoError> {
        let covers = Self::read_covers(path)?;
        Ok(covers
            .iter()
            .find(|cover| cover.kind == CoverKind::Front)
            .or_else(|| covers.first())
            .cloned())
    }

    /// Replace the file's images with `covers`, preserving all text tags (#56).
    ///
    /// The whole set at once, rather than per-image operations: there is no
    /// stable identity for an embedded picture to address, and writing the set
    /// makes add / replace / remove / reorder / retype one operation whose undo
    /// is simply the previous set. An empty slice removes every image.
    ///
    /// Only [`Executor`](crate::plan::Executor) should call this.
    pub fn write_covers(path: &Path, covers: &[CoverArt]) -> Result<(), TagIoError> {
        // Same reason as `write`: going through the generic tag would drop an
        // MP3's non-representable frames, so edit the concrete tag instead.
        if is_id3v2_container(path) {
            let mut tag = read_id3v2(path)?.unwrap_or_default();
            // `remove_picture_type` takes one type at a time and the file may
            // hold types the model maps onto one of ours, so clear by rebuilding
            // from the frames that are not pictures at all.
            tag.retain(|frame| !matches!(frame, Frame::Picture(_)));
            for cover in covers {
                tag.insert_picture(picture_from_cover(cover));
            }
            tag.save_to_path(path, id3_write_options())?;
            return Ok(());
        }
        let mut tag = load_or_new_tag(path)?;
        while !tag.pictures().is_empty() {
            tag.remove_picture(0);
        }
        for cover in covers {
            tag.push_picture(picture_from_cover(cover));
        }
        tag.save_to_path(path, WriteOptions::default())?;
        Ok(())
    }
}

fn cover_from_picture(picture: &Picture) -> CoverArt {
    CoverArt {
        mime: picture
            .mime_type()
            .map(|mime| mime.as_str().to_string())
            .unwrap_or_default(),
        data: picture.data().to_vec(),
        kind: CoverKind::from_picture_type(picture.pic_type()),
        description: picture.description().unwrap_or_default().to_string(),
    }
}

fn picture_from_cover(cover: &CoverArt) -> Picture {
    Picture::new_unchecked(
        cover.kind.to_picture_type(),
        Some(MimeType::from_str(&cover.mime)),
        Some(cover.description.clone()).filter(|d| !d.is_empty()),
        cover.data.clone(),
    )
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
        push_field_items(&mut generic, field, value);
    }
    // `Id3v2Tag::from` joins repeated items of one key into a single
    // multi-value frame with ID3v2.4's own separator, so the several items
    // pushed above become one well-formed TPE1/TCON rather than duplicate
    // frames (#46).
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
    // TDRC (recording time) is a `Timestamp` frame, not `Text`, but the model
    // round-trips it as `Year`. Without excluding the original, a file left with
    // a stale/duplicate TDRC (e.g. two TDRC frames from another tagger) leaks the
    // old year back on write and overwrites the value we just set.
    matches!(
        frame,
        Frame::Text(_) | Frame::UserText(_) | Frame::Comment(_)
    ) || matches!(frame.id().as_str(), "TDRC" | "WOAF")
}

/// The file's concrete ID3v2 tag, if it has one. Dispatches on the container so
/// it works for every ID3v2-carrying format (MP3, AAC, AIFF, WAV), reading the
/// concrete tag lofty's generic representation can't fully reproduce.
fn read_id3v2(path: &Path) -> Result<Option<Id3v2Tag>, TagIoError> {
    let file_type = probe_file(path)?.file_type();
    let mut file = std::fs::File::open(path)?;
    let options = parse_options();
    // `probe_file` resolves the type, so these are the concrete kinds rather
    // than an `Option` of them.
    let tag = match file_type {
        FileType::Mpeg => MpegFile::read_from(&mut file, options)?.id3v2().cloned(),
        FileType::Aac => AacFile::read_from(&mut file, options)?.id3v2().cloned(),
        FileType::Aiff => AiffFile::read_from(&mut file, options)?.id3v2().cloned(),
        FileType::Wav => WavFile::read_from(&mut file, options)?.id3v2().cloned(),
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
            probe_file(path)?.file_type(),
            FileType::Mpeg | FileType::Aac | FileType::Aiff | FileType::Wav
        ))
    };
    probed().unwrap_or(false)
}

/// Write text tags through the generic tag representation, used by every format
/// other than MP3. Starts from the existing tag so non-text items and pictures
/// survive, and drops only the text items the model no longer carries.
fn write_generic(file: &TrackFile) -> Result<(), TagIoError> {
    let tagged = probe_file(&file.path)?;
    let mut tag = tagged
        .primary_tag()
        .or_else(|| tagged.first_tag())
        .cloned()
        .unwrap_or_else(|| Tag::new(file.format.primary_tag_type()));

    let tag_type = tag.tag_type();
    let desired: HashSet<ItemKey> = file
        .tags
        .keys()
        .map(|field| item_key_for(field, tag_type))
        .collect();
    tag.retain(|item| item.value().text().is_none() || desired.contains(item.key()));

    for (field, value) in &file.tags {
        // Clear first rather than append, so repeated writes can't accumulate
        // duplicate entries for the same field — then push what the model says,
        // which for a multi-value field is more than one item (#46).
        tag.remove_key(&item_key_for(field, tag_type));
        push_field_items(&mut tag, field, value);
    }
    tag.save_to_path(&file.path, WriteOptions::default())?;
    Ok(())
}

/// Load the file's primary tag (cloned, owned) or a fresh empty one, so the
/// caller can modify pictures and save it back without disturbing text tags.
fn load_or_new_tag(path: &Path) -> Result<Tag, TagIoError> {
    let tagged = probe_file(path)?;
    let tag_type = tagged.primary_tag_type();
    Ok(tagged
        .primary_tag()
        .or_else(|| tagged.first_tag())
        .cloned()
        .unwrap_or_else(|| Tag::new(tag_type)))
}

/// Push one field's stored string onto `tag` as the items to write: several for
/// a multi-value field carrying several values (#46), one otherwise. The caller
/// is responsible for having cleared any previous items of the key.
///
/// `push_unchecked` rather than `insert_unchecked`, because inserting *replaces*
/// every item of the key and would leave only the last value. The `unchecked`
/// half is unchanged and still needed: the checked calls silently drop
/// `ItemKey::Unknown` (`Custom` fields), which have no mapping to verify.
fn push_field_items(tag: &mut Tag, field: &TagField, value: &str) {
    let key = item_key_for(field, tag.tag_type());
    if !field.is_multi_value() {
        tag.push_unchecked(TagItem::new(key, item_value_for(field, value)));
        return;
    }
    for part in split_multi_value(value) {
        tag.push_unchecked(TagItem::new(key.clone(), item_value_for(field, &part)));
    }
}

/// The `ItemValue` a field's string should be written as. Almost everything is
/// plain `Text`; the URL field is a `Locator` so it lands in a real URL frame
/// (ID3v2 `WOAF`) that players recognise as a link, not a text frame.
fn item_value_for(field: &TagField, value: &str) -> ItemValue {
    match field {
        TagField::Url => ItemValue::Locator(value.to_string()),
        _ => ItemValue::Text(value.to_string()),
    }
}

/// The field an item belongs to, and whether it got there through a legacy
/// spelling rather than the standard one (#171) — which decides who wins when a
/// file carries both.
fn field_for_item_key(key: &ItemKey) -> (TagField, bool) {
    if let ItemKey::Unknown(name) = key {
        if let Some(field) = legacy_alias_field(name) {
            return (field, true);
        }
    }
    (item_key_to_tag_field(key), false)
}

/// User-text (`TXXX`) names other taggers use for fields that have a standard
/// frame of their own (#171). Folding them in means a file stops stating two
/// labels or two media types, and — because a `TXXX` that is not in the model is
/// not carried over — the stale one goes away the next time the file is written.
///
/// Deliberately small, and only for names that mean one unambiguous thing. `KEY`
/// and `BPM` are **not** here on purpose: DJ tools write those in notations that
/// differ from the standard frames (Camelot against musical, decimal against
/// integer), so folding them would silently pick one and drop the other.
/// Anything not listed keeps round-tripping as a `Custom` field.
fn legacy_alias_field(name: &str) -> Option<TagField> {
    match name.trim().to_ascii_uppercase().as_str() {
        "LABEL" | "ORGANIZATION" => Some(TagField::Publisher),
        "COUNTRY" => Some(TagField::Custom("RELEASECOUNTRY".to_string())),
        "ORIGINALMEDIATYPE" | "MEDIA" | "MEDIATYPE" => Some(TagField::MediaType),
        _ => None,
    }
}

/// The item key a field is *written* under, for the tag type receiving it
/// (#165).
///
/// Almost every field maps the same way everywhere, and for those this is
/// exactly [`tag_field_to_item_key`]. BPM is the exception, and the reason this
/// function exists: an `ItemKey` the target tag type has no mapping for is
/// **silently discarded when the tag is saved** — the same trap the `Year`
/// mapping documents above — and `IntegerBpm` (ID3v2's `TBPM`, MP4's `tmpo`) has
/// no Vorbis Comments mapping, so every tempo written to a FLAC or an Ogg
/// vanished. Vorbis spells it plain `BPM`, which is `ItemKey::Bpm`, and read
/// folds both back into [`TagField::Bpm`], so the value round-trips either way.
///
/// Auditing the whole table against lofty's maps (the test below) turned up two
/// more of the same, so they are fixed here too rather than left to be
/// discovered one field at a time:
///
/// - **the label on MP4** — `Publisher` maps only on ID3v2 and Vorbis, so a
///   label written to an M4A was dropped. `Label` is the key that maps
///   everywhere, and both read back as [`TagField::Publisher`], so it is used
///   where `Publisher` doesn't reach.
/// - **the year on APE** — APE spells it `Year`, and has no `RecordingDate`.
///
/// What remains unwritable is BPM and the musical key on APE tags, which map
/// neither spelling. That is lofty's mapping rather than a choice made here, and
/// Musepack / Monkey's Audio / WavPack are the exotic tail;
/// `every_written_field_maps_onto_every_tag_type` pins those two as the known
/// holes so nothing else can join them unnoticed.
fn item_key_for(field: &TagField, tag_type: TagType) -> ItemKey {
    match field {
        TagField::Bpm if tag_type == TagType::VorbisComments => ItemKey::Bpm,
        TagField::Publisher if matches!(tag_type, TagType::Mp4Ilst | TagType::Ape) => {
            ItemKey::Label
        }
        TagField::Year if tag_type == TagType::Ape => ItemKey::Year,
        other => tag_field_to_item_key(other),
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
        TagField::DiscTotal => ItemKey::DiscTotal,
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
        // `ItemKey::Bpm` would land in a TXXX frame instead. Not every tag type
        // can hold it though — see [`item_key_for`], which is what the write
        // path actually calls.
        TagField::Bpm => ItemKey::IntegerBpm,
        TagField::Isrc => ItemKey::Isrc,
        TagField::InitialKey => ItemKey::InitialKey,
        TagField::CatalogNumber => ItemKey::CatalogNumber,
        // WOAF (Official audio file webpage). Its value is a URL locator, not
        // text — the write path uses `ItemValue::Locator` for this field.
        TagField::Url => ItemKey::AudioFileUrl,
        // Standard media frame: ID3v2 `TMED`, Vorbis `MEDIA`, MP4 `MEDIA`.
        TagField::MediaType => ItemKey::OriginalMediaType,
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
        ItemKey::DiscTotal => TagField::DiscTotal,
        // `Year` (ID3v2.3 TYER) is legacy; real-world taggers (verified
        // against files tagged by the common Windows ones) overwhelmingly
        // write the year into `RecordingDate` (ID3v2.4 TDRC) instead. Writing
        // still targets `Year` alone — see `tag_field_to_item_key`.
        ItemKey::Year | ItemKey::RecordingDate => TagField::Year,
        ItemKey::Genre => TagField::Genre,
        ItemKey::Comment => TagField::Comment,
        ItemKey::Composer => TagField::Composer,
        // In ID3v2, lofty maps the publisher frame (TPUB) to `ItemKey::Label`,
        // not `ItemKey::Publisher` — so a `Publisher` we wrote reads back as
        // `Label`. Accept both so it round-trips instead of landing in a stray
        // `Custom("Label")` field (and dropping out of the Publisher column).
        ItemKey::Publisher | ItemKey::Label => TagField::Publisher,
        // Accept either BPM spelling on read, the same way Year accepts both
        // the legacy and modern frame; which one a write uses depends on what
        // the tag type can hold (#165, `item_key_for`).
        ItemKey::Bpm | ItemKey::IntegerBpm => TagField::Bpm,
        ItemKey::Isrc => TagField::Isrc,
        ItemKey::InitialKey => TagField::InitialKey,
        ItemKey::CatalogNumber => TagField::CatalogNumber,
        ItemKey::AudioFileUrl => TagField::Url,
        ItemKey::OriginalMediaType => TagField::MediaType,
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
        TagField::TrackNumber
        | TagField::TrackTotal
        | TagField::DiscNumber
        | TagField::DiscTotal => is_ascii_digits(value),
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
            TagField::DiscTotal,
            TagField::Year,
            TagField::Genre,
            TagField::Comment,
            TagField::Composer,
            TagField::Publisher,
            TagField::Bpm,
            TagField::Isrc,
            TagField::InitialKey,
            TagField::CatalogNumber,
            TagField::MediaType,
        ]
    }

    /// The class of bug behind #165: a tag saves without complaint while
    /// dropping any item whose key has no mapping for that tag type, so the
    /// field is simply missing afterwards. `Year` hit it once (ID3v2.4 has no
    /// plain year frame) and BPM hit it again (Vorbis has no `TBPM`), both
    /// silently. This asks lofty directly, for every field and every tag type
    /// the app writes, whether the key it is about to use can be stored at all.
    #[test]
    fn every_written_field_maps_onto_every_tag_type() {
        let tag_types = [
            TagType::Id3v2,
            TagType::VorbisComments,
            TagType::Mp4Ilst,
            TagType::Ape,
        ];
        let mut holes = Vec::new();
        for tag_type in tag_types {
            for field in known_fields() {
                let key = item_key_for(&field, tag_type);
                if key.map_key(tag_type, false).is_none() {
                    holes.push(format!("{field:?} on {tag_type:?}"));
                }
            }
        }
        // The known holes: APE tags map neither a BPM nor a musical-key field
        // in lofty. Musepack / Monkey's Audio / WavPack are the exotic tail, and
        // inventing a spelling for them is a separate decision — but nothing
        // else may join this list without being noticed.
        assert_eq!(
            holes,
            vec!["Bpm on Ape".to_string(), "InitialKey on Ape".to_string()],
            "silently unwritable"
        );
    }

    #[test]
    fn the_tempo_is_written_under_the_key_each_tag_type_can_hold() {
        // ID3v2 and MP4 keep an integer BPM (`TBPM` / `tmpo`); Vorbis spells it
        // plain `BPM`, which is a different `ItemKey` — using the first there
        // was #165.
        assert_eq!(
            item_key_for(&TagField::Bpm, TagType::Id3v2),
            ItemKey::IntegerBpm
        );
        assert_eq!(
            item_key_for(&TagField::Bpm, TagType::Mp4Ilst),
            ItemKey::IntegerBpm
        );
        assert_eq!(
            item_key_for(&TagField::Bpm, TagType::VorbisComments),
            ItemKey::Bpm
        );
        // Both spellings read back as the same field, so the value round-trips
        // whichever one the file carries.
        assert_eq!(item_key_to_tag_field(&ItemKey::Bpm), TagField::Bpm);
        assert_eq!(item_key_to_tag_field(&ItemKey::IntegerBpm), TagField::Bpm);
        // Every other field on Vorbis is untouched by the tag type.
        for field in known_fields().into_iter().filter(|f| *f != TagField::Bpm) {
            assert_eq!(
                item_key_for(&field, TagType::VorbisComments),
                tag_field_to_item_key(&field)
            );
        }
    }

    #[test]
    fn legacy_user_text_names_fold_into_the_field_they_mean() {
        let alias = |name: &str| field_for_item_key(&ItemKey::Unknown(name.to_string()));
        assert_eq!(alias("Label"), (TagField::Publisher, true));
        assert_eq!(alias("ORGANIZATION"), (TagField::Publisher, true));
        assert_eq!(alias("OriginalMediaType"), (TagField::MediaType, true));
        assert_eq!(alias("media"), (TagField::MediaType, true));
        assert_eq!(
            alias("COUNTRY"),
            (TagField::Custom("RELEASECOUNTRY".to_string()), true)
        );
        // The field we already write keeps its own name, and is not an alias.
        assert_eq!(
            alias("RELEASECOUNTRY"),
            (TagField::Custom("RELEASECOUNTRY".to_string()), false)
        );
        // Anything else is still an ordinary custom field.
        assert_eq!(
            alias("TAGREX_CUSTOM_TEST"),
            (TagField::Custom("TAGREX_CUSTOM_TEST".to_string()), false)
        );
        // Notation-dependent fields stay out of the table on purpose: a DJ tool's
        // TXXX:KEY may be Camelot where TKEY is musical, and folding them would
        // drop one of the two.
        assert_eq!(alias("KEY"), (TagField::Custom("KEY".to_string()), false));
        assert_eq!(alias("BPM"), (TagField::Custom("BPM".to_string()), false));
        // A standard key is untouched by any of this.
        assert_eq!(
            field_for_item_key(&ItemKey::Publisher),
            (TagField::Publisher, false)
        );
    }

    #[test]
    fn the_label_and_the_year_use_the_spelling_the_tag_type_knows() {
        // `Publisher` has no MP4 mapping, so an M4A silently lost its label
        // (#165); `Label` maps there and reads back as the same field.
        assert_eq!(
            item_key_for(&TagField::Publisher, TagType::Mp4Ilst),
            ItemKey::Label
        );
        assert_eq!(item_key_to_tag_field(&ItemKey::Label), TagField::Publisher);
        // Where `Publisher` does map, it is left alone — changing the Vorbis
        // spelling would move the value to a different comment name.
        assert_eq!(
            item_key_for(&TagField::Publisher, TagType::VorbisComments),
            ItemKey::Publisher
        );
        assert_eq!(
            item_key_for(&TagField::Publisher, TagType::Id3v2),
            ItemKey::Publisher
        );
        // APE spells the year plainly and has no recording date.
        assert_eq!(item_key_for(&TagField::Year, TagType::Ape), ItemKey::Year);
        assert_eq!(
            item_key_for(&TagField::Year, TagType::Id3v2),
            ItemKey::RecordingDate
        );
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
    fn only_the_credit_and_classification_fields_are_multi_value() {
        // Several artists / genres on one track is ordinary; several titles or
        // years is not, and splitting those would corrupt a value that happens
        // to contain the separator.
        assert!(TagField::Artist.is_multi_value());
        assert!(TagField::AlbumArtist.is_multi_value());
        assert!(TagField::Genre.is_multi_value());
        assert!(TagField::Composer.is_multi_value());
        for field in [
            TagField::Title,
            TagField::Album,
            TagField::Comment,
            TagField::Year,
            TagField::TrackNumber,
            TagField::Isrc,
            TagField::Custom("MOOD".to_string()),
        ] {
            assert!(!field.is_multi_value(), "{field:?} must not be split");
        }
    }

    #[test]
    fn splitting_a_stored_value_drops_the_separator_formatting() {
        let sep = DEFAULT_MULTI_VALUE_SEPARATOR;
        assert_eq!(
            split_multi_value_with("Autechre; Gescom", sep),
            vec!["Autechre", "Gescom"]
        );
        // A single value is itself, untouched — the ordinary case.
        assert_eq!(split_multi_value_with("Autechre", sep), vec!["Autechre"]);
        // Whitespace around a part is the separator's, not the value's, and a
        // stray separator contributes no value.
        assert_eq!(
            split_multi_value_with("Rock ;  Pop ; ; Jazz", "; "),
            vec!["Rock", "Pop", "Jazz"]
        );
        assert_eq!(split_multi_value_with("A;B", sep), vec!["A;B"]);
        // Nothing but separators still writes something rather than vanishing.
        assert_eq!(split_multi_value_with("; ", sep), vec!["; "]);
        assert_eq!(split_multi_value_with("", sep), vec![""]);
    }

    #[test]
    fn an_empty_configured_separator_falls_back_to_the_default() {
        // Splitting on "" would explode every value into characters, so a blank
        // setting has to mean "the default", not "no separator".
        assert_eq!(multi_value_separator(), DEFAULT_MULTI_VALUE_SEPARATOR);
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
