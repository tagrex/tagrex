//! Discogs metadata provider.
//!
//! Authentication: personal user token, entered by the user in settings, sent
//! as an `Authorization: Discogs token=…` header. Discogs also requires a
//! descriptive `User-Agent`; requests without one are rejected.
//!
//! Rate limit: 60 requests/minute for authenticated requests — plenty for
//! tagging, but 429 responses are honored and surfaced as
//! [`ProviderError::RateLimited`] with the `Retry-After` value.
//!
//! Known data quirk handled here: Discogs disambiguates artist names with a
//! numeric suffix, e.g. `Artist (3)`. That suffix is stripped through the core
//! transform pipeline ([`StripDiscogsSuffix`]) before the mapped data leaves
//! the provider.
//!
//! HTTP is blocking (`ureq`), matching the worker-thread model in
//! architecture.md. The response-mapping logic is factored into pure functions
//! so it can be unit-tested against fixture JSON with no network.

use serde_json::Value;
use tagrex_core::provider::{
    FetchedImage, MetadataProvider, ProviderError, Release, ReleaseCandidate, ReleaseId,
    ReleaseTrack, SearchQuery,
};
use tagrex_core::transform::{TransformChain, TransformStep};

const API_BASE: &str = "https://api.discogs.com";
const USER_AGENT: &str = concat!(
    "TagRex/",
    env!("CARGO_PKG_VERSION"),
    " +https://github.com/tagrex/tagrex"
);

pub struct DiscogsProvider {
    agent: ureq::Agent,
    token: String,
}

impl DiscogsProvider {
    pub fn new(token: impl Into<String>) -> Self {
        // Status-as-error off: we want the response object for every status so
        // a 429 can be read for its Retry-After header rather than collapsing
        // into an opaque error.
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build();
        Self {
            agent: ureq::Agent::new_with_config(config),
            token: token.into(),
        }
    }

    fn get(&self, url: &str, query: &[(&str, &str)]) -> Result<String, ProviderError> {
        let mut request = self
            .agent
            .get(url)
            .header("Authorization", &format!("Discogs token={}", self.token))
            .header("User-Agent", USER_AGENT);
        for (key, value) in query {
            request = request.query(*key, *value);
        }

        let mut response = request
            .call()
            .map_err(|err| ProviderError::Network(err.to_string()))?;

        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok());
            return Err(status_to_error(status, retry_after));
        }

        response
            .body_mut()
            .read_to_string()
            .map_err(|err| ProviderError::Network(err.to_string()))
    }

    /// Download binary content (an image) from a Discogs URL, returning the
    /// bytes and the server-reported MIME type. Sends the same auth + User-Agent
    /// headers as [`get`](Self::get): Discogs serves release images from a CDN
    /// that rejects requests without them.
    ///
    /// Not on [`MetadataProvider`]: image fetching is Discogs-specific plumbing,
    /// and the app instantiates `DiscogsProvider` directly rather than through
    /// the trait object, so keeping it inherent avoids forcing it on every
    /// future provider.
    pub fn fetch_image(&self, url: &str) -> Result<FetchedImage, ProviderError> {
        let mut response = self
            .agent
            .get(url)
            .header("Authorization", &format!("Discogs token={}", self.token))
            .header("User-Agent", USER_AGENT)
            .call()
            .map_err(|err| ProviderError::Network(err.to_string()))?;

        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok());
            return Err(status_to_error(status, retry_after));
        }

        // Prefer the server's Content-Type; fall back to JPEG, which is what
        // Discogs serves for essentially all release art.
        let mime = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .map(|value| value.split(';').next().unwrap_or(value).trim().to_string())
            .filter(|value| value.starts_with("image/"))
            .unwrap_or_else(|| "image/jpeg".to_string());

        let data = response
            .body_mut()
            .read_to_vec()
            .map_err(|err| ProviderError::Network(err.to_string()))?;

        Ok(FetchedImage { mime, data })
    }
}

