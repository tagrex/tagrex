//! Tag data model and the tag I/O engine facade.

use std::collections::{BTreeMap, HashSet};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

use lofty::aac::AacFile;
use lofty::ape::{ApeFile, ApeItem, ApeTag, APE_PICTURE_TYPES};
use lofty::config::{ParseOptions, ParsingMode, WriteOptions};
use lofty::error::{FileEncodingError, FileParseError, TagEncodingError};
use lofty::file::{AudioFile, FileType, TaggedFileExt};
use lofty::flac::FlacFile;
use lofty::id3::v1::Id3v1Tag;
use lofty::id3::v2::{
    BinaryFrame, CommentFrame, ExtendedTextFrame, Frame, FrameId, Id3v2Tag, Id3v2Version,
    TextInformationFrame, TimestampFrame,
};
use lofty::iff::aiff::{AiffFile, AiffTextChunks};
use lofty::iff::wav::{RiffInfoList, WavFile};
use lofty::mp4::{Atom, AtomData, AtomIdent, DataType, Ilst, Mp4File};
use lofty::mpeg::MpegFile;
use lofty::musepack::MpcFile;
use lofty::ogg::tag::VorbisComments;
use lofty::ogg::{OggPictureStorage, OpusFile, SpeexFile, VorbisFile};
use lofty::picture::{MimeType, Picture, PictureInformation, PictureType};
use lofty::probe::Probe;
use lofty::tag::items::{Lang, Timestamp};
use lofty::tag::{Accessor, ItemKey, ItemValue, MergeTag, SplitTag, Tag, TagExt, TagItem, TagType};
use lofty::wavpack::WavPackFile;
use lofty::TextEncoding;
use thiserror::Error;

/// Whether ID3v2 tags are written as v2.3 (`true`) or v2.4 (`false`, the
/// default). App-wide preference (Settings › Tag defaults, #79). Kept as a
/// process global rather than threaded through every `write` call because it is
/// a rarely-changed, app-wide default; the app sets it from saved settings via
/// [`set_write_id3v23`].
static WRITE_ID3V23: AtomicBool = AtomicBool::new(false);

/// The silent MPEG frame [`TagEngine::restore_id3v2_bytes`] pads a journaled
/// ID3v2 chunk with, and how many of them. Enough audio for the backend to
/// recognize the bytes as an MP3 and hand the tag back; never written to disk.
const SILENT_MPEG_FRAME_LEN: usize = 417;
const SILENT_MPEG_FRAMES: usize = 5;

/// Set the ID3v2 version future writes use (`true` = v2.3, `false` = v2.4).
pub fn set_write_id3v23(v23: bool) {
    WRITE_ID3V23.store(v23, Ordering::Relaxed);
}

/// The write options for an ID3v2 save, honoring the version preference.
/// Every write's options: the backend's defaults, with the junk window the
/// reader already allows (#204, #506). The backend checks a file's format again
/// before it writes, looking a bounded distance past the tag for the audio, so
/// a file with more than 1024 bytes of junk there — padding left by an encoder
/// — was read fine and then refused on every write.
fn write_options() -> WriteOptions {
    WriteOptions::default().parse_options(ParseOptions::new().max_junk_bytes(STUBBORN_JUNK_BYTES))
}

fn id3_write_options() -> WriteOptions {
    write_options().use_id3v23(WRITE_ID3V23.load(Ordering::Relaxed))
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

    /// The backend container this format is, for the concrete reads a write
    /// needs. The inverse of [`from_lofty`](Self::from_lofty), and the reason
    /// the write path never has to sniff the file again: the format on a
    /// [`TrackFile`] came from a parse that already succeeded (#204).
    fn to_lofty(self) -> FileType {
        match self {
            Self::Mp3 => FileType::Mpeg,
            Self::Flac => FileType::Flac,
            Self::OggVorbis => FileType::Vorbis,
            Self::M4a => FileType::Mp4,
            Self::Aac => FileType::Aac,
            Self::Aiff => FileType::Aiff,
            Self::Wav => FileType::Wav,
            Self::Opus => FileType::Opus,
            Self::Speex => FileType::Speex,
            Self::Musepack => FileType::Mpc,
            Self::MonkeysAudio => FileType::Ape,
            Self::WavPack => FileType::WavPack,
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

/// A kind of tag block a file can carry (#47).
///
/// The app's own vocabulary, not the backend's: a container is allowed several
/// of these at once, and which ones a file actually has is the difference
/// between "the edit didn't take" and "you were reading the other block" — the
/// confusion behind #194.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagBlockKind {
    Id3v1,
    Id3v2,
    VorbisComments,
    Ape,
    Mp4Ilst,
    RiffInfo,
    AiffText,
}

impl TagBlockKind {
    /// What the block is called where a person reads it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Id3v1 => "ID3v1",
            Self::Id3v2 => "ID3v2",
            Self::VorbisComments => "Vorbis Comments",
            Self::Ape => "APE",
            Self::Mp4Ilst => "MP4",
            Self::RiffInfo => "RIFF INFO",
            Self::AiffText => "AIFF Text",
        }
    }

    fn from_tag_type(tag_type: TagType) -> Option<Self> {
        Some(match tag_type {
            TagType::Id3v1 => Self::Id3v1,
            TagType::Id3v2 => Self::Id3v2,
            TagType::VorbisComments => Self::VorbisComments,
            TagType::Ape => Self::Ape,
            TagType::Mp4Ilst => Self::Mp4Ilst,
            TagType::RiffInfo => Self::RiffInfo,
            TagType::AiffText => Self::AiffText,
            // The backend's list is open-ended; a kind the app has no name for
            // is left out of the report rather than shown as a guess.
            _ => return None,
        })
    }

    /// A lossless string form, for the journal (#47). Distinct from
    /// [`name`](Self::name), which is for reading: that one can be reworded, this
    /// one is written into a database that outlives the build that wrote it.
    pub fn to_storage_key(self) -> &'static str {
        match self {
            Self::Id3v1 => "id3v1",
            Self::Id3v2 => "id3v2",
            Self::VorbisComments => "vorbis",
            Self::Ape => "ape",
            Self::Mp4Ilst => "mp4",
            Self::RiffInfo => "riff",
            Self::AiffText => "aiff",
        }
    }

    /// Inverse of [`to_storage_key`](Self::to_storage_key), or `None` for a key
    /// this build does not know — a journal written by a newer one.
    pub fn from_storage_key(key: &str) -> Option<Self> {
        Some(match key {
            "id3v1" => Self::Id3v1,
            "id3v2" => Self::Id3v2,
            "vorbis" => Self::VorbisComments,
            "ape" => Self::Ape,
            "mp4" => Self::Mp4Ilst,
            "riff" => Self::RiffInfo,
            "aiff" => Self::AiffText,
            _ => return None,
        })
    }

    fn to_tag_type(self) -> TagType {
        match self {
            Self::Id3v1 => TagType::Id3v1,
            Self::Id3v2 => TagType::Id3v2,
            Self::VorbisComments => TagType::VorbisComments,
            Self::Ape => TagType::Ape,
            Self::Mp4Ilst => TagType::Mp4Ilst,
            Self::RiffInfo => TagType::RiffInfo,
            Self::AiffText => TagType::AiffText,
        }
    }

    /// Whether a block of this kind can hold an embedded image. ID3v1 has room
    /// for seven text fields and nothing else, so removing one can never cost a
    /// picture — which is what makes it the one kind undo restores exactly.
    pub fn holds_pictures(self) -> bool {
        matches!(
            self,
            Self::Id3v2 | Self::VorbisComments | Self::Mp4Ilst | Self::Ape
        )
    }

    /// Every kind, in the order the app offers them: the ones a file is most
    /// likely to be converted *to* first.
    pub const ALL: [Self; 7] = [
        Self::Id3v2,
        Self::VorbisComments,
        Self::Ape,
        Self::Mp4Ilst,
        Self::Id3v1,
        Self::RiffInfo,
        Self::AiffText,
    ];

    /// The kinds a container can actually be given (#205) — asked of the
    /// backend, which knows both what each container may hold and which of
    /// those it can only read. Offering a kind a format cannot carry would
    /// produce a conversion that silently writes nothing.
    pub fn writable_for(format: AudioFormat) -> Vec<Self> {
        let file_type = format.to_lofty();
        Self::ALL
            .into_iter()
            .filter(|kind| file_type.tag_support(kind.to_tag_type()).is_writable())
            .collect()
    }
}

/// What converting a block's contents into another kind would cost (#205).
///
/// Worked out by putting the values through the backend's own conversion for
/// the target kind and seeing what comes back, rather than from a table of our
/// own: which fields a kind can hold is the backend's business, it differs in
/// ways no summary captures — ID3v1's seven fixed slots, RIFF INFO's short list
/// — and a table would drift the first time the backend learns a mapping.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConversionLoss {
    /// Fields that would not come back the way they went in — the target has no
    /// key for them, or its own conversion changes the value.
    pub fields: Vec<TagField>,
    /// Images the target kind cannot hold.
    pub pictures: usize,
}

impl ConversionLoss {
    /// Whether the conversion keeps everything, and so needs no warning.
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty() && self.pictures == 0
    }
}

/// Which ID3v2 revision a block is written in (#47). The one tag kind whose
/// version is worth stating: 2.3 and 2.4 differ in ways DJ hardware notices,
/// and the app already has a preference for which one it writes (#79).
///
/// Read on demand, per file, by [`TagEngine::id3v2_revision`] — deliberately not
/// part of the per-file report. The backend only reveals it on a concrete tag,
/// and reading one for every file in a library measured at roughly the cost of
/// the whole listing again. Getting it from the conversion the read already does
/// looked free and isn't: that conversion keeps the revision only when the tag
/// has a frame the generic one cannot represent, so a plainly-tagged file would
/// have reported 2.4 whatever it really was. A value that is right most of the
/// time is worse than one asked for when it matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Id3v2Revision {
    V2,
    V3,
    V4,
}

impl Id3v2Revision {
    pub fn name(self) -> &'static str {
        match self {
            Self::V2 => "2.2",
            Self::V3 => "2.3",
            Self::V4 => "2.4",
        }
    }

    fn from_lofty(version: Id3v2Version) -> Self {
        match version {
            Id3v2Version::V2 => Self::V2,
            Id3v2Version::V3 => Self::V3,
            _ => Self::V4,
        }
    }

    /// A lossless string form, for the journal and the IPC boundary — the same
    /// split between reading and storage as [`TagBlockKind::to_storage_key`].
    pub fn to_storage_key(self) -> &'static str {
        match self {
            Self::V2 => "id3v22",
            Self::V3 => "id3v23",
            Self::V4 => "id3v24",
        }
    }

    /// Inverse of [`to_storage_key`](Self::to_storage_key), or `None` for a key
    /// this build does not know.
    pub fn from_storage_key(key: &str) -> Option<Self> {
        Some(match key {
            "id3v22" => Self::V2,
            "id3v23" => Self::V3,
            "id3v24" => Self::V4,
            _ => return None,
        })
    }

    /// The revisions the app will write. 2.2 is read and reported — plenty of
    /// old files are still in it — but the backend writes only 2.3 and 2.4, so
    /// offering 2.2 as a conversion target would be offering a write that
    /// quietly produced something else.
    pub const WRITABLE: [Self; 2] = [Self::V3, Self::V4];
}

/// One tag block a file carries (#47).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TagBlock {
    pub kind: TagBlockKind,
    /// Whether this is the block the app reads its values from, and the one a
    /// write goes to. Decided by the read priority (#84).
    pub read_from: bool,
}

impl TagBlock {
    /// What to show for this block. The kind alone: see [`Id3v2Revision`] for
    /// why the ID3v2 revision is not part of a per-file report.
    pub fn label(&self) -> &'static str {
        self.kind.name()
    }
}

/// What a tag block held, in enough detail to put it back (#47).
///
/// A rebuild, not a copy of the bytes: the backend can write a tag block out but
/// offers no way to read one back in, so undo reconstructs the block from what
/// it said rather than restoring it verbatim. For ID3v1 that is the whole block
/// — seven text fields, no room for anything else — and the round trip is exact.
/// For every other kind it is the text and the pictures, and a frame the model
/// cannot express (a DJ cue point, a rating) does not come back. That is what
/// [`exact`](Self::exact) is for, and why a preview says so before the removal
/// is staged.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TagBlockContent {
    /// What the block held, in the model's own field vocabulary.
    ///
    /// Deliberately not the format's own spelling of each key: ID3v1 has no
    /// named keys at all — it is a fixed binary layout, and the backend maps
    /// none of them — so a snapshot keyed that way comes out empty for the one
    /// kind this has to be exact about. [`TagField`] round-trips losslessly and
    /// is already what the journal persists.
    pub tags: TagMap,
    /// The images the block held.
    pub covers: Vec<CoverArt>,
}

impl TagBlockContent {
    /// Whether putting this block back would restore it whole.
    ///
    /// True only for ID3v1, and deliberately by *kind* rather than by
    /// inspection: erring towards warning costs a sentence, and erring the other
    /// way costs somebody their cue points. A block that turns out to have held
    /// nothing exotic is warned about needlessly; one that did is never removed
    /// silently.
    pub fn exact(kind: TagBlockKind) -> bool {
        kind == TagBlockKind::Id3v1
    }

    /// What writing this content as a block of `kind` would drop (#205).
    ///
    /// Computed against the backend's own conversion, in memory — nothing is
    /// written and no file is touched — so the preview can state the cost of a
    /// conversion before it is staged.
    pub fn conversion_loss(&self, kind: TagBlockKind) -> ConversionLoss {
        let mut survived = tag_to_map(&round_trip_through(kind, tag_for_block(kind, self)));
        // What the generic tag cannot carry is written by name (#616), onto the
        // block kinds that have names to write it under.
        let tag_type = kind.to_tag_type();
        if matches!(
            kind,
            TagBlockKind::Id3v2
                | TagBlockKind::VorbisComments
                | TagBlockKind::Ape
                | TagBlockKind::Mp4Ilst
        ) {
            for (field, value) in &self.tags {
                let by_name = match field {
                    TagField::Custom(name) => item_key_for_custom_name(tag_type, name).is_none(),
                    other => field_name_on_tag(other, tag_type).is_some(),
                };
                if by_name {
                    survived.insert(field.clone(), value.clone());
                }
            }
        }
        ConversionLoss {
            fields: self
                .tags
                .iter()
                .filter(|(field, value)| survived.get(*field) != Some(*value))
                .map(|(field, _)| field.clone())
                .collect(),
            pictures: if kind.holds_pictures() {
                0
            } else {
                self.covers.len()
            },
        }
    }
}

/// The generic tag one block's worth of content becomes on the way to disk.
fn tag_for_block(kind: TagBlockKind, content: &TagBlockContent) -> Tag {
    let tag_type = kind.to_tag_type();
    let mut tag = Tag::new(tag_type);
    for (field, value) in &content.tags {
        // A field this block type has no key for is dropped rather than guessed
        // at — the same silent-drop rule the writer follows (#165), and part of
        // what makes a rebuild exact only for ID3v1.
        if item_key_for(field, tag_type).is_some() {
            push_field_items(&mut tag, field, value);
        }
    }
    for cover in &content.covers {
        tag.push_picture(picture_from_cover(cover));
    }
    tag
}

/// Put a generic tag through the concrete type for `kind` and back, which is
/// what a write of that block does and therefore what survives it.
fn round_trip_through(kind: TagBlockKind, tag: Tag) -> Tag {
    match kind {
        TagBlockKind::Id3v1 => Tag::from(id3v1_as_written(Id3v1Tag::from(tag))),
        TagBlockKind::Id3v2 => Tag::from(Id3v2Tag::from(tag)),
        TagBlockKind::VorbisComments => Tag::from(vorbis_comments_from(tag)),
        TagBlockKind::Ape => Tag::from(ApeTag::from(tag)),
        TagBlockKind::Mp4Ilst => Tag::from(Ilst::from(tag)),
        TagBlockKind::RiffInfo => Tag::from(RiffInfoList::from(tag)),
        TagBlockKind::AiffText => Tag::from(AiffTextChunks::from(tag)),
    }
}

/// An ID3v1 tag as it reads back once written (#617).
///
/// In memory the tag holds any text; on disk it is 30 bytes of Latin-1 per
/// field, so what survives is only known from the bytes. They are written out
/// and read back the way [`TagEngine::restore_id3v2_bytes`] reads an ID3v2
/// chunk: behind silent MPEG frames, as an MP3. Should that fail, the tag is
/// answered as it was.
fn id3v1_as_written(tag: Id3v1Tag) -> Id3v1Tag {
    let mut synthetic = Vec::new();
    let mut silence = vec![0xFF, 0xFB, 0x90, 0x00];
    silence.resize(SILENT_MPEG_FRAME_LEN, 0);
    for _ in 0..SILENT_MPEG_FRAMES {
        synthetic.extend_from_slice(&silence);
    }
    if tag.dump_to(&mut synthetic, write_options()).is_err() {
        return tag;
    }
    MpegFile::read_from(&mut Cursor::new(synthetic), tag_only_options())
        .ok()
        .and_then(|file| file.id3v1().cloned())
        .unwrap_or(tag)
}

/// A generic tag as Vorbis Comments, with its items in their order (#616).
///
/// The backend's conversion moves the encoder into the vendor string by taking
/// it out with a swap, which puts the tag's first item where the encoder was —
/// reversing the first two values of a multi-value field. It is taken out here
/// instead, in order, and set as the vendor the same way.
fn vorbis_comments_from(mut tag: Tag) -> VorbisComments {
    let vendor = tag.get_string(ItemKey::EncoderSoftware).map(str::to_string);
    tag.retain(|item| item.key() != ItemKey::EncoderSoftware);
    let mut vorbis = VorbisComments::from(tag);
    if let Some(vendor) = vendor {
        vorbis.set_vendor(vendor);
    }
    vorbis
}

/// The comment a tag's comment items are read as (#644), by its language and
/// description. ID3v2 tells its `COMM` frames apart by description, and iTunes
/// adds machine ones (`iTunSMPB` for gapless playback, `iTunNORM` for Sound
/// Check) that the backend hands over as comments too: the comment is the last
/// one without a description, or else the last that is not a machine one.
/// [`owned_comment_frame`] is the same rule on the concrete tag.
fn owned_comment(tag: &Tag) -> Option<(Lang, &str)> {
    let comments = || tag.items().filter(|item| item.key() == ItemKey::Comment);
    comments()
        .filter(|item| item.description().is_empty())
        .last()
        .or_else(|| {
            comments()
                .filter(|item| !is_machine_comment(item.description()))
                .last()
        })
        .map(|item| (*item.lang(), item.description()))
}

/// Whether `item` is a comment other than the one [`owned_comment`] chose.
fn is_unowned_comment(item: &TagItem, owned: Option<(Lang, &str)>) -> bool {
    item.key() == ItemKey::Comment && owned != Some((*item.lang(), item.description()))
}

/// [`owned_comment`] on a concrete ID3v2 tag: the `COMM` frame the model's
/// comment is.
/// (`Id3v2Tag::comments` only lists the ones without a description.)
fn owned_comment_frame<'a>(tag: &'a Id3v2Tag) -> Option<&'a CommentFrame<'a>> {
    let comments = || {
        tag.into_iter().filter_map(|frame| match frame {
            Frame::Comment(comment) => Some(comment),
            _ => None,
        })
    };
    comments()
        .rfind(|comment| comment.description.is_empty())
        .or_else(|| comments().rfind(|comment| !is_machine_comment(&comment.description)))
}

/// A comment description a program writes for itself rather than for the
/// reader: iTunes' `iTunSMPB`, `iTunNORM`, `iTunPGAP`, `iTunes_CDDB_IDs`.
fn is_machine_comment(description: &str) -> bool {
    description.starts_with("iTun")
}

/// A generic tag's text items in the model's field vocabulary. Shared by
/// [`TagEngine::read_block`] and the conversion-loss check so the two agree on
/// what "the block holds this" means.
fn tag_to_map(tag: &Tag) -> TagMap {
    let tag_type = tag.tag_type();
    let mut tags = TagMap::new();
    let comment = owned_comment(tag);
    for item in tag.items() {
        if is_unowned_comment(item, comment) {
            continue;
        }
        let value = match item.value() {
            ItemValue::Text(text) | ItemValue::Locator(text) => text.clone(),
            _ => continue,
        };
        insert_read_value(
            &mut tags,
            item_key_to_tag_field(item.key(), tag_type),
            false,
            unpadded(tag_type, value),
        );
    }
    tags
}

/// A value as read, without the padding an ID3v1 field carries (#664). Its
/// fields are fixed-width, and many encoders fill them with spaces where the
/// standard has NULs; the backend keeps the spaces, which then showed in the
/// table, rendered into file names and were copied into ID3v2.
fn unpadded(tag_type: TagType, value: String) -> String {
    if tag_type == TagType::Id3v1 {
        value.trim_end_matches(' ').to_string()
    } else {
        value
    }
}

/// One read of a file: its tags, its technical properties, and the tag blocks it
/// carries (#40, #172, #47).
///
/// All three from a single probe. The listing needs every one of them per file,
/// and parsing three times over would triple the cost of opening a library.
#[derive(Debug, Clone)]
pub struct TrackRead {
    pub file: TrackFile,
    pub props: AudioProps,
    pub blocks: Vec<TagBlock>,
    /// Whether any tag block carries a picture (#447). Read off the parse the
    /// tags come from, where the pictures are already decoded, so it costs
    /// nothing; a folder image next to the file does not count.
    pub has_cover: bool,
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

/// The same parsing rules without scanning the audio for its properties. The
/// concrete-tag reads below only ever want the tag block, and finding the
/// bitrate of an MP3 means walking its frames — the expensive half of a parse,
/// paid for nothing.
fn tag_only_options() -> ParseOptions {
    parse_options().read_properties(false)
}

/// Tag-only, and without the embedded artwork either. The custom-field read
/// wants nothing but the item names, and decoding a cover it will not look at
/// is the single most expensive thing a second parse could do — the listing
/// reads every file in a library, so this is the difference between a second
/// parse that costs nothing and one that doubles the scan.
fn custom_item_options() -> ParseOptions {
    tag_only_options().read_cover_art(false)
}

/// Parse a file, falling back through the ways a stubborn one can still be read
/// (#204).
///
/// Three attempts, each reached only when the one before it failed — so the file
/// that reads first time, which is very nearly every file, costs exactly what it
/// cost before:
///
/// 1. **What the backend sniffs, relaxed.** The #183 choice, unchanged.
/// 2. **The same container, best-attempt.** The two parsing modes are *not* a
///    hierarchy, which is the trap: relaxed discards an item it cannot decode,
///    best-attempt fills the hole in, and a `UFID` frame with an empty owner is
///    a hard error in relaxed and an empty string in best-attempt. Neither
///    forgives everything the other does, so a file only one of them can read is
///    read by that one.
/// 3. **The container the extension names, with a wide junk window.** Sniffing
///    looks a bounded distance past the tag for a frame sync; a file whose audio
///    starts beyond that can be sniffed as another format entirely — one real
///    example carries 1035 bytes of junk with a stray ADTS sync inside it and
///    was read as AAC. The writer already takes the container from the path, so
///    this is also the reader agreeing with the writer.
///
/// The error reported when all three fail is the **first** one: it describes the
/// file as it actually presents itself, which is what someone looking at the
/// message needs.
fn probe_file(path: &Path) -> Result<lofty::file::TaggedFile, TagIoError> {
    let first = match probe_source(path)?
        .guess_file_type()?
        .options(parse_options())
        .read()
    {
        Ok(file) => return Ok(file),
        Err(error) => error,
    };

    if let Ok(file) = probe_source(path)?
        .guess_file_type()?
        .options(parse_options().parsing_mode(ParsingMode::BestAttempt))
        .read()
    {
        return Ok(file);
    }

    if let Some(by_extension) = FileType::from_path(path) {
        if let Ok(file) = probe_source(path)?
            .set_file_type(by_extension)
            .options(parse_options().max_junk_bytes(STUBBORN_JUNK_BYTES))
            .read()
        {
            return Ok(file);
        }
    }

    // [2026-11-03 14:40] Last: the file without the ID3v2 frames whose text cannot be decoded.
    if let Some(source) = without_undecodable_id3v2_frames_source(path)? {
        let probe = match FileType::from_path(path) {
            Some(file_type) => Probe::with_file_type(source, file_type),
            None => Probe::new(source).guess_file_type()?,
        };
        if let Ok(file) = probe.options(parse_options()).read() {
            return Ok(file);
        }
    }

    Err(first.into())
}

// [2026-11-03 14:40] Added: a read past ID3v2 frames whose text cannot be decoded.
/// The file at `path` with the frames of its leading ID3v2 tag that the backend
/// cannot decode blanked out, or `None` when there are none.
///
/// The relaxed parse (#183) skips a frame whose header it cannot read, but text
/// it cannot decode — UTF-16 with no byte-order mark or an odd length, UTF-8
/// that is not — fails the whole tag, so the file could not be opened at all:
/// exactly the files that need their tags fixed. Each frame is tried on its own
/// behind silent MPEG frames, the way the backend itself reads it; the ones it
/// refuses are overwritten with padding of the same length, so the tag and the
/// audio keep their places. The rest reads as usual, the blanked frame is simply
/// absent from the model, and the next save writes the tag without it.
///
/// Only reached once every ordinary read has failed, so a file that reads costs
/// nothing more.
fn without_undecodable_id3v2_frames_source(path: &Path) -> Result<Option<TagSource>, TagIoError> {
    let raw = std::fs::read(path)?;
    Ok(without_undecodable_id3v2_frames(&raw).map(|fixed| TagSource::Memory(Cursor::new(fixed))))
}

// [2026-11-03 14:40] Added: the MP3 read the ID3v2 writes start from, past undecodable frames.
/// Read the MP3 at `path` from `file`, and when that fails, read it again
/// without the ID3v2 frames the backend cannot decode
/// ([`without_undecodable_id3v2_frames_source`]): a save starts from this read,
/// so a file that now opens can also be saved, which writes the tag afresh.
fn read_mpeg_past_undecodable_frames(
    path: &Path,
    file: &mut TagSource,
    options: ParseOptions,
) -> Result<MpegFile, TagIoError> {
    match MpegFile::read_from(file, options) {
        Ok(mpeg) => Ok(mpeg),
        Err(error) => match without_undecodable_id3v2_frames_source(path)? {
            Some(mut repaired) => Ok(MpegFile::read_from(&mut repaired, options)?),
            None => Err(error.into()),
        },
    }
}

// [2026-11-03 14:40] Added: see without_undecodable_id3v2_frames_source.
fn without_undecodable_id3v2_frames(raw: &[u8]) -> Option<Vec<u8>> {
    if raw.len() < 10 || &raw[..3] != b"ID3" || !matches!(raw[3], 3 | 4) {
        return None;
    }
    let version = raw[3];
    // An unsynchronised tag or one with an extended header is not walked here:
    // its frames do not sit where their headers say.
    if raw[5] & 0xC0 != 0 {
        return None;
    }
    let end = 10 + syncsafe_u32(&raw[6..10]) as usize;
    if end > raw.len() {
        return None;
    }
    let mut spans = Vec::new();
    let mut pos = 10;
    while pos + 10 <= end {
        let id = &raw[pos..pos + 4];
        if id[0] == 0 {
            break; // padding
        }
        if !id
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        {
            return None;
        }
        let size = if version == 4 {
            syncsafe_u32(&raw[pos + 4..pos + 8])
        } else {
            u32::from_be_bytes([raw[pos + 4], raw[pos + 5], raw[pos + 6], raw[pos + 7]])
        } as usize;
        let next = pos + 10 + size;
        if next > end {
            return None;
        }
        spans.push((pos, next));
        pos = next;
    }

    let mut silence = vec![0xFF, 0xFB, 0x90, 0x00];
    silence.resize(SILENT_MPEG_FRAME_LEN, 0);
    let decodes = |frame: &[u8]| {
        let mut probe = raw[..5].to_vec();
        probe.push(0);
        probe.extend_from_slice(&syncsafe_bytes(frame.len() as u32));
        probe.extend_from_slice(frame);
        for _ in 0..SILENT_MPEG_FRAMES {
            probe.extend_from_slice(&silence);
        }
        MpegFile::read_from(&mut Cursor::new(probe), tag_only_options()).is_ok()
    };
    let refused: Vec<(usize, usize)> = spans
        .into_iter()
        .filter(|&(start, stop)| !decodes(&raw[start..stop]))
        .collect();
    if refused.is_empty() {
        return None;
    }
    // Kept frames first, in their order, then padding up to the tag's own end.
    let mut fixed = raw[..10].to_vec();
    let mut pos = 10;
    for &(start, stop) in &refused {
        fixed.extend_from_slice(&raw[pos..start]);
        pos = stop;
    }
    fixed.extend_from_slice(&raw[pos..end]);
    fixed.resize(end, 0);
    fixed.extend_from_slice(&raw[end..]);
    Some(fixed)
}

/// How far past the tag the last-ditch attempt will look for the audio (#204).
/// The backend's own limit is 1024, and the file that prompted this has 1035
/// bytes of junk; 64 KiB is far enough to cover a padded or badly spliced file
/// without being a licence to scan the whole thing.
const STUBBORN_JUNK_BYTES: usize = 64 * 1024;

/// The container a path holds, for a caller that has no parse to take it from.
///
/// This is the *resolved* container — what the file actually parses as, ladder
/// and all — not merely what a sniff of its first bytes suggests, because the
/// concrete reads have to agree with the parse the values came from. Callers on
/// the read path pass the type down from their own probe instead; this is for
/// the ones that only ever hold a path.
fn container_of(path: &Path) -> Result<Option<FileType>, TagIoError> {
    Ok(Some(probe_file(path)?.file_type()))
}

/// A buffered handle for the concrete readers. Parsing a tag is thousands of
/// small reads, and handing the backend a bare `File` makes every one of them a
/// syscall — enough to cost more than the whole first parse, which the probe
/// buffers for itself.
///
/// A file whose ID3v2 block, leading or an AIFF's or a WAV's `ID3 ` chunk, has
/// an extended header is handed over from memory instead, as a copy without
/// that header (#648): the backend reads a 2.3 extended header with the 2.4
/// layout and then takes every frame for padding.
fn buffered(path: &Path) -> Result<TagSource, TagIoError> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    let mut head = [0u8; 12];
    let read = file.read_exact(&mut head).is_ok();
    let leading = read && starts_with_id3v2_extended_header(&head[..10]);
    let chunk = if read && !leading {
        iff_id3_chunk_with_extended_header(&mut file, &head)?
    } else {
        None
    };
    file.seek(SeekFrom::Start(0))?;
    if leading || chunk.is_some() {
        let mut raw = Vec::new();
        file.read_to_end(&mut raw)?;
        let fixed = match chunk {
            Some((at, len)) => without_iff_id3_extended_header(&raw, at, len),
            None => without_id3v2_extended_header(&raw),
        };
        return Ok(TagSource::Memory(Cursor::new(fixed.unwrap_or(raw))));
    }
    Ok(TagSource::File(std::io::BufReader::new(file)))
}

