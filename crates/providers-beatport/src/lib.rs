//! Beatport metadata provider (#162).
//!
//! Why it exists: on digital electronic releases the general-purpose databases
//! are thin — the mix name is often missing, sub-genres are coarse, and a lot of
//! label-only digital catalogue was never entered at all. This store's catalogue
//! is exactly that material.
//!
//! Authentication: OAuth under the user's own account, see [`auth`]. Every call
//! here carries `Authorization: Bearer <access token>`; the shell keeps the
//! token fresh and hands it in the same way it hands the Discogs provider its
//! personal token, so nothing about the command layer changes.
//!
//! Rate limit: none documented, which is not the same as none enforced. The app
//! throttles this source like the others and honors a 429 with its `Retry-After`.
//!
//! Everything is digital here: releases carry no medium, no vinyl sides and one
//! "disc". So [`Release::format`] is always `Digital` and `disc_total` is
//! `Some(1)` — a stated single disc, which is what licenses the import's default
//! disc number (#157). Track positions are plain numbers.
//!
//! HTTP is blocking (`ureq`), matching the worker-thread model in
//! architecture.md, and the response mapping is pure functions over fixture JSON
//! so it is testable with no network.

pub mod auth;

use serde_json::Value;
use tagrex_core::provider::{
    FetchedImage, MetadataProvider, ProviderError, Release, ReleaseCandidate, ReleaseId,
    ReleaseImage, ReleaseLabel, ReleaseTrack, SearchQuery,
};
use tagrex_core::transform::{KeyNotation, KeyStyle, TransformStep};

pub(crate) const API_BASE: &str = "https://api.beatport.com/v4";
const SITE_BASE: &str = "https://www.beatport.com";
pub(crate) const USER_AGENT: &str = concat!(
    "TagRex/",
    env!("CARGO_PKG_VERSION"),
    " +https://github.com/tagrex/tagrex"
);

/// How many tracks to ask for per page. A release never has more, so one request
/// is enough for all but the most extreme compilations.
const TRACKS_PER_PAGE: u32 = 100;

/// The size to ask the image CDN for. Its URLs carry `{w}`/`{h}` placeholders,
/// so the resolution is ours to choose rather than something to discover.
const IMAGE_SIZE: u32 = 1400;

/// What the store sells: one digital medium, no discs, no sides.
const FORMAT: &str = "Digital";

pub struct BeatportProvider {
    agent: ureq::Agent,
    access_token: String,
}

impl BeatportProvider {
    pub fn new(access_token: impl Into<String>) -> Self {
        Self::with_proxy(access_token, None).expect("no proxy can't fail")
    }

    /// Like [`new`](Self::new) but routing requests through an HTTP/SOCKS proxy
    /// when `proxy` is a non-empty URL. Mirrors the other providers so one proxy
    /// setting covers every source.
    pub fn with_proxy(
        access_token: impl Into<String>,
        proxy: Option<&str>,
    ) -> Result<Self, ProviderError> {
        Ok(Self {
            agent: auth::agent(proxy)?,
            access_token: access_token.into(),
        })
    }

    fn get(&self, url: &str, query: &[(&str, &str)]) -> Result<String, ProviderError> {
        let mut request = self
            .agent
            .get(url)
            .header("Authorization", &format!("Bearer {}", self.access_token))
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

    /// Download a cover from the image CDN. No `Authorization` header: the CDN
    /// is public, and sending a bearer token to a host that never asked for one
    /// is how tokens leak.
    pub fn fetch_image(&self, url: &str) -> Result<FetchedImage, ProviderError> {
        let mut response = self
            .agent
            .get(url)
            .header("User-Agent", USER_AGENT)
            .call()
            .map_err(|err| ProviderError::Network(err.to_string()))?;

        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(status_to_error(status, None));
        }

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

impl MetadataProvider for BeatportProvider {
    fn id(&self) -> &'static str {
        "beatport"
    }

    fn display_name(&self) -> &'static str {
        "Beatport"
    }

