//! Soundeo metadata provider.
//!
//! Authentication: none — Soundeo's release and search pages are readable
//! anonymously. It has no metadata API, so this provider scrapes two pages:
//!
//! - **Search** reads `/search?q=…`. Its "Search results" section is a list of
//!   track rows (`.trackitem`), each naming the track, its release and a
//!   duration; the rows are folded to their releases (the picker imports
//!   releases, and one release can list several tracks).
//! - **Release** reads the album page. The release's own facts live in a small
//!   two-column table (`<tr><td>key</td><td>value</td></tr>`) keyed by label —
//!   Release, Artists, Label, Styles, Date, Catalog # — which is the steady
//!   part; the cover and canonical URL come from the OpenGraph tags, and the
//!   tracklist from the page's `.trackitem` rows that point back at this
//!   release.
//!
//! A [`ReleaseId`] here is the release page's absolute URL. Because Soundeo has
//! no clean structured blob (unlike Bandcamp's `data-tralbum`), this is the most
//! markup-dependent of the providers; the mapping is pure functions over
//! fixtures so a layout change is caught by a failing test rather than in the
//! field.
//!
//! Fields Soundeo doesn't publish are left empty: BPM and musical key are absent
//! on the site. The "Catalog #" it lists is the release barcode (a UPC/EAN),
//! carried through as the catalogue number since it is the release's own stated
//! identifier.
//!
//! HTTP is blocking (`ureq`), matching the worker-thread model in
//! architecture.md.

use std::sync::LazyLock;

use regex::Regex;
use tagrex_core::provider::{
    FetchedImage, MetadataProvider, ProviderError, Release, ReleaseCandidate, ReleaseId,
    ReleaseImage, ReleaseLabel, ReleaseTrack, SearchQuery,
};

const BASE: &str = "https://soundeo.com";
const USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) \
     Chrome/124.0.0.0 Safari/537.36";

pub struct SoundeoProvider {
    agent: ureq::Agent,
}

impl SoundeoProvider {
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
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(status_to_error(status));
        }
        response
            .body_mut()
            .read_to_string()
            .map_err(|err| ProviderError::Network(err.to_string()))
    }

    /// Download a cover image from Soundeo's CDN (`covers.sndstatic.com`). No
    /// auth. Not on [`MetadataProvider`], like the other providers.
    pub fn fetch_image(&self, url: &str) -> Result<FetchedImage, ProviderError> {
        let mut response = self
            .agent
            .get(url)
            .header("User-Agent", USER_AGENT)
            .call()
            .map_err(|err| ProviderError::Network(err.to_string()))?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(status_to_error(status));
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

impl Default for SoundeoProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl MetadataProvider for SoundeoProvider {
    fn id(&self) -> &'static str {
        "soundeo"
    }

    fn display_name(&self) -> &'static str {
        "Soundeo"
    }

    fn search(&self, query: &SearchQuery) -> Result<Vec<ReleaseCandidate>, ProviderError> {
        let text = query.album.as_deref().unwrap_or("").trim();
        if text.is_empty() {
            return Ok(Vec::new());
        }
        // A pasted release URL is a direct hit — read that page and present it as
        // the one candidate, so its card shows the real artist/title/cover.
        if is_soundeo_release_url(text) {
            return match self.fetch_release(&ReleaseId(text.to_string())) {
                Ok(release) => Ok(vec![release_to_candidate(&release)]),
                Err(ProviderError::NotFound) => Ok(Vec::new()),
                Err(other) => Err(other),
            };
        }
        // The results page is one list, not paged here, so a later page adds
        // nothing.
        if query.page > 1 {
            return Ok(Vec::new());
        }
        let url = format!("{BASE}/search?q={}", urlencode(text));
        let html = self.get(&url)?;
        Ok(parse_search(&html))
    }

    fn fetch_release(&self, id: &ReleaseId) -> Result<Release, ProviderError> {
        let html = self.get(&id.0)?;
        parse_release(&html, &id.0)
    }
}

fn status_to_error(status: u16) -> ProviderError {
    match status {
        429 | 503 => ProviderError::RateLimited {
            retry_after_secs: 1,
        },
        404 => ProviderError::NotFound,
        401 | 403 => ProviderError::Auth(format!("HTTP {status}")),
        other => ProviderError::Other(format!("HTTP {other}")),
    }
}