impl MetadataProvider for DiscogsProvider {
    fn id(&self) -> &'static str {
        "discogs"
    }

    fn display_name(&self) -> &'static str {
        "Discogs"
    }

    fn search(&self, query: &SearchQuery) -> Result<Vec<ReleaseCandidate>, ProviderError> {
        let mut params: Vec<(&str, &str)> = vec![("type", "release")];
        if let Some(artist) = &query.artist {
            params.push(("artist", artist));
        }
        if let Some(title) = &query.title {
            params.push(("release_title", title));
        }
        if let Some(album) = &query.album {
            params.push(("q", album));
        }
        if let Some(catalog) = &query.catalog_number {
            params.push(("catno", catalog));
        }

        let body = self.get(&format!("{API_BASE}/database/search"), &params)?;
        parse_search_response(&body)
    }

    fn fetch_release(&self, id: &ReleaseId) -> Result<Release, ProviderError> {
        let body = self.get(&format!("{API_BASE}/releases/{}", id.0), &[])?;
        parse_release(&body)
    }
}

/// Strips Discogs' numeric disambiguation suffix (`Artist (3)` -> `Artist`).
/// A [`TransformStep`] so it composes into the core transform pipeline like
/// any other text cleanup, rather than being a bespoke one-off.
pub struct StripDiscogsSuffix;

impl TransformStep for StripDiscogsSuffix {
    fn name(&self) -> &str {
        "strip_discogs_suffix"
    }

    fn apply(&self, input: &str) -> String {
        let trimmed = input.trim_end();
        if trimmed.ends_with(')') {
            if let Some(open) = trimmed.rfind(" (") {
                let inner = &trimmed[open + 2..trimmed.len() - 1];
                if !inner.is_empty() && inner.bytes().all(|byte| byte.is_ascii_digit()) {
                    return trimmed[..open].to_string();
                }
            }
        }
        input.to_string()
    }
}

fn artist_cleaner() -> TransformChain {
    let mut chain = TransformChain::default();
    chain.push(Box::new(StripDiscogsSuffix));
    chain
}

fn status_to_error(status: u16, retry_after: Option<&str>) -> ProviderError {
    match status {
        401 | 403 => ProviderError::Auth(format!("HTTP {status}")),
        404 => ProviderError::NotFound,
        429 => ProviderError::RateLimited {
            // Default to the documented 60s window if the header is absent or
            // unparseable.
            retry_after_secs: retry_after
                .and_then(|value| value.parse().ok())
                .unwrap_or(60),
        },
        _ => ProviderError::Other(format!("HTTP {status}")),
    }
}

fn parse_search_response(body: &str) -> Result<Vec<ReleaseCandidate>, ProviderError> {
    let root: Value =
        serde_json::from_str(body).map_err(|err| ProviderError::Other(err.to_string()))?;
    let results = root
        .get("results")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ProviderError::Other("search response missing `results` array".to_string())
        })?;

    let count = results.len();
    let candidates = results
        .iter()
        .enumerate()
        .filter_map(|(index, result)| {
            let id = value_to_id(result.get("id")?)?;
            let combined = result
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or_default();
            // Discogs search results carry a combined "Artist - Title" string
            // rather than separate fields.
            let (artist, title) = match combined.split_once(" - ") {
                Some((artist, title)) => (artist.trim().to_string(), title.trim().to_string()),
                None => (String::new(), combined.trim().to_string()),
            };
            Some(ReleaseCandidate {
                id,
                artist,
                title,
                year: result.get("year").and_then(value_to_year),
                // Discogs doesn't return a relevance score; approximate one
                // from result order (results come back best-first) so the
                // candidate list has a stable, meaningful ranking.
                score: positional_score(index, count),
            })
        })
        .collect();
    Ok(candidates)
}

