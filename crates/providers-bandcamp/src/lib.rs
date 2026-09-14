//! Bandcamp metadata provider.
//!
//! Authentication: none. Bandcamp has no public metadata API (the old catalog
//! API was retired years ago), so this provider reads two surfaces that the
//! site itself serves as data rather than scraping rendered HTML:
//!
//! - **Search** hits Bandcamp's own autocomplete endpoint
//!   (`/api/bcsearch_public_api/1/autocomplete_elastic`), which answers a plain
//!   query with JSON — the same call the site's search box makes. The public
//!   `/search` *page* is JS-rendered and carries no results in its HTML, so this
//!   is the robust path.
//! - **Release** reads the album page's embedded data: the `data-tralbum`
//!   attribute (artist, tracks with durations, art id, release date) and the
//!   `application/ld+json` `MusicAlbum` block (genre keywords, release format).
//!   Both are structured JSON the page embeds for its own player and for
//!   search engines, so this is far steadier than scraping the visible markup.
//!
//! A [`ReleaseId`] here is the album's page URL — Bandcamp identifies a release
//! by its URL, and [`fetch_release`](BandcampProvider::fetch_release) fetches it
//! directly. [`search`](BandcampProvider::search) also recognises a pasted
//! Bandcamp URL and turns it into a single candidate, since "found it by hand,
//! now tag it" is the common case for a release that is on no other provider.
//!
//! Fields Bandcamp does not carry structurally are left empty rather than
//! guessed: ISRC, BPM and musical key are absent everywhere; a catalogue number
//! and the label appear only in prose (the page owner in the metadata is the
//! Bandcamp account, not necessarily the release's label), so neither is read.
//!
//! HTTP is blocking (`ureq`), matching the worker-thread model in
//! architecture.md. The response-mapping logic is pure functions so it can be
//! unit-tested against fixtures with no network.

use serde_json::{json, Value};
use tagrex_core::provider::{
    FetchedImage, MetadataProvider, ProviderError, Release, ReleaseCandidate, ReleaseId,
    ReleaseImage, ReleaseTrack, SearchQuery,
};

const SEARCH_API: &str = "https://bandcamp.com/api/bcsearch_public_api/1/autocomplete_elastic";
const IMG_BASE: &str = "https://f4.bcbits.com/img";
/// A desktop browser User-Agent. Bandcamp serves these surfaces to ordinary
/// browsers; a plain library agent risks being filtered, and unlike MusicBrainz
/// Bandcamp asks for no descriptive agent of its own.
const USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) \
     Chrome/124.0.0.0 Safari/537.36";

pub struct BandcampProvider {
    agent: ureq::Agent,
}

impl BandcampProvider {
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
        // Status-as-error off: keep the response for every status so a 429/503
        // can be read for its Retry-After header rather than collapsing into an
        // opaque error.
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

    fn get(&self, url: &str) -> Result<String, ProviderError> {
        let mut response = self
            .agent
            .get(url)
            .header("User-Agent", USER_AGENT)
            .call()
            .map_err(|err| ProviderError::Network(err.to_string()))?;
        read_ok(&mut response)
    }

    fn post_json(&self, url: &str, body: &Value) -> Result<String, ProviderError> {
        let payload = body.to_string();
        let mut response = self
            .agent
            .post(url)
            .header("User-Agent", USER_AGENT)
            .header("Content-Type", "application/json")
            .send(payload.as_bytes())
            .map_err(|err| ProviderError::Network(err.to_string()))?;
        read_ok(&mut response)
    }

    /// Download a cover image from a Bandcamp CDN URL (`f4.bcbits.com`). No auth
    /// and no special headers beyond the User-Agent. Not on [`MetadataProvider`]
    /// for the same reason as the other providers: image fetching is
    /// provider-specific plumbing and the app holds the concrete provider.
    pub fn fetch_image(&self, url: &str) -> Result<FetchedImage, ProviderError> {
        let mut response = self
            .agent
            .get(url)
            .header("User-Agent", USER_AGENT)
            .call()
            .map_err(|err| ProviderError::Network(err.to_string()))?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(status_to_error(status, retry_after(&response)));
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

impl Default for BandcampProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl MetadataProvider for BandcampProvider {
    fn id(&self) -> &'static str {
        "bandcamp"
    }

    fn display_name(&self) -> &'static str {
        "Bandcamp"
    }

