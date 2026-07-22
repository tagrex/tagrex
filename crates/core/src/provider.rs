//! Metadata provider boundary.
//!
//! "Plugin" initially means a trait boundary in a separate crate, not dynamic
//! loading (architecture.md). Providers compile into the binary but live in
//! isolated crates: a dead upstream API kills one crate, the core is
//! untouched. Lesson learned from Beatport closing its public API and
//! TagScanner losing the feature entirely.
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
    /// Track-level artist when it differs from the release artist.
    pub artist: Option<String>,
    pub title: String,
    /// Listed length in seconds, when the provider states one. Usually
    /// transcribed by hand from the sleeve, so treat it as a hint rather than
    /// ground truth (#64).
    pub duration_secs: Option<u64>,
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
    /// URL of the release's primary image (full resolution), if it has one.
    /// The bytes still have to be downloaded with the provider's auth + User-
    /// Agent headers — the webview can't load it directly — so this is only a
    /// handle, fetched on demand via the provider's image download.
    pub cover_image_url: Option<String>,
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
