//! Discogs metadata provider.
//!
//! Authentication: personal user token, entered by the user in settings.
//! Rate limit: 60 requests/minute for authenticated requests — plenty for
//! tagging, but the implementation must honor 429 responses and surface
//! [`ProviderError::RateLimited`] with the `Retry-After` value.
//!
//! Known data quirk to handle in post-processing: Discogs disambiguates
//! artist names with a numeric suffix, e.g. `Artist (3)` — strip it via the
//! core transform pipeline before writing tags.

use tagrex_core::provider::{
    MetadataProvider, ProviderError, Release, ReleaseCandidate, ReleaseId, SearchQuery,
};

pub struct DiscogsProvider {
    #[allow(dead_code)] // read once the HTTP client is wired in
    token: String,
}

impl DiscogsProvider {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
        }
    }
}

impl MetadataProvider for DiscogsProvider {
    fn id(&self) -> &'static str {
        "discogs"
    }

    fn display_name(&self) -> &'static str {
        "Discogs"
    }

    fn search(&self, _query: &SearchQuery) -> Result<Vec<ReleaseCandidate>, ProviderError> {
        todo!("GET /database/search with token auth, map results to candidates")
    }

    fn fetch_release(&self, _id: &ReleaseId) -> Result<Release, ProviderError> {
        todo!("GET /releases/{{id}}, map tracklist and genres")
    }
}