/// Where an AIFF's or a WAV's `ID3 ` chunk payload sits, as offset and length,
/// when that payload opens with an extended header (#648). Walked by seeking,
/// so the audio is never read; `head` is the file's first twelve bytes.
fn iff_id3_chunk_with_extended_header(
    file: &mut std::fs::File,
    head: &[u8; 12],
) -> Result<Option<(usize, usize)>, TagIoError> {
    use std::io::{Read, Seek, SeekFrom};
    let big_endian = match (&head[0..4], &head[8..12]) {
        (b"FORM", b"AIFF" | b"AIFC") => true,
        (b"RIFF", b"WAVE") => false,
        _ => return Ok(None),
    };
    let len = file.metadata()?.len();
    let mut pos = 12u64;
    while pos + 8 <= len {
        let mut chunk = [0u8; 18];
        file.seek(SeekFrom::Start(pos))?;
        let got = file.read(&mut chunk)?;
        if got < 8 {
            break;
        }
        let size_bytes = [chunk[4], chunk[5], chunk[6], chunk[7]];
        let size = u64::from(if big_endian {
            u32::from_be_bytes(size_bytes)
        } else {
            u32::from_le_bytes(size_bytes)
        });
        if chunk[..4].eq_ignore_ascii_case(b"ID3 ") {
            let payload = (pos + 8, size);
            let found = got == 18 && starts_with_id3v2_extended_header(&chunk[8..]);
            return Ok((found && payload.0 + payload.1 <= len)
                .then_some((payload.0 as usize, payload.1 as usize)));
        }
        pos += 8 + size + (size & 1);
    }
    Ok(None)
}

/// `raw`, an AIFF or a WAV whose `ID3 ` chunk payload is `len` bytes at `at`,
/// with that payload's extended header taken out (#648) and the chunk's and the
/// container's sizes kept in step.
fn without_iff_id3_extended_header(raw: &[u8], at: usize, len: usize) -> Option<Vec<u8>> {
    let fixed = without_id3v2_extended_header(raw.get(at..at + len)?)?;
    let big_endian = &raw[0..4] == b"FORM";
    let size = |value: usize| {
        if big_endian {
            (value as u32).to_be_bytes()
        } else {
            (value as u32).to_le_bytes()
        }
    };
    let mut out = raw[..at - 4].to_vec();
    out.extend_from_slice(&size(fixed.len()));
    out.extend_from_slice(&fixed);
    if fixed.len() % 2 == 1 {
        out.push(0);
    }
    out.extend_from_slice(raw.get(at + len + (len & 1)..).unwrap_or_default());
    let container = size(out.len() - 8);
    out[4..8].copy_from_slice(&container);
    Some(out)
}

/// What [`buffered`] hands the backend: the file itself, or the copy of it that
/// lost its ID3v2 extended header (#648).
enum TagSource {
    File(std::io::BufReader<std::fs::File>),
    Memory(Cursor<Vec<u8>>),
}

impl std::io::Read for TagSource {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            TagSource::File(file) => file.read(buf),
            TagSource::Memory(bytes) => bytes.read(buf),
        }
    }
}

impl std::io::Seek for TagSource {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        match self {
            TagSource::File(file) => file.seek(pos),
            TagSource::Memory(bytes) => bytes.seek(pos),
        }
    }
}

/// A probe over [`buffered`], with the container the extension names as its
/// fallback, as the backend's own `Probe::open` sets it.
fn probe_source(path: &Path) -> Result<Probe<TagSource>, TagIoError> {
    let source = buffered(path)?;
    Ok(match FileType::from_path(path) {
        Some(file_type) => Probe::with_file_type(source, file_type),
        None => Probe::new(source),
    })
}

/// Whether these first ten bytes open an ID3v2.3 or 2.4 tag with an extended
/// header (#648).
fn starts_with_id3v2_extended_header(head: &[u8]) -> bool {
    head.len() >= 10 && &head[..3] == b"ID3" && matches!(head[3], 3 | 4) && head[5] & 0x40 != 0
}

/// `bytes`, a file or an ID3v2 block that starts with the tag, without the
/// tag's extended header (#648); `None` when there is none, or none this can
/// take out safely.
///
/// The header flag is cleared and the tag size shrunk by the extended header's
/// length, and a 2.4 footer is kept in step; every byte after the extended
/// header, the frames and the audio alike, is kept as it is. All an extended
/// header holds is a CRC, tag restrictions or a padding size, none of which the
/// app shows or writes, and the backend miscounts the tag's end around one.
fn without_id3v2_extended_header(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() < 14 || !starts_with_id3v2_extended_header(bytes) {
        return None;
    }
    let version = bytes[3];
    let flags = bytes[5];
    let size = syncsafe_u32(&bytes[6..10]) as usize;
    let end = 10 + size;
    if end > bytes.len() {
        return None;
    }
    // 2.4 counts the size field in the extended header's size, 2.3 does not;
    // a 2.3 tag that is unsynchronised has its extended header unsynchronised
    // too, so its length on disk has to be walked.
    let extended_len = match (version, flags & 0x80 != 0) {
        (4, _) => syncsafe_u32(&bytes[10..14]) as usize,
        (_, false) => 4 + u32::from_be_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]) as usize,
        (_, true) => unsynchronised_extended_header_len(&bytes[10..end])?,
    };
    if extended_len < 6 || 10 + extended_len > end {
        return None;
    }

    let new_size = syncsafe_bytes((size - extended_len) as u32);
    let mut out = Vec::with_capacity(bytes.len() - extended_len);
    out.extend_from_slice(&bytes[..5]);
    out.push(flags & !0x40);
    out.extend_from_slice(&new_size);
    out.extend_from_slice(&bytes[10 + extended_len..end]);
    let mut rest = &bytes[end..];
    if version == 4 && flags & 0x10 != 0 && rest.len() >= 10 && &rest[..3] == b"3DI" {
        out.extend_from_slice(&rest[..5]);
        out.push(flags & !0x40);
        out.extend_from_slice(&new_size);
        rest = &rest[10..];
    }
    out.extend_from_slice(rest);
    Some(out)
}

/// The length on disk of an unsynchronised ID3v2.3 extended header that starts
/// `raw` (#648): its size field, then as many bytes as that field counts, with
/// each `00` that unsynchronisation put after an `FF` counted in.
fn unsynchronised_extended_header_len(raw: &[u8]) -> Option<usize> {
    let mut decoded = Vec::new();
    let mut want = 4;
    let mut pos = 0;
    while decoded.len() < want {
        let byte = *raw.get(pos)?;
        pos += 1;
        if byte == 0xFF && raw.get(pos) == Some(&0) {
            pos += 1;
        }
        decoded.push(byte);
        if decoded.len() == 4 {
            want =
                4 + u32::from_be_bytes([decoded[0], decoded[1], decoded[2], decoded[3]]) as usize;
        }
    }
    Some(pos)
}

/// Take the extended header out of the file's leading ID3v2 block before a
/// write (#648). The backend finds the end of the tag it replaces by skipping
/// the tag's size from the end of the extended header, so with one in place the
/// write also replaces as many bytes of the audio. Only the header bytes change;
/// a file without an extended header is not touched.
fn drop_id3v2_extended_header(path: &Path) -> Result<(), TagIoError> {
    use std::io::Read;
    let mut head = [0u8; 10];
    if std::fs::File::open(path)?.read_exact(&mut head).is_err()
        || !starts_with_id3v2_extended_header(&head)
    {
        return Ok(());
    }
    let raw = std::fs::read(path)?;
    if let Some(fixed) = without_id3v2_extended_header(&raw) {
        rewrite_in_place(path, &raw, &fixed)?;
    }
    Ok(())
}

/// The file's concrete Vorbis Comments block, if it has one. Dispatches on the
/// container the same way [`read_id3v2`] does, for every format that keeps its
/// tags in Vorbis Comments.
fn read_vorbis(
    path: &Path,
    container: FileType,
    options: ParseOptions,
) -> Result<Option<VorbisComments>, TagIoError> {
    let mut file = buffered(path)?;
    Ok(match Some(container) {
        Some(FileType::Flac) => FlacFile::read_from(&mut file, options)?
            .vorbis_comments()
            .cloned(),
        Some(FileType::Vorbis) => Some(
            VorbisFile::read_from(&mut file, options)?
                .vorbis_comments()
                .clone(),
        ),
        Some(FileType::Opus) => Some(
            OpusFile::read_from(&mut file, options)?
                .vorbis_comments()
                .clone(),
        ),
        Some(FileType::Speex) => Some(
            SpeexFile::read_from(&mut file, options)?
                .vorbis_comments()
                .clone(),
        ),
        _ => None,
    })
}

/// `tag`'s vendor and comments as a Vorbis comment packet, without the framing
/// bit (#669): lengths are little-endian `u32`, each comment is `NAME=value`.
fn vorbis_comment_packet(tag: &VorbisComments) -> Vec<u8> {
    fn put(packet: &mut Vec<u8>, bytes: &[u8]) {
        packet.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        packet.extend_from_slice(bytes);
    }
    let mut packet = Vec::new();
    put(&mut packet, tag.vendor().as_bytes());
    packet.extend_from_slice(&(tag.items().len() as u32).to_le_bytes());
    for (name, value) in tag.items() {
        put(&mut packet, format!("{name}={value}").as_bytes());
    }
    packet
}

/// The inverse of [`vorbis_comment_packet`]; `None` for bytes it did not make.
fn vorbis_comments_from_packet(bytes: &[u8]) -> Option<VorbisComments> {
    let mut rest = bytes;
    let mut take = |len: usize| -> Option<&[u8]> {
        let (head, tail) = rest.split_at_checked(len)?;
        rest = tail;
        Some(head)
    };
    let vendor_len = u32::from_le_bytes(take(4)?.try_into().ok()?) as usize;
    let vendor = std::str::from_utf8(take(vendor_len)?).ok()?.to_string();
    let count = u32::from_le_bytes(take(4)?.try_into().ok()?) as usize;
    let mut tag = VorbisComments::default();
    tag.set_vendor(vendor);
    for _ in 0..count {
        let len = u32::from_le_bytes(take(4)?.try_into().ok()?) as usize;
        let comment = std::str::from_utf8(take(len)?).ok()?;
        let (name, value) = comment.split_once('=')?;
        tag.push(name.to_string(), value.to_string());
    }
    Some(tag)
}

/// The file's concrete APE tag, if it has one.
fn read_ape(
    path: &Path,
    container: FileType,
    options: ParseOptions,
) -> Result<Option<ApeTag>, TagIoError> {
    let mut file = buffered(path)?;
    Ok(match Some(container) {
        Some(FileType::Ape) => ApeFile::read_from(&mut file, options)?.ape().cloned(),
        Some(FileType::Mpc) => MpcFile::read_from(&mut file, options)?.ape().cloned(),
        Some(FileType::WavPack) => WavPackFile::read_from(&mut file, options)?.ape().cloned(),
        // An MP3 can carry an APE block beside its ID3v2 one (#608).
        // [2026-11-03 14:40] Read past ID3v2 frames the backend cannot decode, as every MP3 read is.
        Some(FileType::Mpeg) => read_mpeg_past_undecodable_frames(path, &mut file, options)?
            .ape()
            .cloned(),
        _ => None,
    })
}

/// The pictures of the file's APE tag (#608). APE keeps each one as a binary
/// item named after its picture type (`Cover Art (Front)`, …), and the backend
/// never turns those items into pictures, so they are read off the concrete
/// tag. An item whose bytes are not an image format the backend knows is
/// skipped.
fn read_ape_pictures(path: &Path, container: FileType) -> Result<Vec<Picture>, TagIoError> {
    let Some(tag) = read_ape(path, container, tag_only_options())? else {
        return Ok(Vec::new());
    };
    Ok(APE_PICTURE_TYPES
        .iter()
        .filter_map(|key| match tag.get(key)?.value() {
            ItemValue::Binary(bytes) => Picture::from_ape_bytes(key, bytes).ok(),
            _ => None,
        })
        .collect())
}

/// Replace the pictures of the file's APE tag with `covers` (#608), leaving
/// every other item as it is. APE has one item per picture type, so of two
/// images of the same type only the last is kept.
fn write_ape_pictures(
    path: &Path,
    container: FileType,
    covers: &[CoverArt],
) -> Result<(), TagIoError> {
    let existing = read_ape(path, container, tag_only_options())?;
    // No block and nothing to put in one: writing would create an empty block.
    if existing.is_none() && covers.is_empty() {
        return Ok(());
    }
    let mut tag = existing.unwrap_or_default();
    for key in APE_PICTURE_TYPES {
        tag.remove(key);
    }
    for cover in covers {
        let picture = picture_from_cover(cover);
        let Some(key) = picture.pic_type().as_ape_key().map(str::to_string) else {
            continue;
        };
        if let Ok(item) = ApeItem::new(key, ItemValue::Binary(picture.as_ape_bytes())) {
            tag.insert(item);
        }
    }
    tag.save_to_path(path, write_options())?;
    Ok(())
}

/// One format-specific item: its name as the file spells it (a `TXXX`
/// description, a Vorbis comment name, an APE item key, an MP4 freeform atom
/// name) and its value.
type CustomItem = (String, String);

/// Whether the backend's generic [`Tag`] has a key of its own for this
/// format-specific name.
///
/// It is the dividing line the whole custom-field system now rests on (#201).
/// An item whose name maps is carried by the generic tag, is read as an ordinary
/// item, and is written back under the backend's own per-format spelling. One
/// whose name does not map is invisible to the generic tag, and is the app's
/// [`TagField::Custom`]: read from and written to the concrete tag by name.
fn maps_to_item_key(tag_type: TagType, name: &str) -> bool {
    ItemKey::from_key(tag_type, name).is_some()
}

/// The reverse-DNS mean an MP4 freeform atom is written under — what every
/// tagger uses, and what the backend's own MP4 mappings are spelled with.
const FREEFORM_MEAN: &str = "com.apple.iTunes";

/// The generic key a *field name* has for `tag_type`, if any.
///
/// [`maps_to_item_key`] answers the same question about a key as the file
/// spells it; this one asks it about the name the field carries, and the two
/// differ on MP4: the file's key is the whole `----:mean:name`, the field is
/// named after the `name` half alone — because that is what a person sees, and
/// what every other format would have called it.
fn item_key_for_custom_name(tag_type: TagType, name: &str) -> Option<ItemKey> {
    // The release country is one field under every format's own name for it
    // (#612): `RELEASECOUNTRY` is that name only on Vorbis and APE, and written
    // by name it went into ID3v2 and MP4 as an item nothing reads as the country.
    if name.eq_ignore_ascii_case(RELEASE_COUNTRY)
        && ItemKey::ReleaseCountry.map_key(tag_type).is_some()
    {
        return Some(ItemKey::ReleaseCountry);
    }
    if tag_type == TagType::Mp4Ilst {
        if let Some(key) = ItemKey::from_key(tag_type, &format!("----:{FREEFORM_MEAN}:{name}")) {
            return Some(key);
        }
    }
    ItemKey::from_key(tag_type, name)
}

/// The field name a generic key takes on `tag_type` — the inverse of
/// [`item_key_for_custom_name`], and the same `name` half on MP4.
fn custom_name_for_item_key(key: ItemKey, tag_type: TagType) -> Option<&'static str> {
    let mapped = key.map_key(tag_type)?;
    if tag_type == TagType::Mp4Ilst {
        if let Some(name) = mapped.strip_prefix("----:") {
            return name.split_once(':').map(|(_, name)| name).or(Some(name));
        }
    }
    Some(mapped)
}

/// A `TXXX` frame the generic tag has no key for, as a custom item.
fn user_text_item(frame: &Frame<'_>) -> Option<CustomItem> {
    let Frame::UserText(user) = frame else {
        return None;
    };
    if user.description.is_empty() || maps_to_item_key(TagType::Id3v2, &user.description) {
        return None;
    }
    Some((user.description.to_string(), user.content.to_string()))
}

/// A freeform MP4 atom the generic tag has no key for, as custom items: one per
/// text value, because an atom can carry several and every one of them is the
/// file's (#609). An atom with no text in it gives none.
fn freeform_items(atom: &Atom<'_>) -> Vec<CustomItem> {
    let AtomIdent::Freeform { mean, name } = atom.ident() else {
        return Vec::new();
    };
    if maps_to_item_key(TagType::Mp4Ilst, &format!("----:{mean}:{name}")) {
        return Vec::new();
    }
    atom.data()
        .filter_map(|data| match data {
            AtomData::UTF8(text) | AtomData::UTF16(text) => Some((name.to_string(), text.clone())),
            _ => None,
        })
        .collect()
}

/// Every custom item of an MP4 `ilst`, in the order the file has them.
fn ilst_custom_items(ilst: &Ilst) -> Vec<CustomItem> {
    ilst.into_iter()
        .flat_map(|atom| freeform_items(atom))
        .collect()
}

/// The MP4 tempo atom.
const MP4_TEMPO: AtomIdent<'static> = AtomIdent::Fourcc(*b"tmpo");

/// The tempo an MP4 `tmpo` atom states as an integer, which is how the format
/// defines it (#610). The backend only turns a *text* `tmpo` into a generic
/// item; the integer one every other tagger writes is left on the concrete tag,
/// so this is where it is picked up.
fn ilst_integer_tempo(ilst: &Ilst) -> Option<String> {
    ilst.get(&MP4_TEMPO)
        .and_then(atom_integer_tempo)
        .map(|n| n.to_string())
}

/// The tempo one `tmpo` atom states as an integer. A tempo of 0 is how many
/// taggers say there is none, and is read as none (#611).
fn atom_integer_tempo(atom: &Atom<'_>) -> Option<i64> {
    atom.data()
        .find_map(|data| match data {
            AtomData::SignedInteger(n) => Some(i64::from(*n)),
            AtomData::UnsignedInteger(n) => Some(i64::from(*n)),
            AtomData::Unknown { data, .. } if (1..=4).contains(&data.len()) => {
                Some(data.iter().fold(0i64, |n, byte| n << 8 | i64::from(*byte)))
            }
            _ => None,
        })
        .filter(|n| *n > 0)
}

/// The `tmpo` atom a BPM string is written as: a 2-byte big-endian integer,
/// the size the format defines and the only one some readers look at (#610).
/// A value that is not a tempo in that range is written as text, which is
/// what the backend did with every value before.
fn mp4_tempo_atom(bpm: &str) -> Atom<'static> {
    let tempo = bpm
        .trim()
        .parse::<f64>()
        .ok()
        .map(f64::round)
        .filter(|n| (0.0..=f64::from(i16::MAX)).contains(n));
    let data = match tempo {
        Some(n) => AtomData::Unknown {
            code: DataType::BeSignedInteger,
            data: (n as i16).to_be_bytes().to_vec(),
        },
        None => AtomData::UTF8(bpm.to_string()),
    };
    Atom::new(MP4_TEMPO, data)
}

/// The values a custom field is written back as: the file's own, when it had
/// several and the model still says what they read as joined together (#609),
/// or else the model's one value. An untouched field therefore goes back the
/// way it came instead of collapsing into a single joined string.
fn custom_values_to_write(own: Vec<String>, value: &str) -> Vec<String> {
    if own.len() > 1 && own.join(&multi_value_separator()) == value {
        own
    } else {
        vec![value.to_string()]
    }
}