    /// One free-text search box, so every field the query carries is folded into
    /// `q`. The media-type filter (#103) is deliberately ignored: everything
    /// here is digital, so filtering by Vinyl or CD could only ever return
    /// nothing.
    fn search(&self, query: &SearchQuery) -> Result<Vec<ReleaseCandidate>, ProviderError> {
        let joined = [
            query.artist.as_deref(),
            query.album.as_deref(),
            query.title.as_deref(),
            query.catalog_number.as_deref(),
        ]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
        let q = sanitize_query(&joined);

        let mut params: Vec<(&str, &str)> = vec![("q", &q), ("type", "releases")];
        let (page, per_page);
        if query.per_page > 0 {
            page = query.page.max(1).to_string();
            per_page = query.per_page.to_string();
            params.push(("page", &page));
            params.push(("per_page", &per_page));
        }

        let body = self.get(&format!("{API_BASE}/catalog/search/"), &params)?;
        parse_search_response(&body)
    }

    /// Two calls: the release itself, then its tracklist, which the release
    /// object does not carry.
    fn fetch_release(&self, id: &ReleaseId) -> Result<Release, ProviderError> {
        let release = self.get(&format!("{API_BASE}/catalog/releases/{}/", id.0), &[])?;
        let per_page = TRACKS_PER_PAGE.to_string();
        let tracks = self.get(
            &format!("{API_BASE}/catalog/releases/{}/tracks/", id.0),
            &[("per_page", per_page.as_str())],
        )?;
        parse_release(&release, &tracks)
    }
}

/// Clean a query before it goes into the store's single free-text box (#168).
///
/// The search is loose, so punctuation and physical-medium markers act as
/// content and pull in releases that merely share a word or two: `La Bush -
/// Music From The Temple Of House` came back with *Temple Of The Dog* twice in
/// the top five. The app's own re-scoring can't undo that, since it compares
/// against the same string.
///
/// So: non-alphanumeric characters become spaces, apostrophes are dropped
/// instead (`90's` is one word, not `90 s`), and `CD1` / `disc 2` — which a
/// folder name often carries and a digital store has no concept of — are
/// removed. Only this provider does it; the other two take structured fields
/// and are perfectly happy with punctuation.
fn sanitize_query(query: &str) -> String {
    let flattened: String = query
        .chars()
        .filter(|c| !matches!(c, '\'' | '\u{2019}'))
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();

    let words: Vec<&str> = flattened.split_whitespace().collect();
    let mut out: Vec<&str> = Vec::with_capacity(words.len());
    let mut index = 0;
    while index < words.len() {
        let word = words[index];
        let lower = word.to_lowercase();
        // `CD1` / `DISC2`: a medium marker with its number attached.
        let split_marker = lower
            .strip_prefix("cd")
            .or_else(|| lower.strip_prefix("disc"))
            .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()));
        if split_marker {
            index += 1;
            continue;
        }
        // `CD 1` / `disc 2`: the same, spelled apart. Only a following number
        // makes it one — a release actually called "Disc" keeps its word.
        if matches!(lower.as_str(), "cd" | "disc")
            && words
                .get(index + 1)
                .is_some_and(|next| next.bytes().all(|b| b.is_ascii_digit()))
        {
            index += 2;
            continue;
        }
        out.push(word);
        index += 1;
    }
    out.join(" ")
}