    fn search(&self, query: &SearchQuery) -> Result<Vec<ReleaseCandidate>, ProviderError> {
        // The single search box maps to `album` as free text (see the frontend).
        let text = query.album.as_deref().unwrap_or("").trim();
        if text.is_empty() {
            return Ok(Vec::new());
        }
        // A pasted release URL is a direct hit — fetch that page and present it
        // as the one candidate, so its card shows the real artist/title/cover.
        if is_bandcamp_url(text) {
            return match self.fetch_release(&ReleaseId(text.to_string())) {
                Ok(release) => Ok(vec![release_to_candidate(&release)]),
                // A URL that doesn't parse as a release is simply no result.
                Err(ProviderError::NotFound) => Ok(Vec::new()),
                Err(other) => Err(other),
            };
        }
        // The autocomplete endpoint returns a single ranked set, not pages, so a
        // request past the first page has nothing to add.
        if query.page > 1 {
            return Ok(Vec::new());
        }
        let body = self.post_json(
            SEARCH_API,
            &json!({
                "search_text": text,
                "search_filter": "a", // albums
                "full_page": false,
                "fan_id": Value::Null,
            }),
        )?;
        parse_search(&body)
    }

    fn fetch_release(&self, id: &ReleaseId) -> Result<Release, ProviderError> {
        let html = self.get(&id.0)?;
        parse_release(&html, &id.0)
    }
}

/// Read a `ureq` response as a string, mapping a non-2xx status to the matching
/// [`ProviderError`].
fn read_ok(response: &mut ureq::http::Response<ureq::Body>) -> Result<String, ProviderError> {
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(status_to_error(status, retry_after(response)));
    }
    response
        .body_mut()
        .read_to_string()
        .map_err(|err| ProviderError::Network(err.to_string()))
}

fn retry_after(response: &ureq::http::Response<ureq::Body>) -> Option<u64> {
    response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
}

fn status_to_error(status: u16, retry_after_secs: Option<u64>) -> ProviderError {
    match status {
        429 | 503 => ProviderError::RateLimited {
            retry_after_secs: retry_after_secs.unwrap_or(1),
        },
        404 => ProviderError::NotFound,
        401 | 403 => ProviderError::Auth(format!("HTTP {status}")),
        other => ProviderError::Other(format!("HTTP {other}")),
    }
}

/// Whether the text is a Bandcamp release URL (an album or a single track),
/// rather than a free-text search.
fn is_bandcamp_url(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.starts_with("http")
        && lower.contains("bandcamp.com/")
        && (lower.contains("/album/") || lower.contains("/track/"))
}