/// Every format-specific item of the file's `tag_type` block — what
/// [`TagField::Custom`] is made of (#201).
///
/// The 0.22 backend handed these over as `ItemKey::Unknown(name)` items of the
/// generic tag, so one read of that tag saw everything. 0.23 removed that
/// variant: an item the generic tag has no key for now stays on the concrete
/// tag, where the generic representation cannot see it at all — and for Vorbis
/// Comments and APE it is not even carried across the conversion. So the custom
/// fields are read from the concrete tag, by their own name.
///
/// ID3v2 and MP4 are the cheap half: the backend keeps their format-specific
/// items beside the generic tag, so converting the tag back to its concrete form
/// recovers them from the parse already done. Vorbis Comments and APE have no
/// such companion — the conversion drops the items on the floor — so those pay
/// for a second read of the file. It is a tag-only, artwork-free one
/// ([`custom_item_options`]), which is the cheapest parse the backend offers.
fn custom_items(path: &Path, container: FileType, tag: Tag) -> Result<Vec<CustomItem>, TagIoError> {
    Ok(match tag.tag_type() {
        TagType::Id3v2 => {
            let concrete = Id3v2Tag::from(tag);
            concrete.iter().filter_map(user_text_item).collect()
        }
        TagType::Mp4Ilst => ilst_custom_items(&Ilst::from(tag)),
        // Without the backend's implicit conversion, which parses a track or
        // disc number into an integer: the comment comes through as the file
        // spells it, for `insert_concrete_items` to put the digits back (#638).
        TagType::VorbisComments => read_vorbis(
            path,
            container,
            custom_item_options().implicit_conversions(false),
        )?
        .map(|tag| {
            tag.items()
                .filter(|(name, _)| {
                    !maps_to_item_key(TagType::VorbisComments, name)
                        || number_pair_fields(name).is_some()
                })
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect()
        })
        .unwrap_or_default(),
        TagType::Ape => read_ape(path, container, custom_item_options())?
            .map(|tag| {
                tag.into_iter()
                    .filter(|item| !maps_to_item_key(TagType::Ape, item.key()))
                    .filter_map(|item| {
                        // Binary items (APE keeps cover art in one) are not text
                        // and have no place in the text-only `TagMap`; the write
                        // path carries them over untouched.
                        match item.value() {
                            ItemValue::Text(text) | ItemValue::Locator(text) => {
                                Some((item.key().to_string(), text.clone()))
                            }
                            _ => None,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    })
}

/// The custom fields that have no generic key for `tag_type`, and so have to be
/// put onto the concrete tag by name. A custom field whose name *does* map is
/// left to the generic tag, which spells it the way the format wants.
///
/// A model field the generic tag cannot carry on `tag_type` at all goes the same
/// way, under its name from [`field_name_on_tag`].
fn raw_custom_fields(tags: &TagMap, tag_type: TagType) -> impl Iterator<Item = (&str, &str)> {
    tags.iter().filter_map(move |(field, value)| match field {
        TagField::Custom(name) => item_key_for_custom_name(tag_type, name)
            .is_none()
            .then_some((name.as_str(), value.as_str())),
        other => field_name_on_tag(other, tag_type).map(|name| (name, value.as_str())),
    })
}

/// Put into `tags` the fields only the concrete form of `tag` holds.
///
/// The rest of the fields are the format-specific items the generic tag has no
/// key for, which it therefore cannot hand over at all (#201) — they come off
/// the concrete tag, under their own names. Shared by the read of a file and
/// the read of one block, so a block's content is everything the file shows
/// from it (#616).
fn insert_concrete_items(
    path: &Path,
    container: FileType,
    tag: Tag,
    tags: &mut TagMap,
) -> Result<(), TagIoError> {
    let tag_type = tag.tag_type();
    let custom = if tag_type == TagType::Mp4Ilst {
        // One conversion serves both the custom fields and the integer
        // tempo the generic tag left behind (#610).
        let ilst = Ilst::from(tag);
        if let Some(bpm) = ilst_integer_tempo(&ilst) {
            tags.entry(TagField::Bpm).or_insert(bpm);
        }
        ilst_custom_items(&ilst)
    } else {
        custom_items(path, container, tag)?
    };
    // A name the block repeats — a comment stated twice, an atom with
    // several values — is the file stating several values, and they
    // are joined the way a multi-value field is (#609). Keeping only
    // the last showed one of them and let any edit drop the rest.
    if tag_type == TagType::Id3v2 {
        split_unparsed_number_pair(tags, &TagField::TrackNumber, &TagField::TrackTotal);
        split_unparsed_number_pair(tags, &TagField::DiscNumber, &TagField::DiscTotal);
    }
    let mut repeated = std::collections::BTreeSet::new();
    for (name, value) in custom {
        // A Vorbis track or disc number as the file spells it (#638): it only
        // puts the digits back on the number the generic read already holds.
        if tag_type == TagType::VorbisComments {
            if let Some((number, total)) = number_pair_fields(&name) {
                keep_spelled_number_pair(tags, &number, &total, &value);
                continue;
            }
        }
        let (field, from_alias) = match field_for_name_on_tag(&name, tag_type) {
            Some(field) => (field, false),
            None => field_for_custom_name(&name),
        };
        if !from_alias && !repeated.insert(field.clone()) {
            if let Some(joined) = tags.get_mut(&field) {
                joined.push_str(&multi_value_separator());
                joined.push_str(&value);
                continue;
            }
        }
        insert_read_value(tags, field, from_alias, value);
    }
    Ok(())
}

/// Whether a block's values say what the track is: a title, an artist or an
/// album that is not blank. What decides whether a block the read would not
/// have chosen is read instead of an empty one (#615). Any field at all is not
/// enough: in a real library nearly every ID3v1 tag behind an empty ID3v2 one
/// was zero-filled, holding only the genre its zero byte decodes to ("Blues"),
/// a comment of spaces, or a lone year — and reading that would show it and let
/// the next write copy it into ID3v2.
fn names_the_track(tags: &TagMap) -> bool {
    [TagField::Title, TagField::Artist, TagField::Album]
        .iter()
        .any(|field| {
            tags.get(field)
                .is_some_and(|value| !value.trim().is_empty())
        })
}

/// The name the URL is stored under where the format has no frame for it.
const URL_ITEM_NAME: &str = "WWW";

/// The name a model field is written under, by hand, on a tag type whose
/// generic tag has no key for it (#613). The backend maps the URL to ID3v2's
/// `WOAF` and nothing else, and drops the item from every other tag type on
/// save; `WWW` is what most taggers write there instead.
///
/// The BPM and the musical key the same way on APE (#663): the backend maps
/// neither there, so both were dropped on save. They go under the names Vorbis
/// gives them, which is what taggers write into APE as well.
fn field_name_on_tag(field: &TagField, tag_type: TagType) -> Option<&'static str> {
    match field {
        TagField::Url
            if matches!(
                tag_type,
                TagType::VorbisComments | TagType::Ape | TagType::Mp4Ilst
            ) =>
        {
            Some(URL_ITEM_NAME)
        }
        TagField::Bpm if tag_type == TagType::Ape => Some("BPM"),
        TagField::InitialKey if tag_type == TagType::Ape => Some("INITIALKEY"),
        _ => None,
    }
}

/// The model field a format-specific item name is, on `tag_type` — the inverse
/// of [`field_name_on_tag`].
fn field_for_name_on_tag(name: &str, tag_type: TagType) -> Option<TagField> {
    [TagField::Url, TagField::Bpm, TagField::InitialKey]
        .into_iter()
        .find(|field| {
            field_name_on_tag(field, tag_type).is_some_and(|own| own.eq_ignore_ascii_case(name))
        })
}

/// Facade over the tag I/O backend (lofty). The rest of the codebase must
/// never touch the backend directly — this is the only place format-specific
/// knowledge is allowed to live.
pub struct TagEngine;

// [2026-11-03 15:30] Added: the boundary that turns a tag-library panic into an error.
/// Run `body`, one public [`TagEngine`] operation on the file at `path`, and
/// turn a panic inside it into an error about that file.
///
/// The tag library panics on some damaged files instead of returning an error
/// (an Opus header cut short fails an `expect`, a Speex one divides by zero).
/// Every tag read of a library listing runs under the app's one lock, so one
/// such file in a folder poisoned that lock and every later command failed
/// until a restart. As an error, the file is listed as unreadable like any
/// other damaged file (#83) and everything else goes on.
fn guarded<T>(path: &Path, body: impl FnOnce() -> Result<T, TagIoError>) -> Result<T, TagIoError> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)).unwrap_or_else(|_| {
        Err(TagIoError::Malformed(format!(
            "{} is damaged: the tag library failed while reading it",
            path.display()
        )))
    })
}

// [2026-11-03 15:50] Added: an Ogg write that cannot leave the file shorter or unreadable.
/// Run `body`, a write to the file at `path`, and when that file is Ogg
/// (Vorbis, Opus, Speex) put its bytes back if the write left it unreadable or
/// cut its audio short, returning an error instead.
///
/// The tag library's Ogg writer drops pages when the stream holds bytes between
/// two pages, which a damaged file can: a Speex file that read and played came
/// back 7 KB shorter, its audio gone, from a write that reported success. The
/// last page's granule position is how far the audio runs, so a write that
/// keeps the file readable and that position has kept the audio. Ogg files are
/// small and the writer reads them whole anyway, so holding a copy costs little.
fn ogg_safe_write<T>(
    path: &Path,
    body: impl FnOnce() -> Result<T, TagIoError>,
) -> Result<T, TagIoError> {
    if !matches!(
        FileType::from_path(path),
        Some(FileType::Vorbis | FileType::Opus | FileType::Speex)
    ) {
        return body();
    }
    let before = std::fs::read(path)?;
    let result = body();
    let after = std::fs::read(path)?;
    if after == before {
        return result;
    }
    let readable =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| probe_file(path).is_ok()))
            .unwrap_or(false);
    let intact = readable && last_ogg_granule(&after) == last_ogg_granule(&before);
    if result.is_ok() && intact {
        return result;
    }
    rewrite_in_place(path, &after, &before)?;
    match result {
        Err(error) => Err(error),
        Ok(_) => Err(TagIoError::Malformed(format!(
            "could not write tags to {}: its Ogg stream is damaged, and the write would have cut its audio",
            path.display()
        ))),
    }
}

// [2026-11-03 15:50] Added: how far an Ogg stream's audio runs.
/// The granule position of the last Ogg page in `bytes`, found from the end,
/// or `None` when no page header is there.
fn last_ogg_granule(bytes: &[u8]) -> Option<i64> {
    let at = bytes.windows(4).rposition(|window| window == b"OggS")?;
    let header = bytes.get(at..at + 14)?;
    if header[4] != 0 {
        return None;
    }
    Some(i64::from_le_bytes(header[6..14].try_into().ok()?))
}

impl TagEngine {
    /// Read tags from a file on disk.
    pub fn read(path: &Path) -> Result<TrackFile, TagIoError> {
        // [2026-11-03 15:30] Guarded: a panic in the tag library on a damaged file becomes an error.
        guarded(path, || Ok(Self::read_with_props(path)?.file))
    }

    /// Read the tags, the technical properties **and** the tag blocks from one
    /// probe (#172, #47).
    ///
    /// The file listing needs all three — the tags for the columns, the duration
    /// for the Length column, the blocks for the Tags column — and parsing the
    /// file once per answer would multiply the cost of opening a library of
    /// thousands of files by the number of questions asked of it.
    /// [`read`](Self::read) is this keeping only the first.
    pub fn read_with_props(path: &Path) -> Result<TrackRead, TagIoError> {
        // [2026-11-03 15:30] Guarded: a panic in the tag library on a damaged file becomes an error.
        guarded(path, || {
            let mut tagged_file = probe_file(path)?;
            // The container the file actually parsed as — which after #204 is not
            // always what a sniff of its first bytes says. Passed down so the
            // concrete reads look at the same file the values came from.
            let container = tagged_file.file_type();
            let format = AudioFormat::from_lofty(container)?;
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
            // An APE tag's pictures never reach the generic tag, so a file with one
            // is asked again, concretely (#608). Only those files pay for it.
            let has_cover = tagged_file
                .tags()
                .iter()
                .any(|tag| !tag.pictures().is_empty())
                || (present.contains(&TagType::Ape)
                    && read_ape_pictures(path, container)
                        .is_ok_and(|pictures| !pictures.is_empty()));
            let priority_type = READ_PRIORITY
                .read()
                .ok()
                .and_then(|order| choose_priority_type(&present, &order));

            // The block is taken out of the file rather than borrowed from it: the
            // custom fields are read off its *concrete* form, and for ID3v2 and MP4
            // that conversion recovers them from the parse already done — but it
            // consumes the tag to do it (#201). The choice itself is unchanged: the
            // prioritized block, else the primary one, else the first present.
            let primary = tagged_file.primary_tag_type();
            let first_choice = priority_type
                .or_else(|| present.contains(&primary).then_some(primary))
                .or_else(|| present.first().copied());
            // A block that holds no field at all is not where the values are while
            // another block holds some (#615): an ID3v2 tag made only to carry a
            // picture hid the ID3v1 tag beside it, and the next write, keeping the
            // two in step, rewrote ID3v1 with the nothing it had read. So the others
            // are tried in turn, and the first one that names the track is read
            // instead — see [`names_the_track`] for why a field alone is not enough.
            let candidates: Vec<TagType> = first_choice
                .into_iter()
                .chain(present.iter().copied().filter(|t| Some(*t) != first_choice))
                .collect();
            let mut chosen = first_choice;

            let mut tags = TagMap::new();
            for candidate in candidates {
                let Some(tag) = tagged_file.remove(candidate) else {
                    continue;
                };
                let tag_type = tag.tag_type();
                let comment = owned_comment(&tag);
                for item in tag.items() {
                    // What made the file is not what the file is about (#197).
                    if is_provenance_key(item.key()) {
                        continue;
                    }
                    if is_unowned_comment(item, comment) {
                        continue;
                    }
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
                        let field = item_key_to_tag_field(item.key(), tag_type);
                        insert_read_value(&mut tags, field, false, unpadded(tag_type, value));
                    }
                }
                insert_concrete_items(path, container, tag, &mut tags)?;
                if tag_type == TagType::Id3v2 && !tags.contains_key(&TagField::Year) {
                    if let Some(year) = lost_v23_year(path, container) {
                        tags.insert(TagField::Year, year);
                    }
                }
                if Some(candidate) == first_choice {
                    if !tags.is_empty() {
                        break;
                    }
                } else if names_the_track(&tags) {
                    chosen = Some(candidate);
                    break;
                } else {
                    tags.clear();
                }
            }

            // Which blocks the file carries, in the order it carries them, with the
            // one being read marked (#47).
            let blocks = present
                .iter()
                .filter_map(|tag_type| {
                    let kind = TagBlockKind::from_tag_type(*tag_type)?;
                    let read_from = chosen == Some(*tag_type);
                    Some(TagBlock { kind, read_from })
                })
                .collect();

            Ok(TrackRead {
                file: TrackFile {
                    path: path.to_path_buf(),
                    format,
                    tags,
                },
                props,
                blocks,
                has_cover,
            })
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
        // [2026-11-03 15:30] Guarded: a panic in the tag library on a damaged file becomes an error.
        guarded(&file.path, || {
            // [2026-11-03 15:50] An Ogg file is put back if the write damaged it.
            ogg_safe_write(&file.path, || {
                ensure_audio_after_tags(&file.path)?;
                ensure_no_misread_id3v1(&file.path)?;
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
                // The rating the same way (#654): the backend drops a rating it cannot
                // parse instead of reporting it, so a `5` typed into the field deleted
                // the file's rating.
                let tag_type = file.format.primary_tag_type();
                for (field, value) in &file.tags {
                    if item_key_for(field, tag_type) == Some(ItemKey::Popularimeter)
                        && !is_writable_rating(value)
                    {
                        return Err(TagIoError::Malformed(format!(
                    "invalid rating {value:?}: a rating is written as email|stars|plays, with 1 to 5 stars (e.g. rater@example.com|4|0)"
                )));
                    }
                }
                drop_id3v2_extended_header(&file.path)?;
                if file.format.uses_id3v2() {
                    write_id3v2(&file.path, file.format.to_lofty(), &file.tags)
                } else {
                    write_generic(file)
                }
            })
        })
    }

    /// Read the track's playback duration from its audio properties. Returns
    /// `Duration::ZERO` when the backend can't determine it. Used by the preview
    /// player for the seek bar / total time.
    pub fn read_duration(path: &Path) -> Result<std::time::Duration, TagIoError> {
        // [2026-11-03 15:30] Guarded: a panic in the tag library on a damaged file becomes an error.
        guarded(path, || {
            let tagged = probe_file(path)?;
            Ok(tagged.properties().duration())
        })
    }

    /// Read the technical audio properties from one probe (#40): duration and
    /// bitrate, the two the duplicate view needs beyond file size, plus sample
    /// rate and channel count for the `%_samplerate%` / `%_channels%` mask
    /// placeholders (#147). One probe, so a mask asking for several of them
    /// costs no more than asking for one.
    pub fn read_audio_props(path: &Path) -> Result<AudioProps, TagIoError> {
        // [2026-11-03 15:30] Guarded: a panic in the tag library on a damaged file becomes an error.
        guarded(path, || {
            let tagged = probe_file(path)?;
            let props = tagged.properties();
            Ok(AudioProps {
                duration_secs: props.duration().as_secs(),
                bitrate_kbps: props.audio_bitrate(),
                sample_rate_hz: props.sample_rate(),
                channels: props.channels(),
            })
        })
    }

    /// Which ID3v2 revision this file's ID3v2 block is written in (#47), or
    /// `None` when it has no ID3v2 block.
    ///
    /// A deliberate second read of one file, not part of the listing: see
    /// [`Id3v2Revision`] for the measurement that put it here. Ask for it where
    /// it is actionable — a file in front of you, a conversion about to run —
    /// never per row of a library.
    pub fn id3v2_revision(path: &Path) -> Result<Option<Id3v2Revision>, TagIoError> {
        // [2026-11-03 15:30] Guarded: a panic in the tag library on a damaged file becomes an error.
        guarded(path, || {
            let Some(container) = container_of(path)? else {
                return Ok(None);
            };
            Ok(read_id3v2(path, container, custom_item_options())?
                .map(|tag| Id3v2Revision::from_lofty(tag.original_version())))
        })
    }

    /// The file's ID3v2 block as bytes, so undo can put it back frame for
    /// frame (#206). `None` for a file with no ID3v2 block, and for every other
    /// kind of block.
    ///
    /// A rebuild from [`TagBlockContent`] cannot carry a frame the model has no
    /// field for — a cue point, a rating — and a removal or a conversion
    /// destroys the block before anything can re-read it, so the bytes are the
    /// only way back. ID3v2 alone because that is where those frames live on the
    /// containers people tag, and it is the block #52 is about.
    ///
    /// Pictures are stripped: they already travel beside this as
    /// [`TagBlockContent::covers`], and leaving them in would make an
    /// artwork-heavy selection pay for its covers twice — once here and once
    /// there. [`restore_id3v2_bytes`](Self::restore_id3v2_bytes) puts them back.
    pub fn dump_id3v2(path: &Path) -> Result<Option<Vec<u8>>, TagIoError> {
        // [2026-11-03 15:30] Guarded: a panic in the tag library on a damaged file becomes an error.
        guarded(path, || {
            let Some(container) = container_of(path)? else {
                return Ok(None);
            };
            let Some(mut tag) = read_id3v2(path, container, custom_item_options())? else {
                return Ok(None);
            };
            tag.retain(|frame| !matches!(frame, Frame::Picture(_)));
            let mut bytes = Vec::new();
            tag.dump_to(&mut bytes, write_options())?;
            Ok(Some(bytes))
        })
    }

    /// Put back a block [`dump_id3v2`](Self::dump_id3v2) took, with `covers` as
    /// its pictures (#206).
    ///
    /// The backend can write an ID3v2 chunk out but has no public way to read
    /// one back, so the bytes are handed to it as what they already are: an
    /// ID3v2 chunk followed by MPEG frames IS an MP3, and reading that back
    /// gives every frame — binary ones included — without this module having to
    /// understand any of them. The silent frames exist only to make the parse
    /// succeed and are never written anywhere.
    ///
    /// Only [`Executor`](crate::plan::Executor) should call this.
    pub fn restore_id3v2_bytes(
        path: &Path,
        bytes: &[u8],
        covers: &[CoverArt],
        revision: Option<Id3v2Revision>,
    ) -> Result<(), TagIoError> {
        // [2026-11-03 15:30] Guarded: a panic in the tag library on a damaged file becomes an error.
        guarded(path, || {
            ensure_audio_after_tags(path)?;
            ensure_no_misread_id3v1(path)?;
            drop_id3v2_extended_header(path)?;
            // A block journaled before #648 can still carry its extended header.
            let mut synthetic =
                without_id3v2_extended_header(bytes).unwrap_or_else(|| bytes.to_vec());
            let mut silence = vec![0xFF, 0xFB, 0x90, 0x00];
            silence.resize(SILENT_MPEG_FRAME_LEN, 0);
            for _ in 0..SILENT_MPEG_FRAMES {
                synthetic.extend_from_slice(&silence);
            }
            let mut tag = MpegFile::read_from(&mut Cursor::new(synthetic), custom_item_options())?
                .id3v2()
                .cloned()
                .ok_or_else(|| {
                    TagIoError::UnsupportedFormat(
                        "the journaled ID3v2 block did not parse".to_string(),
                    )
                })?;
            // The parse just now trims a timestamp the same way a file read does;
            // the bytes still hold its text (#504).
            keep_timestamp_text(&mut tag, bytes);
            for cover in covers {
                tag.insert_picture(picture_from_cover(cover));
            }
            let options = match revision {
                Some(revision) => write_options().use_id3v23(revision == Id3v2Revision::V3),
                None => id3_write_options(),
            };
            match container_of(path)? {
                Some(container) => save_id3v2_tag_with(path, container, &tag, options)?,
                None => tag.save_to_path(path, options)?,
            }
            Ok(())
        })
    }

    /// The file's Vorbis comment block as bytes, so undo can put it back
    /// comment for comment (#669): the vendor string and every comment in the
    /// order the file holds them, provenance included, laid out as a Vorbis
    /// comment packet. `None` for a file with no such block.
    ///
    /// Pictures are left out, as in [`dump_id3v2`](Self::dump_id3v2): they
    /// travel beside this as [`TagBlockContent::covers`].
    pub fn dump_vorbis(path: &Path) -> Result<Option<Vec<u8>>, TagIoError> {
        // [2026-11-03 15:30] Guarded: a panic in the tag library on a damaged file becomes an error.
        guarded(path, || {
            let Some(container) = container_of(path)? else {
                return Ok(None);
            };
            // As the file spells them: no number parsed, no name folded.
            let options = custom_item_options().implicit_conversions(false);
            Ok(read_vorbis(path, container, options)?.map(|tag| vorbis_comment_packet(&tag)))
        })
    }

    /// Put back a block [`dump_vorbis`](Self::dump_vorbis) took, with `covers`
    /// as its pictures (#669).
    ///
    /// Only [`Executor`](crate::plan::Executor) should call this.
    pub fn restore_vorbis_bytes(
        path: &Path,
        bytes: &[u8],
        covers: &[CoverArt],
    ) -> Result<(), TagIoError> {
        // [2026-11-03 15:30] Guarded: a panic in the tag library on a damaged file becomes an error.
        guarded(path, || {
            // [2026-11-03 15:50] An Ogg file is put back if the write damaged it.
            ogg_safe_write(path, || {
                ensure_audio_after_tags(path)?;
                ensure_no_misread_id3v1(path)?;
                let mut tag = vorbis_comments_from_packet(bytes).ok_or_else(|| {
                    TagIoError::UnsupportedFormat(
                        "the journaled Vorbis comment block did not parse".to_string(),
                    )
                })?;
                for cover in covers {
                    let picture = picture_from_cover(cover);
                    let information =
                        PictureInformation::from_picture(&picture).unwrap_or_default();
                    tag.insert_picture(picture, Some(information)).ok();
                }
                tag.save_to_path(path, write_options())?;
                Ok(())
            })
        })
    }

    /// Rewrite the file's ID3v2 block at `revision`, keeping every frame it
    /// holds (#205). `false` when the file carries no ID3v2 block.
    ///
    /// Deliberately not a rebuild from the model's own view of the block, which
    /// is what [`write_block`](Self::write_block) does: that view does not
    /// surface a binary frame at all, so rebuilding would drop the cue points,
    /// ratings and ReplayGain data that #52 exists to protect. Changing 2.3 to
    /// 2.4 is meant to change the header, not to empty the tag — so this is the
    /// concrete path, the same one an ordinary write uses.
    ///
    /// Only [`Executor`](crate::plan::Executor) should call this.
    pub fn set_id3v2_revision(path: &Path, revision: Id3v2Revision) -> Result<bool, TagIoError> {
        // [2026-11-03 15:30] Guarded: a panic in the tag library on a damaged file becomes an error.
        guarded(path, || {
            ensure_audio_after_tags(path)?;
            ensure_no_misread_id3v1(path)?;
            drop_id3v2_extended_header(path)?;
            let Some(container) = container_of(path)? else {
                return Ok(false);
            };
            // With the pictures: what is read here is what gets written back, and a
            // read without them saved a tag without them (#607).
            let Some(tag) = read_id3v2(path, container, tag_only_options())? else {
                return Ok(false);
            };
            save_id3v2_tag_with(
                path,
                container,
                &tag,
                write_options().use_id3v23(revision == Id3v2Revision::V3),
            )?;
            Ok(true)
        })
    }

    /// What one of the file's tag blocks holds, as a snapshot undo can rebuild
    /// it from (#47). `None` when the file carries no block of that kind.
    ///
    /// Taken from the generic representation of that block, which is where the
    /// backend surfaces every item it has a key for, plus the pictures. What it
    /// does *not* surface — a format-specific item, a binary frame — is the
    /// difference [`TagBlockContent::exact`] warns about.
    pub fn read_block(
        path: &Path,
        kind: TagBlockKind,
    ) -> Result<Option<TagBlockContent>, TagIoError> {
        // [2026-11-03 15:30] Guarded: a panic in the tag library on a damaged file becomes an error.
        guarded(path, || {
            let tagged = probe_file(path)?;
            let tag_type = kind.to_tag_type();
            let Some(tag) = tagged.tag(tag_type) else {
                return Ok(None);
            };
            // An APE tag's pictures never reach the generic tag (#608).
            let covers = if tag_type == TagType::Ape {
                read_ape_pictures(path, tagged.file_type())?
                    .iter()
                    .map(cover_from_picture)
                    .collect()
            } else {
                tag.pictures().iter().map(cover_from_picture).collect()
            };
            // The custom fields too (#616): without them a conversion wrote a
            // block without them and removed the one that had them.
            let mut tags = tag_to_map(tag);
            insert_concrete_items(path, tagged.file_type(), tag.clone(), &mut tags)?;
            Ok(Some(TagBlockContent { tags, covers }))
        })
    }

    /// Remove one tag block, leaving every other block in the file alone (#47).
    ///
    /// Not `remove_from_path`: on this backend that opens the file for reading
    /// only and then cannot shorten it, so the block survives — the same trap
    /// #194's ID3v1 removal hit. A handle we open for writing is the same code
    /// path with a file it can actually write.
    ///
    /// Only [`Executor`](crate::plan::Executor) should call this.
    pub fn remove_block(path: &Path, kind: TagBlockKind) -> Result<(), TagIoError> {
        // [2026-11-03 15:30] Guarded: a panic in the tag library on a damaged file becomes an error.
        guarded(path, || {
            // [2026-11-03 15:50] An Ogg file is put back if the write damaged it.
            ogg_safe_write(path, || {
                ensure_audio_after_tags(path)?;
                ensure_no_misread_id3v1(path)?;
                drop_id3v2_extended_header(path)?;
                // An AIFF's or a WAV's ID3v2 is a chunk this module writes itself
                // (#358, #505); taking it out goes the same way.
                if kind == TagBlockKind::Id3v2
                    && matches!(container_of(path)?, Some(FileType::Aiff | FileType::Wav))
                    && write_iff_id3(path, None)?
                {
                    return Ok(());
                }
                let mut handle = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(path)?;
                kind.to_tag_type()
                    .remove_from(&mut handle, write_options())?;
                Ok(())
            })
        })
    }

    /// Put a removed tag block back from its snapshot (#47) — the undo of
    /// [`remove_block`](Self::remove_block).
    ///
    /// Only [`Executor`](crate::plan::Executor) should call this.
    pub fn restore_block(
        path: &Path,
        kind: TagBlockKind,
        content: &TagBlockContent,
    ) -> Result<(), TagIoError> {
        // [2026-11-03 15:30] Guarded: a panic in the tag library on a damaged file becomes an error.
        guarded(path, || Self::write_block(path, kind, None, content))
    }

    /// Write one tag block of `kind`, replacing whatever block of that kind the
    /// file carries and leaving every other block alone (#205).
    ///
    /// `revision` chooses the ID3v2 revision for this write and is ignored for
    /// every other kind; `None` means the app-wide preference (#79), which is
    /// what an ordinary write and a restore both want. A conversion passes the
    /// revision the user picked, because "make this file's ID3v2 a 2.3" is the
    /// single most-asked-for case of #205 and the app-wide default is the wrong
    /// lever for one file.
    ///
    /// Only [`Executor`](crate::plan::Executor) should call this.
    pub fn write_block(
        path: &Path,
        kind: TagBlockKind,
        revision: Option<Id3v2Revision>,
        content: &TagBlockContent,
    ) -> Result<(), TagIoError> {
        // [2026-11-03 15:30] Guarded: a panic in the tag library on a damaged file becomes an error.
        guarded(path, || {
            // [2026-11-03 15:50] An Ogg file is put back if the write damaged it.
            ogg_safe_write(path, || {
                ensure_audio_after_tags(path)?;
                ensure_no_misread_id3v1(path)?;
                drop_id3v2_extended_header(path)?;
                let options = match (kind, revision) {
                    (TagBlockKind::Id3v2, Some(revision)) => {
                        write_options().use_id3v23(revision == Id3v2Revision::V3)
                    }
                    (TagBlockKind::Id3v2, None) => id3_write_options(),
                    _ => write_options(),
                };
                let mut tag = tag_for_block(kind, content);
                // The fields the generic tag cannot carry go onto the concrete one by
                // name (#616), the way an ordinary write puts them there.
                let named: Vec<(String, String)> =
                    raw_custom_fields(&content.tags, kind.to_tag_type())
                        .map(|(name, value)| (name.to_string(), value.to_string()))
                        .collect();
                match kind {
                    TagBlockKind::Id3v2 => {
                        let mut id3 = Id3v2Tag::from(tag);
                        spell_id3v2_number_pairs(&mut id3, &content.tags);
                        for (name, value) in named {
                            id3.insert(Frame::UserText(ExtendedTextFrame::new(
                                TextEncoding::UTF8,
                                name,
                                value,
                            )));
                        }
                        order_frames(&mut id3, None);
                        match container_of(path)? {
                            Some(container) => save_id3v2_tag_with(path, container, &id3, options)?,
                            None => id3.save_to_path(path, options)?,
                        }
                    }
                    TagBlockKind::VorbisComments => {
                        // Carried by hand, as in [`save_with_custom_items`]: the
                        // conversion drops an image it cannot measure.
                        let pictures: Vec<Picture> = tag.pictures().to_vec();
                        while !tag.pictures().is_empty() {
                            tag.remove_picture(0);
                        }
                        let mut vorbis = vorbis_comments_from(tag);
                        for picture in pictures {
                            let information =
                                PictureInformation::from_picture(&picture).unwrap_or_default();
                            vorbis.insert_picture(picture, Some(information)).ok();
                        }
                        for (name, value) in named {
                            vorbis.push(name, value);
                        }
                        vorbis.save_to_path(path, options)?;
                    }
                    TagBlockKind::Ape => {
                        let mut ape = ApeTag::from(tag);
                        spell_ape_number_pairs(&mut ape, &content.tags);
                        for (name, value) in named {
                            if let Ok(item) = ApeItem::new(name, ItemValue::Text(value)) {
                                ape.insert(item);
                            }
                        }
                        ape.save_to_path(path, options)?;
                        // The APE tag carries its pictures as items of its own (#608).
                        if let Some(container) = container_of(path)? {
                            write_ape_pictures(path, container, &content.covers)?;
                        }
                    }
                    TagBlockKind::Mp4Ilst => {
                        let mut ilst = Ilst::from(tag);
                        for (name, value) in named {
                            ilst.insert(Atom::new(
                                AtomIdent::Freeform {
                                    mean: FREEFORM_MEAN.into(),
                                    name: name.into(),
                                },
                                AtomData::UTF8(value),
                            ));
                        }
                        // One integer tempo, as an ordinary write leaves it (#610),
                        // taken out in order (#618).
                        ilst.retain(|atom| atom.ident() != &MP4_TEMPO);
                        if let Some(bpm) = content.tags.get(&TagField::Bpm) {
                            ilst.insert(mp4_tempo_atom(bpm));
                        }
                        ilst.save_to_path(path, options)?;
                    }
                    _ => tag.save_to_path(path, options)?,
                }
                Ok(())
            })
        })
    }

    /// Every image the file carries (#56), in the order the tag holds them.
    pub fn read_covers(path: &Path) -> Result<Vec<CoverArt>, TagIoError> {
        // [2026-11-03 15:30] Guarded: a panic in the tag library on a damaged file becomes an error.
        guarded(path, || {
            let tagged = probe_file(path)?;
            let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else {
                return Ok(Vec::new());
            };
            // An APE tag's pictures never reach the generic tag (#608).
            if tag.tag_type() == TagType::Ape {
                return Ok(read_ape_pictures(path, tagged.file_type())?
                    .iter()
                    .map(cover_from_picture)
                    .collect());
            }
            Ok(tag.pictures().iter().map(cover_from_picture).collect())
        })
    }

    /// The file's front cover (or the first embedded image if there's no
    /// explicit front cover), if any — "the cover" as the rest of the app means
    /// it, over the set [`read_covers`](Self::read_covers) returns.
    pub fn read_cover(path: &Path) -> Result<Option<CoverArt>, TagIoError> {
        // [2026-11-03 15:30] Guarded: a panic in the tag library on a damaged file becomes an error.
        guarded(path, || {
            let covers = Self::read_covers(path)?;
            Ok(covers
                .iter()
                .find(|cover| cover.kind == CoverKind::Front)
                .or_else(|| covers.first())
                .cloned())
        })
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
        // [2026-11-03 15:30] Guarded: a panic in the tag library on a damaged file becomes an error.
        guarded(path, || {
            // [2026-11-03 15:50] An Ogg file is put back if the write damaged it.
            ogg_safe_write(path, || {
                ensure_audio_after_tags(path)?;
                ensure_no_misread_id3v1(path)?;
                drop_id3v2_extended_header(path)?;
                // Same reason as `write`: going through the generic tag would drop an
                // MP3's non-representable frames, so edit the concrete tag instead.
                if is_id3v2_container(path) {
                    let container = container_of(path)?.ok_or(TagIoError::UnsupportedFormat(
                        path.to_string_lossy().into_owned(),
                    ))?;
                    let mut tag =
                        read_id3v2(path, container, tag_only_options())?.unwrap_or_default();
                    // `remove_picture_type` takes one type at a time and the file may
                    // hold types the model maps onto one of ours, so clear by rebuilding
                    // from the frames that are not pictures at all.
                    tag.retain(|frame| !matches!(frame, Frame::Picture(_)));
                    for cover in covers {
                        tag.insert_picture(picture_from_cover(cover));
                    }
                    // AIFF and WAV get the hand-written `ID3 ` chunk (#358, #505), same
                    // as the text write, so lofty's IFF writers can't drop the cover or
                    // break the container on a later edit.
                    save_id3v2_tag(path, container, &tag)?;
                    return Ok(());
                }
                let mut tag = load_or_new_tag(path)?;
                // An APE tag's pictures are items of the concrete tag, which is edited
                // in place: nothing else in it is touched (#608).
                if tag.tag_type() == TagType::Ape {
                    if let Some(container) = container_of(path)? {
                        return write_ape_pictures(path, container, covers);
                    }
                }
                while !tag.pictures().is_empty() {
                    tag.remove_picture(0);
                }
                for cover in covers {
                    tag.push_picture(picture_from_cover(cover));
                }
                // A plain save of the generic tag drops every custom field a Vorbis
                // Comments or APE tag holds (#201), so those go out the way a text write
                // does, with the file's own fields (#606).
                match tag.tag_type() {
                    TagType::VorbisComments | TagType::Ape => {
                        let file = Self::read(path)?;
                        save_with_custom_items(path, file.format.to_lofty(), tag, &file.tags)?;
                    }
                    _ => tag.save_to_path(path, write_options())?,
                }
                Ok(())
            })
        })
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
    let mut builder = Picture::unchecked(cover.data.clone())
        .pic_type(cover.kind.to_picture_type())
        .mime_type(MimeType::from_str(&cover.mime));
    if !cover.description.is_empty() {
        builder = builder.description(cover.description.clone());
    }
    builder.build()
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
/// Whether the file ends in an ID3v1 tag block. Read by hand — the block is
/// always the last 128 bytes and starts with `TAG` — rather than by probing the
/// file again, which a write has already done once.
fn has_id3v1(path: &Path) -> Result<bool, TagIoError> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    let length = file.metadata()?.len();
    if length < 128 {
        return Ok(false);
    }
    file.seek(SeekFrom::Start(length - 128))?;
    let mut magic = [0u8; 3];
    file.read_exact(&mut magic)?;
    Ok(&magic == b"TAG")
}

/// Keep an ID3v1 tag the file already carries in step with what was just
/// written (#194).
///
/// A file with both tags has two answers to every question, and only the ID3v2
/// one was ever updated: the app showed the new value while anything reading
/// ID3v1 — older DJ hardware, other software — still read the old one. Clearing
/// the tags made it visible, because the stale ID3v1 was then the only tag left
/// and read back as if nothing had been cleared, with every value cut to the 30
/// bytes ID3v1 allows.
///
/// So a file that has one gets it rewritten from the model, and loses it when
/// the model has nothing it can hold. One is never created where there wasn't
/// one — nobody asked for a legacy tag — and neither cover art nor DJ cue
/// points are involved, since ID3v1 cannot hold either.
///
/// A field the model has no value for keeps the file's own, unless the file
/// read with a value for it (`before`) and the model cleared it (#653). A file
/// with an ID3v2 tag is read from that alone, so an album or a genre only the
/// ID3v1 tag states never reached the model, and rewriting the tag from the
/// model deleted it on a save that changed nothing.
fn sync_id3v1(
    path: &Path,
    container: FileType,
    tags: &TagMap,
    before: &TagMap,
) -> Result<(), TagIoError> {
    if !has_id3v1(path)? {
        return Ok(());
    }
    let old = {
        let mut file = buffered(path)?;
        match container {
            // [2026-11-03 14:40] Read past ID3v2 frames the backend cannot decode, as every MP3 read is.
            FileType::Mpeg => {
                read_mpeg_past_undecodable_frames(path, &mut file, tag_only_options())?
                    .id3v1()
                    .cloned()
            }
            FileType::Aac => AacFile::read_from(&mut file, tag_only_options())?
                .id3v1()
                .cloned(),
            _ => None,
        }
        .unwrap_or_default()
    };
    let stated = |map: &TagMap, field: &TagField| map.get(field).is_some_and(|v| !v.is_empty());
    let untouched = |field: &TagField| !stated(tags, field) && !stated(before, field);
    // The seven fields ID3v1 has room for; lofty truncates the text on write.
    let mut legacy = Id3v1Tag {
        title: tags.get(&TagField::Title).cloned(),
        artist: tags.get(&TagField::Artist).cloned(),
        album: tags.get(&TagField::Album).cloned(),
        // Four digits, so a full date (ID3v2.4 allows one) contributes its year.
        year: tags
            .get(&TagField::Year)
            .and_then(|year| year.get(..4).unwrap_or(year).parse::<u16>().ok()),
        comment: tags.get(&TagField::Comment).cloned(),
        // One byte: a track number that doesn't fit is left out rather than
        // wrapped around.
        track_number: tags
            .get(&TagField::TrackNumber)
            .and_then(|track| track.parse::<u8>().ok())
            .filter(|track| *track > 0),
        ..Default::default()
    };
    // A genre is an index into ID3v1's fixed list, so anything outside it has
    // nowhere to go; `set_genre` leaves the field alone in that case.
    if let Some(genre) = tags.get(&TagField::Genre) {
        legacy.set_genre(genre.clone());
    }
    if untouched(&TagField::Title) {
        legacy.title.clone_from(&old.title);
    }
    if untouched(&TagField::Artist) {
        legacy.artist.clone_from(&old.artist);
    }
    if untouched(&TagField::Album) {
        legacy.album.clone_from(&old.album);
    }
    if untouched(&TagField::Year) {
        legacy.year = old.year;
    }
    if untouched(&TagField::Comment) {
        legacy.comment.clone_from(&old.comment);
    }
    if untouched(&TagField::TrackNumber) {
        legacy.track_number = old.track_number;
    }
    if untouched(&TagField::Genre) {
        legacy.genre = old.genre;
    }
    // A genre the save left as it was, with no index in ID3v1's list, keeps
    // the file's own byte (#670): "none" over it would lose what the tag said
    // although nobody changed the genre. An edit to such a genre still writes
    // "none", since the old byte no longer describes the track.
    if legacy.genre.is_none() && tags.get(&TagField::Genre) == before.get(&TagField::Genre) {
        legacy.genre = old.genre;
    }
    // A value the save leaves as it was keeps the file's own spelling of it,
    // padding and all (#664): the read drops an ID3v1 field's trailing spaces
    // and the backend writes NULs in their place. What the field has room for
    // is all it can hold, so a longer value cut to the same text is the same.
    // A tag nothing changed in is then not written at all.
    let comment_width = if legacy.track_number.is_some() {
        28
    } else {
        30
    };
    for (new, own, width) in [
        (&mut legacy.title, &old.title, 30),
        (&mut legacy.artist, &old.artist, 30),
        (&mut legacy.album, &old.album, 30),
        (&mut legacy.comment, &old.comment, comment_width),
    ] {
        let same = match (new.as_deref(), own.as_deref()) {
            (Some(value), Some(own)) => {
                let held: String = value.chars().take(width).collect();
                held.trim_end_matches(' ') == own.trim_end_matches(' ')
            }
            _ => false,
        };
        if same {
            new.clone_from(own);
        }
    }
    if legacy == old {
        return Ok(());
    }
    if legacy.is_empty() {
        // Not `remove_from_path`: on the 0.25 backend that opens the file for
        // reading only and then fails to shorten it, so the stale tag survives a
        // clear — the very thing #194 was about. Handing it a handle we opened
        // for writing is the same code path with a file it can actually write.
        let mut handle = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)?;
        TagType::Id3v1.remove_from(&mut handle, write_options())?;
    } else {
        legacy.save_to_path(path, write_options())?;
    }
    Ok(())
}

/// Whether an MP3 carries an APE block (#655). Only an MP3 is asked: it is the
/// one container written as ID3v2 that can also hold APE.
fn has_ape(path: &Path, container: FileType) -> Result<bool, TagIoError> {
    Ok(container == FileType::Mpeg && read_ape(path, container, tag_only_options())?.is_some())
}

/// The model field an APE item states, as a read of the block names it.
fn ape_item_field(key: &str) -> TagField {
    match ItemKey::from_key(TagType::Ape, key) {
        Some(item_key) => item_key_to_tag_field(item_key, TagType::Ape),
        None => field_for_name_on_tag(key, TagType::Ape)
            .unwrap_or_else(|| TagField::Custom(key.to_string())),
    }
}

/// Keep an MP3's APE block in step with the ID3v2 tag just written (#655), the
/// way [`sync_id3v1`] keeps an ID3v1 tag.
///
/// A file whose only tag was an APE block was read from it and then written as
/// ID3v2 alone: a clear left the block, so the file read back as if nothing had
/// been cleared, and an edit left it stating the old values beside the new tag.
///
/// So an item of a field the write changed takes the model's value, and one of
/// a field it cleared is taken out. A field the write did not touch keeps what
/// the block says, which on a file with an ID3v2 tag too is a field the model
/// never read. Nothing is added, since a field the block never stated is not
/// stale; pictures and the other binary items stay; a block left with nothing
/// is removed; and a save that changed nothing does not write the block.
fn sync_ape(
    path: &Path,
    container: FileType,
    tags: &TagMap,
    before: &TagMap,
) -> Result<(), TagIoError> {
    if container != FileType::Mpeg {
        return Ok(());
    }
    let Some(old) = read_ape(path, container, tag_only_options())? else {
        return Ok(());
    };
    let stated = |map: &TagMap, field: &TagField| map.get(field).filter(|v| !v.is_empty()).cloned();
    let edited = |field: &TagField| stated(tags, field) != stated(before, field);
    // A number pair is one APE item holding two fields.
    let item_edited = |field: &TagField| match field {
        TagField::TrackNumber => edited(field) || edited(&TagField::TrackTotal),
        TagField::DiscNumber => edited(field) || edited(&TagField::DiscTotal),
        other => edited(other),
    };

    // What the model's values are as APE items, spelled as a write of an APE
    // file spells them.
    let mut generic = Tag::new(TagType::Ape);
    for (field, value) in tags {
        if !value.is_empty() {
            push_field_items(&mut generic, field, value);
        }
    }
    let mut fresh = ApeTag::from(generic);
    spell_ape_number_pairs(&mut fresh, tags);
    for (name, value) in raw_custom_fields(tags, TagType::Ape) {
        if value.is_empty() {
            continue;
        }
        if let Ok(item) = ApeItem::new(name.to_string(), ItemValue::Text(value.to_string())) {
            fresh.insert(item);
        }
    }

    let mut synced = old.clone();
    let mut changed = false;
    for item in &old {
        if item.value().text().is_none() {
            continue;
        }
        if ItemKey::from_key(TagType::Ape, item.key()).is_some_and(is_provenance_key) {
            continue;
        }
        let field = ape_item_field(item.key());
        if !item_edited(&field) {
            continue;
        }
        synced.remove(item.key());
        changed = true;
        if let Some(value) = (&fresh)
            .into_iter()
            .find(|candidate| ape_item_field(candidate.key()) == field)
        {
            synced.insert(value.clone());
        }
    }
    if !changed {
        return Ok(());
    }
    if synced.is_empty() {
        // Not `remove_from_path`, for the reason [`sync_id3v1`] gives.
        let mut handle = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)?;
        TagType::Ape.remove_from(&mut handle, write_options())?;
    } else {
        synced.save_to_path(path, write_options())?;
    }
    Ok(())
}