fn parse_release(body: &str) -> Result<Release, ProviderError> {
    let root: Value =
        serde_json::from_str(body).map_err(|err| ProviderError::Other(err.to_string()))?;
    let cleaner = artist_cleaner();

    let id = root
        .get("id")
        .and_then(value_to_id)
        .ok_or_else(|| ProviderError::Other("release missing `id`".to_string()))?;

    let artist = join_artists(root.get("artists"), &cleaner)
        .ok_or_else(|| ProviderError::Other("release missing `artists`".to_string()))?;

    let title = root
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    // Discogs splits broad "genres" (e.g. Electronic) from specific "styles"
    // (e.g. House). Kept separate here so the app can prefer styles for the
    // genre tag (#26); the coarse genres remain available as a fallback.
    let genres = string_array(root.get("genres"));
    let styles = string_array(root.get("styles"));

    let tracks = root
        .get("tracklist")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter(|entry| is_track(entry))
                .map(|entry| ReleaseTrack {
                    position: entry
                        .get("position")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    artist: join_artists(entry.get("artists"), &cleaner),
                    title: entry
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    duration_secs: entry
                        .get("duration")
                        .and_then(Value::as_str)
                        .and_then(parse_duration),
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(Release {
        id,
        artist,
        title,
        year: root.get("year").and_then(value_to_year),
        genres,
        styles,
        tracks,
        cover_image_url: primary_image_url(root.get("images")),
    })
}

/// Parse a listed track length into seconds.
///
/// Discogs writes these as `M:SS`, occasionally `H:MM:SS`, and very often as an
/// empty string — the field is transcribed by hand, so absent or malformed
/// values are the norm rather than an error worth surfacing.
fn parse_duration(value: &str) -> Option<u64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut seconds: u64 = 0;
    let mut parts = 0;
    for part in trimmed.split(':') {
        let number: u64 = part.trim().parse().ok()?;
        seconds = seconds.checked_mul(60)?.checked_add(number)?;
        parts += 1;
    }
    // Bare "238" is not a duration; require at least minutes and seconds.
    (2..=3).contains(&parts).then_some(seconds)
}

/// The full-resolution URL of a release's primary image. Discogs marks one
/// image `type: "primary"`; when none is flagged (older releases just list
/// images), fall back to the first. Uses `uri` (full size) rather than `uri150`
/// (the 150px thumbnail) since this is what gets embedded.
fn primary_image_url(images: Option<&Value>) -> Option<String> {
    let images = images?.as_array()?;
    let primary = images
        .iter()
        .find(|image| image.get("type").and_then(Value::as_str) == Some("primary"))
        .or_else(|| images.first())?;
    primary
        .get("uri")
        .and_then(Value::as_str)
        .filter(|uri| !uri.is_empty())
        .map(str::to_string)
}

/// Tracklist entries can be headings or index tracks, not just playable
/// tracks; keep only real tracks. Discogs marks these with `type_`, treating a
/// missing value as a track.
fn is_track(entry: &Value) -> bool {
    match entry.get("type_").and_then(Value::as_str) {
        Some(kind) => kind == "track",
        None => true,
    }
}

fn join_artists(value: Option<&Value>, cleaner: &TransformChain) -> Option<String> {
    let artists = value?.as_array()?;
    let names: Vec<String> = artists
        .iter()
        .filter_map(|artist| artist.get("name").and_then(Value::as_str))
        .map(|name| cleaner.apply(name))
        .filter(|name| !name.is_empty())
        .collect();
    if names.is_empty() {
        None
    } else {
        Some(names.join(", "))
    }
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Discogs ids come back as JSON integers, but accept a string form too.
fn value_to_id(value: &Value) -> Option<ReleaseId> {
    if let Some(number) = value.as_i64() {
        return Some(ReleaseId(number.to_string()));
    }
    value.as_str().map(|text| ReleaseId(text.to_string()))
}

/// Year is an integer in the release endpoint but a string (`"1996"`) in
/// search results; `0` means unknown.
fn value_to_year(value: &Value) -> Option<u16> {
    let year = if let Some(number) = value.as_u64() {
        u16::try_from(number).ok()?
    } else {
        value.as_str()?.parse().ok()?
    };
    (year != 0).then_some(year)
}

fn positional_score(index: usize, count: usize) -> f32 {
    if count <= 1 {
        1.0
    } else {
        1.0 - (index as f32 / count as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_numeric_disambiguation_suffix() {
        let step = StripDiscogsSuffix;
        assert_eq!(step.apply("Aphex Twin (3)"), "Aphex Twin");
        assert_eq!(step.apply("AC/DC"), "AC/DC");
        // Not a disambiguation suffix -- a real parenthetical stays.
        assert_eq!(
            step.apply("Godspeed You! Black Emperor (F#A#)"),
            "Godspeed You! Black Emperor (F#A#)"
        );
        assert_eq!(step.apply("The B-52's"), "The B-52's");
    }

    #[test]
    fn maps_search_results() {
        let body = r#"{
            "results": [
                {"id": 249504, "title": "Rick Astley - Never Gonna Give You Up", "year": "1987"},
                {"id": 1234, "title": "No Separator Title"}
            ]
        }"#;
        let candidates = parse_search_response(body).unwrap();
        assert_eq!(candidates.len(), 2);

        assert_eq!(candidates[0].id, ReleaseId("249504".to_string()));
        assert_eq!(candidates[0].artist, "Rick Astley");
        assert_eq!(candidates[0].title, "Never Gonna Give You Up");
        assert_eq!(candidates[0].year, Some(1987));
        assert!(candidates[0].score > candidates[1].score);

        // No " - " separator: whole string becomes the title.
        assert_eq!(candidates[1].artist, "");
        assert_eq!(candidates[1].title, "No Separator Title");
    }

    #[test]
    fn maps_release_with_styles_and_strips_artist_suffix() {
        let body = r#"{
            "id": 249504,
            "title": "Never Gonna Give You Up",
            "year": 1987,
            "artists": [{"name": "Rick Astley (2)"}],
            "genres": ["Electronic"],
            "styles": ["Synth-pop"],
            "images": [
                {"type": "secondary", "uri": "https://img.discogs.com/back.jpg"},
                {"type": "primary", "uri": "https://img.discogs.com/front.jpg", "uri150": "https://img.discogs.com/front-150.jpg"}
            ],
            "tracklist": [
                {"type_": "heading", "position": "", "title": "Side A"},
                {"type_": "track", "position": "A", "title": "Never Gonna Give You Up", "duration": "3:32"},
                {"position": "B", "title": "Never Gonna Give You Up (Instrumental)", "duration": ""}
            ]
        }"#;
        let release = parse_release(body).unwrap();

        assert_eq!(release.id, ReleaseId("249504".to_string()));
        assert_eq!(release.artist, "Rick Astley"); // suffix stripped
        assert_eq!(release.year, Some(1987));
        // Genres and styles are kept separate (#26).
        assert_eq!(release.genres, vec!["Electronic"]);
        assert_eq!(release.styles, vec!["Synth-pop"]);
        // Primary image picked (full-res `uri`), not the secondary/back one.
        assert_eq!(
            release.cover_image_url.as_deref(),
            Some("https://img.discogs.com/front.jpg")
        );
        // Heading filtered out; two real tracks remain.
        assert_eq!(release.tracks.len(), 2);
        assert_eq!(release.tracks[0].position, "A");
        assert_eq!(release.tracks[0].duration_secs, Some(212));
        // An empty duration is normal on Discogs, not an error.
        assert_eq!(release.tracks[1].duration_secs, None);
        assert_eq!(
            release.tracks[1].title,
            "Never Gonna Give You Up (Instrumental)"
        );
    }

    #[test]
    fn primary_image_falls_back_to_first_and_absent_when_empty() {
        // No image flagged "primary" -> first image wins.
        let images = serde_json::json!([
            {"type": "secondary", "uri": "https://img.discogs.com/a.jpg"},
            {"type": "secondary", "uri": "https://img.discogs.com/b.jpg"}
        ]);
        assert_eq!(
            primary_image_url(Some(&images)).as_deref(),
            Some("https://img.discogs.com/a.jpg")
        );

        // No images at all -> None.
        assert_eq!(primary_image_url(None), None);
        assert_eq!(primary_image_url(Some(&serde_json::json!([]))), None);
    }

    #[test]
    fn parses_listed_durations_and_tolerates_junk() {
        assert_eq!(parse_duration("4:38"), Some(278));
        assert_eq!(parse_duration("2:22"), Some(142));
        assert_eq!(parse_duration(" 5:21 "), Some(321));
        assert_eq!(parse_duration("1:04:38"), Some(3878));
        // Absent or unusable values are simply unknown.
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("   "), None);
        assert_eq!(parse_duration("?"), None);
        // A bare number is not a duration.
        assert_eq!(parse_duration("238"), None);
    }

    #[test]
    fn status_mapping_covers_the_documented_cases() {
        assert!(matches!(status_to_error(403, None), ProviderError::Auth(_)));
        assert!(matches!(
            status_to_error(404, None),
            ProviderError::NotFound
        ));
        assert!(matches!(
            status_to_error(429, Some("120")),
            ProviderError::RateLimited {
                retry_after_secs: 120
            }
        ));
        assert!(matches!(
            status_to_error(429, None),
            ProviderError::RateLimited {
                retry_after_secs: 60
            }
        ));
        assert!(matches!(
            status_to_error(500, None),
            ProviderError::Other(_)
        ));
    }
}