fn is_soundeo_release_url(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.starts_with("http") && lower.contains("soundeo.com/release/")
}

// One `.trackitem` row's fields: the release it belongs to (href + name), the
// track's own title, and the listed duration — plus what the row says about the
// release itself (#396): its cover thumbnail, label and date, so a search
// result can show them without fetching the release page.
struct TrackItem {
    release_href: String,
    release_name: String,
    track_title: String,
    duration: Option<String>,
    cover: Option<String>,
    label: Option<String>,
    date: Option<String>,
}

static TRACKITEM_SPLIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<div class="trackitem""#).unwrap());
static TRACK_TITLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<strong>\s*<a href="/track/[^"]+">([^<]+)</a>"#).unwrap());
static TRACK_TIME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<time>([^<]+)</time>").unwrap());
static TRACK_RELEASE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<span>\s*<a href="(/release/[^"]+)">([^<]+)</a>"#).unwrap());
static TABLE_ROW: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<tr>\s*<td>([^<]+)</td>\s*<td>(.*?)</td>\s*</tr>").unwrap());
static TRACK_COVER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<img src="(https://covers\.sndstatic\.com/[^"]+\.jpg)""#).unwrap()
});
static TRACK_LABEL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<i>by</i>\s*<a [^>]*>([^<]+)</a>"#).unwrap());
static TRACK_DATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<i>at</i>\s*(\d{4}-\d{2}-\d{2})").unwrap());
static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());
static COVER_SIZE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"-\d+\.jpg$").unwrap());

/// The `.trackitem` rows in an HTML fragment. Each row that names both a track
/// and a release is returned; the caller decides which to keep.
fn parse_trackitems(html: &str) -> Vec<TrackItem> {
    let mut items = Vec::new();
    // Split on the row marker; the first piece is whatever came before the first
    // row, so it is skipped.
    for block in TRACKITEM_SPLIT.split(html).skip(1) {
        let (Some(title), Some(release)) =
            (TRACK_TITLE.captures(block), TRACK_RELEASE.captures(block))
        else {
            continue;
        };
        items.push(TrackItem {
            release_href: release[1].to_string(),
            release_name: unescape(&release[2]),
            track_title: unescape(&title[1]),
            duration: TRACK_TIME
                .captures(block)
                .map(|caps| caps[1].trim().to_string()),
            cover: TRACK_COVER.captures(block).map(|caps| caps[1].to_string()),
            label: TRACK_LABEL.captures(block).map(|caps| unescape(&caps[1])),
            date: TRACK_DATE.captures(block).map(|caps| caps[1].to_string()),
        });
    }
    items
}

/// Parse the "Search results" section into release candidates, folding the track
/// rows to their releases (one release, one candidate) in first-seen order.
fn parse_search(html: &str) -> Vec<ReleaseCandidate> {
    let section = results_section(html);
    let mut seen: Vec<String> = Vec::new();
    let mut candidates = Vec::new();
    for item in parse_trackitems(section) {
        if seen.contains(&item.release_href) {
            continue;
        }
        seen.push(item.release_href.clone());
        // The row's thumbnail is the 50 px size; the card wants something that
        // stays sharp at its size, and the same URL serves the others (#396).
        let cover = item.cover.as_deref();
        candidates.push(ReleaseCandidate {
            id: ReleaseId(absolute(&item.release_href)),
            artist: artist_of(&item.track_title),
            title: item.release_name,
            year: item.date.as_deref().and_then(year_from_date),
            score: 0.0, // the app re-scores against the query text (#53)
            thumb_url: cover.map(|url| sized_cover(url, 500)),
            cover_url: cover.map(upgrade_cover),
            country: None,
            label: item.label,
            format: None,
            catalog_number: None,
        });
    }
    candidates
}

/// The slice of the page holding the search results, so unrelated `.trackitem`
/// rows elsewhere on the page are never read as hits. Falls back to the whole
/// page if the markers move.
fn results_section(html: &str) -> &str {
    let start = html.find("Search results").unwrap_or(0);
    let rest = &html[start..];
    let end = rest.find("HOW IT WORKS").unwrap_or(rest.len());
    &rest[..end]
}