/// Keep the RIFF INFO list of a WAV, or the text chunks of an AIFF, in step
/// with the ID3v2 tag just written (#621), the way [`sync_id3v1`] keeps an
/// ID3v1 tag.
///
/// Both containers are written as ID3v2, so a file that arrived with only the
/// legacy block got the edit in a new ID3v2 chunk beside it, and the old block
/// went on stating the old title to everything that reads it. So what a block
/// the file has states is kept in step with the model: a value the model
/// disagrees with is replaced, one it cleared is taken out. Nothing is added,
/// since a field the block never stated is not stale, and a block is never
/// created. What the model has no field of its own for (the encoder, a
/// copyright, an INFO id it does not know) stays as the file has it, and a
/// block nothing changed in is not written at all.
///
/// "Cleared" means the field read with a value (`before`) and the model now
/// has none (#652). A file that also has an ID3v2 chunk reads from that alone,
/// so a genre or an annotation only the legacy block states never reached the
/// model, and taking it out as cleared deleted data nobody had touched.
fn sync_iff_text(
    path: &Path,
    container: FileType,
    tags: &TagMap,
    before: &TagMap,
) -> Result<(), TagIoError> {
    // The release country is a custom field with a standard item on each format
    // (#612), so a RIFF INFO `ICNT` is the model's like the title is.
    let owned = |key: Option<ItemKey>, tag_type: TagType| {
        key.is_some_and(|key| {
            !is_provenance_key(key)
                && (key == ItemKey::ReleaseCountry
                    || !matches!(item_key_to_tag_field(key, tag_type), TagField::Custom(_)))
        })
    };
    match container {
        FileType::Wav => {
            let mut file = buffered(path)?;
            let Some(old) = WavFile::read_from(&mut file, tag_only_options())?
                .riff_info()
                .cloned()
            else {
                return Ok(());
            };
            drop(file);
            let field_of = |id: &str| {
                ItemKey::from_key(TagType::RiffInfo, id)
                    .map(|key| item_key_to_tag_field(key, TagType::RiffInfo))
            };
            let model = |field: &TagField| tags.get(field).filter(|value| !value.is_empty());
            let cleared = |field: &TagField| {
                model(field).is_none() && before.get(field).is_some_and(|value| !value.is_empty())
            };
            let mut synced = RiffInfoList::new();
            let mut written: std::collections::BTreeSet<TagField> = Default::default();
            for (id, value) in &old {
                if !owned(ItemKey::from_key(TagType::RiffInfo, id), TagType::RiffInfo) {
                    synced.insert(id.clone(), value.clone());
                    continue;
                }
                // The file's own id keeps the value: a track number stays in
                // `ITRK` if that is where the file had it.
                let Some(field) = field_of(id) else { continue };
                // A track number that states the total as well, as `1/15`,
                // goes on stating both.
                let wanted = if field == TagField::TrackNumber && value.contains('/') {
                    model(&field).map(|number| match model(&TagField::TrackTotal) {
                        Some(total) => format!("{number}/{total}"),
                        None => number.clone(),
                    })
                } else {
                    model(&field).cloned()
                };
                if let Some(wanted) = wanted {
                    if written.insert(field.clone()) {
                        synced.insert(id.clone(), agreeing_value(&field, value, &wanted));
                    }
                } else if !cleared(&field) {
                    synced.insert(id.clone(), value.clone());
                }
            }
            // In any order: the backend's writer does not keep the list's.
            let sorted = |list: &RiffInfoList| {
                let mut items: Vec<(String, String)> = list.into_iter().cloned().collect();
                items.sort();
                items
            };
            if sorted(&old) == sorted(&synced) {
                return Ok(());
            }
            if synced.is_empty() {
                let mut handle = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(path)?;
                TagType::RiffInfo.remove_from(&mut handle, write_options())?;
            } else {
                synced.save_to_path(path, write_options())?;
            }
        }
        FileType::Aiff => {
            let mut file = buffered(path)?;
            let Some(old) = AiffFile::read_from(&mut file, tag_only_options())?
                .text_chunks()
                .cloned()
            else {
                return Ok(());
            };
            drop(file);
            let model = |field: &TagField| tags.get(field).filter(|value| !value.is_empty());
            let cleared = |field: &TagField| {
                model(field).is_none() && before.get(field).is_some_and(|value| !value.is_empty())
            };
            let mut synced = old.clone();
            if old.name.is_some()
                && (model(&TagField::Title).is_some() || cleared(&TagField::Title))
            {
                synced.name = model(&TagField::Title).cloned();
            }
            if old.author.is_some()
                && (model(&TagField::Artist).is_some() || cleared(&TagField::Artist))
            {
                synced.author = model(&TagField::Artist).cloned();
            }
            // Annotations and comments both read as the comment. One that
            // still says what the model says leaves both as they were, since a
            // comment carries a timestamp and a marker the model has no room
            // for; otherwise the model's comment becomes the one annotation.
            let comment = model(&TagField::Comment);
            let stated: Vec<&str> = old
                .annotations
                .iter()
                .flatten()
                .map(String::as_str)
                .chain(old.comments.iter().flatten().map(|note| note.text.as_str()))
                .collect();
            let kept = stated.is_empty()
                || (comment.is_none() && !cleared(&TagField::Comment))
                || comment.is_some_and(|comment| stated.contains(&comment.as_str()));
            if !kept {
                synced.annotations = comment.map(|comment| vec![comment.clone()]);
                synced.comments = None;
            }
            if synced == old {
                return Ok(());
            }
            if synced.is_empty() {
                let mut handle = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(path)?;
                TagType::AiffText.remove_from(&mut handle, write_options())?;
            } else {
                synced.save_to_path(path, write_options())?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// The value a legacy block gets for `field`: its own `old` one where that
/// already agrees with the model, so a full date the model shows as a year is
/// not cut down to it (#621); the model's otherwise.
fn agreeing_value(field: &TagField, old: &str, model: &str) -> String {
    let stated = old.trim_end_matches('\0');
    let agrees = match field {
        TagField::Year => stated.get(..4) == Some(model) || stated == model,
        _ => stated == model,
    };
    if agrees { old } else { model }.to_string()
}

/// The number and total fields a format's number-pair item states: ID3v2's
/// `TRCK`/`TPOS`, APE's `Track`/`Disc`, Vorbis's `TRACKNUMBER`/`DISCNUMBER`.
fn number_pair_fields(name: &str) -> Option<(TagField, TagField)> {
    match name.to_ascii_uppercase().as_str() {
        "TRCK" | "TRACK" | "TRACKNUMBER" => Some((TagField::TrackNumber, TagField::TrackTotal)),
        "TPOS" | "DISC" | "DISCNUMBER" => Some((TagField::DiscNumber, TagField::DiscTotal)),
        _ => None,
    }
}

/// Whether two spellings state the same number, as `01` and `1` do.
fn same_number(a: &str, b: &str) -> bool {
    matches!(
        (a.trim().parse::<u32>(), b.trim().parse::<u32>()),
        (Ok(a), Ok(b)) if a == b
    )
}

/// A number pair the backend built from integers, as `1/3`, with each part
/// spelled the way the model spells that number (#638). Only the digits come
/// from the model; what the pair states is the backend's.
fn number_pair_as_spelled(
    text: &str,
    number: &TagField,
    total: &TagField,
    tags: &TagMap,
) -> String {
    let spelled = |part: &str, field: &TagField| match tags.get(field) {
        Some(model) if same_number(part, model) => model.trim().to_string(),
        _ => part.to_string(),
    };
    match text.split_once('/') {
        Some((first, second)) => format!("{}/{}", spelled(first, number), spelled(second, total)),
        None => spelled(text, number),
    }
}

/// The conversion to ID3v2 builds `TRCK` and `TPOS` from the numbers as
/// integers, so a model's `01` went out as `1` (#638); this spells them the
/// model's way again. A pair that is not numbers at all, as a vinyl position
/// `B1/04`, the conversion drops, so it goes on as the model states it (#640).
fn spell_id3v2_number_pairs(tag: &mut Id3v2Tag, tags: &TagMap) {
    tag.retain_mut(|frame| {
        if let Frame::Text(text) = frame {
            if let Some((number, total)) = number_pair_fields(text.id().as_str()) {
                text.value = number_pair_as_spelled(&text.value, &number, &total, tags).into();
            }
        }
        true
    });
    for id in ["TRCK", "TPOS"] {
        let Some((number, total)) = number_pair_fields(id) else {
            continue;
        };
        if let Some(text) = verbatim_number_pair(&number, &total, tags) {
            tag.insert(Frame::Text(TextInformationFrame::new(
                FrameId::Valid(id.into()),
                TextEncoding::UTF8,
                text,
            )));
        }
    }
}

/// APE's `Track` and `Disc`, built from the numbers as integers the way ID3v2's
/// `TRCK` is, spelled the model's way again (#638), and a pair that is not
/// numbers put back as the model states it (#640).
fn spell_ape_number_pairs(tag: &mut ApeTag, tags: &TagMap) {
    for name in ["Track", "Disc"] {
        let Some((number, total)) = number_pair_fields(name) else {
            continue;
        };
        let spelled = verbatim_number_pair(&number, &total, tags).or_else(|| {
            let text = tag.get(name)?.value().text()?;
            Some(number_pair_as_spelled(text, &number, &total, tags))
        });
        if let Some(Ok(item)) =
            spelled.map(|text| ApeItem::new(name.to_string(), ItemValue::Text(text)))
        {
            tag.insert(item);
        }
    }
}

/// The text of a number pair the model states with something other than
/// digits, as a vinyl position `B1` of `04` (#640), or `None` when both parts
/// are numbers and the backend's own conversion writes the pair.
fn verbatim_number_pair(number: &TagField, total: &TagField, tags: &TagMap) -> Option<String> {
    let part = |field: &TagField| {
        tags.get(field)
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
    };
    let (number, total) = (part(number), part(total));
    let numeric = |value: Option<&str>| value.is_none_or(|value| value.parse::<u32>().is_ok());
    if numeric(number) && numeric(total) {
        return None;
    }
    Some(match (number, total) {
        (Some(number), Some(total)) => format!("{number}/{total}"),
        (Some(number), None) => number.to_string(),
        // How the backend states a total without a number.
        (None, Some(total)) => format!("0/{total}"),
        (None, None) => return None,
    })
}

/// A number pair the backend could not split into integers, which an ID3v2
/// read hands over whole as the total (#640): a vinyl position `B1/04` showed
/// as a track total of `B1/04` and no track number. It is split here the way a
/// pair of numbers is.
fn split_unparsed_number_pair(tags: &mut TagMap, number: &TagField, total: &TagField) {
    if tags.contains_key(number) {
        return;
    }
    let Some(text) = tags
        .get(total)
        .filter(|text| text.trim().parse::<u32>().is_err())
    else {
        return;
    };
    let (first, second) = match text.split_once('/') {
        Some((first, second)) => (first.trim().to_string(), Some(second.trim().to_string())),
        None => (text.trim().to_string(), None),
    };
    tags.insert(number.clone(), first);
    match second.filter(|second| !second.is_empty()) {
        Some(second) => tags.insert(total.clone(), second),
        None => tags.remove(total),
    };
}

/// Put a number pair's own digits back on the fields a read parsed it into
/// (#638): where the field holds the same number, it takes the file's spelling.
fn keep_spelled_number_pair(tags: &mut TagMap, number: &TagField, total: &TagField, text: &str) {
    let (first, second) = match text.split_once('/') {
        Some((first, second)) => (first, Some(second)),
        None => (text, None),
    };
    for (field, part) in [(number, Some(first)), (total, second)] {
        let Some(part) = part.map(str::trim) else {
            continue;
        };
        if let Some(value) = tags.get_mut(field) {
            if same_number(value, part) {
                *value = part.to_string();
            }
        }
    }
}

/// Write text tags into an MP3's ID3v2 tag, then confirm they actually landed.
///
/// A structurally broken container — most often an AIFF whose chunk chain is
/// severed after the audio by junk another encoder appended (Bandcamp's AIFF
/// export does this) — lets lofty read what it can but silently swallows the
/// write: `save_to_path` returns `Ok` while adding nothing readable, so an edit
/// would look applied and then vanish on the next read. This verifies the write
/// took; for an AIFF it first tries to repair the container and write again, and
/// only then gives up with an honest error rather than a false success.
fn write_id3v2(path: &Path, container: FileType, tags: &TagMap) -> Result<(), TagIoError> {
    // What the file read as before this write, so the legacy text blocks of a
    // WAV or an AIFF, an ID3v1 tag and an MP3's APE block lose only what the
    // model cleared (#652, #653, #655).
    let mut before = if matches!(container, FileType::Wav | FileType::Aiff)
        || has_id3v1(path)?
        || has_ape(path, container)?
    {
        TagEngine::read(path)?.tags
    } else {
        TagMap::new()
    };
    // A write that takes away the title, the artist and the album leaves an
    // ID3v2 tag the next read passes over for a legacy block that still names
    // the track (#615), so a field only that block stated would come back
    // (#656). Then every field a legacy block states counts as read, and so as
    // cleared where the model has none.
    if names_the_track(&before) && !names_the_track(tags) {
        for kind in [
            TagBlockKind::Id3v1,
            TagBlockKind::Ape,
            TagBlockKind::RiffInfo,
            TagBlockKind::AiffText,
        ] {
            if let Some(content) = TagEngine::read_block(path, kind)? {
                for (field, value) in content.tags {
                    before.entry(field).or_insert(value);
                }
            }
        }
    }
    save_id3v2(path, container, tags, &before)?;
    if id3v2_write_landed(path, container, tags)? {
        return sync_iff_text(path, container, tags, &before);
    }
    // The write was a silent no-op. Repair a broken AIFF once and retry.
    if container == FileType::Aiff && repair_aiff_container(path)? {
        save_id3v2(path, container, tags, &before)?;
        if id3v2_write_landed(path, container, tags)? {
            return sync_iff_text(path, container, tags, &before);
        }
    }
    Err(TagIoError::Malformed(format!(
        "could not write tags to {}: its container is malformed and could not be repaired",
        path.display()
    )))
}

/// Whether an ID3v2 write actually persisted. When the write set any non-empty
/// text field, the file must read back an ID3v2 block that holds something;
/// clearing everything has nothing to confirm. This is what catches a container
/// that accepts the write and keeps none of it.
fn id3v2_write_landed(path: &Path, container: FileType, tags: &TagMap) -> Result<bool, TagIoError> {
    let wrote_something = tags.values().any(|value| !value.trim().is_empty());
    if !wrote_something {
        return Ok(true);
    }
    let readback = read_id3v2(path, container, tag_only_options())?;
    Ok(readback.is_some_and(|tag| !tag.is_empty()))
}

fn save_id3v2(
    path: &Path,
    container: FileType,
    tags: &TagMap,
    before: &TagMap,
) -> Result<(), TagIoError> {
    let mut generic = Tag::new(TagType::Id3v2);
    for (field, value) in tags {
        push_field_items(&mut generic, field, value);
    }
    // `Id3v2Tag::from` joins repeated items of one key into a single
    // multi-value frame with ID3v2.4's own separator, so the several items
    // pushed above become one well-formed TPE1/TCON rather than duplicate
    // frames (#46).
    let mut updated = Id3v2Tag::from(generic);
    spell_id3v2_number_pairs(&mut updated, tags);

    // The custom fields ID3v2 has no frame of its own for go on as `TXXX` frames
    // named the way the file spells them; the ones it does have a frame for went
    // through the generic tag above, under that frame (#201).
    for (name, value) in raw_custom_fields(tags, TagType::Id3v2) {
        updated.insert(Frame::UserText(ExtendedTextFrame::new(
            TextEncoding::UTF8,
            name.to_string(),
            value.to_string(),
        )));
    }

    if let Some(original) = read_id3v2(path, container, tag_only_options())? {
        // Lyrics reach the model through the custom-field catch-all, under the
        // frame's own name — so the model holds the text and nothing else, while
        // `USLT` is identified by a language and a description it has no room
        // for. Written from the model alone the frame goes out as `XXX`,
        // "undefined", *beside* the one the file already had, and a file that
        // said `eng` ends up stating its lyrics twice, one of them stale (#264).
        // The two descriptors come off the file's own frame, so the value the
        // model carries lands in it instead of next to it.
        let owned_lyrics = original
            .unsync_text()
            .last()
            .map(|lyrics| (lyrics.language, lyrics.description.to_string()));
        if let Some((language, description)) = &owned_lyrics {
            // Here, before the carry loop, what the model wrote is still the
            // only lyrics frame in `updated` — after it, the file's other
            // languages are in here too and must keep their own descriptors.
            updated.retain_mut(|frame| {
                if let Frame::UnsynchronizedText(lyrics) = frame {
                    lyrics.language = *language;
                    lyrics.description = description.clone().into();
                }
                true
            });
        }
        // The comment (#644), the same way: the model holds its text, and the
        // file's own frame says which language and description it goes under.
        // Every other `COMM`, iTunes' gapless and Sound Check data among them,
        // is carried over below.
        let owned_comment = owned_comment_frame(&original)
            .map(|comment| (comment.language, comment.description.to_string()));
        if let Some((language, description)) = &owned_comment {
            updated.retain_mut(|frame| {
                if let Frame::Comment(comment) = frame {
                    comment.language = *language;
                    comment.description = description.clone().into();
                }
                true
            });
        }
        // A `TXXX` the backend knows by a name of its own (ReplayGain is the
        // usual one) is written under the backend's spelling of it, in capitals,
        // whatever case the file used (#645). It is the same frame either way,
        // so it goes back under the description the file gave it.
        let own_descriptions: Vec<String> = (&original)
            .into_iter()
            .filter_map(|frame| match frame {
                Frame::UserText(text) => Some(text.description.to_string()),
                _ => None,
            })
            .collect();
        updated.retain_mut(|frame| {
            if let Frame::UserText(text) = frame {
                let own = own_descriptions.iter().find(|own| {
                    own.eq_ignore_ascii_case(&text.description) && **own != text.description
                });
                if let Some(own) = own {
                    text.description = own.clone().into();
                }
            }
            true
        });
        // A text frame holding several values (#649): the read gives the field
        // the last of them, or all of them joined for a `TXXX` the backend has
        // no key for (#609), and the model's one value was written over all of
        // them. Where the model still holds what the read gave, nobody edited
        // the field, so the file's values go back.
        let several: Vec<(String, String, String)> = (&original)
            .into_iter()
            .filter_map(|frame| match frame {
                Frame::Text(text) if text.value.contains('\0') => Some((
                    frame.id().as_str().to_string(),
                    String::new(),
                    text.value.to_string(),
                )),
                Frame::UserText(text) if text.content.contains('\0') => Some((
                    frame.id().as_str().to_string(),
                    text.description.to_string(),
                    text.content.to_string(),
                )),
                _ => None,
            })
            .collect();
        let as_read = |own: &str, value: &str| {
            own.rsplit('\0').next() == Some(value)
                || own
                    .split('\0')
                    .collect::<Vec<_>>()
                    .join(&multi_value_separator())
                    == value
        };
        updated.retain_mut(|frame| {
            let id = frame.id().as_str().to_string();
            match frame {
                Frame::Text(text) => {
                    if let Some((_, _, own)) = several
                        .iter()
                        .find(|(own_id, _, own)| *own_id == id && as_read(own, &text.value))
                    {
                        text.value = own.clone().into();
                    }
                }
                Frame::UserText(text) => {
                    if let Some((_, _, own)) = several.iter().find(|(own_id, description, own)| {
                        *own_id == id
                            && *description == text.description
                            && as_read(own, &text.content)
                    }) {
                        text.content = own.clone().into();
                    }
                }
                _ => {}
            }
            true
        });
        // Dates (#504). The model holds every timestamp frame and has just
        // written each one from what it read — for a timestamp lofty cannot
        // parse whole, the trimmed date. Where the model still holds that date,
        // nobody edited it, so the file's own text goes back instead; where it
        // holds something else the edit stands, and where it holds nothing the
        // field was cleared and stays cleared.
        let verbatim: Vec<(String, String)> = (&original)
            .into_iter()
            .filter(|frame| {
                matches!(frame, Frame::Binary(_)) && is_timestamp_frame_id(frame.id().as_str())
            })
            .filter_map(|frame| {
                Some((
                    frame.id().as_str().to_string(),
                    timestamp_frame_value(frame)?,
                ))
            })
            .collect();
        for (id, text) in &verbatim {
            let trimmed = text
                .parse::<Timestamp>()
                .ok()
                .map(|stamp| stamp.to_string());
            let modelled = (&updated)
                .into_iter()
                .find(|frame| frame.id().as_str() == id)
                .and_then(timestamp_frame_value);
            if modelled.is_some() && modelled == trimmed {
                updated.retain(|frame| frame.id().as_str() != id);
                if let Some(frame) = verbatim_timestamp_frame(id, text) {
                    updated.insert(frame);
                }
            }
        }
        // The rating (#619). The read keeps the file's last `POPM` frame, as the
        // custom field `POPM`, so that frame is the model's: the carry loop below
        // put it back over the one just written from the model, and an edit or a
        // clear never reached the file. A rating nobody edited goes back as the
        // file's own frame instead, since its 0-255 value does not survive the
        // round trip through stars; frames for other emails are not the model's
        // and carry over as they are.
        let owned_rating = (&original).into_iter().rev().find_map(|frame| match frame {
            Frame::Popularimeter(rating) => Some(rating.clone()),
            _ => None,
        });
        let owned_email = owned_rating.and_then(|rating| {
            let mut alone = Id3v2Tag::new();
            alone.insert(Frame::Popularimeter(rating.clone()));
            let as_read = Tag::from(alone)
                .get_string(ItemKey::Popularimeter)
                .map(str::to_string);
            let field = item_key_to_tag_field(ItemKey::Popularimeter, TagType::Id3v2);
            if tags.get(&field) == as_read.as_ref() {
                updated.retain(|frame| !matches!(frame, Frame::Popularimeter(_)));
                None
            } else {
                Some(rating.email.to_string())
            }
        });
        // The edited frame has to stay the file's last one, or the next read
        // keeps another email's frame carried in after it.
        let edited_rating: Vec<Frame<'static>> = if owned_email.is_some() {
            let taken = (&updated)
                .into_iter()
                .filter(|frame| matches!(frame, Frame::Popularimeter(_)))
                .cloned()
                .collect();
            updated.retain(|frame| !matches!(frame, Frame::Popularimeter(_)));
            taken
        } else {
            Vec::new()
        };
        for frame in &original {
            let foreign_comment = matches!(frame, Frame::Comment(comment)
            if !owned_comment.as_ref().is_some_and(|(language, description)| {
                *language == comment.language && *description == comment.description
            }));
            if is_model_text_frame(frame) && !foreign_comment {
                continue;
            }
            // With no comment in the model, a `COMM` under a description of its
            // own is what the next read would take for the comment (#644), so
            // carrying it brought a cleared comment back (#657). Only the
            // machine ones stay.
            if let Frame::Comment(comment) = frame {
                let has_comment = tags
                    .get(&TagField::Comment)
                    .is_some_and(|value| !value.is_empty());
                if !has_comment && !is_machine_comment(&comment.description) {
                    continue;
                }
            }
            if let (Frame::Popularimeter(rating), Some(email)) = (frame, &owned_email) {
                if rating.email == email.as_str() {
                    continue;
                }
            }
            // Lyrics in any other language are lyrics the model never saw —
            // `unsync_text().last()` is the one the read kept, since a second
            // value for one field overwrites the first. Those carry over
            // untouched, like everything else the model has no room for; the
            // one it did read has just been written from the model.
            if let Frame::UnsynchronizedText(lyrics) = frame {
                let owned = owned_lyrics
                    .as_ref()
                    .is_some_and(|(language, description)| {
                        *language == lyrics.language && *description == lyrics.description
                    });
                if owned {
                    continue;
                }
            }
            // Carry the frame over with its per-frame unsynchronisation cleared
            // (#348). lofty does NOT re-apply unsynchronisation when it writes —
            // it keeps whatever flag the frame arrived with but emits plain,
            // synchronised data (its own source flags this as a footgun). A frame
            // the file already carried unsynchronised (a cover is the usual one)
            // would then go out flag-set but plain, and the next read rejects it
            // as `InvalidUnsynchronisation` — poisoning the whole file so lofty
            // can neither read nor rewrite it. Write it plainly instead, clearing
            // the data-length indicator that the unsynchronisation flag pairs with.
            let mut carried = frame.clone();
            let mut flags = carried.flags();
            flags.unsynchronisation = false;
            flags.data_length_indicator = None;
            carried.set_flags(flags);
            updated.insert(carried);
        }
        for frame in edited_rating {
            updated.insert(frame);
        }
        // [2026-11-03 14:10] A text frame nobody edited keeps the encoding the file wrote it in.
        keep_unedited_encodings(&mut updated, &original);
        order_frames(&mut updated, Some(&original));
    } else {
        order_frames(&mut updated, None);
    }
    save_id3v2_tag(path, container, &updated)?;
    // Whatever the file also carries in an ID3v1 block has to say the same
    // thing (#194).
    sync_id3v1(path, container, tags, before)?;
    // And so does an APE block beside it (#655).
    sync_ape(path, container, tags, before)?;
    Ok(())
}

/// Put a tag's frames in a stable order before it is written (#623).
///
/// The backend's conversion from a generic tag collects the frames in a hash
/// set with a random seed, so the same tags came out in a different order on
/// every save: the file's frames moved around on each edit and a save that
/// changed nothing still rewrote the tag. A frame the file already has (the
/// same frame by the backend's own reckoning, which is what replaces it on
/// insert) keeps its place in `original`; a new one goes after those, by its id
/// and then its description.
// [2026-11-03 14:10] Added: the original encoding of every unedited text frame.
/// Give every text, user-text and comment frame of `updated` whose value is the
/// one `original` holds the encoding `original` wrote it in.
///
/// Every frame is rebuilt from the model on a save, in the backend's own
/// encoding. A Latin-1 frame holding cp1251 bytes — what Russian and Ukrainian
/// files from older rippers carry, read here as `Êèíî` for `Кино` — came back as
/// that mojibake spelled in UTF-16, so an edit of the genre alone destroyed the
/// title for every player and tagger that read those bytes as cp1251. Written
/// again in Latin-1, the same value is the same bytes. An edited value has no
/// such frame to match and is written as before.
fn keep_unedited_encodings(updated: &mut Id3v2Tag, original: &Id3v2Tag) {
    updated.retain_mut(|frame| {
        let id = frame.id().as_str().to_string();
        match frame {
            Frame::Text(text) => {
                if let Some(encoding) = original.into_iter().find_map(|own| match own {
                    Frame::Text(own_text)
                        if own.id().as_str() == id && own_text.value == text.value =>
                    {
                        Some(own_text.encoding)
                    }
                    _ => None,
                }) {
                    text.encoding = encoding;
                }
            }
            Frame::UserText(text) => {
                if let Some(encoding) = original.into_iter().find_map(|own| match own {
                    Frame::UserText(own_text)
                        if own_text.description == text.description
                            && own_text.content == text.content =>
                    {
                        Some(own_text.encoding)
                    }
                    _ => None,
                }) {
                    text.encoding = encoding;
                }
            }
            Frame::Comment(comment) => {
                if let Some(encoding) = original.into_iter().find_map(|own| match own {
                    Frame::Comment(own_comment)
                        if own_comment.language == comment.language
                            && own_comment.description == comment.description
                            && own_comment.content == comment.content =>
                    {
                        Some(own_comment.encoding)
                    }
                    _ => None,
                }) {
                    comment.encoding = encoding;
                }
            }
            _ => {}
        }
        true
    });
}

fn order_frames(tag: &mut Id3v2Tag, original: Option<&Id3v2Tag>) {
    let place = |frame: &Frame<'static>| {
        let known = original.and_then(|original| original.into_iter().position(|old| old == frame));
        (
            known.unwrap_or(usize::MAX),
            frame.id_str().to_string(),
            frame_description(frame),
        )
    };
    let mut frames: Vec<Frame<'static>> = (&*tag).into_iter().cloned().collect();
    frames.sort_by_cached_key(place);
    tag.retain(|_| false);
    for frame in frames {
        tag.insert(frame);
    }
}

/// Put an `ilst`'s atoms in a stable order before it is written (#665), the
/// way [`order_frames`] does for ID3v2: the backend's conversion hands them
/// over in no fixed order, so every save moved them around and one that
/// changed nothing still rewrote the tag. An atom the file already had keeps
/// the file's place for it; a new one follows, ordered by its identifier.
fn order_atoms(ilst: &mut Ilst, original: Option<&Ilst>) {
    let place = |atom: &Atom<'static>| {
        let known = original.and_then(|original| {
            original
                .into_iter()
                .position(|old| old.ident() == atom.ident())
        });
        (known.unwrap_or(usize::MAX), format!("{:?}", atom.ident()))
    };
    let mut atoms: Vec<Atom<'static>> = (&*ilst).into_iter().cloned().collect();
    atoms.sort_by_cached_key(place);
    ilst.retain(|_| false);
    for atom in atoms {
        ilst.insert(atom);
    }
}

/// What tells two frames with one id apart, for [`order_frames`].
fn frame_description(frame: &Frame<'_>) -> String {
    match frame {
        Frame::UserText(text) => text.description.to_string(),
        Frame::UserUrl(url) => url.description.to_string(),
        Frame::Comment(comment) => format!("{:?}{}", comment.language, comment.description),
        Frame::UnsynchronizedText(lyrics) => {
            format!("{:?}{}", lyrics.language, lyrics.description)
        }
        Frame::Popularimeter(rating) => rating.email.to_string(),
        Frame::Picture(picture) => format!(
            "{:?}{}",
            picture.picture.pic_type(),
            picture.picture.description().unwrap_or_default()
        ),
        _ => String::new(),
    }
}

/// Persist an [`Id3v2Tag`] to a file, choosing the writer by container.
///
/// AIFF and WAV go through [`write_iff_id3`] rather than lofty's `save_to_path`.
/// The backend's AIFF writer leaves a run of zero-length chunks after the tag on
/// a rewrite, which strict decoders reject and which, on the *next* write, makes
/// it drop the embedded cover (#358); its WAV writer leaves the RIFF size short
/// of the file, and the next write then appends a fresh tag past what it takes
/// for junk, until the file ends in a chunk that claims gigabytes (#505).
/// Writing the `ID3 ` chunk into the container directly sidesteps both — the
/// audio is preserved byte for byte and the container is clean every time.
/// Every other ID3v2 container keeps lofty's container-aware save, which is
/// well-behaved for them.
fn save_id3v2_tag(path: &Path, container: FileType, tag: &Id3v2Tag) -> Result<(), TagIoError> {
    save_id3v2_tag_with(path, container, tag, id3_write_options())
}

/// [`save_id3v2_tag`] at an explicit revision — the restore of a journaled
/// block and a revision switch choose theirs (#205, #206); they go through the
/// same writers so that neither can undo what #358 and #505 fixed.
fn save_id3v2_tag_with(
    path: &Path,
    container: FileType,
    tag: &Id3v2Tag,
    options: WriteOptions,
) -> Result<(), TagIoError> {
    // What a 2.3 write would drop, put back into the block it produces (#667).
    // The options say which revision only through equality: no getter.
    let dropped = if options == { options }.use_id3v23(true) {
        frames_dropped_from_v23(tag, options)
    } else {
        Vec::new()
    };
    if matches!(container, FileType::Aiff | FileType::Wav) {
        let mut bytes = Vec::new();
        tag.dump_to(&mut bytes, options)?;
        if let Some(spliced) = splice_v23_frames(&bytes, &dropped) {
            bytes = spliced;
        }
        if write_iff_id3(path, Some(&bytes))? {
            return Ok(());
        }
    }
    tag.save_to_path(path, options)?;
    if !dropped.is_empty() && matches!(container, FileType::Mpeg | FileType::Aac) {
        let raw = std::fs::read(path)?;
        if raw.len() >= 10 && &raw[..3] == b"ID3" {
            let block_len = 10 + syncsafe_u32(&raw[6..10]) as usize;
            if let Some(mut spliced) = raw
                .get(..block_len)
                .and_then(|block| splice_v23_frames(block, &dropped))
            {
                spliced.extend_from_slice(&raw[block_len..]);
                rewrite_in_place(path, &raw, &spliced)?;
            }
        }
    }
    Ok(())
}

/// The frames the backend discards from an ID3v2.3 write as 2.4-only (#667).
/// The sort-order ones among them are a de-facto standard in 2.3 tags, and the
/// rest are worth keeping as much as any frame a file carries.
const FRAMES_A_V23_WRITE_DROPS: [&str; 14] = [
    "ASPI", "EQU2", "RVA2", "SEEK", "SIGN", "TDEN", "TDRL", "TDTG", "TMOO", "TPRO", "TSOA", "TSOP",
    "TSOT", "TSST",
];

/// Each frame of `tag` a 2.3 write would drop, as its id and its body encoded
/// for 2.3 by the backend's own frame encoders (#667). A frame of a kind none of
/// them covers is left to the drop, as before.
fn frames_dropped_from_v23(tag: &Id3v2Tag, options: WriteOptions) -> Vec<([u8; 4], Vec<u8>)> {
    tag.into_iter()
        .filter(|frame| FRAMES_A_V23_WRITE_DROPS.contains(&frame.id().as_str()))
        .filter_map(|frame| {
            let body = match frame {
                Frame::Text(text) => text.as_bytes(options).ok()?,
                Frame::Timestamp(timestamp) => timestamp.as_bytes(options).ok()?,
                Frame::RelativeVolumeAdjustment(volume) => volume.as_bytes(options).ok()?,
                Frame::Binary(binary) => binary.data.to_vec(),
                _ => return None,
            };
            let id = <[u8; 4]>::try_from(frame.id().as_str().as_bytes()).ok()?;
            Some((id, body))
        })
        .collect()
}

/// `block`, an ID3v2.3 block, with `frames` added after its last frame (#667).
/// `None`, leaving the block as it is, when there is nothing to add or the block
/// is not a plain 2.3 one this can walk: unsynchronised, with an extended
/// header, or with a frame running past its end.
fn splice_v23_frames(block: &[u8], frames: &[([u8; 4], Vec<u8>)]) -> Option<Vec<u8>> {
    if frames.is_empty()
        || block.len() < 10
        || &block[..3] != b"ID3"
        || block[3] != 3
        || block[5] & 0xC0 != 0
    {
        return None;
    }
    let size = syncsafe_u32(&block[6..10]) as usize;
    let end = (10 + size).min(block.len());
    let mut pos = 10;
    while pos + 10 <= end && block[pos] != 0 {
        let frame_size = u32::from_be_bytes(block[pos + 4..pos + 8].try_into().ok()?) as usize;
        pos += 10 + frame_size;
    }
    if pos > end {
        return None;
    }
    let mut added = Vec::new();
    for (id, body) in frames {
        added.extend_from_slice(id);
        added.extend_from_slice(&u32::try_from(body.len()).ok()?.to_be_bytes());
        added.extend_from_slice(&[0, 0]);
        added.extend_from_slice(body);
    }
    let new_size = u32::try_from(size + added.len())
        .ok()
        .filter(|size| *size < 1 << 28)?;
    let mut spliced = Vec::with_capacity(block.len() + added.len());
    spliced.extend_from_slice(&block[..6]);
    spliced.extend_from_slice(&syncsafe_bytes(new_size));
    spliced.extend_from_slice(&block[10..pos]);
    spliced.extend_from_slice(&added);
    spliced.extend_from_slice(&block[pos..]);
    Some(spliced)
}

/// Put `id3_bytes` into an AIFF or a WAV as its `ID3 ` chunk — or, given
/// `None`, take the chunk out — rebuilding the container by hand (#358, #505).
///
/// Every other chunk — `COMM`/`SSND` or `fmt `/`data` (the audio), `COMT`,
/// `LIST`, `JUNK` and the rest — is kept byte for byte; any existing ID3 chunk
/// (`ID3 ` or `id3 `) is replaced, and anything past the first unreadable chunk
/// (junk another encoder appended, #357, or a broken write left, #505) is
/// dropped. The fresh `ID3 ` chunk goes last, with no reserved padding. The
/// size in the header is recomputed, never trusted: a WAV whose RIFF size falls
/// short of the file is walked to its real end, so the chunks past the stated
/// size are kept and the stale size corrected.
///
/// Returns `false`, touching nothing, for a file that is not a FORM/AIFF or a
/// RIFF/WAVE — or one whose audio this walk cannot reach, so that a rebuild
/// could never lose it. Written in place, by [`rewrite_in_place`].
fn write_iff_id3(path: &Path, id3_bytes: Option<&[u8]>) -> Result<bool, TagIoError> {
    let raw = std::fs::read(path)?;
    if raw.len() < 12 {
        return Ok(false);
    }
    let (big_endian, audio_id): (bool, &[u8; 4]) = match (&raw[0..4], &raw[8..12]) {
        (b"FORM", _) => (true, b"SSND"),
        (b"RIFF", b"WAVE") => (false, b"data"),
        _ => return Ok(false),
    };
    let read_size = |bytes: [u8; 4]| {
        if big_endian {
            u32::from_be_bytes(bytes)
        } else {
            u32::from_le_bytes(bytes)
        }
    };
    let write_size = |size: u32| {
        if big_endian {
            size.to_be_bytes()
        } else {
            size.to_le_bytes()
        }
    };

    // Keep every valid chunk except an existing ID3 one; stop at the first
    // chunk that isn't well-formed, which is where a broken container's junk
    // begins.
    let mut body: Vec<u8> = Vec::new();
    let mut saw_audio = false;
    let mut pos = 12usize;
    while pos + 8 <= raw.len() {
        let id = &raw[pos..pos + 4];
        let size = read_size([raw[pos + 4], raw[pos + 5], raw[pos + 6], raw[pos + 7]]) as usize;
        let payload_end = pos + 8 + size;
        let id_ok = id.iter().all(|b| b.is_ascii_graphic() || *b == b' ');
        if !id_ok || payload_end > raw.len() {
            break;
        }
        if id == audio_id {
            saw_audio = true;
        }
        let chunk_end = (payload_end + (size & 1)).min(raw.len());
        if !id.eq_ignore_ascii_case(b"ID3 ") {
            body.extend_from_slice(&raw[pos..chunk_end]);
        }
        pos = chunk_end;
    }
    if !saw_audio {
        return Ok(false);
    }

    // The fresh ID3 chunk, word-aligned.
    if let Some(id3_bytes) = id3_bytes {
        body.extend_from_slice(b"ID3 ");
        body.extend_from_slice(&write_size(id3_bytes.len() as u32));
        body.extend_from_slice(id3_bytes);
        if id3_bytes.len() % 2 == 1 {
            body.push(0);
        }
    }

    let container_size = (4 + body.len()) as u32; // the form type plus the chunks
    let mut out = Vec::with_capacity(12 + body.len());
    out.extend_from_slice(&raw[0..4]);
    out.extend_from_slice(&write_size(container_size));
    out.extend_from_slice(&raw[8..12]);
    out.extend_from_slice(&body);

    rewrite_in_place(path, &raw, &out)?;
    Ok(true)
}

/// Turn the file at `path`, whose bytes are `old`, into `new` (#620): through a
/// handle on the file itself, so it keeps its mode, owner, hard links and
/// extended attributes, and a read-only file is refused as every other format's
/// write refuses it. A temp file renamed over the original lost all of that.
/// Only the 12-byte container header and what follows the first changed byte
/// are written; the `ID3 ` chunk sits after the audio, which then stays put.
fn rewrite_in_place(path: &Path, old: &[u8], new: &[u8]) -> Result<(), TagIoError> {
    use std::io::{Seek, SeekFrom, Write};
    let mut file = std::fs::OpenOptions::new().write(true).open(path)?;
    let header = new.len().min(12);
    let from = header
        + old
            .get(header..)
            .unwrap_or_default()
            .iter()
            .zip(&new[header..])
            .take_while(|(was, now)| was == now)
            .count();
    file.seek(SeekFrom::Start(from as u64))?;
    file.write_all(&new[from..])?;
    file.set_len(new.len() as u64)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&new[..header])?;
    file.flush()?;
    Ok(())
}