pub(crate) fn status_to_error(status: u16, retry_after: Option<&str>) -> ProviderError {
    match status {
        401 | 403 => ProviderError::Auth(format!("HTTP {status} — sign in to Beatport again")),
        404 => ProviderError::NotFound,
        429 => ProviderError::RateLimited {
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
    // A type-filtered search answers under the type's own key; an unfiltered one
    // (and the paginated list endpoints) answers under `results`.
    let results = root
        .get("releases")
        .or_else(|| root.get("results"))
        .and_then(Value::as_array)
        .ok_or_else(|| ProviderError::Other("search response missing `releases`".to_string()))?;

    Ok(results.iter().map(candidate_from).collect())
}

fn candidate_from(release: &Value) -> ReleaseCandidate {
    let label = label_of(release);
    let images = images_of(release);
    ReleaseCandidate {
        id: ReleaseId(id_of(release)),
        artist: artist_string(release.get("artists")),
        title: string(release, "name"),
        year: year_of(release),
        // Beatport states no relevance score; the app re-scores every candidate
        // against the query text anyway (#53).
        score: 0.0,
        thumb_url: images.first().map(|image| image.url.clone()),
        cover_url: images.first().map(|image| image.url.clone()),
        // Beatport is a single global store; a release has no country.
        country: None,
        label: label.as_ref().map(|label| label.name.clone()),
        format: Some(FORMAT.to_string()),
        catalog_number: label.and_then(|label| label.catalog_number),
    }
}

fn parse_release(release_body: &str, tracks_body: &str) -> Result<Release, ProviderError> {
    let release: Value =
        serde_json::from_str(release_body).map_err(|err| ProviderError::Other(err.to_string()))?;
    let tracks_root: Value =
        serde_json::from_str(tracks_body).map_err(|err| ProviderError::Other(err.to_string()))?;
    let track_values = tracks_root
        .get("results")
        .or(Some(&tracks_root))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut tracks = Vec::with_capacity(track_values.len());
    let mut genres: Vec<String> = Vec::new();
    let mut styles: Vec<String> = Vec::new();
    for (index, value) in track_values.iter().enumerate() {
        // The listing is already in release order, so the position falls back to
        // it when a track states no number of its own.
        let number = value
            .get("number")
            .and_then(Value::as_u64)
            .unwrap_or(index as u64 + 1);
        tracks.push(ReleaseTrack {
            position: number.to_string(),
            // Nothing here is multi-disc; `disc_total: Some(1)` below is what
            // tells the import it may write disc 1 (#157).
            disc: None,
            artist: Some(artist_string(value.get("artists"))).filter(|a| !a.is_empty()),
            title: track_title(value),
            duration_secs: value
                .get("length_ms")
                .and_then(Value::as_u64)
                .map(|ms| ms / 1000)
                .filter(|secs| *secs > 0),
            isrc: value
                .get("isrc")
                .and_then(Value::as_str)
                .map(str::to_string)
                .filter(|isrc| !isrc.is_empty()),
            bpm: value
                .get("bpm")
                .and_then(Value::as_u64)
                .filter(|bpm| *bpm > 0)
                .map(|bpm| bpm as u16),
            key: track_key(value),
        });
        // Genre lives on the track, not the release; collected in listing order,
        // deduplicated, so a release that mixes two styles keeps both.
        push_unique(&mut genres, named(value.get("genre")));
        push_unique(&mut styles, named(value.get("sub_genre")));
    }

    let label = label_of(&release);
    let images = images_of(&release);
    Ok(Release {
        id: ReleaseId(id_of(&release)),
        artist: artist_string(release.get("artists")),
        title: string(&release, "name"),
        year: year_of(&release),
        genres,
        styles,
        tracks,
        labels: label.into_iter().collect(),
        country: None,
        format: Some(FORMAT.to_string()),
        disc_total: Some(1),
        url: release
            .get("slug")
            .and_then(Value::as_str)
            .map(|slug| format!("{SITE_BASE}/release/{slug}/{}", id_of(&release))),
        cover_image_url: images.first().map(|image| image.url.clone()),
        images,
    })
}

/// The title as the store shows it: the name with the mix in parentheses. The
/// mix name is the whole reason to look a digital release up here, so it is kept
/// even when it is the unremarkable "Original Mix" — that is the convention the
/// files are expected to follow, and dropping it silently would make two
/// different mixes of one track indistinguishable.
fn track_title(track: &Value) -> String {
    let name = string(track, "name");
    match track.get("mix_name").and_then(Value::as_str) {
        Some(mix) if !mix.trim().is_empty() => format!("{name} ({})", mix.trim()),
        _ => name,
    }
}

/// The key in the compact spelling a tag wants. The store publishes it as
/// `A Minor` / `Ab Major`, which is a display string, not a tag value — so it
/// goes through the core key transform, the same one the GENERATOR offers, and
/// comes out as `Am` / `Ab`. A spelling the transform doesn't recognise is left
/// exactly as stated rather than dropped.
fn track_key(track: &Value) -> Option<String> {
    let name = named(track.get("key"))?;
    Some(KeyNotation::new(KeyStyle::Musical).apply(&name))
}

/// Artists joined the way a tag wants them. Beatport lists every credited artist
/// separately, including on compilations where that would produce a wall of
/// names, so a long list collapses to the usual placeholder.
fn artist_string(artists: Option<&Value>) -> String {
    let names: Vec<String> = artists
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(|a| named(Some(a))).collect())
        .unwrap_or_default();
    if names.len() >= VARIOUS_ARTISTS_THRESHOLD {
        return "Various Artists".to_string();
    }
    names.join(", ")
}

/// From how many credited artists a release reads as a compilation.
const VARIOUS_ARTISTS_THRESHOLD: usize = 4;

fn label_of(release: &Value) -> Option<ReleaseLabel> {
    let name = named(release.get("label"))?;
    Some(ReleaseLabel {
        name,
        catalog_number: release
            .get("catalog_number")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|catno| !catno.is_empty())
            .map(str::to_string),
    })
}