/// Parse the autocomplete endpoint's JSON into album candidates. Non-album
/// results (artists, tracks) are dropped — the picker imports releases.
fn parse_search(body: &str) -> Result<Vec<ReleaseCandidate>, ProviderError> {
    let value: Value =
        serde_json::from_str(body).map_err(|err| ProviderError::Other(err.to_string()))?;
    let results = value
        .get("auto")
        .and_then(|auto| auto.get("results"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let candidates = results
        .iter()
        .filter(|result| result.get("type").and_then(Value::as_str) == Some("a"))
        .filter_map(|result| {
            let url = str_field(result, "item_url_path")?;
            let art_id = result.get("art_id").and_then(Value::as_u64);
            Some(ReleaseCandidate {
                id: ReleaseId(url),
                artist: str_field(result, "band_name").unwrap_or_default(),
                title: str_field(result, "name").unwrap_or_default(),
                year: None, // autocomplete states no year; the release fetch has it
                score: 0.0, // the app re-scores against the query text (#53)
                // Both thumbnails come from the art id, not the autocomplete's
                // own `img` field: that field is missing the `a` prefix the CDN
                // needs (`0702…_3.jpg` 404s; `a0702…_3.jpg` is the real URL), so
                // the collapsed card showed only a placeholder (#359).
                thumb_url: art_id.map(|art| cover_url(art, 3)),
                cover_url: art_id.map(|art| cover_url(art, 16)),
                country: None,
                label: None,
                format: None,
                catalog_number: None,
            })
        })
        .collect();
    Ok(candidates)
}

/// Parse a release from an album page's embedded data. `url` is the page we
/// fetched, used as the release id and as a fallback for the release URL.
fn parse_release(html: &str, url: &str) -> Result<Release, ProviderError> {
    let tralbum = extract_data_tralbum(html)
        .ok_or_else(|| ProviderError::Other("no data-tralbum on the page".to_string()))?;
    // The JSON-LD block is a bonus (genres, format); its absence is not fatal.
    let ld = extract_ld_json(html);

    let current = tralbum.get("current");
    let title = current
        .and_then(|current| str_field(current, "title"))
        .or_else(|| ld.as_ref().and_then(|ld| str_field(ld, "name")))
        .unwrap_or_default();
    let artist = str_field(&tralbum, "artist")
        .or_else(|| {
            ld.as_ref()
                .and_then(|ld| ld.get("byArtist"))
                .and_then(|by| str_field(by, "name"))
        })
        .unwrap_or_default();
    let release_date = current
        .and_then(|current| str_field(current, "release_date"))
        .or_else(|| str_field(&tralbum, "album_release_date"))
        .or_else(|| ld.as_ref().and_then(|ld| str_field(ld, "datePublished")));
    let year = release_date.as_deref().and_then(year_from_date);

    let art_id = tralbum
        .get("art_id")
        .and_then(Value::as_u64)
        .or_else(|| ld.as_ref().and_then(art_id_from_ld));
    let cover_image_url = art_id.map(|art| cover_url(art, 0));

    let genres = ld
        .as_ref()
        .and_then(|ld| ld.get("keywords"))
        .and_then(Value::as_array)
        .map(|keywords| {
            keywords
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    let format = ld.as_ref().and_then(release_format);

    let tracks = tralbum
        .get("trackinfo")
        .and_then(Value::as_array)
        .map(|tracks| tracks.iter().map(parse_track).collect())
        .unwrap_or_default();

    let release_url = str_field(&tralbum, "url").unwrap_or_else(|| url.to_string());

    Ok(Release {
        id: ReleaseId(url.to_string()),
        artist,
        title,
        year,
        genres,
        styles: Vec::new(), // Bandcamp has no Discogs-style curated styles
        tracks,
        labels: Vec::new(), // not carried structurally (see the module note)
        country: None,
        format,
        disc_total: None,
        url: Some(release_url),
        cover_image_url: cover_image_url.clone(),
        images: cover_image_url
            .map(|url| {
                vec![ReleaseImage {
                    url,
                    width: 0,
                    height: 0,
                }]
            })
            .unwrap_or_default(),
    })
}

/// One `trackinfo` entry. Bandcamp gives a plain 1-based track number and a
/// duration in fractional seconds; the title carries the per-track artist
/// inline on split releases (`"Artist - Title"`), which is left verbatim rather
/// than split on a `-` that a real title may contain.
fn parse_track(track: &Value) -> ReleaseTrack {
    let position = track
        .get("track_num")
        .and_then(Value::as_u64)
        .map(|num| num.to_string())
        .unwrap_or_default();
    let duration_secs = track
        .get("duration")
        .and_then(Value::as_f64)
        .filter(|secs| *secs > 0.0)
        .map(|secs| secs.round() as u64);
    ReleaseTrack {
        position,
        disc: None,
        artist: None,
        title: str_field(track, "title").unwrap_or_default(),
        duration_secs,
        isrc: None,
        bpm: None,
        key: None,
        section: None,
    }
}

/// A release candidate view of a fully fetched release, for the pasted-URL path.
fn release_to_candidate(release: &Release) -> ReleaseCandidate {
    ReleaseCandidate {
        id: release.id.clone(),
        artist: release.artist.clone(),
        title: release.title.clone(),
        year: release.year,
        score: 1.0, // a direct URL hit is as exact as it gets
        thumb_url: release.cover_image_url.clone(),
        cover_url: release.cover_image_url.clone(),
        country: release.country.clone(),
        label: release.labels.first().map(|label| label.name.clone()),
        format: release.format.clone(),
        catalog_number: None,
    }
}

/// The `f4.bcbits.com` cover URL for an art id at a given size code (`0` = the
/// original, `10` ≈ 1200px, `16` ≈ 700px). The id is zero-padded to ten digits
/// behind an `a`, which is the album-art URL scheme.
fn cover_url(art_id: u64, size: u32) -> String {
    format!("{IMG_BASE}/a{art_id:010}_{size}.jpg")
}

/// Pull the ten-digit art id back out of a JSON-LD image URL
/// (`…/a0702813439_10.jpg` → `702813439`), for the rare page with no
/// `data-tralbum` art id.
fn art_id_from_ld(ld: &Value) -> Option<u64> {
    let image = match ld.get("image")? {
        Value::String(url) => url.clone(),
        Value::Array(urls) => urls.first().and_then(Value::as_str)?.to_string(),
        _ => return None,
    };
    let file = image.rsplit('/').next()?;
    let digits: String = file
        .trim_start_matches('a')
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// Map the JSON-LD `albumRelease[0].musicReleaseFormat` to a media descriptor
/// the app understands (`File` for a digital release, otherwise the medium).
fn release_format(ld: &Value) -> Option<String> {
    let format = ld
        .get("albumRelease")
        .and_then(Value::as_array)
        .and_then(|releases| releases.first())
        .and_then(|release| str_field(release, "musicReleaseFormat"))?;
    let mapped = match format.as_str() {
        "DigitalFormat" => "File",
        "VinylFormat" => "Vinyl",
        "CDFormat" => "CD",
        "CassetteFormat" => "Cassette",
        _ => return None,
    };
    Some(mapped.to_string())
}

/// The four-digit year out of a Bandcamp date string (`"04 Mar 2024 00:00:00
/// GMT"` → `2024`). Tolerant: the first whitespace token that reads as a
/// plausible year wins, so a leading day or a bare year both work.
fn year_from_date(date: &str) -> Option<u16> {
    date.split_whitespace()
        .filter_map(|token| token.parse::<u16>().ok())
        .find(|year| (1900..=2100).contains(year))
}

/// Extract and parse the `data-tralbum` attribute's JSON. The value sits in a
/// double-quoted attribute with its own quotes HTML-escaped as `&quot;`, so it
/// is un-escaped before parsing.
fn extract_data_tralbum(html: &str) -> Option<Value> {
    let marker = "data-tralbum=\"";
    let start = html.find(marker)? + marker.len();
    let end = start + html[start..].find('"')?;
    let json = html_unescape(&html[start..end]);
    serde_json::from_str(&json).ok()
}

/// Extract and parse the first `application/ld+json` block. Its content is raw
/// JSON (not entity-escaped).
fn extract_ld_json(html: &str) -> Option<Value> {
    let marker = "application/ld+json";
    let after_marker = html.find(marker)? + marker.len();
    let open = after_marker + html[after_marker..].find('>')? + 1;
    let end = open + html[open..].find("</script>")?;
    serde_json::from_str(html[open..end].trim()).ok()
}

/// Decode the handful of HTML entities Bandcamp's `data-tralbum` attribute uses.
/// `&amp;` is decoded last so an already-decoded `&` can't be misread.
fn html_unescape(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#039;", "'")
        .replace("&gt;", ">")
        .replace("&lt;", "<")
        .replace("&amp;", "&")
}

fn str_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_a_pasted_release_url() {
        assert!(is_bandcamp_url(
            "https://madagain.bandcamp.com/album/kc001-crossbreeds"
        ));
        assert!(is_bandcamp_url("http://x.bandcamp.com/track/a-single"));
        assert!(!is_bandcamp_url("crossbreeds mad again"));
        assert!(!is_bandcamp_url("https://madagain.bandcamp.com")); // no /album/ or /track/
    }

    #[test]
    fn parses_the_autocomplete_search_json() {
        let body = r#"{
            "auto": { "results": [
              { "type": "b", "name": "Some Band", "band_name": "Some Band", "item_url_path": "https://x.bandcamp.com" },
              { "type": "a", "id": 1, "art_id": 702813439, "name": "KC001 - CROSSBREEDS",
                "band_name": "Mad.Again & Max Stedeford",
                "item_url_path": "https://madagain.bandcamp.com/album/kc001-crossbreeds",
                "img": "https://f4.bcbits.com/img/0702813439_3.jpg" }
            ] }
        }"#;
        let hits = parse_search(body).unwrap();
        // The band result is dropped; only the album survives.
        assert_eq!(hits.len(), 1);
        let hit = &hits[0];
        assert_eq!(
            hit.id.0,
            "https://madagain.bandcamp.com/album/kc001-crossbreeds"
        );
        assert_eq!(hit.artist, "Mad.Again & Max Stedeford");
        assert_eq!(hit.title, "KC001 - CROSSBREEDS");
        // The thumbnail is built from the art id with the `a` prefix the CDN
        // needs, not from the autocomplete's own (broken) `img` field (#359).
        assert_eq!(
            hit.thumb_url.as_deref(),
            Some("https://f4.bcbits.com/img/a0702813439_3.jpg")
        );
        assert_eq!(
            hit.cover_url.as_deref(),
            Some("https://f4.bcbits.com/img/a0702813439_16.jpg")
        );
    }

    #[test]
    fn parses_a_release_from_embedded_page_data() {
        // A minimal album page: the data-tralbum attribute (entity-escaped, as on
        // the real page) plus a JSON-LD MusicAlbum for the genres and format.
        let html = r#"<html><head>
          <script type="application/ld+json">
            { "@type": "MusicAlbum", "name": "KC001 - CROSSBREEDS",
              "keywords": ["Electronic", "Tech House"],
              "albumRelease": [ { "musicReleaseFormat": "DigitalFormat" } ] }
          </script>
          <script data-tralbum="{&quot;artist&quot;:&quot;Mad.Again &amp; Max Stedeford&quot;,&quot;art_id&quot;:702813439,&quot;url&quot;:&quot;https://madagain.bandcamp.com/album/kc001-crossbreeds&quot;,&quot;current&quot;:{&quot;title&quot;:&quot;KC001 - CROSSBREEDS&quot;,&quot;release_date&quot;:&quot;04 Mar 2024 00:00:00 GMT&quot;},&quot;trackinfo&quot;:[{&quot;track_num&quot;:1,&quot;title&quot;:&quot;Mad.Again - Overdrive&quot;,&quot;duration&quot;:428.295},{&quot;track_num&quot;:2,&quot;title&quot;:&quot;Max Stedeford - Sub&quot;,&quot;duration&quot;:417.0}]}"></script>
          </head></html>"#;
        let release = parse_release(
            html,
            "https://madagain.bandcamp.com/album/kc001-crossbreeds",
        )
        .unwrap();
        assert_eq!(release.artist, "Mad.Again & Max Stedeford"); // &amp; decoded
        assert_eq!(release.title, "KC001 - CROSSBREEDS");
        assert_eq!(release.year, Some(2024));
        assert_eq!(release.genres, vec!["Electronic", "Tech House"]);
        assert_eq!(release.format.as_deref(), Some("File"));
        assert_eq!(
            release.cover_image_url.as_deref(),
            Some("https://f4.bcbits.com/img/a0702813439_0.jpg")
        );
        assert_eq!(release.tracks.len(), 2);
        assert_eq!(release.tracks[0].position, "1");
        assert_eq!(release.tracks[0].title, "Mad.Again - Overdrive");
        assert_eq!(release.tracks[0].duration_secs, Some(428)); // 428.295 rounded
        assert_eq!(release.tracks[1].duration_secs, Some(417));
        // Fields Bandcamp doesn't carry are empty, not guessed.
        assert!(release.tracks[0].isrc.is_none());
        assert!(release.labels.is_empty());
    }

    #[test]
    fn art_id_falls_back_to_the_json_ld_image() {
        let ld = serde_json::json!({ "image": "https://f4.bcbits.com/img/a0702813439_10.jpg" });
        assert_eq!(art_id_from_ld(&ld), Some(702813439));
    }

    #[test]
    fn year_parses_out_of_a_bandcamp_date() {
        assert_eq!(year_from_date("04 Mar 2024 00:00:00 GMT"), Some(2024));
        assert_eq!(year_from_date("2019"), Some(2019));
        assert_eq!(year_from_date(""), None);
    }
}