/// Rebuild a malformed AIFF whose chunk chain is severed after the audio.
///
/// The trigger is real: an AIFF export that appends its own metadata as junk
/// past the standard chunks (a bogus chunk with a length larger than the file,
/// a dangling ID3 tag) leaves a file every parser reads — the audio and the
/// leading chunks are intact — but none can rewrite, because the walk hits the
/// break. This keeps the leading chunks that parse cleanly (through `SSND`, the
/// audio) and drops everything from the break onward, then rewrites a clean
/// `FORM`. The audio and the standard metadata chunks are preserved byte for
/// byte; only the unreadable trailing junk is lost, which is exactly what a tag
/// write is about to replace.
///
/// Returns `true` when it rebuilt the file, `false` when there was nothing to
/// repair (a well-formed AIFF, or one this can't safely rewrite). Never touches
/// a file whose chunks all parse to its end, so a healthy AIFF is left alone.
/// Written in place, by [`rewrite_in_place`].
fn repair_aiff_container(path: &Path) -> Result<bool, TagIoError> {
    let bytes = std::fs::read(path)?;
    // A FORM/AIFF (or AIFF-C) header, or there is nothing here to repair.
    if bytes.len() < 12 || &bytes[0..4] != b"FORM" {
        return Ok(false);
    }
    let form_type = &bytes[8..12];
    if form_type != b"AIFF" && form_type != b"AIFC" {
        return Ok(false);
    }

    // Walk the chunks, keeping the valid leading run. A chunk is valid when its
    // four-byte id is printable and its payload fits inside the file; the first
    // one that isn't is the break.
    let mut pos = 12usize;
    let mut kept_end = 12usize;
    let mut saw_ssnd = false;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_be_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;
        let payload_end = pos + 8 + size;
        let id_ok = id.iter().all(|b| b.is_ascii_graphic() || *b == b' ');
        if !id_ok || payload_end > bytes.len() {
            break;
        }
        if id == b"SSND" {
            saw_ssnd = true;
        }
        // Chunks are word-aligned: an odd length carries a pad byte.
        kept_end = (payload_end + (size & 1)).min(bytes.len());
        pos = kept_end;
    }

    // Repair only a file that actually broke *after* its audio: no audio means
    // this isn't a file we can safely shorten, and a chain that reached the end
    // is well-formed and must be left untouched.
    if !saw_ssnd || kept_end >= bytes.len() {
        return Ok(false);
    }

    let body = &bytes[12..kept_end];
    let form_size = (4 + body.len()) as u32; // the form-type plus the kept chunks
    let mut rebuilt = Vec::with_capacity(12 + body.len());
    rebuilt.extend_from_slice(b"FORM");
    rebuilt.extend_from_slice(&form_size.to_be_bytes());
    rebuilt.extend_from_slice(form_type);
    rebuilt.extend_from_slice(body);

    rewrite_in_place(path, &bytes, &rebuilt)?;
    Ok(true)
}