/// A release carries one cover. The CDN URL is templated with the size, so ask
/// for a large one; a response that only states a fixed `uri` is taken as is,
/// with unknown dimensions.
fn images_of(release: &Value) -> Vec<ReleaseImage> {
    let Some(image) = release.get("image") else {
        return Vec::new();
    };
    if let Some(dynamic) = image.get("dynamic_uri").and_then(Value::as_str) {
        if dynamic.contains("{w}") || dynamic.contains("{h}") {
            return vec![ReleaseImage {
                url: dynamic
                    .replace("{w}", &IMAGE_SIZE.to_string())
                    .replace("{h}", &IMAGE_SIZE.to_string()),
                width: IMAGE_SIZE,
                height: IMAGE_SIZE,
            }];
        }
    }
    image
        .get("uri")
        .and_then(Value::as_str)
        .filter(|uri| !uri.is_empty())
        .map(|uri| {
            vec![ReleaseImage {
                url: uri.to_string(),
                width: image.get("width").and_then(Value::as_u64).unwrap_or(0) as u32,
                height: image.get("height").and_then(Value::as_u64).unwrap_or(0) as u32,
            }]
        })
        .unwrap_or_default()
}

/// The release year. `publish_date` is the store's own release date and is what
/// the year tag should carry; `new_release_date` is the fallback for older
/// catalogue entries that state only that.
fn year_of(release: &Value) -> Option<u16> {
    ["publish_date", "new_release_date", "release_date"]
        .iter()
        .find_map(|key| release.get(*key).and_then(Value::as_str))
        .and_then(|date| date.get(0..4).and_then(|year| year.parse().ok()))
}

fn id_of(value: &Value) -> String {
    value
        .get("id")
        .map(|id| match id {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        })
        .unwrap_or_default()
}