/// Parse a release from its album page. `url` is the page we fetched, used as
/// the id and the release URL.
fn parse_release(html: &str, url: &str) -> Result<Release, ProviderError> {
    let table = info_table(html);
    let get = |key: &str| table.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());

    // Artist/title come from the info table, with the OpenGraph title as a
    // fallback ("Artist - Title » Label").
    let og_title = og_content(html, "title").map(|value| unescape(&value));
    let (og_artist, og_release, og_label) =
        og_title.as_deref().map(split_og_title).unwrap_or_default();

    let title = get("Release").or(og_release).unwrap_or_default();
    let artist = get("Artists").or(og_artist).unwrap_or_default();
    if title.is_empty() && artist.is_empty() {
        // Neither the table nor the OpenGraph title parsed — not a release page.
        return Err(ProviderError::NotFound);
    }
    let label = get("Label").or(og_label);
    let genres = get("Styles")
        .map(|styles| {
            styles
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let catalog = get("Catalog #").filter(|value| !value.is_empty());
    let year = get("Date").as_deref().and_then(year_from_date);

    let cover_image_url = og_content(html, "image").map(|thumb| upgrade_cover(&thumb));

    // The tracklist: the page's track rows that point back at this release, in
    // page order. The leading "Artist - " that Soundeo repeats in each track
    // title is dropped when it matches the release artist.
    let release_key = release_id_from_url(url);
    let tracks: Vec<ReleaseTrack> = parse_trackitems(html)
        .into_iter()
        .filter(|item| {
            release_key
                .as_deref()
                .is_none_or(|id| item.release_href.contains(id))
        })
        .enumerate()
        .map(|(index, item)| ReleaseTrack {
            position: (index + 1).to_string(),
            disc: None,
            artist: None,
            title: strip_artist_prefix(&item.track_title, &artist),
            duration_secs: item.duration.as_deref().and_then(duration_secs),
            isrc: None,
            bpm: None,
            key: None,
            section: None,
        })
        .collect();

    Ok(Release {
        id: ReleaseId(url.to_string()),
        artist,
        title,
        year,
        genres,
        styles: Vec::new(),
        tracks,
        labels: label
            .filter(|name| !name.is_empty())
            .map(|name| {
                vec![ReleaseLabel {
                    name,
                    catalog_number: catalog.clone(),
                }]
            })
            .unwrap_or_default(),
        country: None,
        format: Some("File".to_string()), // Soundeo sells digital downloads
        disc_total: None,
        url: og_content(html, "url").or_else(|| Some(url.to_string())),
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

/// A release-candidate view of a fully fetched release, for the pasted-URL path.
fn release_to_candidate(release: &Release) -> ReleaseCandidate {
    ReleaseCandidate {
        id: release.id.clone(),
        artist: release.artist.clone(),
        title: release.title.clone(),
        year: release.year,
        score: 1.0,
        thumb_url: release.cover_image_url.clone(),
        cover_url: release.cover_image_url.clone(),
        country: None,
        label: release.labels.first().map(|label| label.name.clone()),
        format: release.format.clone(),
        catalog_number: release
            .labels
            .first()
            .and_then(|l| l.catalog_number.clone()),
    }
}

/// The release info table as key → value pairs, tags stripped from the value.
/// The page carries other two-column tables (a hotkeys legend); only the release
/// keys are looked up, and those names don't collide.
fn info_table(html: &str) -> Vec<(String, String)> {
    TABLE_ROW
        .captures_iter(html)
        .map(|caps| {
            let key = unescape(caps[1].trim());
            let value = unescape(TAG.replace_all(&caps[2], "").trim());
            (key, value)
        })
        .collect()
}

/// Split an OpenGraph title `"Artist - Title » Label"` into its parts. Any part
/// may be absent (a title with no `»`, or no `-`).
fn split_og_title(title: &str) -> (Option<String>, Option<String>, Option<String>) {
    let (artist_title, label) = match title.rsplit_once('»') {
        Some((left, right)) => (left.trim(), Some(right.trim().to_string())),
        None => (title.trim(), None),
    };
    match artist_title.split_once(" - ") {
        Some((artist, release)) => (
            Some(artist.trim().to_string()),
            Some(release.trim().to_string()),
            label,
        ),
        None => (None, Some(artist_title.to_string()), label),
    }
}

/// The artist part of a `"Artist - Title (Mix)"` track label, for a search
/// candidate. The whole string when there is no `" - "`.
fn artist_of(track_title: &str) -> String {
    track_title
        .split_once(" - ")
        .map(|(artist, _)| artist.trim().to_string())
        .unwrap_or_else(|| track_title.trim().to_string())
}

/// Drop a leading `"{artist} - "` from a track title so it doesn't repeat the
/// release artist; left untouched when it doesn't match (e.g. a guest credit).
fn strip_artist_prefix(track_title: &str, artist: &str) -> String {
    let prefix = format!("{artist} - ");
    track_title
        .strip_prefix(&prefix)
        .unwrap_or(track_title)
        .to_string()
}

/// The numeric release id out of a Soundeo release URL
/// (`…/gianesini-nothing-5972146.html` → `5972146`), used to keep only this
/// release's own tracks.
fn release_id_from_url(url: &str) -> Option<String> {
    let file = url.rsplit('/').next()?.trim_end_matches(".html");
    let id: String = file
        .rsplit('-')
        .next()?
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect();
    (!id.is_empty()).then_some(id)
}

/// Turn a relative Soundeo path into an absolute URL.
fn absolute(path: &str) -> String {
    if path.starts_with("http") {
        path.to_string()
    } else {
        format!("{BASE}{path}")
    }
}

/// Swap a cover URL's size suffix for the largest Soundeo serves
/// (`…-500.jpg` → `…-1400.jpg`), for a cover good enough to embed.
fn upgrade_cover(url: &str) -> String {
    sized_cover(url, 1400)
}

/// The same cover at another of Soundeo's sizes (50, 500, 1400); a URL without
/// a size suffix is returned as it is.
fn sized_cover(url: &str, size: u32) -> String {
    if COVER_SIZE.is_match(url) {
        COVER_SIZE.replace(url, format!("-{size}.jpg")).into_owned()
    } else {
        url.to_string()
    }
}

/// `mm:ss` (or `h:mm:ss`) to whole seconds.
fn duration_secs(text: &str) -> Option<u64> {
    let mut total = 0u64;
    for part in text.trim().split(':') {
        total = total * 60 + part.trim().parse::<u64>().ok()?;
    }
    (total > 0).then_some(total)
}

/// The four-digit year out of a Soundeo date (`"2026-09-11"` → `2026`).
fn year_from_date(date: &str) -> Option<u16> {
    date.split(['-', '/', ' '])
        .filter_map(|token| token.parse::<u16>().ok())
        .find(|year| (1900..=2100).contains(year))
}

/// The `content` of an OpenGraph `<meta property="og:{key}">` tag.
fn og_content(html: &str, key: &str) -> Option<String> {
    let marker = format!("property=\"og:{key}\"");
    let at = html.find(&marker)?;
    // The content attribute may sit either side of the property attribute.
    let tag_start = html[..at].rfind('<')?;
    let tag_end = at + html[at..].find('>')?;
    let tag = &html[tag_start..tag_end];
    let content = tag.find("content=\"")? + "content=\"".len();
    let rest = &tag[content..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Encode a query string for a URL (the handful of bytes a search term can carry
/// that a query string can't).
fn urlencode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push_str("%20"),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// Decode the HTML entities Soundeo's markup uses.
fn unescape(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#039;", "'")
        .replace("&raquo;", "»")
        .replace("&amp;", "&")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_a_pasted_release_url() {
        assert!(is_soundeo_release_url(
            "https://soundeo.com/release/gianesini-nothing-5972146.html"
        ));
        assert!(!is_soundeo_release_url("gianesini nothing"));
        assert!(!is_soundeo_release_url(
            "https://soundeo.com/track/x-23153495.html"
        ));
    }

    #[test]
    fn folds_search_track_rows_to_releases() {
        let html = r#"<h1>Search results</h1>
          <div class="folder">
            <div class="trackitem" data-track-id="1">
              <a href="/release/gianesini-deep-dive-5740905.html"><img src="https://covers.sndstatic.com/2025/11/03/5740905-gianesini-deep-dive-50.jpg" class="cover cover-30"></a>
              <div class="info"><strong><a href="/track/a-deep-dive-1.html">Gianesini - Deep Dive (Original Mix)</a></strong>
              <time>4:53</time>
              <span><a href="/release/gianesini-deep-dive-5740905.html">Deep Dive</a> <i>by</i> <a href="/list/x">GIBI</a> <i>at</i> 2025-11-03</span></div>
            </div>
            <div class="trackitem" data-track-id="2">
              <div class="info"><strong><a href="/track/a-b-freak-2.html">Gianesini - B-Freak (Original Mix)</a></strong>
              <time>5:06</time>
              <span><a href="/release/gianesini-deep-dive-5740905.html">Deep Dive</a></span></div>
            </div>
          </div>
          <h2>HOW IT WORKS</h2>
          <div class="trackitem" data-track-id="9"><strong><a href="/track/other-9.html">Other - Thing</a></strong><span><a href="/release/other-9.html">Other</a></span></div>"#;
        let hits = parse_search(html);
        // Two tracks share the release -> one candidate; the row after "HOW IT
        // WORKS" is outside the results section and dropped.
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].artist, "Gianesini");
        assert_eq!(hits[0].title, "Deep Dive");
        assert_eq!(
            hits[0].id.0,
            "https://soundeo.com/release/gianesini-deep-dive-5740905.html"
        );
        // The row's own cover, label and date fill the card before the release
        // page is ever fetched (#396).
        assert_eq!(
            hits[0].thumb_url.as_deref(),
            Some("https://covers.sndstatic.com/2025/11/03/5740905-gianesini-deep-dive-500.jpg")
        );
        assert_eq!(
            hits[0].cover_url.as_deref(),
            Some("https://covers.sndstatic.com/2025/11/03/5740905-gianesini-deep-dive-1400.jpg")
        );
        assert_eq!(hits[0].label.as_deref(), Some("GIBI"));
        assert_eq!(hits[0].year, Some(2025));
    }

    #[test]
    fn parses_a_release_from_its_info_table_and_tracklist() {
        let html = r#"<html><head>
          <meta property="og:title" content="Gianesini - Nothing &raquo; GIBI">
          <meta property="og:image" content="https://covers.sndstatic.com/2026/09/11/5972146-gianesini-nothing-500.jpg">
          <meta property="og:url" content="https://soundeo.com/release/gianesini-nothing-5972146.html">
          </head><body>
          <table><tr><td>Play/Pause</td><td>SPACE</td></tr></table>
          <table>
            <tr><td>Release</td><td>Nothing</td></tr>
            <tr><td>Artists</td><td><a href="/list/x">Gianesini</a></td></tr>
            <tr><td>Label</td><td><a href="/list/y">GIBI</a></td></tr>
            <tr><td>Styles</td><td><a href="/list/z">Indie Dance</a></td></tr>
            <tr><td>Date</td><td>2026-09-11</td></tr>
            <tr><td>Catalog #</td><td>787960334641</td></tr>
          </table>
          <div class="trackitem" data-track-id="23153495">
            <div class="info"><strong><a href="/track/gianesini-nothing-original-mix-23153495.html">Gianesini - Nothing (Original Mix)</a></strong>
            <time>5:02</time>
            <span><a href="/release/gianesini-nothing-5972146.html">Nothing</a></span></div>
          </div>
          </body></html>"#;
        let release = parse_release(
            html,
            "https://soundeo.com/release/gianesini-nothing-5972146.html",
        )
        .unwrap();
        assert_eq!(release.artist, "Gianesini");
        assert_eq!(release.title, "Nothing");
        assert_eq!(release.year, Some(2026));
        assert_eq!(release.genres, vec!["Indie Dance"]);
        assert_eq!(release.format.as_deref(), Some("File"));
        assert_eq!(release.labels.len(), 1);
        assert_eq!(release.labels[0].name, "GIBI");
        assert_eq!(
            release.labels[0].catalog_number.as_deref(),
            Some("787960334641")
        );
        assert_eq!(
            release.cover_image_url.as_deref(),
            Some("https://covers.sndstatic.com/2026/09/11/5972146-gianesini-nothing-1400.jpg")
        );
        assert_eq!(release.tracks.len(), 1);
        assert_eq!(release.tracks[0].position, "1");
        // The "Gianesini - " prefix is stripped; the mix is kept.
        assert_eq!(release.tracks[0].title, "Nothing (Original Mix)");
        assert_eq!(release.tracks[0].duration_secs, Some(302)); // 5:02
    }

    #[test]
    fn durations_and_years_parse() {
        assert_eq!(duration_secs("5:02"), Some(302));
        assert_eq!(duration_secs("1:02:03"), Some(3723));
        assert_eq!(duration_secs(""), None);
        assert_eq!(year_from_date("2026-09-11"), Some(2026));
        assert_eq!(year_from_date(""), None);
    }
}