/// The Vorbis comment that names what encoded the file. Provenance, and the one
/// the backend's own conversion moves into the vendor string — see
/// [`save_with_custom_items`].
const VORBIS_ENCODER: &str = "ENCODER";

/// Tags that describe how the file was MADE, not what it holds (#197): the
/// encoder and its settings (`TSSE`/`TENC`), and the two technical statements
/// about the audio itself (`TLEN` length in milliseconds, `TFLT` file type).
/// They are provenance, in the same class as the LAME header and the DJ cue
/// points the write pipeline already leaves alone — so the model neither offers
/// them for editing nor destroys them: they are skipped on read and carried
/// over on write, by both writers.
///
/// Stated twice because the two sides work on different things: the reader and
/// the generic writer hold lofty [`ItemKey`]s, the ID3v2 carry loop holds
/// concrete frames. The four ids are spelled the same in ID3v2.3 and 2.4.
///
/// `TSSE` reads back as `EncoderSoftware` on the 0.25 backend where it used to
/// be `EncoderSettings` — the same frame under a second name, so both belong
/// here. `ItemKey::FileType` is gone with that backend and `TFLT` no longer maps
/// to any key at all, which leaves it a frame the model never sees; the frame-id
/// half below is what keeps it (#201).
fn is_provenance_key(key: ItemKey) -> bool {
    matches!(
        key,
        ItemKey::EncoderSettings | ItemKey::EncoderSoftware | ItemKey::EncodedBy | ItemKey::Length
    )
}

fn is_provenance_frame_id(id: &str) -> bool {
    matches!(id, "TSSE" | "TENC" | "TLEN" | "TFLT")
}

/// Whether a frame is one the text-only [`TagMap`] already represents, and so
/// must come from the model rather than being carried over from the old tag.
fn is_model_text_frame(frame: &Frame<'_>) -> bool {
    // Not represented, and not to be dropped either (#197) — the carry loop is
    // what keeps these, since every other text frame the model doesn't hold is
    // deliberately left behind.
    if is_provenance_frame_id(frame.id().as_str()) {
        return false;
    }
    // TDRC (recording time) is a `Timestamp` frame, not `Text`, but the model
    // round-trips it as `Year`. Without excluding the original, a file left with
    // a stale/duplicate TDRC (e.g. two TDRC frames from another tagger) leaks the
    // old year back on write and overwrites the value we just set. The other
    // timestamp frames come back through the model too, as fields of their
    // own, so carrying them as well left two of each on every write (#504).
    matches!(
        frame,
        Frame::Text(_) | Frame::UserText(_) | Frame::Comment(_)
    ) || is_timestamp_frame_id(frame.id().as_str())
        || frame.id().as_str() == "WOAF"
}

/// The file's concrete ID3v2 tag, if it has one. Dispatches on the container so
/// it works for every ID3v2-carrying format (MP3, AAC, AIFF, WAV), reading the
/// concrete tag lofty's generic representation can't fully reproduce.
fn read_id3v2(
    path: &Path,
    container: FileType,
    options: ParseOptions,
) -> Result<Option<Id3v2Tag>, TagIoError> {
    let mut file = buffered(path)?;
    let mut tag = match Some(container) {
        // [2026-11-03 14:40] An MP3 whose tag holds a frame the backend cannot decode is read without it.
        Some(FileType::Mpeg) => read_mpeg_past_undecodable_frames(path, &mut file, options)?
            .id3v2()
            .cloned(),
        Some(FileType::Aac) => AacFile::read_from(&mut file, options)?.id3v2().cloned(),
        Some(FileType::Aiff) => AiffFile::read_from(&mut file, options)?.id3v2().cloned(),
        Some(FileType::Wav) => WavFile::read_from(&mut file, options)?.id3v2().cloned(),
        _ => None,
    };
    // Every write, undo snapshot and revision switch starts here, so this is
    // where a timestamp lofty would trim gets its text back (#504). Only a tag
    // that has timestamp frames pays for reading the block a second time.
    if let Some(tag) = tag.as_mut() {
        if (&*tag)
            .into_iter()
            .any(|frame| is_timestamp_frame_id(frame.id().as_str()))
        {
            if let Some(block) = raw_id3v2_block(path, container)? {
                keep_timestamp_text(tag, &block);
            }
        }
        // And the year a 2.3 tag states as a full date, which the parse loses
        // (#668), so a revision switch or a restore writes it back too.
        let tdrc = FrameId::Valid("TDRC".into());
        if tag.get(&tdrc).is_none() {
            if let Some(stamp) =
                lost_v23_year(path, container).and_then(|year| year.parse::<Timestamp>().ok())
            {
                tag.insert(Frame::Timestamp(TimestampFrame::new(
                    tdrc,
                    TextEncoding::Latin1,
                    stamp,
                )));
            }
        }
    }
    Ok(tag)
}

/// The year of an ID3v2.3 `TYER` frame that holds a full date, `2021-09-08`,
/// which the backend's parse drops (#668): it folds `TDAT`/`TIME` into the
/// year, and a year that already has a month is removed rather than kept. A
/// `TYER` that is no date at all is not recovered; written back as the year it
/// would be refused on every save, as a malformed year is.
///
/// Walks the frame headers and seeks past the bodies, so a tag with a large
/// cover costs a few reads; only a 2.3 tag without a year gets here.
fn lost_v23_year(path: &Path, container: FileType) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let start = match container {
        FileType::Mpeg | FileType::Aac => 0,
        FileType::Aiff | FileType::Wav => {
            let big_endian = container == FileType::Aiff;
            let mut pos = 12u64;
            let mut chunk = [0u8; 8];
            loop {
                file.seek(SeekFrom::Start(pos)).ok()?;
                file.read_exact(&mut chunk).ok()?;
                let size_bytes = [chunk[4], chunk[5], chunk[6], chunk[7]];
                let size = u64::from(if big_endian {
                    u32::from_be_bytes(size_bytes)
                } else {
                    u32::from_le_bytes(size_bytes)
                });
                if chunk[..4].eq_ignore_ascii_case(b"ID3 ") {
                    break pos + 8;
                }
                pos += 8 + size + (size & 1);
            }
        }
        _ => return None,
    };
    let mut header = [0u8; 10];
    file.seek(SeekFrom::Start(start)).ok()?;
    file.read_exact(&mut header).ok()?;
    if &header[..3] != b"ID3" || header[3] != 3 || header[5] & 0xC0 != 0 {
        return None;
    }
    let end = start + 10 + u64::from(syncsafe_u32(&header[6..10]));
    let mut pos = start + 10;
    let mut frame = [0u8; 10];
    while pos + 10 <= end {
        file.seek(SeekFrom::Start(pos)).ok()?;
        file.read_exact(&mut frame).ok()?;
        if frame[0] == 0 {
            return None;
        }
        let size = u64::from(u32::from_be_bytes([frame[4], frame[5], frame[6], frame[7]]));
        if &frame[..4] == b"TYER" {
            if frame[9] & 0xE0 != 0 || size > 64 {
                return None;
            }
            let mut body = vec![0u8; size as usize];
            file.read_exact(&mut body).ok()?;
            let text = timestamp_text(&body)?;
            let stamp = text.parse::<Timestamp>().ok()?;
            return (stamp.month.is_some() && is_writable_year(&text)).then_some(text);
        }
        pos += 10 + size;
    }
    None
}

/// The ID3v2 frames lofty reads as dates (#504).
const TIMESTAMP_FRAME_IDS: [&str; 5] = ["TDEN", "TDOR", "TDRC", "TDRL", "TDTG"];

fn is_timestamp_frame_id(id: &str) -> bool {
    TIMESTAMP_FRAME_IDS.contains(&id)
}

/// The file's ID3v2 block as it sits on disk, header included: at the start of
/// an MP3 or AAC stream, or the `ID3 ` chunk of an AIFF or a WAV. Read by
/// seeking, so the audio is never loaded.
fn raw_id3v2_block(path: &Path, container: FileType) -> Result<Option<Vec<u8>>, TagIoError> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    let mut header = [0u8; 12];
    match container {
        FileType::Mpeg | FileType::Aac => {
            if file.read_exact(&mut header[..10]).is_err() || &header[..3] != b"ID3" {
                return Ok(None);
            }
            let footer = if header[5] & 0x10 != 0 { 10 } else { 0 };
            let len = 10 + syncsafe_u32(&header[6..10]) as usize + footer;
            let mut block = vec![0u8; len];
            file.seek(SeekFrom::Start(0))?;
            if file.read_exact(&mut block).is_err() {
                return Ok(None);
            }
            Ok(Some(block))
        }
        FileType::Aiff | FileType::Wav => {
            let big_endian = container == FileType::Aiff;
            if file.read_exact(&mut header).is_err() {
                return Ok(None);
            }
            let mut pos = 12u64;
            let mut chunk = [0u8; 8];
            loop {
                file.seek(SeekFrom::Start(pos))?;
                if file.read_exact(&mut chunk).is_err() {
                    return Ok(None);
                }
                let size_bytes = [chunk[4], chunk[5], chunk[6], chunk[7]];
                let size = if big_endian {
                    u32::from_be_bytes(size_bytes)
                } else {
                    u32::from_le_bytes(size_bytes)
                } as u64;
                if chunk[..4].eq_ignore_ascii_case(b"ID3 ") {
                    let mut block = vec![0u8; size as usize];
                    if file.read_exact(&mut block).is_err() {
                        return Ok(None);
                    }
                    return Ok(Some(block));
                }
                pos += 8 + size + (size & 1);
            }
        }
        _ => Ok(None),
    }
}

/// Refuse to write a file whose tags run to its very end — a FLAC with no
/// audio frame after its metadata, an MP3 or AAC that is nothing but an ID3v2
/// block (#416). The tag library's writer cuts such a file to the wrong
/// length when its tags get shorter (lofty-rs#731), destroying what is left
/// of it. A real track always has audio after its tags, so this costs one
/// nothing; the only shape that could be damaged gets an honest error instead,
/// before anything is touched.
fn ensure_audio_after_tags(path: &Path) -> Result<(), TagIoError> {
    if tags_reach_eof(path)? {
        return Err(TagIoError::Malformed(format!(
            "{} holds tags but no audio after them; writing to it could cut it short",
            path.display()
        )));
    }
    Ok(())
}

/// Refuse to write a file whose APE tag the tag library mistakes for an ID3v1
/// tag (#614). It takes `TAG` at 128 bytes from the end as an ID3v1 tag without
/// checking, so an APE tag that ends the file with those bytes at that offset
/// (a 131-byte one always does: the `TAG` of its own header lands there) is
/// read as ID3v1 — and the writer then writes over the APE tag, destroying
/// every item in it. An APE footer in the last 32 bytes says the file ends in
/// an APE tag, so there is no ID3v1 tag after it.
fn ensure_no_misread_id3v1(path: &Path) -> Result<(), TagIoError> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    if file.metadata()?.len() < 128 {
        return Ok(());
    }
    let mut tail = [0u8; 128];
    file.seek(SeekFrom::End(-128))?;
    file.read_exact(&mut tail)?;
    if &tail[..3] == b"TAG" && &tail[96..104] == b"APETAGEX" {
        return Err(TagIoError::Malformed(format!(
            "{} ends in an APE tag the tag library misreads as an ID3v1 tag; writing to it would destroy the APE tag",
            path.display()
        )));
    }
    Ok(())
}

/// Whether the file ends where its tags do: an ID3v2 block at the start that
/// runs to the end, or a FLAC's metadata blocks that do. Every other shape —
/// a container with its own structure, a file with audio after its tags —
/// answers `false`.
fn tags_reach_eof(path: &Path) -> Result<bool, TagIoError> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let mut pos = 0u64;
    // An ID3v2 block first: where MP3 and AAC keep their tag, and one a FLAC
    // can carry ahead of its `fLaC` marker.
    let mut head = [0u8; 10];
    if file.read_exact(&mut head).is_ok() && &head[..3] == b"ID3" {
        let footer = if head[5] & 0x10 != 0 { 10 } else { 0 };
        pos = 10 + u64::from(syncsafe_u32(&head[6..10])) + footer;
        if pos >= len {
            return Ok(true);
        }
    }
    file.seek(SeekFrom::Start(pos))?;
    let mut magic = [0u8; 4];
    if file.read_exact(&mut magic).is_err() || &magic != b"fLaC" {
        return Ok(false);
    }
    pos += 4;
    loop {
        let mut header = [0u8; 4];
        file.seek(SeekFrom::Start(pos))?;
        if file.read_exact(&mut header).is_err() {
            return Ok(true);
        }
        pos += 4 + u64::from(u32::from_be_bytes([0, header[1], header[2], header[3]]));
        if header[0] & 0x80 != 0 {
            return Ok(pos >= len);
        }
    }
}

fn syncsafe_u32(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .fold(0, |acc, byte| (acc << 7) | u32::from(byte & 0x7F))
}

fn syncsafe_bytes(value: u32) -> [u8; 4] {
    [
        ((value >> 21) & 0x7F) as u8,
        ((value >> 14) & 0x7F) as u8,
        ((value >> 7) & 0x7F) as u8,
        (value & 0x7F) as u8,
    ]
}

/// The timestamp frames of a raw ID3v2 block whose text lofty would not write
/// back as it found it (#504). lofty parses `TDEN`/`TDRC`/… into a date, and a
/// timestamp written with a space where ID3v2.4 wants a `T` —
/// `2010-05-27 14:43:18`, as ffmpeg writes it — parses as `2010-05-27`, so a
/// plain re-save drops the time. A frame whose text survives the round trip is
/// not listed; nor is anything this walk cannot read safely — a tag or frame
/// that is unsynchronised, compressed or encrypted, a non-ASCII value.
fn lossy_timestamps(block: &[u8]) -> Vec<(String, String)> {
    let mut lossy = Vec::new();
    if block.len() < 10 || &block[..3] != b"ID3" {
        return lossy;
    }
    let version = block[3];
    let flags = block[5];
    if !(version == 3 || version == 4) || flags & 0x80 != 0 {
        return lossy;
    }
    let end = (10 + syncsafe_u32(&block[6..10]) as usize).min(block.len());
    let mut pos = 10;
    if flags & 0x40 != 0 && block.len() >= 14 {
        pos += if version == 4 {
            syncsafe_u32(&block[10..14]) as usize
        } else {
            4 + u32::from_be_bytes([block[10], block[11], block[12], block[13]]) as usize
        };
    }
    while pos + 10 <= end && block[pos] != 0 {
        let id = &block[pos..pos + 4];
        let size = if version == 4 {
            syncsafe_u32(&block[pos + 4..pos + 8])
        } else {
            u32::from_be_bytes([
                block[pos + 4],
                block[pos + 5],
                block[pos + 6],
                block[pos + 7],
            ])
        } as usize;
        let format_flags = block[pos + 9];
        let start = pos + 10;
        pos = start + size;
        if pos > end {
            break;
        }
        let encoded = if version == 4 {
            format_flags & 0x0F != 0
        } else {
            format_flags & 0xE0 != 0
        };
        let Ok(id) = std::str::from_utf8(id) else {
            continue;
        };
        if encoded || !is_timestamp_frame_id(id) || size < 2 {
            continue;
        }
        let Some(text) = timestamp_text(&block[start..pos]) else {
            continue;
        };
        let reread = text
            .parse::<Timestamp>()
            .ok()
            .map(|stamp| stamp.to_string());
        if reread.as_deref() != Some(text.as_str()) {
            lossy.push((id.to_string(), text));
        }
    }
    lossy
}

/// The text of a timestamp frame's content (an encoding byte, then the text),
/// without its terminator. `None` unless it is plain ASCII — which every
/// timestamp is — in any of the four ID3v2 encodings.
fn timestamp_text(content: &[u8]) -> Option<String> {
    let (encoding, data) = content.split_first()?;
    let text = match encoding {
        0 | 3 => String::from_utf8(data.to_vec()).ok()?,
        1 | 2 => {
            let mut units: Vec<u16> = data
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| {
                    if *encoding == 2 {
                        u16::from_be_bytes(*pair)
                    } else {
                        u16::from_le_bytes(*pair)
                    }
                })
                .collect();
            if *encoding == 1 {
                match units.first() {
                    Some(0xFEFF) => {
                        units.remove(0);
                    }
                    Some(0xFFFE) => {
                        units.remove(0);
                        for unit in &mut units {
                            *unit = unit.swap_bytes();
                        }
                    }
                    _ => {}
                }
            }
            String::from_utf16(&units).ok()?
        }
        _ => return None,
    };
    let text = text.trim_end_matches('\0');
    (text.is_ascii() && !text.is_empty()).then(|| text.to_string())
}

/// Swap each timestamp frame lofty trimmed for one that carries the text as it
/// was on disk (#504). The frame is kept verbatim — lofty writes a binary frame
/// as it gets it — in ISO-8859-1, which holds any timestamp and is valid in both
/// ID3v2.3 and 2.4.
fn keep_timestamp_text(tag: &mut Id3v2Tag, block: &[u8]) {
    let lossy = lossy_timestamps(block);
    if lossy.is_empty() {
        return;
    }
    tag.retain(|frame| !lossy.iter().any(|(id, _)| frame.id().as_str() == id));
    for (id, text) in lossy {
        if let Some(frame) = verbatim_timestamp_frame(&id, &text) {
            tag.insert(frame);
        }
    }
}

fn verbatim_timestamp_frame(id: &str, text: &str) -> Option<Frame<'static>> {
    let id = FrameId::new(id.to_string()).ok()?;
    let mut data = vec![0u8];
    data.extend_from_slice(text.as_bytes());
    Some(Frame::Binary(BinaryFrame::new(id, data)))
}

/// What a timestamp frame says, whichever form it is in: lofty's date, a text
/// frame, or the verbatim frame [`keep_timestamp_text`] puts in place (#504).
fn timestamp_frame_value(frame: &Frame<'_>) -> Option<String> {
    match frame {
        Frame::Timestamp(stamp) => Some(stamp.timestamp.to_string()),
        Frame::Text(text) => Some(text.value.to_string()),
        Frame::Binary(binary) => timestamp_text(&binary.data),
        _ => None,
    }
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
    // A single-value field the file holds several values of reads as the last
    // of them, and writing that one back cut the rest (#649). Where the model
    // still holds what the read gave, nobody edited the field, so the file's
    // values, under whichever of the field's keys they are, stay as they are.
    let as_read = tag_to_map(&tag);
    let untouched: std::collections::BTreeSet<TagField> = file
        .tags
        .iter()
        .filter(|(field, value)| {
            !field.is_multi_value()
                && as_read.get(*field) == Some(*value)
                && tag
                    .items()
                    .filter(|item| item_key_to_tag_field(item.key(), tag_type) == **field)
                    .nth(1)
                    .is_some()
        })
        .map(|(field, _)| field.clone())
        .collect();

    let desired: HashSet<ItemKey> = file
        .tags
        .keys()
        .filter_map(|field| item_key_for(field, tag_type))
        .collect();
    tag.retain(|item| {
        item.value().text().is_none()
            || desired.contains(&item.key())
            // Provenance the model deliberately doesn't carry (#197) — without
            // this the retain reads "not in the model" as "delete".
            || is_provenance_key(item.key())
            || untouched.contains(&item_key_to_tag_field(item.key(), tag_type))
    });

    for (field, value) in &file.tags {
        // Clear first rather than append, so repeated writes can't accumulate
        // duplicate entries for the same field — then push what the model says,
        // which for a multi-value field is more than one item (#46).
        if let Some(key) = item_key_for(field, tag_type) {
            if untouched.contains(field) {
                continue;
            }
            tag.remove_key(key);
            push_field_items(&mut tag, field, value);
        }
    }
    save_with_custom_items(&file.path, file.format.to_lofty(), tag, &file.tags)
}

/// Save a generic tag, putting the custom fields it cannot carry onto the
/// concrete tag on the way out (#201).
///
/// The 0.22 backend let a format-specific item ride along in the generic tag as
/// `ItemKey::Unknown`, so one save wrote everything. It no longer does, and — for
/// Vorbis Comments and APE — the conversion to the generic tag drops those items
/// outright, which would make a plain generic save *delete* every custom field
/// the file had. So the file's own concrete tag is the starting point: the model
/// supplies the text, the concrete tag keeps everything the model has no room
/// for, and the stale custom items are replaced by what the model now says.
fn save_with_custom_items(
    path: &Path,
    container: FileType,
    mut tag: Tag,
    tags: &TagMap,
) -> Result<(), TagIoError> {
    let tag_type = tag.tag_type();
    match tag_type {
        TagType::VorbisComments => {
            let existing = read_vorbis(path, container, tag_only_options())?.unwrap_or_default();
            // The conversion below folds an `ENCODER` comment into the vendor
            // string and drops the comment. That moves what made the file from
            // one place in it to another, which is exactly what #197 says not to
            // do — so both are put back the way the file had them.
            let vendor = existing.vendor().to_string();
            let encoders: Vec<String> = existing
                .get_all(VORBIS_ENCODER)
                .map(str::to_string)
                .collect();
            let own: Vec<(String, String)> = existing
                .items()
                .filter(|(name, _)| !maps_to_item_key(tag_type, name))
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect();
            // Several fields have more than one name here, and the backend
            // writes each under one of them whichever the file used, so a save
            // turned the standard `ORGANIZATION` (or `LABEL`) into `PUBLISHER`
            // (#646), `ALBUM ARTIST` into `ALBUMARTIST` and `TOTALTRACKS` into
            // `TRACKTOTAL` (#647). Each goes back under the name its value was
            // read from.
            let own_names: Vec<(TagField, String)> = existing
                .items()
                .filter_map(|(name, _)| Some((vorbis_field_of(name)?, name.to_string())))
                .collect();
            let own_fields: Vec<(TagField, String, String)> = existing
                .items()
                .filter_map(|(name, value)| {
                    Some((vorbis_field_of(name)?, name.to_string(), value.to_string()))
                })
                .collect();
            let (remainder, _) = existing.split_tag();
            // Carried by hand, because the conversion below measures every image
            // and silently drops one it cannot read — where a plain save wrote it
            // out with a zeroed description instead. An image the app was asked to
            // keep is kept whether or not the backend can make sense of its bytes.
            let pictures: Vec<Picture> = tag.pictures().to_vec();
            while !tag.pictures().is_empty() {
                tag.remove_picture(0);
            }
            let mut merged = remainder.merge_tag(tag);
            rename_to_own_names(&mut merged, &own_names);
            keep_unedited_fields(&mut merged, &own_fields);
            merged.set_vendor(vendor);
            merged.remove(VORBIS_ENCODER).for_each(drop);
            for encoder in encoders {
                merged.push(VORBIS_ENCODER.to_string(), encoder);
            }
            for picture in pictures {
                let information = PictureInformation::from_picture(&picture).unwrap_or_default();
                merged.insert_picture(picture, Some(information)).ok();
            }
            let stale: Vec<String> = merged
                .items()
                .map(|(name, _)| name.to_string())
                .filter(|name| !maps_to_item_key(tag_type, name))
                .collect();
            for name in stale {
                merged.remove(&name).for_each(drop);
            }
            for (name, value) in raw_custom_fields(tags, tag_type) {
                let own_values = own
                    .iter()
                    .filter(|(own_name, _)| own_name.eq_ignore_ascii_case(name))
                    .map(|(_, own_value)| own_value.clone())
                    .collect();
                for value in custom_values_to_write(own_values, value) {
                    merged.push(name.to_string(), value);
                }
            }
            merged.save_to_path(path, write_options())?;
        }
        TagType::Ape => {
            let (remainder, _) = read_ape(path, container, tag_only_options())?
                .unwrap_or_default()
                .split_tag();
            let mut merged = remainder.merge_tag(tag);
            spell_ape_number_pairs(&mut merged, tags);
            let stale: Vec<String> = (&merged)
                .into_iter()
                .filter(|item| {
                    // Only the text items are the model's to replace; a binary
                    // one (APE keeps cover art in one) is left where it is.
                    !maps_to_item_key(tag_type, item.key()) && item.value().text().is_some()
                })
                .map(|item| item.key().to_string())
                .collect();
            for name in &stale {
                merged.remove(name);
            }
            for (name, value) in raw_custom_fields(tags, tag_type) {
                if let Ok(item) = ApeItem::new(name.to_string(), ItemValue::Text(value.to_string()))
                {
                    merged.insert(item);
                }
            }
            merged.save_to_path(path, write_options())?;
        }
        TagType::Mp4Ilst => {
            // The MP4 conversion *does* carry the file's own atoms across, so
            // here the stale ones have to be taken back off before the model's
            // go on.
            let mut merged = Ilst::from(tag);
            let own: Vec<Atom<'static>> = (&merged)
                .into_iter()
                .filter(|atom| !freeform_items(atom).is_empty())
                .cloned()
                .collect();
            merged.retain(|atom| freeform_items(atom).is_empty());
            for (name, value) in raw_custom_fields(tags, tag_type) {
                let own_atoms: Vec<&Atom<'static>> = own
                    .iter()
                    .filter(|atom| {
                        matches!(atom.ident(), AtomIdent::Freeform { name: own_name, .. } if own_name == name)
                    })
                    .collect();
                let own_values = own_atoms
                    .iter()
                    .flat_map(|atom| freeform_items(atom))
                    .map(|(_, own_value)| own_value)
                    .collect();
                // An untouched field goes back as the very atoms it came from;
                // a changed one keeps the namespace the file filed it under
                // (#609), which only a field new to the file lacks.
                if custom_values_to_write(own_values, value).len() > 1 {
                    for atom in own_atoms {
                        merged.insert(atom.clone());
                    }
                    continue;
                }
                let mean = match own_atoms.first().map(|atom| atom.ident()) {
                    Some(AtomIdent::Freeform { mean, .. }) => mean.to_string(),
                    _ => FREEFORM_MEAN.to_string(),
                };
                merged.insert(Atom::new(
                    AtomIdent::Freeform {
                        mean: mean.into(),
                        name: name.to_string().into(),
                    },
                    AtomData::UTF8(value.to_string()),
                ));
            }
            // The tempo is the model's to state, once and as an integer
            // (#610). The conversion above writes it as text and leaves the
            // file's own integer atom beside it; both go.
            match tags.get(&TagField::Bpm) {
                Some(bpm) => {
                    // `retain`, not `remove`: the backend's remove swaps each
                    // match to the front, moving the first atom — one value of
                    // a multi-value field — out of its place (#618).
                    merged.retain(|atom| atom.ident() != &MP4_TEMPO);
                    merged.insert(mp4_tempo_atom(bpm));
                }
                // With no BPM in the model a tempo the file states goes, but
                // an atom stating none (#611) was never the model's to drop.
                None => merged.retain(|atom| {
                    atom.ident() != &MP4_TEMPO || atom_integer_tempo(atom).is_none()
                }),
            }
            let original = Mp4File::read_from(&mut buffered(path)?, tag_only_options())?
                .ilst()
                .cloned();
            order_atoms(&mut merged, original.as_ref());
            merged.save_to_path(path, write_options())?;
        }
        _ => tag.save_to_path(path, write_options())?,
    }
    Ok(())
}

