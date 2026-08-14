//! Metadata provider boundary.
//!
//! "Plugin" initially means a trait boundary in a separate crate, not dynamic
//! loading (architecture.md). Providers compile into the binary but live in
//! isolated crates: a dead upstream API kills one crate, the core is
//! untouched. The lesson comes from music stores that have closed their public
//! APIs: a tagger with the integration wired straight into its core loses the
//! feature outright, while here only the one crate dies.
//!
//! Concurrency decision: providers are called from worker threads; blocking
//! HTTP inside implementations is acceptable. Revisit if the UI shell needs
//! async streams.

use thiserror::Error;

/// What the user is searching for. All fields optional; providers use what
/// they support.
#[derive(Debug, Clone, Default)]
pub struct SearchQuery {
    pub artist: Option<String>,
    pub title: Option<String>,
    pub album: Option<String>,
    /// Catalog number or barcode — first-class because it is the highest
    /// precision key on Discogs.
    pub catalog_number: Option<String>,
    /// Media-type filter (#103): `CD` / `Vinyl` / `LP` / `File`, or `None` for
    /// no filter. Maps to the Discogs `format` search param; MusicBrainz folds it
    /// into its Lucene `format:` field where it maps sensibly.
    pub format: Option<String>,
    /// 1-based page for paginated search; 0 is treated as page 1. Lets the UI
    /// pull results in batches ("Load more") instead of a whole page at once,
    /// keeping traffic to the provider down (#95).
    pub page: u32,
    /// Results per page; 0 means "use the provider's default page size".
    pub per_page: u32,
}

/// Provider-scoped release identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseId(pub String);

/// A search hit, cheap to display in a candidate list.
#[derive(Debug, Clone)]
pub struct ReleaseCandidate {
    pub id: ReleaseId,
    pub artist: String,
    pub title: String,
    pub year: Option<u16>,
    /// Provider-reported or locally computed relevance, 0.0..=1.0.
    pub score: f32,
    /// Small cover thumbnail URL, when the provider offers one — enough to tell
    /// releases apart visually in the candidate list. Fetch its bytes through
    /// the app layer (auth headers) the same way a full cover is fetched.
    pub thumb_url: Option<String>,
    /// Larger cover image URL (bigger than the thumbnail) — for a grid of tiles
    /// where the small thumbnail would look upscaled.
    pub cover_url: Option<String>,
    /// Release country, e.g. `Belgium`.
    pub country: Option<String>,
    /// Primary label, e.g. `Antler-Subway`.
    pub label: Option<String>,
    /// Human-readable format descriptors joined, e.g. `CD, Compilation, Mixed`.
    pub format: Option<String>,
    /// Catalogue number, e.g. `TOTH 006` — the DJ's most precise match key.
    pub catalog_number: Option<String>,
}

/// One track of a fetched release.
#[derive(Debug, Clone)]
pub struct ReleaseTrack {
    /// Position as the provider reports it ("A1", "3", "1-05", ...).
    pub position: String,
    /// Which disc of the set this track sits on (#146), 1-based, when the
    /// provider says so — Discogs encodes it in the position (`1-05`), and on
    /// MusicBrainz it is the medium the track was flattened out of. `None` for a
    /// single-disc release, and for a vinyl side, which is a different thing
    /// entirely and is resolved from the side letter at import time.
    pub disc: Option<u32>,
    /// Track-level artist when it differs from the release artist.
    pub artist: Option<String>,
    pub title: String,
    /// Listed length in seconds, when the provider states one. Usually
    /// transcribed by hand from the sleeve, so treat it as a hint rather than
    /// ground truth (#64).
    pub duration_secs: Option<u64>,
    /// ISRC (per-recording code), when the provider exposes it — a definitive
    /// match key (#54). MusicBrainz supplies these; Discogs does not.
    pub isrc: Option<String>,
    /// Tempo in BPM, when the provider states one (#162). A store that sells to
    /// DJs measures it; a general-purpose database does not, so this is `None`
    /// for most sources.
    pub bpm: Option<u16>,
    /// Musical key in the compact spelling the tag wants (`Am`, `F#`) — the
    /// provider normalizes whatever notation it publishes through
    /// [`crate::transform::KeyNotation`], so the value here is ready to write
    /// and can be converted to Camelot or Open Key like any other key value.
    pub key: Option<String>,
}

/// A fully fetched release, ready to be mapped onto local files.
#[derive(Debug, Clone)]
pub struct Release {
    pub id: ReleaseId,
    pub artist: String,
    pub title: String,
    pub year: Option<u16>,
    /// Broad, coarse genres (Discogs `genres`, e.g. `Electronic`). Kept
    /// separate from [`styles`](Self::styles) so the caller can choose which to
    /// write to the genre tag — for Discogs, `styles` is the more meaningful
    /// value (#26).
    pub genres: Vec<String>,
    /// Specific styles (Discogs `styles`, e.g. `Trance`, `Tribal`, `Techno`).
    pub styles: Vec<String>,
    pub tracks: Vec<ReleaseTrack>,
    /// Label / catalogue-number pairs the release lists (#90). A release can
    /// carry several — even from the same label — so the caller picks which one
    /// to write (label → Publisher, catno → CatalogNumber) rather than merging
    /// them. In listing order; the first is the primary.
    pub labels: Vec<ReleaseLabel>,
    /// Release country, e.g. `Belgium` (full name as the provider states it).
    pub country: Option<String>,
    /// Physical/source format descriptor, e.g. `Vinyl, 12", 33 ⅓ RPM` or `CD`
    /// (#106). Drives the media-type tag and the vinyl side view.
    pub format: Option<String>,
    /// How many discs the set holds (#146) — Discogs states it as the format
    /// quantity (`2×CD`), MusicBrainz as the number of media. `None` when the
    /// provider doesn't say; `Some(1)` is a stated single disc, which is not the
    /// same thing and is worth writing.
    pub disc_total: Option<u32>,
    /// Public webpage for the release (the provider's release page), if any.
    pub url: Option<String>,
    /// URL of the release's primary image (full resolution), if it has one.
    /// The bytes still have to be downloaded with the provider's auth + User-
    /// Agent headers — the webview can't load it directly — so this is only a
    /// handle, fetched on demand via the provider's image download.
    pub cover_image_url: Option<String>,
    /// Every image the release carries, in listing order — the primary first
    /// (#102). Used to show the cover resolution + image count and to save the
    /// images to disk. `cover_image_url` is the first entry's URL; this exposes
    /// the rest, plus each image's dimensions when the provider states them.
    pub images: Vec<ReleaseImage>,
}

