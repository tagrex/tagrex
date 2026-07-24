//! MusicBrainz metadata provider.
//!
//! Authentication: none. MusicBrainz is a free, open database and needs no
//! token — but it *requires* a descriptive `User-Agent` on every request, and
//! asks unauthenticated clients to stay under ~1 request/second. This crate
//! sends the User-Agent; the ~1 req/s spacing is enforced by the app layer
//! (which serializes provider calls), and a `503` is surfaced as
//! [`ProviderError::RateLimited`].
//!
//! Covers: MusicBrainz release JSON carries no image URLs. Front cover art
//! lives in the separate Cover Art Archive, addressable by MBID at
//! `coverartarchive.org/release/<mbid>/front`. [`fetch_release`] fills
//! [`Release::cover_image_url`] with that URL so the existing fetch-and-embed
//! path (#24) downloads it on import; a release with no archived art simply
//! 404s and yields no cover.
//!
//! Genre: MusicBrainz has no Discogs-style `styles`, only community genre tags.
//! Its release-level `genres` map onto [`Release::genres`]; [`Release::styles`]
//! stays empty, so the app's "styles else genres" rule (#26) writes the
//! MusicBrainz genres to the genre tag.
//!
//! HTTP is blocking (`ureq`), matching the worker-thread model in
//! architecture.md. The response-mapping logic is factored into pure functions
//! so it can be unit-tested against fixture JSON with no network.

use serde_json::Value;
use tagrex_core::provider::{
    FetchedImage, MetadataProvider, ProviderError, Release, ReleaseCandidate, ReleaseId,
    ReleaseTrack, SearchQuery,
};

const API_BASE: &str = "https://musicbrainz.org/ws/2";
const COVER_ART_BASE: &str = "https://coverartarchive.org";
const SEARCH_LIMIT: &str = "25";
/// `inc` parameters needed to map a full release: its tracks (recordings),
/// per-track and release artists, label/catalogue numbers, and genre tags.
const RELEASE_INC: &str = "recordings+artist-credits+labels+genres";
const USER_AGENT: &str = concat!(
    "TagRex/",
    env!("CARGO_PKG_VERSION"),
    " +https://github.com/tagrex/tagrex"
);

pub struct MusicBrainzProvider {
    agent: ureq::Agent,
}

impl MusicBrainzProvider {
    pub fn new() -> Self {
        Self::build(None).expect("no proxy can't fail")
    }

    /// Like [`new`](Self::new) but routing requests through an HTTP/SOCKS proxy
    /// when `proxy` is a non-empty URL. An invalid proxy URL is reported rather
    /// than silently ignored.
    pub fn with_proxy(proxy: Option<&str>) -> Result<Self, ProviderError> {
        Self::build(proxy.filter(|p| !p.trim().is_empty()))
    }

    fn build(proxy: Option<&str>) -> Result<Self, ProviderError> {
        // Status-as-error off: we want the response object for every status so a
        // 503 can be read for its Retry-After header rather than collapsing into
        // an opaque error.
        let mut builder = ureq::Agent::config_builder().http_status_as_error(false);
        if let Some(proxy) = proxy {
            let proxy = ureq::Proxy::new(proxy.trim())
                .map_err(|err| ProviderError::Network(format!("invalid proxy: {err}")))?;
            builder = builder.proxy(Some(proxy));
        }
        Ok(Self {
            agent: ureq::Agent::new_with_config(builder.build()),
        })
    }