/// The model field a Vorbis comment name is read into, if the backend knows the
/// name.
fn vorbis_field_of(name: &str) -> Option<TagField> {
    ItemKey::from_key(TagType::VorbisComments, name)
        .map(|key| item_key_to_tag_field(key, TagType::VorbisComments))
}

/// Move each field `vorbis` states under a name the file did not use for it
/// to the name the file did (#646, #647). `own_names` is the file's comments
/// in order, by field; where it has several names for one field, the last is
/// the one the read kept. A field the file already states under the written
/// name, or does not state at all, is left as written.
fn rename_to_own_names(vorbis: &mut VorbisComments, own_names: &[(TagField, String)]) {
    let mut written: Vec<String> = vorbis.items().map(|(name, _)| name.to_string()).collect();
    written.dedup();
    for name in written {
        let Some(field) = vorbis_field_of(&name) else {
            continue;
        };
        let own: Vec<&String> = own_names
            .iter()
            .filter(|(own_field, _)| *own_field == field)
            .map(|(_, own_name)| own_name)
            .collect();
        if own
            .iter()
            .any(|own_name| own_name.eq_ignore_ascii_case(&name))
        {
            continue;
        }
        let Some(own_name) = own.last() else {
            continue;
        };
        let values: Vec<String> = vorbis.remove(&name).collect();
        for value in values {
            vorbis.push(own_name.to_string(), value);
        }
    }
}