fn string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// The `name` of a `{id, name}` object, which is how the API states every
/// artist, label, genre and sub-genre.
fn named(value: Option<&Value>) -> Option<String> {
    value
        .and_then(|value| value.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

fn push_unique(list: &mut Vec<String>, value: Option<String>) {
    if let Some(value) = value {
        if !list.contains(&value) {
            list.push(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEARCH_JSON: &str = r#"{
      "releases": [
        {
          "id": 4321,
          "name": "Test EP",
          "slug": "test-ep",
          "artists": [{"id": 1, "name": "Alpha"}],
          "label": {"id": 9, "name": "Test Records"},
          "catalog_number": "TR001",
          "publish_date": "2019-03-15",
          "image": {"id": 5, "uri": "https://cdn/plain.jpg",
                    "dynamic_uri": "https://cdn/{w}x{h}/img.jpg"}
        }
      ]
    }"#;

    const RELEASE_JSON: &str = r#"{
      "id": 4321,
      "name": "Test EP",
      "slug": "test-ep",
      "artists": [{"id": 1, "name": "Alpha"}, {"id": 2, "name": "Beta"}],
      "label": {"id": 9, "name": "Test Records"},
      "catalog_number": "TR001",
      "publish_date": "2019-03-15",
      "image": {"uri": "https://cdn/plain.jpg", "dynamic_uri": "https://cdn/{w}x{h}/img.jpg"}
    }"#;

    const TRACKS_JSON: &str = r#"{
      "count": 2,
      "results": [
        {
          "id": 11, "name": "First", "mix_name": "Original Mix", "number": 1,
          "artists": [{"id": 1, "name": "Alpha"}],
          "length_ms": 384000, "isrc": "GBABC1900001",
          "bpm": 132, "key": {"id": 3, "name": "A Minor"},
          "genre": {"id": 5, "name": "Techno"},
          "sub_genre": {"id": 51, "name": "Peak Time"}
        },
        {
          "id": 12, "name": "Second", "mix_name": "Beta Remix", "number": 2,
          "artists": [{"id": 1, "name": "Alpha"}, {"id": 2, "name": "Beta"}],
          "length_ms": 0,
          "genre": {"id": 5, "name": "Techno"},
          "sub_genre": {"id": 52, "name": "Hypnotic"}
        }
      ]
    }"#;

    #[test]
    fn a_query_loses_its_punctuation_and_medium_markers() {
        // The one that started #168: the separator was matching as content.
        assert_eq!(
            sanitize_query("La Bush - Music From The Temple Of House"),
            "La Bush Music From The Temple Of House"
        );
        assert_eq!(
            sanitize_query("Artist – Title [Remixes]"),
            "Artist Title Remixes"
        );
        // A disc marker names something a digital store doesn't have, in either
        // spelling.
        assert_eq!(sanitize_query("Some Album CD1"), "Some Album");
        assert_eq!(sanitize_query("Some Album (disc 2)"), "Some Album");
        // But a word that merely starts like one is a word.
        assert_eq!(sanitize_query("Disc Jockey"), "Disc Jockey");
        assert_eq!(sanitize_query("CDX Machine"), "CDX Machine");
        // An apostrophe joins rather than splits, so this stays two tokens.
        assert_eq!(sanitize_query("90's Rave"), "90s Rave");
        // Non-Latin text is words too, and a query of pure punctuation is empty
        // rather than a string of spaces.
        assert_eq!(sanitize_query("Кристалл — Мечта"), "Кристалл Мечта");
        assert_eq!(sanitize_query("  ---  "), "");
    }

    #[test]
    fn maps_a_search_hit() {
        let hits = parse_search_response(SEARCH_JSON).unwrap();
        assert_eq!(hits.len(), 1);
        let hit = &hits[0];
        assert_eq!(hit.id, ReleaseId("4321".to_string()));
        assert_eq!(hit.artist, "Alpha");
        assert_eq!(hit.title, "Test EP");
        assert_eq!(hit.year, Some(2019));
        assert_eq!(hit.label.as_deref(), Some("Test Records"));
        assert_eq!(hit.catalog_number.as_deref(), Some("TR001"));
        assert_eq!(hit.format.as_deref(), Some("Digital"));
        // The templated CDN URL wins over the fixed one, at the size we ask for.
        assert_eq!(
            hit.cover_url.as_deref(),
            Some("https://cdn/1400x1400/img.jpg")
        );
    }

    #[test]
    fn a_numeric_id_becomes_the_string_the_app_addresses_it_by() {
        // The API states ids as numbers; every id in the app is a string, and
        // `4321.0` or `"4321"` with quotes would both break the release URL.
        let hits = parse_search_response(SEARCH_JSON).unwrap();
        assert_eq!(hits[0].id.0, "4321");
    }

    #[test]
    fn maps_a_release_with_its_tracklist() {
        let release = parse_release(RELEASE_JSON, TRACKS_JSON).unwrap();
        assert_eq!(release.title, "Test EP");
        assert_eq!(release.artist, "Alpha, Beta");
        assert_eq!(release.year, Some(2019));
        assert_eq!(release.labels.len(), 1);
        assert_eq!(release.labels[0].name, "Test Records");
        assert_eq!(release.labels[0].catalog_number.as_deref(), Some("TR001"));
        assert_eq!(
            release.url.as_deref(),
            Some("https://www.beatport.com/release/test-ep/4321")
        );
        // Genre from the tracks, deduplicated; sub-genres stay separate as the
        // more specific value (the same split as Discogs genres vs styles).
        assert_eq!(release.genres, vec!["Techno".to_string()]);
        assert_eq!(
            release.styles,
            vec!["Peak Time".to_string(), "Hypnotic".to_string()]
        );

        assert_eq!(release.tracks.len(), 2);
        assert_eq!(release.tracks[0].position, "1");
        assert_eq!(release.tracks[0].title, "First (Original Mix)");
        assert_eq!(release.tracks[0].artist.as_deref(), Some("Alpha"));
        assert_eq!(release.tracks[0].duration_secs, Some(384));
        assert_eq!(release.tracks[0].isrc.as_deref(), Some("GBABC1900001"));
        assert_eq!(release.tracks[1].title, "Second (Beta Remix)");
        assert_eq!(release.tracks[1].artist.as_deref(), Some("Alpha, Beta"));
        // A missing length is unknown, not zero.
        assert_eq!(release.tracks[1].duration_secs, None);
        assert_eq!(release.tracks[1].isrc, None);
    }

    #[test]
    fn maps_the_tempo_and_the_key_that_only_this_source_states() {
        let release = parse_release(RELEASE_JSON, TRACKS_JSON).unwrap();
        assert_eq!(release.tracks[0].bpm, Some(132));
        // "A Minor" is a display string; the tag wants the compact spelling.
        assert_eq!(release.tracks[0].key.as_deref(), Some("Am"));
        // A track that states neither says nothing, rather than 0 / "".
        assert_eq!(release.tracks[1].bpm, None);
        assert_eq!(release.tracks[1].key, None);
    }

    #[test]
    fn a_key_spelling_we_do_not_model_is_kept_verbatim() {
        let tracks = r#"{"results":[{"id":1,"name":"A","key":{"name":"Phrygian"},"bpm":0}]}"#;
        let release = parse_release(RELEASE_JSON, tracks).unwrap();
        assert_eq!(release.tracks[0].key.as_deref(), Some("Phrygian"));
        // A stated zero tempo is not a tempo.
        assert_eq!(release.tracks[0].bpm, None);
    }

    #[test]
    fn everything_is_one_digital_disc() {
        let release = parse_release(RELEASE_JSON, TRACKS_JSON).unwrap();
        assert_eq!(release.format.as_deref(), Some("Digital"));
        // A stated single disc (#157) — and no track claims a disc of its own,
        // because a digital release has none to claim.
        assert_eq!(release.disc_total, Some(1));
        assert!(release.tracks.iter().all(|track| track.disc.is_none()));
    }

    #[test]
    fn a_track_without_a_mix_name_keeps_its_plain_title() {
        let tracks = r#"{"results":[{"id":1,"name":"Plain","number":1}]}"#;
        let release = parse_release(RELEASE_JSON, tracks).unwrap();
        assert_eq!(release.tracks[0].title, "Plain");
        assert_eq!(release.tracks[0].artist, None);
    }

    #[test]
    fn a_crowded_credit_list_reads_as_a_compilation() {
        let release = r#"{"id":1,"name":"V/A","artists":[{"name":"A"},{"name":"B"},
                         {"name":"C"},{"name":"D"}]}"#;
        let release = parse_release(release, r#"{"results":[]}"#).unwrap();
        assert_eq!(release.artist, "Various Artists");
    }

    #[test]
    fn tracks_fall_back_to_listing_order_when_unnumbered() {
        let tracks = r#"{"results":[{"id":1,"name":"A"},{"id":2,"name":"B"}]}"#;
        let release = parse_release(RELEASE_JSON, tracks).unwrap();
        assert_eq!(release.tracks[0].position, "1");
        assert_eq!(release.tracks[1].position, "2");
    }

    #[test]
    fn a_release_without_art_is_not_an_error() {
        let release = r#"{"id":7,"name":"Bare"}"#;
        let release = parse_release(release, r#"{"results":[]}"#).unwrap();
        assert!(release.images.is_empty());
        assert_eq!(release.cover_image_url, None);
        assert_eq!(release.labels.len(), 0);
        assert_eq!(release.year, None);
        assert_eq!(release.url, None);
    }

    #[test]
    fn an_unexpected_search_body_is_reported_not_guessed() {
        assert!(matches!(
            parse_search_response(r#"{"detail":"Not found."}"#),
            Err(ProviderError::Other(_))
        ));
    }

    #[test]
    fn an_auth_failure_says_to_sign_in_again() {
        assert!(matches!(status_to_error(401, None), ProviderError::Auth(_)));
        assert!(matches!(
            status_to_error(404, None),
            ProviderError::NotFound
        ));
        assert!(matches!(
            status_to_error(429, Some("30")),
            ProviderError::RateLimited {
                retry_after_secs: 30
            }
        ));
    }
}