/// One image of a release: a download handle plus its pixel dimensions when the
/// provider states them (`0` means unknown — MusicBrainz/CAA doesn't report
/// sizes in the release JSON, Discogs does). Ordered with the primary first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseImage {
    pub url: String,
    pub width: u32,
    pub height: u32,
}

/// One label imprint of a release, with its catalogue number when stated (#90).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseLabel {
    pub name: String,
    pub catalog_number: Option<String>,
}

/// An image downloaded from a provider: raw bytes plus the MIME type reported
/// by the server. The provider boundary's counterpart to
/// [`crate::model::CoverArt`], kept separate so the network layer doesn't reach
/// into the plan model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedImage {
    pub mime: String,
    pub data: Vec<u8>,
}

/// The disc a multi-disc track position names (#146): `1-05` -> 1, `2-1` -> 2,
/// `CD1-3` -> 1, `D2-04` -> 2.
///
/// A provider that holds several discs in one flat tracklist has to say which
/// disc a track is on somewhere, and the position is where Discogs puts it. Only
/// the part *before* the separator is read here; the track number itself comes
/// out of the tail as it always did.
///
/// Deliberately narrow. It returns `None` for a plain number (`5` — one disc,
/// nothing to say), for a vinyl side (`A1` — a side is not a disc, and mapping
/// one to the other is a separate, opt-in decision at import time), and for
/// anything else it doesn't recognise, because inventing a disc number is worse
/// than leaving the tag alone.
pub fn disc_from_position(position: &str) -> Option<u32> {
    let (head, _) = position.trim().split_once(['-', '.', '/'])?;
    // An optional media prefix, so `CD1-3` and `D2-04` read like `1-3`. Matched
    // case-insensitively and only as leading letters; anything left after
    // stripping them must be plain digits.
    let digits = head.trim_start_matches(|c: char| c.is_ascii_alphabetic());
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // A letter prefix is only credible as a medium name. `A-1` is a vinyl side
    // written with a separator, not disc 1 of a set with no number.
    let prefix = &head[..head.len() - digits.len()];
    if !prefix.is_empty() && !matches!(prefix.to_ascii_lowercase().as_str(), "cd" | "d" | "disc") {
        return None;
    }
    digits.parse::<u32>().ok().filter(|disc| *disc > 0)
}

pub trait MetadataProvider: Send + Sync {
    /// Stable machine identifier ("discogs", "musicbrainz").
    fn id(&self) -> &'static str;

    /// Name for the UI.
    fn display_name(&self) -> &'static str;

    fn search(&self, query: &SearchQuery) -> Result<Vec<ReleaseCandidate>, ProviderError>;

    fn fetch_release(&self, id: &ReleaseId) -> Result<Release, ProviderError>;
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("network error: {0}")]
    Network(String),
    #[error("rate limited, retry after {retry_after_secs} s")]
    RateLimited { retry_after_secs: u64 },
    #[error("authentication failed: {0}")]
    Auth(String),
    #[error("release not found")]
    NotFound,
    #[error("{0}")]
    Other(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_disc_out_of_a_multi_disc_position() {
        assert_eq!(disc_from_position("1-05"), Some(1));
        assert_eq!(disc_from_position("2-1"), Some(2));
        assert_eq!(disc_from_position("CD1-3"), Some(1));
        assert_eq!(disc_from_position("cd2-04"), Some(2));
        assert_eq!(disc_from_position("D2-04"), Some(2));
        assert_eq!(disc_from_position("Disc3-1"), Some(3));
        // Other separators the same release listing might use.
        assert_eq!(disc_from_position("2.14"), Some(2));
        assert_eq!(disc_from_position("2/14"), Some(2));
    }

    #[test]
    fn refuses_positions_that_do_not_name_a_disc() {
        // A single-disc release: nothing to say.
        assert_eq!(disc_from_position("5"), None);
        assert_eq!(disc_from_position(""), None);
        // A vinyl side is not a disc. Mapping one to the other is a separate,
        // opt-in decision made at import time (#105), not something to infer.
        assert_eq!(disc_from_position("A1"), None);
        assert_eq!(disc_from_position("A-1"), None);
        // A letter prefix that isn't a medium name, and a non-numeric head.
        assert_eq!(disc_from_position("X1-3"), None);
        assert_eq!(disc_from_position("side-3"), None);
        assert_eq!(disc_from_position("-3"), None);
        // Disc zero is not a disc.
        assert_eq!(disc_from_position("0-3"), None);
    }
}