    fn get(&self, url: &str, query: &[(&str, &str)]) -> Result<String, ProviderError> {
        let mut request = self.agent.get(url).header("User-Agent", USER_AGENT);
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

    /// Download binary content (a cover image) from a URL, returning the bytes
    /// and the server-reported MIME type. The Cover Art Archive redirects to
    /// archive.org storage; `ureq` follows the redirect. Sends the required
    /// `User-Agent` but no auth.
    ///
    /// Not on [`MetadataProvider`]: image fetching is provider-specific plumbing
    /// (the Cover Art Archive here, an authenticated CDN for Discogs), and the
    /// app instantiates the concrete provider, so keeping it inherent avoids
    /// forcing it onto the trait.
    pub fn fetch_image(&self, url: &str) -> Result<FetchedImage, ProviderError> {
        let mut response = self
            .agent
            .get(url)
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

        // Prefer the server's Content-Type; fall back to JPEG, which is what the
        // Cover Art Archive serves for essentially all front covers.
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

impl Default for MusicBrainzProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl MetadataProvider for MusicBrainzProvider {
    fn id(&self) -> &'static str {
        "musicbrainz"
    }

    fn display_name(&self) -> &'static str {
        "MusicBrainz"
    }

    fn search(&self, query: &SearchQuery) -> Result<Vec<ReleaseCandidate>, ProviderError> {
        let (query_str, dismax) = build_query(query);
        // An empty query would make MusicBrainz reject the request; nothing to
        // search for, so return no candidates.
        if query_str.is_empty() {
            return Ok(Vec::new());
        }
        let mut params: Vec<(&str, &str)> = vec![
            ("query", &query_str),
            ("fmt", "json"),
            ("limit", SEARCH_LIMIT),
        ];
        // dismax matches a plain user query across fields (see `build_query`).
        if dismax {
            params.push(("dismax", "true"));
        }
        let body = self.get(&format!("{API_BASE}/release"), &params)?;
        parse_search_response(&body)
    }

    fn fetch_release(&self, id: &ReleaseId) -> Result<Release, ProviderError> {
        let body = self.get(
            &format!("{API_BASE}/release/{}", id.0),
            &[("inc", RELEASE_INC), ("fmt", "json")],
        )?;
        parse_release(&body)
    }
}

/// The Cover Art Archive front-cover URL for a release MBID. The archive 302s to
/// the actual image; a release with no art 404s (handled as "no cover").
fn cover_art_front_url(mbid: &str) -> String {
    format!("{COVER_ART_BASE}/release/{mbid}/front")
}

/// Build the MusicBrainz search query and whether to request `dismax` mode.
///
/// The app's single search box maps to `album` as free text (often
/// "artist album title"), so when that's all we have we use MusicBrainz's
/// `dismax` mode: it matches a plain user query across the relevant fields
/// (artist, release, …) like a search engine — which is what the box wants. A
/// strict Lucene title phrase would miss any query that isn't exactly the
/// release title. When structured fields are present we build a precise fielded
/// Lucene query instead (no dismax), quoting each value as a phrase so its
/// spaces and punctuation are literal.
fn build_query(query: &SearchQuery) -> (String, bool) {
    let mut fielded: Vec<String> = Vec::new();
    if let Some(artist) = non_empty(query.artist.as_deref()) {
        fielded.push(format!("artist:{}", lucene_phrase(artist)));
    }
    if let Some(title) = non_empty(query.title.as_deref()) {
        fielded.push(format!("release:{}", lucene_phrase(title)));
    }
    if let Some(catno) = non_empty(query.catalog_number.as_deref()) {
        fielded.push(format!("catno:{}", lucene_phrase(catno)));
    }
    let album = non_empty(query.album.as_deref());
    if fielded.is_empty() {
        // Free-text only → hand the plain query to dismax.
        return match album {
            Some(text) => (text.to_string(), true),
            None => (String::new(), false),
        };
    }
    if let Some(text) = album {
        fielded.push(lucene_phrase(text));
    }
    (fielded.join(" AND "), false)
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// Quote a value as a Lucene phrase, escaping the two characters that would
/// break the quoting (`\` and `"`). Phrase-quoting sidesteps the need to escape
/// every other Lucene operator inside the term.
fn lucene_phrase(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn status_to_error(status: u16, retry_after: Option<&str>) -> ProviderError {
    match status {
        // MusicBrainz throttles with 503 (not 429); treat both as rate limiting.
        429 | 503 => ProviderError::RateLimited {
            // Etiquette is ~1 req/s; default to a 1s backoff when unstated.
            retry_after_secs: retry_after
                .and_then(|value| value.parse().ok())
                .unwrap_or(1),
        },
        401 | 403 => ProviderError::Auth(format!("HTTP {status}")),
        404 => ProviderError::NotFound,
        _ => ProviderError::Other(format!("HTTP {status}")),
    }
}

fn parse_search_response(body: &str) -> Result<Vec<ReleaseCandidate>, ProviderError> {
    let root: Value =
        serde_json::from_str(body).map_err(|err| ProviderError::Other(err.to_string()))?;
    let releases = root
        .get("releases")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ProviderError::Other("search response missing `releases` array".to_string())
        })?;

    let candidates = releases
        .iter()
        .filter_map(|release| {
            let id = string_field(release, "id")?;
            let string = |key: &str| string_field(release, key);
            Some(ReleaseCandidate {
                id: ReleaseId(id),
                artist: join_artist_credit(release.get("artist-credit")).unwrap_or_default(),
                title: string("title").unwrap_or_default(),
                year: release.get("date").and_then(date_to_year),
                // MusicBrainz returns a 0..100 relevance score, best-first;
                // normalize to 0.0..=1.0 (the app re-scores against the query
                // text anyway).
                score: release
                    .get("score")
                    .and_then(Value::as_u64)
                    .map(|s| (s as f32 / 100.0).clamp(0.0, 1.0))
                    .unwrap_or(0.0),
                // Search hits carry no cover; the front cover is filled on
                // fetch_release from the Cover Art Archive. Fetching a CAA image
                // per candidate would be a request storm of mostly-404s.
                thumb_url: None,
                cover_url: None,
                country: string("country"),
                label: first_label(release.get("label-info")),
                format: media_formats(release.get("media")),
                catalog_number: first_catalog_number(release.get("label-info")),
            })
        })
        .collect();
    Ok(candidates)
}

fn parse_release(body: &str) -> Result<Release, ProviderError> {
    let root: Value =
        serde_json::from_str(body).map_err(|err| ProviderError::Other(err.to_string()))?;

    let id = string_field(&root, "id")
        .ok_or_else(|| ProviderError::Other("release missing `id`".to_string()))?;

    let artist = join_artist_credit(root.get("artist-credit"))
        .ok_or_else(|| ProviderError::Other("release missing `artist-credit`".to_string()))?;

    let title = string_field(&root, "title").unwrap_or_default();

    // MusicBrainz has no `styles`; only community genre tags. Keep them in
    // `genres` (styles stays empty) so the app's "styles else genres" rule
    // writes these to the genre tag.
    let genres = genre_names(root.get("genres"));

    // Tracks live under media[].tracks[]; flatten every medium in order.
    let tracks = root
        .get("media")
        .and_then(Value::as_array)
        .map(|media| {
            media
                .iter()
                .filter_map(|medium| medium.get("tracks").and_then(Value::as_array))
                .flatten()
                .map(parse_track)
                .collect()
        })
        .unwrap_or_default();

    Ok(Release {
        id: ReleaseId(id.clone()),
        artist,
        title,
        year: root.get("date").and_then(date_to_year),
        genres,
        styles: Vec::new(),
        tracks,
        cover_image_url: Some(cover_art_front_url(&id)),
    })
}

fn parse_track(track: &Value) -> ReleaseTrack {
    ReleaseTrack {
        // `number` is the printed position ("1", "A1", "1-05"); fall back to the
        // numeric `position` when it's absent.
        position: string_field(track, "number")
            .or_else(|| track.get("position").and_then(Value::as_u64).map(|n| n.to_string()))
            .unwrap_or_default(),
        artist: join_artist_credit(track.get("artist-credit")),
        // The track title lives on the track, or on its nested recording.
        title: string_field(track, "title")
            .or_else(|| track.get("recording").and_then(|r| string_field(r, "title")))
            .unwrap_or_default(),
        // `length` is milliseconds; the track's own value wins, else the
        // recording's.
        duration_secs: track
            .get("length")
            .and_then(Value::as_u64)
            .or_else(|| {
                track
                    .get("recording")
                    .and_then(|r| r.get("length"))
                    .and_then(Value::as_u64)
            })
            .map(|ms| ms / 1000),
    }
}

/// Join a MusicBrainz `artist-credit` array into one display string, honoring
/// each entry's `joinphrase` (e.g. `" & "`, `" feat. "`). Returns `None` when
/// there is no usable name.
fn join_artist_credit(value: Option<&Value>) -> Option<String> {
    let credits = value?.as_array()?;
    let mut out = String::new();
    for credit in credits {
        if let Some(name) = credit.get("name").and_then(Value::as_str) {
            out.push_str(name);
        } else if let Some(name) = credit
            .get("artist")
            .and_then(|a| a.get("name"))
            .and_then(Value::as_str)
        {
            out.push_str(name);
        }
        if let Some(join) = credit.get("joinphrase").and_then(Value::as_str) {
            out.push_str(join);
        }
    }
    let out = out.trim().to_string();
    (!out.is_empty()).then_some(out)
}

/// Genre names from a `genres` array (`[{ "name": "techno", "count": 3 }, …]`).
fn genre_names(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|genres| {
            genres
                .iter()
                .filter_map(|g| g.get("name").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// First label name from `label-info` (`[{ label: { name } }, …]`).
fn first_label(value: Option<&Value>) -> Option<String> {
    value?.as_array()?.iter().find_map(|info| {
        info.get("label")
            .and_then(|l| l.get("name"))
            .and_then(Value::as_str)
            .map(str::to_string)
    })
}

/// First catalogue number from `label-info` (`[{ "catalog-number": "…" }, …]`).
fn first_catalog_number(value: Option<&Value>) -> Option<String> {
    value?.as_array()?.iter().find_map(|info| {
        info.get("catalog-number")
            .and_then(Value::as_str)
            .filter(|c| !c.is_empty())
            .map(str::to_string)
    })
}

/// Distinct media format descriptors joined, e.g. `CD` or `2×Vinyl` collapses to
/// `Vinyl, CD`. Mirrors the Discogs candidate's `format` string.
fn media_formats(value: Option<&Value>) -> Option<String> {
    let media = value?.as_array()?;
    let mut formats: Vec<String> = Vec::new();
    for medium in media {
        if let Some(format) = medium.get("format").and_then(Value::as_str) {
            if !format.is_empty() && !formats.iter().any(|f| f == format) {
                formats.push(format.to_string());
            }
        }
    }
    (!formats.is_empty()).then(|| formats.join(", "))
}

/// A non-empty trimmed string field, else `None`.
fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// MusicBrainz dates are `YYYY`, `YYYY-MM`, or `YYYY-MM-DD`; take the year.
fn date_to_year(value: &Value) -> Option<u16> {
    let text = value.as_str()?;
    let year: u16 = text.get(0..4)?.parse().ok()?;
    (year != 0).then_some(year)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_fielded_query_when_structured_fields_present() {
        let q = SearchQuery {
            artist: Some("Boards of Canada".into()),
            title: None,
            album: Some("Music Has the Right".into()),
            catalog_number: Some("warp55".into()),
        };
        // Fielded clauses are ANDed; the free-text album rides along as a phrase.
        assert_eq!(
            build_query(&q),
            (
                "artist:\"Boards of Canada\" AND catno:\"warp55\" AND \"Music Has the Right\""
                    .to_string(),
                false
            )
        );
    }

    #[test]
    fn free_text_album_uses_dismax() {
        // The single search box (free text) goes through dismax, plain and
        // unquoted, so it matches across artist/release like a search engine.
        let only_album = SearchQuery {
            album: Some("boards of canada music has the right".into()),
            ..Default::default()
        };
        assert_eq!(
            build_query(&only_album),
            ("boards of canada music has the right".to_string(), true)
        );
        // Nothing to search → empty, so the provider skips the request.
        assert_eq!(build_query(&SearchQuery::default()), (String::new(), false));
    }

    #[test]
    fn lucene_phrase_escapes_quotes_and_backslashes() {
        assert_eq!(lucene_phrase(r#"a"b\c"#), r#""a\"b\\c""#);
    }

    #[test]
    fn maps_search_results() {
        let body = r#"{
            "count": 2,
            "releases": [
                {
                    "id": "aeb1c1c0-0000-0000-0000-000000000001",
                    "score": 100,
                    "title": "La Bush",
                    "date": "1996-05-01",
                    "country": "BE",
                    "artist-credit": [{ "name": "Various Artists" }],
                    "label-info": [{ "catalog-number": "TOTH 006", "label": { "name": "Antler-Subway" } }],
                    "media": [{ "format": "CD", "track-count": 12 }]
                },
                {
                    "id": "aeb1c1c0-0000-0000-0000-000000000002",
                    "score": 55,
                    "title": "Nothing",
                    "artist-credit": [{ "name": "Nobody" }]
                }
            ]
        }"#;
        let candidates = parse_search_response(body).unwrap();
        assert_eq!(candidates.len(), 2);

        let first = &candidates[0];
        assert_eq!(first.id, ReleaseId("aeb1c1c0-0000-0000-0000-000000000001".into()));
        assert_eq!(first.artist, "Various Artists");
        assert_eq!(first.title, "La Bush");
        assert_eq!(first.year, Some(1996));
        assert_eq!(first.country.as_deref(), Some("BE"));
        assert_eq!(first.label.as_deref(), Some("Antler-Subway"));
        assert_eq!(first.catalog_number.as_deref(), Some("TOTH 006"));
        assert_eq!(first.format.as_deref(), Some("CD"));
        assert!((first.score - 1.0).abs() < f32::EPSILON);
        assert!(first.score > second_score(&candidates));
        // No cover in search hits.
        assert_eq!(first.thumb_url, None);
        assert_eq!(first.cover_url, None);

        let second = &candidates[1];
        assert_eq!(second.year, None);
        assert_eq!(second.label, None);
        assert_eq!(second.catalog_number, None);
    }

    fn second_score(candidates: &[ReleaseCandidate]) -> f32 {
        candidates[1].score
    }

    #[test]
    fn joins_artist_credit_with_joinphrases() {
        let credit = serde_json::json!([
            { "name": "Zolex", "joinphrase": " & " },
            { "name": "Carat Trax" }
        ]);
        assert_eq!(
            join_artist_credit(Some(&credit)).as_deref(),
            Some("Zolex & Carat Trax")
        );
        // Falls back to the nested artist name when the credit has no `name`.
        let nested = serde_json::json!([{ "artist": { "name": "Aphex Twin" } }]);
        assert_eq!(join_artist_credit(Some(&nested)).as_deref(), Some("Aphex Twin"));
    }

    #[test]
    fn maps_release_with_tracks_genres_and_cover() {
        let body = r#"{
            "id": "aeb1c1c0-0000-0000-0000-000000000001",
            "title": "Music Has the Right to Children",
            "date": "1998",
            "artist-credit": [{ "name": "Boards of Canada" }],
            "genres": [{ "name": "electronic", "count": 5 }, { "name": "idm", "count": 3 }],
            "media": [
                {
                    "format": "CD",
                    "tracks": [
                        { "number": "1", "title": "Wildlife Analysis", "length": 68000 },
                        {
                            "number": "2",
                            "title": "An Eagle in Your Mind",
                            "length": 387000,
                            "artist-credit": [{ "name": "BoC" }]
                        }
                    ]
                },
                {
                    "format": "CD",
                    "tracks": [
                        { "position": 1, "recording": { "title": "Roygbiv", "length": 137000 } }
                    ]
                }
            ]
        }"#;
        let release = parse_release(body).unwrap();
        assert_eq!(release.artist, "Boards of Canada");
        assert_eq!(release.title, "Music Has the Right to Children");
        assert_eq!(release.year, Some(1998));
        assert_eq!(release.genres, vec!["electronic", "idm"]);
        assert!(release.styles.is_empty());
        assert_eq!(
            release.cover_image_url.as_deref(),
            Some("https://coverartarchive.org/release/aeb1c1c0-0000-0000-0000-000000000001/front")
        );

        assert_eq!(release.tracks.len(), 3);
        assert_eq!(release.tracks[0].position, "1");
        assert_eq!(release.tracks[0].title, "Wildlife Analysis");
        assert_eq!(release.tracks[0].duration_secs, Some(68));
        assert_eq!(release.tracks[0].artist, None);
        // Track-level artist credit is surfaced when present.
        assert_eq!(release.tracks[1].artist.as_deref(), Some("BoC"));
        // Position/title/length fall back to the numeric position and the
        // nested recording.
        assert_eq!(release.tracks[2].position, "1");
        assert_eq!(release.tracks[2].title, "Roygbiv");
        assert_eq!(release.tracks[2].duration_secs, Some(137));
    }

    #[test]
    fn status_503_is_rate_limited() {
        assert!(matches!(
            status_to_error(503, Some("2")),
            ProviderError::RateLimited { retry_after_secs: 2 }
        ));
        assert!(matches!(
            status_to_error(503, None),
            ProviderError::RateLimited { retry_after_secs: 1 }
        ));
        assert!(matches!(status_to_error(404, None), ProviderError::NotFound));
    }
}