/// Put back, as the file had them, the comments of every field `vorbis` states
/// with exactly the values the file did, in the same order (#650). Nobody
/// edited such a field, yet the backend writes it under one upper-case name:
/// `title` came back as `TITLE`, and a field the file stated under two names
/// (`ORGANIZATION` and `LABEL`, `TRACKTOTAL` and `totaltracks`) lost one of
/// them. `own_fields` is the file's comments in order, by field.
fn keep_unedited_fields(vorbis: &mut VorbisComments, own_fields: &[(TagField, String, String)]) {
    let items: Vec<(String, String)> = vorbis
        .items()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    let fields: std::collections::BTreeSet<&TagField> =
        own_fields.iter().map(|(field, _, _)| field).collect();
    let mut unedited: Vec<(&TagField, Vec<(&str, &str)>)> = Vec::new();
    for field in fields {
        let own: Vec<(&str, &str)> = own_fields
            .iter()
            .filter(|(own_field, _, _)| own_field == field)
            .map(|(_, name, value)| (name.as_str(), value.as_str()))
            .collect();
        let mut written_values: Vec<&str> = items
            .iter()
            .filter(|(name, _)| vorbis_field_of(name).as_ref() == Some(field))
            .map(|(_, value)| value.as_str())
            .collect();
        // In order where the file used one name for the field. The backend
        // groups a field's values by name when it reads several names, so
        // across names only the values themselves can be compared.
        let mut own_values: Vec<&str> = own.iter().map(|(_, value)| *value).collect();
        if own.iter().any(|(name, _)| *name != own[0].0) {
            written_values.sort_unstable();
            own_values.sort_unstable();
        }
        if written_values == own_values {
            unedited.push((field, own));
        }
    }

    // The file's comments go where the field's first written one is, and
    // every other comment keeps its place.
    let mut kept: Vec<(String, String)> = Vec::with_capacity(items.len());
    let mut placed: Vec<&TagField> = Vec::new();
    for (name, value) in &items {
        let field = vorbis_field_of(name);
        let own = field.as_ref().and_then(|field| {
            unedited
                .iter()
                .find(|(unedited_field, _)| *unedited_field == field)
        });
        match own {
            Some((field, own)) => {
                if !placed.contains(field) {
                    placed.push(field);
                    kept.extend(
                        own.iter()
                            .map(|(name, value)| (name.to_string(), value.to_string())),
                    );
                }
            }
            None => kept.push((name.clone(), value.clone())),
        }
    }
    if kept == items {
        return;
    }
    // Emptied by name and refilled in order: the backend's removal moves the
    // last comment into the gap, which would reorder another field's values.
    let mut names: Vec<&String> = items.iter().map(|(name, _)| name).collect();
    names.dedup();
    for name in names {
        vorbis.remove(name).for_each(drop);
    }
    for (name, value) in kept {
        vorbis.push(name, value);
    }
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
/// half keeps the checked call from quietly deciding what the tag type can hold
/// — the two known holes are pinned by a test instead.
///
/// A field the tag type has no key for writes nothing here: that is a custom
/// field spelled in the file's own terms, and it goes onto the concrete tag in
/// [`save_with_custom_items`] (#201).
fn push_field_items(tag: &mut Tag, field: &TagField, value: &str) {
    let Some(key) = item_key_for(field, tag.tag_type()) else {
        return;
    };
    if !field.is_multi_value() {
        tag.push_unchecked(TagItem::new(key, item_value_for(field, value)));
        return;
    }
    for part in split_multi_value(value) {
        tag.push_unchecked(TagItem::new(key, item_value_for(field, &part)));
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

/// The field a format-specific item name belongs to, and whether it got there
/// through a legacy spelling rather than the standard one (#171) — which decides
/// who wins when a file carries both.
fn field_for_custom_name(name: &str) -> (TagField, bool) {
    match legacy_alias_field(name) {
        Some(field) => (field, true),
        None => (TagField::Custom(name.to_string()), false),
    }
}

/// Put one value the read found into the map.
///
/// A file really can carry several artists or genres, as separate Vorbis
/// comments or as one multi-value ID3v2.4 frame — the backend surfaces both as
/// repeated items. Joining them is what keeps the extras (#46); overwriting, as
/// this once did, kept only the last and any edit then wrote that one value back
/// over all of them. Fields that are not multi-valued keep the last-wins
/// behaviour.
fn insert_read_value(tags: &mut TagMap, field: TagField, from_alias: bool, value: String) {
    // A legacy spelling never overrides the standard frame (#171). Both can be
    // in one file — another tagger's `TXXX:Label` next to the `TPUB` an import
    // just wrote — and the standard one is the current value.
    if from_alias && tags.contains_key(&field) {
        return;
    }
    match tags.entry(field) {
        std::collections::btree_map::Entry::Vacant(slot) => {
            slot.insert(value);
        }
        std::collections::btree_map::Entry::Occupied(mut slot) => {
            // ID3v2.3 states a genre as a numeric reference refined by its
            // name, `(17)Rock`, and the backend reads that as the genre twice
            // (#660). One already read is not read again.
            let repeated = *slot.key() == TagField::Genre
                && slot
                    .get()
                    .split(multi_value_separator().as_str())
                    .any(|genre| genre.eq_ignore_ascii_case(&value));
            if repeated {
                return;
            }
            if slot.key().is_multi_value() {
                let joined = format!("{}{}{}", slot.get(), multi_value_separator(), value);
                slot.insert(joined);
            } else {
                slot.insert(value);
            }
        }
    }
}

/// The release country's field name: the portable spelling, which is what the
/// import, the mask and the import settings know it by (#612).
const RELEASE_COUNTRY: &str = "RELEASECOUNTRY";

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
///
/// `RELEASECOUNTRY` is here too (#612). Where it is the format's own name the
/// generic tag carries it and it never reaches this table; where it arrives by
/// name it is what an older version wrote into ID3v2 or MP4, and the standard
/// item beside it is the current value.
fn legacy_alias_field(name: &str) -> Option<TagField> {
    match name.trim().to_ascii_uppercase().as_str() {
        "LABEL" | "ORGANIZATION" => Some(TagField::Publisher),
        "COUNTRY" | RELEASE_COUNTRY => Some(TagField::Custom(RELEASE_COUNTRY.to_string())),
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
/// A custom field is the one case with no answer here: its name is the file's
/// own, and it has a generic key only if the backend happens to know that name
/// for this tag type. When it doesn't, the field is written onto the concrete
/// tag instead — see [`raw_custom_fields`].
fn item_key_for(field: &TagField, tag_type: TagType) -> Option<ItemKey> {
    match field {
        // Written by name instead, onto the concrete tag (#613).
        field if field_name_on_tag(field, tag_type).is_some() => None,
        TagField::Bpm if tag_type == TagType::VorbisComments => Some(ItemKey::Bpm),
        TagField::Publisher if matches!(tag_type, TagType::Mp4Ilst | TagType::Ape) => {
            Some(ItemKey::Label)
        }
        TagField::Year if tag_type == TagType::Ape => Some(ItemKey::Year),
        TagField::Custom(name) => item_key_for_custom_name(tag_type, name),
        other => tag_field_to_item_key(other),
    }
}

fn tag_field_to_item_key(field: &TagField) -> Option<ItemKey> {
    Some(match field {
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
        // Named by the file, not by the model — resolved against the tag type in
        // [`item_key_for`], which is what the write path actually calls.
        TagField::Custom(_) => return None,
    })
}

/// A key the model has no field of its own for (e.g. `Mood`, `ReplayGainTrackGain`
/// — the backend recognizes far more keys than the twenty modeled here) becomes a
/// custom field named the way the file spells it for this tag type: the same name
/// [`read_custom_items`] would have produced, so the two halves of a read agree and
/// the next write puts the value back where it was.
///
/// Only a key with no per-format spelling at all falls back to its Rust `Debug`
/// name; before #201 every one of them did, and a `REPLAYGAIN_TRACK_GAIN` frame
/// came back as `ReplayGainTrackGain` and was written to a frame of that name.
fn item_key_to_tag_field(key: ItemKey, tag_type: TagType) -> TagField {
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
        // Under its portable name whatever the format calls it (#612), or an
        // ID3v2 file showed it as a field named after its `TXXX` description.
        ItemKey::ReleaseCountry => TagField::Custom(RELEASE_COUNTRY.to_string()),
        other => TagField::Custom(
            custom_name_for_item_key(other, tag_type)
                .map(str::to_string)
                .unwrap_or_else(|| format!("{other:?}")),
        ),
    }
}

/// Whether a rating value is one the backend can write (#654): empty, which
/// clears it, or `email|stars|plays` with 1 to 5 stars. Asked of the backend
/// itself, by converting the value into an ID3v2 frame, so the answer is its
/// own rules and not a copy of them.
fn is_writable_rating(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    let mut generic = Tag::new(TagType::Id3v2);
    generic.insert_text(ItemKey::Popularimeter, value.to_string());
    Id3v2Tag::from(generic)
        .into_iter()
        .any(|frame| matches!(frame, Frame::Popularimeter(_)))
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
/// - **track / disc** are a plain non-negative integer or a vinyl position
///   such as `A1` (#641), which every format but MP4 keeps as text; see
///   [`is_writable_value_at`] for the file-aware rule.
/// - **track-total / disc-total** are a plain non-negative integer.
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
        TagField::TrackNumber | TagField::DiscNumber => {
            is_ascii_digits(value) || is_vinyl_position(value)
        }
        TagField::TrackTotal | TagField::DiscTotal => is_ascii_digits(value),
        TagField::Bpm => is_decimal(value),
        _ => true,
    }
}

/// [`is_writable_value`] for the file at `path`. An MP4 stores the track and
/// disc numbers as integers, so a vinyl position, which the other formats keep
/// as text, has nowhere to go there and would be dropped (#641).
pub fn is_writable_value_at(path: &Path, field: &TagField, value: &str) -> bool {
    let integers_only = matches!(field, TagField::TrackNumber | TagField::DiscNumber)
        && FileType::from_path(path) == Some(FileType::Mp4);
    let unstorable_name = match field {
        TagField::Custom(name) => !value.is_empty() && !is_storable_custom_name_at(path, name),
        _ => false,
    };
    is_writable_value(field, value)
        && !(integers_only && is_vinyl_position(value))
        && !unstorable_name
}

/// Whether a custom field called `name` can be stored in the tag the file at
/// `path` is written to. The backend skips a name the tag type forbids without
/// an error, so the value would look applied and then be gone on the next read:
/// - every tag needs a name at all;
/// - Vorbis Comments (FLAC, Ogg, Opus, Speex) allow ASCII 0x20..=0x7D, `=` excluded;
/// - APE (Monkey's Audio, Musepack, WavPack) allows 2..=255 bytes of ASCII
///   0x20..=0x7E and reserves `ID3`, `TAG`, `OggS` and `MP+`.
///
/// ID3v2 (`TXXX`) and MP4 freeform atoms take any non-empty name. A file whose
/// type the extension does not tell is given the benefit of the doubt.
pub fn is_storable_custom_name_at(path: &Path, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    match FileType::from_path(path) {
        Some(FileType::Flac | FileType::Vorbis | FileType::Opus | FileType::Speex) => name
            .bytes()
            .all(|byte| (0x20..=0x7D).contains(&byte) && byte != b'='),
        Some(FileType::Ape | FileType::Mpc | FileType::WavPack) => {
            const RESERVED: [&str; 4] = ["ID3", "TAG", "OggS", "MP+"];
            (2..=255).contains(&name.len())
                && name.bytes().all(|byte| (0x20..=0x7E).contains(&byte))
                && !RESERVED.iter().any(|key| key.eq_ignore_ascii_case(name))
        }
        _ => true,
    }
}

/// A vinyl (or cassette) position as a track or disc number (#641): one or two
/// letters with up to three digits after them (`A`, `B2`, `Aa1`), or digits
/// with one letter after them (`12a`). Plain digits are a number, not this.
pub fn is_vinyl_position(value: &str) -> bool {
    let letters = value.bytes().take_while(u8::is_ascii_alphabetic).count();
    let digits = value[letters..]
        .bytes()
        .take_while(u8::is_ascii_digit)
        .count();
    let rest = &value[letters + digits..];
    if letters > 0 {
        return letters <= 2 && digits <= 3 && rest.is_empty();
    }
    let digits = value.bytes().take_while(u8::is_ascii_digit).count();
    (1..=3).contains(&digits)
        && value.len() == digits + 1
        && value.as_bytes()[digits].is_ascii_alphabetic()
}

#[derive(Debug, Error)]
pub enum TagIoError {
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("malformed tag data: {0}")]
    Malformed(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    // The backend split its one error type in two (#201): reading a file and
    // writing one now fail with different types.
    #[error("tag backend error: {0}")]
    Backend(#[from] FileParseError),
    #[error("tag backend error: {0}")]
    BackendWrite(#[from] FileEncodingError),
    // Encoding a tag to bytes rather than to a file — the journaled ID3v2
    // snapshot (#206) is the only thing that does this.
    #[error("tag encoding error: {0}")]
    Encoding(#[from] TagEncodingError),
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
            TagField::Url,
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
                // Either the generic tag can carry it, or it is written onto
                // the concrete tag by name (#613) — the URL was missing from
                // this list, and so was its hole.
                let mapped = item_key_for(&field, tag_type)
                    .and_then(|key| key.map_key(tag_type))
                    .is_some()
                    || field_name_on_tag(&field, tag_type).is_some();
                if !mapped {
                    holes.push(format!("{field:?} on {tag_type:?}"));
                }
            }
        }
        // APE's BPM and key were holes until they were written by name
        // (#663); nothing may join this list without being noticed.
        assert!(holes.is_empty(), "silently unwritable: {holes:?}");
    }

    #[test]
    fn the_tempo_is_written_under_the_key_each_tag_type_can_hold() {
        // ID3v2 and MP4 keep an integer BPM (`TBPM` / `tmpo`); Vorbis spells it
        // plain `BPM`, which is a different `ItemKey` — using the first there
        // was #165.
        assert_eq!(
            item_key_for(&TagField::Bpm, TagType::Id3v2),
            Some(ItemKey::IntegerBpm)
        );
        assert_eq!(
            item_key_for(&TagField::Bpm, TagType::Mp4Ilst),
            Some(ItemKey::IntegerBpm)
        );
        assert_eq!(
            item_key_for(&TagField::Bpm, TagType::VorbisComments),
            Some(ItemKey::Bpm)
        );
        // Both spellings read back as the same field, so the value round-trips
        // whichever one the file carries.
        assert_eq!(
            item_key_to_tag_field(ItemKey::Bpm, TagType::VorbisComments),
            TagField::Bpm
        );
        assert_eq!(
            item_key_to_tag_field(ItemKey::IntegerBpm, TagType::Id3v2),
            TagField::Bpm
        );
        // Every other field on Vorbis is untouched by the tag type, bar the URL,
        // which is written by name (#613).
        for field in known_fields()
            .into_iter()
            .filter(|f| !matches!(f, TagField::Bpm | TagField::Url))
        {
            assert_eq!(
                item_key_for(&field, TagType::VorbisComments),
                tag_field_to_item_key(&field)
            );
        }
    }

    #[test]
    fn legacy_user_text_names_fold_into_the_field_they_mean() {
        let alias = field_for_custom_name;
        assert_eq!(alias("Label"), (TagField::Publisher, true));
        assert_eq!(alias("ORGANIZATION"), (TagField::Publisher, true));
        assert_eq!(alias("OriginalMediaType"), (TagField::MediaType, true));
        assert_eq!(alias("media"), (TagField::MediaType, true));
        assert_eq!(
            alias("COUNTRY"),
            (TagField::Custom("RELEASECOUNTRY".to_string()), true)
        );
        // Arriving by name, the field's own spelling is what an older version
        // wrote where the format has a standard item for it (#612).
        assert_eq!(
            alias("RELEASECOUNTRY"),
            (TagField::Custom("RELEASECOUNTRY".to_string()), true)
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
        // A standard key is untouched by any of this — it never reaches the
        // alias table, because the generic tag carries it as an item of its own.
        assert_eq!(
            item_key_to_tag_field(ItemKey::Publisher, TagType::Id3v2),
            TagField::Publisher
        );
    }

    /// #612: the release country goes through the backend's own key on every
    /// format that has one, so each spells it its standard way, and reads back
    /// under the one field name whatever the file called it.
    #[test]
    fn the_release_country_is_one_field_under_each_formats_name() {
        let field = TagField::Custom("RELEASECOUNTRY".to_string());
        for (tag_type, standard) in [
            (TagType::Id3v2, "MusicBrainz Album Release Country"),
            (
                TagType::Mp4Ilst,
                "----:com.apple.iTunes:MusicBrainz Album Release Country",
            ),
            (TagType::VorbisComments, "RELEASECOUNTRY"),
            (TagType::Ape, "RELEASECOUNTRY"),
            (TagType::RiffInfo, "ICNT"),
        ] {
            let key = item_key_for(&field, tag_type);
            assert_eq!(key, Some(ItemKey::ReleaseCountry), "{tag_type:?}");
            assert_eq!(ItemKey::ReleaseCountry.map_key(tag_type), Some(standard));
            assert_eq!(
                item_key_to_tag_field(ItemKey::ReleaseCountry, tag_type),
                field
            );
        }
        // Any spelling of the name is the same field.
        assert_eq!(
            item_key_for(
                &TagField::Custom("ReleaseCountry".to_string()),
                TagType::Id3v2
            ),
            Some(ItemKey::ReleaseCountry)
        );
    }

    #[test]
    fn the_label_and_the_year_use_the_spelling_the_tag_type_knows() {
        // `Publisher` has no MP4 mapping, so an M4A silently lost its label
        // (#165); `Label` maps there and reads back as the same field.
        assert_eq!(
            item_key_for(&TagField::Publisher, TagType::Mp4Ilst),
            Some(ItemKey::Label)
        );
        assert_eq!(
            item_key_to_tag_field(ItemKey::Label, TagType::Mp4Ilst),
            TagField::Publisher
        );
        // Where `Publisher` does map, it is left alone — changing the Vorbis
        // spelling would move the value to a different comment name.
        assert_eq!(
            item_key_for(&TagField::Publisher, TagType::VorbisComments),
            Some(ItemKey::Publisher)
        );
        assert_eq!(
            item_key_for(&TagField::Publisher, TagType::Id3v2),
            Some(ItemKey::Publisher)
        );
        // APE spells the year plainly and has no recording date.
        assert_eq!(
            item_key_for(&TagField::Year, TagType::Ape),
            Some(ItemKey::Year)
        );
        assert_eq!(
            item_key_for(&TagField::Year, TagType::Id3v2),
            Some(ItemKey::RecordingDate)
        );
    }

    #[test]
    fn known_fields_round_trip_through_item_key() {
        for field in known_fields() {
            let key = tag_field_to_item_key(&field).expect("a known field has a key");
            assert_eq!(item_key_to_tag_field(key, TagType::Id3v2), field);
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
    fn a_track_or_disc_number_can_be_a_vinyl_position() {
        for position in ["A", "B2", "Aa1", "AA", "C123", "12a", "1B"] {
            assert!(
                is_writable_value(&TagField::TrackNumber, position),
                "{position}"
            );
            assert!(
                is_writable_value(&TagField::DiscNumber, position),
                "{position}"
            );
            // A total is a count.
            assert!(
                !is_writable_value(&TagField::TrackTotal, position),
                "{position}"
            );
            assert!(
                !is_writable_value(&TagField::DiscTotal, position),
                "{position}"
            );
        }
        for not_one in [
            "one", "Bonus", "A1/04", "A 1", "A1b", "A1234", "1234a", "12ab", "-1",
        ] {
            assert!(
                !is_writable_value(&TagField::TrackNumber, not_one),
                "{not_one}"
            );
        }
        // MP4 keeps the numbers as integers, so only there it is refused.
        let field = TagField::TrackNumber;
        assert!(is_writable_value_at(Path::new("a.mp3"), &field, "A1"));
        assert!(is_writable_value_at(Path::new("a.flac"), &field, "A1"));
        assert!(!is_writable_value_at(Path::new("a.m4a"), &field, "A1"));
        assert!(is_writable_value_at(Path::new("a.m4a"), &field, "7"));
        assert!(!is_writable_value_at(Path::new("a.mp3"), &field, "one"));
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

    /// A custom field is named by the file, and the two halves of a read have to
    /// agree on that name or a write puts the value somewhere else (#201).
    ///
    /// A name the backend has no key for is the app's to carry verbatim, onto
    /// the concrete tag. A name it *does* know is carried by the generic tag —
    /// and must come back under the same spelling, which is what the fallback in
    /// [`item_key_to_tag_field`] is for.
    #[test]
    fn a_custom_field_keeps_the_name_the_file_spells_it_with() {
        // Nothing in the backend's maps is called this, on any tag type.
        let mine = TagField::Custom("TAGREX_CUSTOM_TEST".to_string());
        assert_eq!(item_key_for(&mine, TagType::Id3v2), None);
        assert_eq!(item_key_for(&mine, TagType::VorbisComments), None);
        assert_eq!(field_for_custom_name("TAGREX_CUSTOM_TEST").0, mine);

        // One it does know goes through the generic tag, and reads back under
        // the name that tag type spells it with rather than a Rust variant name.
        let gain = ItemKey::ReplayGainTrackGain;
        assert_eq!(
            item_key_to_tag_field(gain, TagType::VorbisComments),
            TagField::Custom("REPLAYGAIN_TRACK_GAIN".to_string())
        );
        assert_eq!(
            item_key_for(
                &TagField::Custom("REPLAYGAIN_TRACK_GAIN".to_string()),
                TagType::VorbisComments
            ),
            Some(gain)
        );
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

    /// Build a self-contained MP3 in memory: an ID3v2.4 tag dumped by lofty
    /// followed by the silent MPEG frames [`TagEngine::restore_id3v2_bytes`]
    /// uses, so the backend recognizes the bytes as an MP3 and hands the tag
    /// back.
    fn mp3_with_tag(tag: &Id3v2Tag) -> Vec<u8> {
        let mut bytes = Vec::new();
        tag.dump_to(&mut bytes, WriteOptions::default())
            .expect("dump the tag");
        let mut silence = vec![0xFF, 0xFB, 0x90, 0x00];
        silence.resize(SILENT_MPEG_FRAME_LEN, 0);
        for _ in 0..SILENT_MPEG_FRAMES {
            bytes.extend_from_slice(&silence);
        }
        bytes
    }

    /// The unsynchronisation flag of the file's cover frame, or `None` when it
    /// has no cover. Also the readability oracle: a poisoned file fails to parse
    /// as an MP3 here, so a `Some` at all means the whole file still reads.
    fn cover_unsync_flag(path: &Path) -> Option<bool> {
        let tag = read_id3v2(path, FileType::Mpeg, tag_only_options())
            .expect("the file must still parse as an MP3")?;
        tag.into_iter().find_map(|frame| match frame {
            Frame::Picture(_) => Some(frame.flags().unsynchronisation),
            _ => None,
        })
    }

    /// #348: a frame carried over on write must lose ID3v2's per-frame
    /// unsynchronisation flag. lofty reads unsynchronised frame data but never
    /// re-applies the unsynchronisation when it writes (its own source flags
    /// this as a footgun) — so a frame left flagged goes out flag-set but plain,
    /// and the next read rejects the whole file as `InvalidUnsynchronisation`. A
    /// "Clear tags" poisoned three of a user's files exactly this way, through
    /// the cover frame each carried over.
    #[test]
    fn a_write_clears_a_carried_frames_unsynchronisation_flag() {
        use lofty::id3::v2::{AttachedPictureFrame, FrameFlags};

        // A cover whose bytes contain no 0xFF: for such data the plain and the
        // unsynchronised encodings are byte-identical, so a frame flagged
        // unsynchronised is genuinely valid and lofty reads it back — the
        // precondition the bug needs. (A real JPEG is full of 0xFF, which is why
        // the wild files turned unreadable the moment the flag was written
        // without the matching stuffing.)
        let cover = vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x01, 0x02, 0x03,
        ];
        let picture = Picture::unchecked(cover.clone())
            .pic_type(PictureType::CoverFront)
            .mime_type(MimeType::Png)
            .build();
        let mut apic = Frame::Picture(AttachedPictureFrame::new(TextEncoding::UTF8, picture));
        apic.set_flags(FrameFlags {
            unsynchronisation: true,
            ..FrameFlags::default()
        });

        let mut tag = Id3v2Tag::new();
        tag.set_title("Before".to_string());
        tag.insert(apic);

        let path = std::env::temp_dir().join(format!(
            "tagrex-unsync-carry-{}-{}.mp3",
            std::process::id(),
            SILENT_MPEG_FRAMES // any stable suffix; the pid already makes it unique
        ));
        std::fs::write(&path, mp3_with_tag(&tag)).expect("write the fixture");

        // The file reads, and its cover frame arrives unsynchronised — the exact
        // state a write must not carry through unchanged.
        assert_eq!(cover_unsync_flag(&path), Some(true), "fixture precondition");

        // A perfectly ordinary edit: change the title, nothing about the cover.
        let mut tags = TagMap::new();
        tags.insert(TagField::Title, "After".to_string());
        write_id3v2(&path, FileType::Mpeg, &tags).expect("the write must succeed");

        // Before the fix this read failed outright (InvalidUnsynchronisation);
        // now the file still parses and the cover frame is written plainly.
        assert_eq!(
            cover_unsync_flag(&path),
            Some(false),
            "the carried cover frame must be written with unsynchronisation cleared"
        );

        // And the write did what it was asked, keeping the cover intact.
        let saved = read_id3v2(&path, FileType::Mpeg, tag_only_options())
            .expect("parse")
            .expect("tag present");
        assert_eq!(saved.title().as_deref(), Some("After"));
        let saved_cover = saved.into_iter().find_map(|frame| match frame {
            Frame::Picture(p) => Some(p.picture.data().to_vec()),
            _ => None,
        });
        assert_eq!(saved_cover.as_deref(), Some(cover.as_slice()));

        std::fs::remove_file(&path).ok();
    }

    /// An MP3 whose ID3v2.4 block is written by hand, frame by frame, so it can
    /// hold what lofty would never write itself (#504). Each frame is a
    /// latin-1 text frame: an encoding byte, the text, a terminator.
    fn mp3_with_raw_frames(name: &str, frames: &[(&str, &str)]) -> PathBuf {
        let syncsafe = |n: usize| {
            [
                ((n >> 21) & 0x7F) as u8,
                ((n >> 14) & 0x7F) as u8,
                ((n >> 7) & 0x7F) as u8,
                (n & 0x7F) as u8,
            ]
        };
        let mut body = Vec::new();
        for (id, text) in frames {
            let mut content = vec![0u8];
            content.extend_from_slice(text.as_bytes());
            content.push(0);
            body.extend_from_slice(id.as_bytes());
            body.extend_from_slice(&syncsafe(content.len()));
            body.extend_from_slice(&[0, 0]);
            body.extend_from_slice(&content);
        }
        let mut bytes = b"ID3\x04\x00\x00".to_vec();
        bytes.extend_from_slice(&syncsafe(body.len()));
        bytes.extend(body);
        let mut silence = vec![0xFF, 0xFB, 0x90, 0x00];
        silence.resize(SILENT_MPEG_FRAME_LEN, 0);
        for _ in 0..SILENT_MPEG_FRAMES {
            bytes.extend_from_slice(&silence);
        }
        let path = std::env::temp_dir().join(format!(
            "tagrex-timestamps-{name}-{}.mp3",
            std::process::id()
        ));
        std::fs::write(&path, bytes).expect("write the fixture");
        path
    }

    /// Every timestamp frame in the file's ID3v2 block as `(id, text)`, read
    /// from the bytes on disk rather than through lofty, which would trim them.
    fn timestamps_on_disk(path: &Path) -> Vec<(String, String)> {
        let block = raw_id3v2_block(path, FileType::Mpeg)
            .expect("read the block")
            .expect("an ID3v2 block");
        let version = block[3];
        let end = 10 + syncsafe_u32(&block[6..10]) as usize;
        let mut found = Vec::new();
        let mut pos = 10;
        while pos + 10 <= end && block[pos] != 0 {
            let id = String::from_utf8_lossy(&block[pos..pos + 4]).to_string();
            let size = if version == 4 {
                syncsafe_u32(&block[pos + 4..pos + 8])
            } else {
                u32::from_be_bytes([
                    block[pos + 4],
                    block[pos + 5],
                    block[pos + 6],
                    block[pos + 7],
                ])
            } as usize;
            if is_timestamp_frame_id(&id) {
                let text = timestamp_text(&block[pos + 10..pos + 10 + size]).unwrap_or_default();
                found.push((id, text));
            }
            pos += 10 + size;
        }
        found.sort();
        found
    }

    const FFMPEG_STYLE_FRAMES: [(&str, &str); 5] = [
        ("TIT2", "Before"),
        // A space where ID3v2.4 wants a `T`, as ffmpeg writes it: lofty reads
        // it as the date alone.
        ("TDEN", "2010-05-27 14:43:18"),
        ("TDOR", "1996-03-01"),
        ("TDRC", "1996"),
        ("TDTG", "2011-02-03T04:05:06"),
    ];

    fn expected(changes: &[(&str, Option<&str>)]) -> Vec<(String, String)> {
        let mut want: Vec<(String, String)> = FFMPEG_STYLE_FRAMES[1..]
            .iter()
            .map(|(id, text)| (id.to_string(), text.to_string()))
            .collect();
        for (id, text) in changes {
            want.retain(|(have, _)| have != id);
            if let Some(text) = text {
                want.push((id.to_string(), text.to_string()));
            }
        }
        want.sort();
        want
    }

    /// #504: editing an unrelated field rewrote every timestamp lofty could
    /// not parse whole as its date alone — and wrote it twice, once from the
    /// model and once carried over. Each timestamp now comes back once, as the
    /// file had it.
    #[test]
    fn an_edit_leaves_every_untouched_timestamp_as_it_was() {
        let path = mp3_with_raw_frames("edit", &FFMPEG_STYLE_FRAMES);
        let mut file = TagEngine::read(&path).expect("read");
        file.tags.insert(TagField::Title, "After".to_string());
        TagEngine::write(&file).expect("write");

        assert_eq!(timestamps_on_disk(&path), expected(&[]));
        assert_eq!(
            TagEngine::read(&path)
                .expect("reread")
                .tags
                .get(&TagField::Title)
                .map(String::as_str),
            Some("After")
        );
        std::fs::remove_file(&path).ok();
    }

    /// #504: a timestamp the user did change is written as they typed it, and
    /// one they cleared stays gone — the file's own text only stands in for a
    /// value nobody touched.
    #[test]
    fn an_edited_or_cleared_timestamp_is_not_overridden_by_the_old_text() {
        let path = mp3_with_raw_frames("change", &FFMPEG_STYLE_FRAMES);
        let mut file = TagEngine::read(&path).expect("read");
        file.tags.insert(
            TagField::Custom("TDEN".to_string()),
            "2011-01-01".to_string(),
        );
        TagEngine::write(&file).expect("write");
        assert_eq!(
            timestamps_on_disk(&path),
            expected(&[("TDEN", Some("2011-01-01"))])
        );
        std::fs::remove_file(&path).ok();

        let path = mp3_with_raw_frames("clear", &FFMPEG_STYLE_FRAMES);
        let mut file = TagEngine::read(&path).expect("read");
        file.tags.remove(&TagField::Custom("TDEN".to_string()));
        TagEngine::write(&file).expect("write");
        assert_eq!(timestamps_on_disk(&path), expected(&[("TDEN", None)]));
        std::fs::remove_file(&path).ok();
    }

    /// #504: undoing the removal of an ID3v2 block puts its timestamps back as
    /// they were, through the snapshot taken before the removal and the restore
    /// that parses it again — both of which trimmed them.
    #[test]
    fn a_restored_id3v2_block_keeps_its_timestamps_text() {
        let path = mp3_with_raw_frames("restore", &FFMPEG_STYLE_FRAMES);
        let bytes = TagEngine::dump_id3v2(&path)
            .expect("dump")
            .expect("a block");
        TagEngine::remove_block(&path, TagBlockKind::Id3v2).expect("remove");
        TagEngine::restore_id3v2_bytes(&path, &bytes, &[], Some(Id3v2Revision::V4))
            .expect("restore");
        assert_eq!(timestamps_on_disk(&path), expected(&[]));
        std::fs::remove_file(&path).ok();
    }

    /// Build a RIFF/WAVE from `(chunk-id, payload)` pairs; `riff_size` overrides
    /// the size in the header, for a file whose header lies about it (#505).
    fn wav_bytes(chunks: &[(&[u8; 4], &[u8])], riff_size: Option<u32>) -> Vec<u8> {
        let mut body = Vec::new();
        for (id, payload) in chunks {
            body.extend_from_slice(*id);
            body.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            body.extend_from_slice(payload);
            if payload.len() % 2 == 1 {
                body.push(0); // word-align
            }
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&riff_size.unwrap_or((4 + body.len()) as u32).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(&body);
        out
    }

    /// A playable mono 16-bit PCM WAV: the `fmt ` chunk lofty needs to read the
    /// file, and a short run of silence as its audio.
    const PCM_FMT: [u8; 16] = [1, 0, 1, 0, 0x40, 0x1F, 0, 0, 0x80, 0x3E, 0, 0, 2, 0, 16, 0];

    fn riff_size_matches(bytes: &[u8]) -> bool {
        u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize == bytes.len() - 8
    }

    /// #506: an MP3 whose audio starts more than 1024 bytes past its tag — an
    /// empty, padded ID3v2.3 block followed by a run of zeros, as an encoder
    /// left five files in a real library — read fine but refused every write:
    /// the backend checks the format again before writing, with a narrower junk
    /// window than the reader allows. It now writes, and the audio, junk and all,
    /// is untouched.
    #[test]
    fn an_mp3_with_junk_before_its_audio_can_be_written() {
        let mut bytes = b"ID3\x03\x00\x00\x00\x00\x00\x64".to_vec(); // 100 bytes of padding
        bytes.resize(10 + 100 + 1100, 0); // and 1100 zeros before the audio
        let audio_at = bytes.len();
        let mut silence = vec![0xFF, 0xFB, 0x90, 0x00];
        silence.resize(SILENT_MPEG_FRAME_LEN, 0);
        for _ in 0..SILENT_MPEG_FRAMES {
            bytes.extend_from_slice(&silence);
        }
        let audio = bytes[audio_at..].to_vec();
        let path = std::env::temp_dir().join(format!("tagrex-junk-{}.mp3", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();

        let mut file = TagEngine::read(&path).expect("read");
        file.tags.insert(TagField::Title, "Written".to_string());
        TagEngine::write(&file).expect("the write must succeed");

        let reread = TagEngine::read(&path).expect("reread");
        assert_eq!(
            reread.tags.get(&TagField::Title).map(String::as_str),
            Some("Written")
        );
        let out = std::fs::read(&path).unwrap();
        assert!(out.ends_with(&audio), "the audio is untouched");
        std::fs::remove_file(&path).ok();
    }

    /// #416: a file that is tags and nothing else — no audio frame after a
    /// FLAC's metadata, an MP3 that is only its ID3v2 block — is refused before
    /// anything is written, since the tag library's writer can cut exactly that
    /// shape to the wrong length. The file is left byte for byte as it was.
    #[test]
    fn a_file_with_no_audio_after_its_tags_is_refused_untouched() {
        let mut flac = b"fLaC\x80\x00\x00\x22".to_vec(); // STREAMINFO, last block
        flac.extend_from_slice(&[
            0x10, 0x00, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0a, 0xc4, 0x42, 0xf0,
        ]);
        flac.resize(8 + 34, 0);
        let mut mp3 = b"ID3\x04\x00\x00\x00\x00\x00\x0b".to_vec();
        mp3.extend_from_slice(b"TIT2\x00\x00\x00\x01\x00\x00\x00"); // one empty title
        for (name, bytes) in [("flac", flac), ("mp3", mp3)] {
            let path =
                std::env::temp_dir().join(format!("tagrex-no-audio-{}.{name}", std::process::id()));
            std::fs::write(&path, &bytes).unwrap();
            assert!(
                tags_reach_eof(&path).unwrap(),
                "{name}: tags run to the end"
            );
            let mut tags = TagMap::new();
            tags.insert(TagField::Title, "Anything".to_string());
            let written = TagEngine::write(&TrackFile {
                path: path.clone(),
                format: if name == "flac" {
                    AudioFormat::Flac
                } else {
                    AudioFormat::Mp3
                },
                tags,
            });
            assert!(
                matches!(written, Err(TagIoError::Malformed(_))),
                "{name}: {written:?}"
            );
            assert!(TagEngine::remove_block(&path, TagBlockKind::Id3v2).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes, "{name}: untouched");
            std::fs::remove_file(&path).ok();
        }
        // A file with audio after its tags answers no, and is written as usual.
        let path = mp3_with_raw_frames("with-audio", &[("TIT2", "Title")]);
        assert!(!tags_reach_eof(&path).unwrap());
        std::fs::remove_file(&path).ok();
    }

    /// #505: the WAV chunk writer keeps the audio and every other chunk, swaps
    /// the ID3 chunk whatever case its id is in, corrects a RIFF size that falls
    /// short of the file, and drops the junk a broken write left at the end.
    #[test]
    fn write_iff_id3_repairs_a_wav_and_replaces_its_tag() {
        let mut bytes = wav_bytes(
            &[
                (b"fmt ", &PCM_FMT),
                (b"data", &[0; 8]),
                (b"LIST", b"INFOtags"),
                (b"id3 ", b"old-tag"),
            ],
            Some(20),
        );
        bytes.extend_from_slice(b"\xbe\xf0\x39\x77");
        bytes.extend_from_slice(&u32::MAX.to_le_bytes()); // a chunk running off the end

        let path = std::env::temp_dir().join(format!("tagrex-wav-id3-{}.wav", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();
        assert!(write_iff_id3(&path, Some(b"NEW-ID3-BYTES")).unwrap());
        let out = std::fs::read(&path).unwrap();
        assert!(riff_size_matches(&out));
        let expected = wav_bytes(
            &[
                (b"fmt ", &PCM_FMT),
                (b"data", &[0; 8]),
                (b"LIST", b"INFOtags"),
                (b"ID3 ", b"NEW-ID3-BYTES"),
            ],
            None,
        );
        assert_eq!(out, expected);

        // Taking the tag out leaves the rest exactly as it was.
        assert!(write_iff_id3(&path, None).unwrap());
        let out = std::fs::read(&path).unwrap();
        let expected = wav_bytes(
            &[
                (b"fmt ", &PCM_FMT),
                (b"data", &[0; 8]),
                (b"LIST", b"INFOtags"),
            ],
            None,
        );
        assert_eq!(out, expected);
        std::fs::remove_file(&path).ok();
    }

    /// #505: a WAV whose audio chunk this walk cannot reach — a streamed file
    /// with an open-ended `data` size — is left untouched rather than rebuilt
    /// without its audio.
    #[test]
    fn write_iff_id3_leaves_a_wav_alone_when_it_cannot_reach_the_audio() {
        let mut bytes = wav_bytes(&[(b"fmt ", &PCM_FMT)], None);
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(&[0; 32]);
        let path =
            std::env::temp_dir().join(format!("tagrex-wav-stream-{}.wav", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();
        assert!(!write_iff_id3(&path, Some(b"TAG")).unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        std::fs::remove_file(&path).ok();
    }

    /// #505: the whole path, as a user meets it — repeated edits, the undo of a
    /// block removal — keeps a WAV's RIFF size equal to its length, so the tag
    /// reads back every time. The backend's own WAV writer left it short, and
    /// the next write corrupted the file; one that arrived short could not be
    /// edited at all.
    #[test]
    fn a_wav_survives_repeated_writes_and_a_block_removal_undo() {
        for (name, riff_size) in [("healthy", None), ("short", Some(100u32))] {
            let bytes = wav_bytes(&[(b"fmt ", &PCM_FMT), (b"data", &[0; 4000])], riff_size);
            let path = std::env::temp_dir().join(format!(
                "tagrex-wav-writes-{name}-{}.wav",
                std::process::id()
            ));
            std::fs::write(&path, &bytes).unwrap();
            for title in ["First", "A longer second title", "3"] {
                let mut file = TagEngine::read(&path).expect("read");
                file.tags.insert(TagField::Title, title.to_string());
                TagEngine::write(&file).expect("write");
                let out = std::fs::read(&path).unwrap();
                assert!(
                    riff_size_matches(&out),
                    "{name}: RIFF size after writing {title:?}"
                );
                let reread = TagEngine::read(&path).expect("reread");
                assert_eq!(
                    reread.tags.get(&TagField::Title).map(String::as_str),
                    Some(title)
                );
            }
            let saved = TagEngine::dump_id3v2(&path)
                .expect("dump")
                .expect("a block");
            TagEngine::remove_block(&path, TagBlockKind::Id3v2).expect("remove");
            assert!(riff_size_matches(&std::fs::read(&path).unwrap()));
            assert!(!TagEngine::read(&path)
                .expect("read")
                .tags
                .contains_key(&TagField::Title));
            TagEngine::restore_id3v2_bytes(&path, &saved, &[], None).expect("restore");
            assert!(riff_size_matches(&std::fs::read(&path).unwrap()));
            assert_eq!(
                TagEngine::read(&path)
                    .expect("read")
                    .tags
                    .get(&TagField::Title)
                    .map(String::as_str),
                Some("3")
            );
            std::fs::remove_file(&path).ok();
        }
    }

    /// Build a FORM/AIFF from `(chunk-id, payload)` pairs, for the repair tests.
    fn aiff_bytes(chunks: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
        let mut body = Vec::new();
        for (id, payload) in chunks {
            body.extend_from_slice(*id);
            body.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            body.extend_from_slice(payload);
            if payload.len() % 2 == 1 {
                body.push(0); // word-align
            }
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"FORM");
        out.extend_from_slice(&((4 + body.len()) as u32).to_be_bytes());
        out.extend_from_slice(b"AIFF");
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn repair_rebuilds_an_aiff_broken_after_the_audio() {
        // A well-formed COMM + SSND, then a junk chunk whose length runs past the
        // file — the shape a Bandcamp AIFF export leaves (#357).
        let mut bytes = aiff_bytes(&[(b"COMM", &[1, 2, 3, 4]), (b"SSND", &[5, 6, 7, 8])]);
        bytes.extend_from_slice(b"SBRE"); // a plausible id …
        bytes.extend_from_slice(&u32::MAX.to_be_bytes()); // … with an impossible length
        bytes.extend_from_slice(b"trailing junk metadata");

        let path = std::env::temp_dir().join(format!("tagrex-repair-{}.aiff", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();
        assert!(
            repair_aiff_container(&path).unwrap(),
            "a broken file is repaired"
        );

        let fixed = std::fs::read(&path).unwrap();
        // FORM header rebuilt, junk gone, audio chunks kept byte for byte.
        assert_eq!(&fixed[0..4], b"FORM");
        assert_eq!(&fixed[8..12], b"AIFF");
        assert_eq!(
            u32::from_be_bytes([fixed[4], fixed[5], fixed[6], fixed[7]]) as usize,
            fixed.len() - 8
        );
        assert_eq!(
            fixed,
            aiff_bytes(&[(b"COMM", &[1, 2, 3, 4]), (b"SSND", &[5, 6, 7, 8])])
        );
        assert!(!fixed.windows(4).any(|w| w == b"SBRE"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn repair_leaves_a_well_formed_aiff_untouched() {
        // Every chunk parses to the end — nothing to repair, so the file must not
        // be rewritten (a healthy AIFF is never shortened).
        let bytes = aiff_bytes(&[(b"COMM", &[1, 2, 3, 4]), (b"SSND", &[5, 6, 7, 8])]);
        let path =
            std::env::temp_dir().join(format!("tagrex-repair-ok-{}.aiff", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();
        assert!(
            !repair_aiff_container(&path).unwrap(),
            "a healthy file is left alone"
        );
        assert_eq!(
            std::fs::read(&path).unwrap(),
            bytes,
            "the file is byte-for-byte unchanged"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn repair_declines_a_non_aiff_or_audioless_file() {
        // Not a FORM/AIFF at all.
        let path =
            std::env::temp_dir().join(format!("tagrex-repair-non-{}.bin", std::process::id()));
        std::fs::write(&path, b"not an aiff at all").unwrap();
        assert!(!repair_aiff_container(&path).unwrap());
        // A broken chain but no SSND — nothing safe to keep, so declined.
        let mut bytes = aiff_bytes(&[(b"COMM", &[1, 2, 3, 4])]);
        bytes.extend_from_slice(b"SBRE");
        bytes.extend_from_slice(&u32::MAX.to_be_bytes());
        std::fs::write(&path, &bytes).unwrap();
        assert!(
            !repair_aiff_container(&path).unwrap(),
            "no audio chunk -> not repaired"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn write_aiff_id3_replaces_the_tag_keeps_audio_drops_junk() {
        // COMM + SSND (audio), an old ID3 to replace, then junk to drop (#358).
        let mut bytes = aiff_bytes(&[
            (b"COMM", &[1, 2, 3, 4]),
            (b"SSND", &[5, 6, 7, 8]),
            (b"ID3 ", b"old-tag-bytes"),
        ]);
        bytes.extend_from_slice(b"SBRE");
        bytes.extend_from_slice(&u32::MAX.to_be_bytes()); // trailing junk

        let path =
            std::env::temp_dir().join(format!("tagrex-aiff-id3-{}.aiff", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();
        assert!(write_iff_id3(&path, Some(b"NEW-ID3-BYTES")).unwrap());

        let out = std::fs::read(&path).unwrap();
        // Well-formed FORM/AIFF with the size field matching the file.
        assert_eq!(&out[0..4], b"FORM");
        assert_eq!(&out[8..12], b"AIFF");
        assert_eq!(
            u32::from_be_bytes([out[4], out[5], out[6], out[7]]) as usize,
            out.len() - 8
        );
        // Audio kept, the old tag and the junk gone, exactly one fresh ID3 with
        // the new bytes as its payload.
        let expected = aiff_bytes(&[
            (b"COMM", &[1, 2, 3, 4]),
            (b"SSND", &[5, 6, 7, 8]),
            (b"ID3 ", b"NEW-ID3-BYTES"),
        ]);
        assert_eq!(out, expected);
        assert_eq!(out.windows(4).filter(|w| *w == b"ID3 ").count(), 1);
        assert!(!out.windows(13).any(|w| w == b"old-tag-bytes"));
        assert!(!out.windows(4).any(|w| w == b"SBRE"));
        std::fs::remove_file(&path).ok();
    }

    /// #620: the WAV/AIFF writers and the AIFF repair change the file itself,
    /// not a temp file renamed over it: the mode and a second hard link stay,
    /// and a read-only file is refused and left as it was.
    #[cfg(unix)]
    #[test]
    fn iff_rewrites_keep_the_file_itself() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let mut broken_aiff = aiff_bytes(&[(b"COMM", &[1, 2, 3, 4]), (b"SSND", &[5, 6, 7, 8])]);
        broken_aiff.extend_from_slice(b"SBRE");
        broken_aiff.extend_from_slice(&u32::MAX.to_be_bytes());
        let wav = wav_bytes(
            &[(b"fmt ", &PCM_FMT), (b"data", &[0; 8]), (b"id3 ", b"old")],
            None,
        );
        type Rewrite<'a> = &'a dyn Fn(&Path) -> Result<bool, TagIoError>;
        let tag_write = |path: &Path| write_iff_id3(path, Some(b"NEW-ID3-BYTES"));
        let cases: [(&str, &[u8], Rewrite); 3] = [
            ("wav", &wav, &tag_write),
            ("aiff", &broken_aiff, &tag_write),
            ("aiff", &broken_aiff, &repair_aiff_container),
        ];
        for (index, (extension, bytes, rewrite)) in cases.into_iter().enumerate() {
            let dir = std::env::temp_dir();
            let name = format!("tagrex-in-place-{}-{index}", std::process::id());
            let path = dir.join(format!("{name}.{extension}"));
            let link = dir.join(format!("{name}-link.{extension}"));
            std::fs::remove_file(&link).ok();
            std::fs::write(&path, bytes).unwrap();
            std::fs::hard_link(&path, &link).unwrap();

            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
            assert!(
                rewrite(&path).is_err(),
                "case {index}: read-only file written"
            );
            assert_eq!(std::fs::read(&path).unwrap(), bytes, "case {index}");

            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
            assert!(rewrite(&path).unwrap(), "case {index}: nothing rewritten");
            let out = std::fs::read(&path).unwrap();
            assert_ne!(out, bytes, "case {index}");
            let meta = std::fs::metadata(&path).unwrap();
            assert_eq!(meta.mode() & 0o777, 0o640, "case {index}");
            assert_eq!(meta.nlink(), 2, "case {index}");
            assert_eq!(std::fs::read(&link).unwrap(), out, "case {index}");
            std::fs::remove_file(&path).ok();
            std::fs::remove_file(&link).ok();
        }
    }

    /// A custom field name the file's tag cannot store was written with `Ok` and
    /// then gone on the next read: the preview has to flag it, on exactly the
    /// tag types that forbid it.
    #[test]
    fn a_custom_name_the_tag_cannot_store_is_not_writable() {
        let custom = |name: &str| TagField::Custom(name.to_string());
        for name in ["ÜNI", "a=b", "\u{1F3B5}", ""] {
            assert!(
                !is_storable_custom_name_at(Path::new("a.flac"), name),
                "{name:?}"
            );
            assert!(
                !is_storable_custom_name_at(Path::new("a.ogg"), name),
                "{name:?}"
            );
            assert!(
                !is_storable_custom_name_at(Path::new("a.opus"), name),
                "{name:?}"
            );
            assert!(!is_writable_value_at(
                Path::new("a.flac"),
                &custom(name),
                "value"
            ));
        }
        for name in ["X", "TAG", "oggs", "Ü", &"A".repeat(256)] {
            assert!(
                !is_storable_custom_name_at(Path::new("a.ape"), name),
                "{name:?}"
            );
            assert!(
                !is_storable_custom_name_at(Path::new("a.wv"), name),
                "{name:?}"
            );
        }
        // Allowed where the tag allows it.
        for name in ["MOOD", "Spaces In Name", "DISCOGS_RELEASE_ID", "a-b_c.d"] {
            assert!(
                is_storable_custom_name_at(Path::new("a.flac"), name),
                "{name:?}"
            );
            assert!(
                is_storable_custom_name_at(Path::new("a.ape"), name),
                "{name:?}"
            );
        }
        // `~` is 0x7E: past what Vorbis allows, within what APE allows.
        assert!(!is_storable_custom_name_at(Path::new("a.flac"), "a~b"));
        assert!(is_storable_custom_name_at(Path::new("a.ape"), "a~b"));
        // ID3v2 TXXX and MP4 freeform atoms take any name but an empty one.
        for path in ["a.mp3", "a.m4a", "a.wav", "a.aiff"] {
            assert!(is_storable_custom_name_at(
                Path::new(path),
                "ÜNI a=b \u{1F3B5}"
            ));
            assert!(!is_storable_custom_name_at(Path::new(path), ""));
        }
        // Clearing a field is always fine, whatever it is called.
        assert!(is_writable_value_at(
            Path::new("a.flac"),
            &custom("ÜNI"),
            ""
        ));
        assert!(is_writable_value_at(
            Path::new("a.flac"),
            &custom("MOOD"),
            "Calm"
        ));
    }
}
