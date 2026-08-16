// The ONLINE sub-tab of TAGGER (#27, #143 split it out of app.js).
//
// A metadata source: the provider search, its paging and the interruptible
// per-release prefetch, the release cards with their lazy tracklists and cover
// browser, the query presets that build a search string from the selection, and
// the import that merges a release onto the selected files. Everything it
// stages goes through the ordinary preview/apply/undo path.
import { confirmDialog, el, escapeHtml, fileName, ico, plural, toast } from "./dom.js";
import { invoke } from "./invoke.js";
import { hooks } from "./hooks.js";
import { fmtTime } from "./player.js";
import { refreshFieldEditor } from "./editor.js";
import { updateSettingsDot } from "./settings.js";
import {
  edits,
  previewPlan,
  selectedPaths,
  setPreviewPlan,
  setPreviewSource,
  setSortKey,
  setTracks,
  tag,
  trackAt,
  trackByPath,
  tracks,
} from "./state.js";

// ---- Discogs import (release picker cards, #27 step 2) ----
// Each search hit is a card; expanding it lazily fetches the release (tracklist)
// and its cover. Import / auto-match / embed-cover are per-card and route
// through the same preview/apply/undo path as before.
let releaseCandidates = []; // last search results (CandidateDto[])
let releaseLayout = "list"; // "list" | "grid"
// Which provider produced the current results (#33). Captured at search time so
// every follow-up fetch (release, cover) hits the same source even if the user
// changes the Source dropdown afterward.
let releaseSource = "discogs";
const releaseCache = new Map(); // releaseId -> fetched ReleaseDto (with tracks)
const coverCache = new Map(); // releaseId -> CoverArtDto (full cover, for embed)
// Fetched images as data URIs, so re-rendering (layout toggle) never re-fetches.
const imageCache = new Map(); // releaseId -> { thumb?, cover? }
const expandedIds = new Set(); // cards currently expanded — survive a re-render

// Paged search (#95/#96): results come in batches of `searchPerPage`; "Load
// more" pulls the next page and appends. `searchGen` is bumped on every new
// search and on Stop, so any in-flight page fetch or background count sweep from
// an older batch bails instead of writing stale cards.
// Batch size; mirrors the #search-per-page select. A display preference,
// persisted and defaulting to 5 (#108).
const PERPAGE_STORAGE_KEY = "tagrex.searchPerPage";
function searchPerPagePref() {
  try {
    const v = parseInt(localStorage.getItem(PERPAGE_STORAGE_KEY), 10);
    return [5, 10, 15].includes(v) ? v : 5;
  } catch (e) {
    return 5;
  }
}
let searchPerPage = searchPerPagePref();
let searchPage = 0; // last page fetched (0 = none yet)
let searchHasMore = false; // provider likely has another page
let searchGen = 0; // generation token
let loadingResults = false; // a page fetch is in flight
let prefetching = false; // the background per-release detail sweep is running

// Whether a search is doing background work the user might want to interrupt
// (#108) — a page fetch or the per-release prefetch sweep.
function searchBusy() {
  return loadingResults || prefetching;
}

async function discogsSearch() {
  return runSearch(true);
}

async function loadMoreResults() {
  if (loadingResults || !searchHasMore) return;
  return runSearch(false);
}

// Stop the background loading (#96) without discarding what's already shown:
// bumping the generation makes in-flight workers bail; the "Load more" button
// stays available so the user can resume.
function stopLoading() {
  if (!searchBusy()) return;
  searchGen++;
  loadingResults = false;
  prefetching = false;
  updateLoadMoreUi();
  toast("Stopped loading results");
}

// The credential the given source needs (#162). Discogs takes the personal
// token from settings; Beatport takes an OAuth access token the backend keeps
// fresh behind `beatport_token`; MusicBrainz takes none. An empty string means
// "no usable credential" — the caller decides whether that is worth a message,
// and a request sent without one fails with the provider's own auth error,
// which is what the user needs to read anyway.
async function providerToken(source) {
  if (source !== "beatport") return el("discogs-token").value.trim();
  try {
    return await invoke("beatport_token", {});
  } catch (e) {
    return "";
  }
}

async function runSearch(reset) {
  const source = el("online-source").value;
  const token = await providerToken(source);
  const query = el("discogs-query").value.trim();
  // Only Discogs needs a token; MusicBrainz is unauthenticated (#33).
  if (source === "discogs" && !token) {
    toast("Enter your Discogs token", true);
    return;
  }
  if (source === "beatport" && !token) {
    toast("Sign in to Beatport in Settings", true);
    return;
  }
  // Remember the token locally so it's prefilled next time — the Discogs one
  // only: the Beatport token is an OAuth token the backend already stores, and
  // writing it into the Discogs token file would break the next Discogs search.
  if (source === "discogs" && token) invoke("save_discogs_token", { token }).catch(() => {});

  if (reset) {
    releaseSource = source;
    searchPerPage = Number(el("search-per-page").value) || 5;
    searchPage = 0;
    searchHasMore = false;
    releaseCandidates = [];
    releaseCache.clear();
    coverCache.clear();
    imageCache.clear();
    expandedIds.clear();
    searchGen++;
  }
  const gen = searchGen;
  const page = searchPage + 1;
  loadingResults = true;
  updateLoadMoreUi();
  try {
    const hits = await invoke("provider_search", {
      source: releaseSource,
      token,
      query: { album: query, format: el("search-format").value || null, page, per_page: searchPerPage },
    });
    if (gen !== searchGen) return; // a newer search / Stop superseded this
    searchPage = page;
    // A full page back suggests there's more to fetch.
    searchHasMore = hits.length >= searchPerPage;
    // Append, skipping ids already shown in case pages overlap.
    const seen = new Set(releaseCandidates.map((c) => c.id));
    const added = hits.filter((c) => !seen.has(c.id));
    releaseCandidates.push(...added);
    renderReleaseList();
    prefetchReleaseCounts(added, gen); // count only the newly added page
  } catch (e) {
    toast(String(e), true);
  } finally {
    if (gen === searchGen) {
      loadingResults = false;
      updateLoadMoreUi();
    }
  }
}

// Show/hide the Load more / Stop footer to match the current loading state.
function updateLoadMoreUi() {
  const busy = searchBusy();
  // The search button is one toggling slot (#111): magnifier ⇄ Stop square.
  const btn = el("discogs-search");
  btn.classList.toggle("busy", busy);
  btn.title = busy ? "Stop loading" : "Search";
  btn.setAttribute("aria-label", btn.title);
  const wrap = el("release-more");
  if (!wrap) return;
  wrap.hidden = releaseCandidates.length === 0;
  el("load-more").hidden = busy || !searchHasMore;
}

async function loadSavedToken() {
  try {
    const token = await invoke("saved_discogs_token", {});
    if (token) el("discogs-token").value = token;
  } catch (e) {
    /* no saved token yet */
  }
  updateSettingsDot();
}


// Meta line "Country · Year · Format" from whatever fields the candidate carries.
function candidateMeta(c) {
  return [c.country, c.year, c.format].filter(Boolean).join(" · ");
}

function releaseList() {
  return el("release-list");
}

function cardEl(id) {
  return releaseList().querySelector(`.release-card[data-id="${cssEscape(id)}"]`);
}

function coverElOf(id) {
  return releaseList().querySelector(
    `[data-id="${cssEscape(id)}"] .release-cover, [data-id="${cssEscape(id)}"] .tile-cover`,
  );
}

// The track/disc-count pill, whichever layout this release is shown in.
function countPillOf(id) {
  return releaseList().querySelector(`[data-id="${cssEscape(id)}"] .tk-count`);
}

// "N tracks", or "N tracks · M discs" once the release is fetched; a dash before.
function countLabel(id) {
  const release = releaseCache.get(id);
  if (!release) return "— tracks";
  const discs = discCount(release);
  const tracks = plural(release.tracks.length, "track", "tracks");
  return discs > 1 ? `${tracks} · ${plural(discs, "disc", "discs")}` : tracks;
}

// Highest disc number across track positions ("2-1" -> disc 2); 1 if unmarked.
function discCount(release) {
  let max = 1;
  for (const t of release.tracks) {
    const m = /^(\d+)-/.exec(t.position || "");
    if (m) max = Math.max(max, Number(m[1]));
  }
  return max;
}

// ---- media-type badge on the cover (#98, per the Design spec) ----
// Infer the medium from the provider's free `format` text; first hit wins in
// this order (vinyl → cd → digital → generic). Case-insensitive substring.
function mediaKind(format) {
  const f = (format || "").toLowerCase();
  const has = (...ks) => ks.some((k) => f.includes(k));
  if (has("cassette", "tape")) return "cassette";
  if (has("vinyl", "lp", "ep", '7"', '10"', '12"', "shellac")) return "vinyl";
  if (has("sacd", "hdcd", "cdr", "compact disc", "cd")) return "cd";
  if (has("file", "flac", "mp3", "wav", "aac", "digital", "download", "streaming")) return "digital";
  return "generic";
}

const MEDIA_LABEL = { vinyl: "Vinyl", cd: "CD", cassette: "Cassette", digital: "Digital", generic: "" };

// The value written to the MEDIA tag on import (#106): a clean normalized label,
// or the raw provider format string when the kind is unrecognised (so nothing is
// lost). Drives the vinyl side notation (%side% / Position column).
function mediaTagValue(format) {
  const label = MEDIA_LABEL[mediaKind(format)];
  return label || (format || "").trim() || null;
}

// Inline SVG glyphs (currentColor, CSP-safe) — from the Design deliverable.
const MEDIA_GLYPH = {
  vinyl: `<svg viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="7" fill="none" stroke="currentColor" stroke-width="1"/><circle cx="8" cy="8" r="4.2" fill="none" stroke="currentColor" stroke-width=".8" opacity=".55"/><circle cx="8" cy="8" r="1.4" fill="currentColor"/></svg>`,
  cd: `<svg viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="7" fill="none" stroke="currentColor" stroke-width="1"/><circle cx="8" cy="8" r="2.5" fill="none" stroke="currentColor" stroke-width="1"/></svg>`,
  cassette: `<svg viewBox="0 0 16 16" aria-hidden="true"><rect x="1.5" y="3.5" width="13" height="9" rx="1.2" fill="none" stroke="currentColor" stroke-width="1"/><circle cx="5.5" cy="8" r="1.4" fill="none" stroke="currentColor" stroke-width=".8"/><circle cx="10.5" cy="8" r="1.4" fill="none" stroke="currentColor" stroke-width=".8"/><rect x="4.5" y="10.5" width="7" height="1.2" fill="currentColor"/></svg>`,
  digital: `<svg viewBox="0 0 16 16" aria-hidden="true"><g fill="currentColor"><rect x="2.2" y="6" width="1.6" height="4" rx=".8"/><rect x="5.2" y="3" width="1.6" height="10" rx=".8"/><rect x="8.2" y="5" width="1.6" height="6" rx=".8"/><rect x="11.2" y="7" width="1.6" height="2" rx=".8"/></g></svg>`,
  generic: `<svg viewBox="0 0 16 16" aria-hidden="true"><circle cx="6" cy="11.5" r="2.2" fill="currentColor"/><rect x="7.9" y="3" width="1.3" height="8.5" fill="currentColor"/><path d="M8.2 3.2q4 .6 4 3.8" fill="none" stroke="currentColor" stroke-width="1.3"/></svg>`,
};

// The badge for a candidate. The media glyph is known up front (from `format`);
// the ×N disc count only appears once the release is fetched (disc count needs
// its tracklist), so re-render it via `updateMediaBadge` after the prefetch.
function mediaBadgeMarkup(c) {
  const kind = mediaKind(c.format);
  const release = releaseCache.get(c.id);
  const discs = release ? discCount(release) : 1;
  const n = discs > 1 ? `<span class="n">×${discs}</span>` : "";
  const label = MEDIA_LABEL[kind];
  return `<span class="media-badge"${label ? ` title="${label}"` : ""}>${MEDIA_GLYPH[kind]}${n}</span>`;
}

// Refresh the badge (its ×N) for one release after its tracklist is fetched.
function updateMediaBadge(c) {
  const badge = coverElOf(c.id)?.querySelector(".media-badge");
  if (badge) badge.outerHTML = mediaBadgeMarkup(c);
}

function renderReleaseList() {
  const list = releaseList();
  list.innerHTML = "";
  el("release-toolbar").hidden = releaseCandidates.length === 0;
  // The whole phrase, not just the number: "Found 1 entries" was the same
  // disagreement as the card counts (#167).
  el("release-found").textContent = `Found ${plural(releaseCandidates.length, "entry", "entries")}`;
  el("discogs-empty").hidden = releaseCandidates.length > 0;
  if (releaseCandidates.length === 0) {
    el("discogs-empty").textContent = "No releases found.";
    return;
  }
  list.classList.toggle("grid", releaseLayout === "grid");
  for (const c of releaseCandidates) {
    list.insertAdjacentHTML("beforeend", releaseLayout === "grid" ? tileMarkup(c) : cardMarkup(c));
  }
  // Restore images (from cache) and any expanded tracklists after the re-render.
  for (const c of releaseCandidates) {
    applyImage(c);
    const card = cardEl(c.id);
    if (releaseLayout === "list" && card && expandedIds.has(c.id) && releaseCache.has(c.id)) {
      card.setAttribute("aria-expanded", "true");
      card.querySelector(".release-caret").innerHTML = ico("caret-down");
      renderTracklist(card, releaseCache.get(c.id));
      card.querySelector(".release-tracklist").dataset.loaded = "1";
    }
  }
  updateLoadMoreUi();
}

// ---- query presets (#97) ----
// Fill the search box from the current selection instead of only manual typing.
function baseNameNoExt(path) {
  const base = fileName(path);
  const dot = base.lastIndexOf(".");
  return dot > 0 ? base.slice(0, dot) : base;
}

function folderNameOf(path) {
  const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  const dir = i >= 0 ? path.slice(0, i) : "";
  const j = Math.max(dir.lastIndexOf("/"), dir.lastIndexOf("\\"));
  return j >= 0 ? dir.slice(j + 1) : dir;
}

// The track a preset draws from: the first selected row, else the first loaded.
function presetSourceTrack() {
  const byPath = trackByPath();
  const first = selectedPaths()[0];
  return (first && byPath.get(first)) || tracks[0] || null;
}

// A name off the disk, made searchable (#158). Downloaded music is routinely
// filed with underscores for spaces, and a provider asked for
// `various_-_la_bush_-_music_from_the_temple_of_house` matches nothing — which
// reads as "this release isn't in the database" rather than "that isn't a query".
//
// Underscores only. A dot carries meaning in real titles (`Ltd.`, `Vol. 2`,
// `M.I.A.`), so rewriting those would break as many queries as it fixed;
// an underscore in a release title is essentially unheard of.
function searchableName(name) {
  return name.replace(/_/g, " ").replace(/\s+/g, " ").trim();
}

function queryFromPreset(kind) {
  const t = presetSourceTrack();
  if (!t) return "";
  switch (kind) {
    // The two path-derived presets normalise; the tag-derived ones below don't —
    // an underscore in a tag was put there on purpose, and cleaning tag values
    // up is what GENERATOR is for.
    case "folder":
      return searchableName(folderNameOf(t.path));
    case "filename":
      return searchableName(baseNameNoExt(t.path));
    case "album":
      return (t.tags.album || "").trim();
    case "artist-title":
      return [t.tags.artist, t.tags.title].filter(Boolean).join(" ").trim();
    default:
      return "";
  }
}

// Apply the chosen preset: fill the box (leaving "manual" alone) and, if we have
// something to search for, run the search straight away.
function applyQueryPreset() {
  const kind = el("query-preset").value;
  if (kind === "manual") return;
  const text = queryFromPreset(kind);
  if (!text) {
    toast("Nothing selected to build the query from", true);
    return;
  }
  el("discogs-query").value = text;
  discogsSearch();
}

// The catalogue-number + track-count match key as one segmented badge (#124):
// a single unified border wraps both, the catalogue segment accent-filled and
// the count segment neutral, split by a divider in the same border colour. The
// catalogue segment is omitted when the release has no catalogue number; the
// count segment keeps the `tk-count` class so prefetchReleaseCounts can fill it
// in once the release is fetched.
function releaseBadge(c) {
  // The catalogue segment doubles as a link to the release's provider page (#92);
  // the click is caught in the release-list handler, which reads the card's id.
  const catno = c.catalog_number
    ? `<span class="rb-catno" title="Open the release page">${escapeHtml(c.catalog_number)}</span>`
    : "";
  return `<span class="rel-badge">${catno}<span class="rb-count tk-count">${escapeHtml(countLabel(c.id))}</span></span>`;
}

// Open a candidate's provider release page (#92). Source is the committed search
// source (all shown candidates share it); the backend builds + validates the URL.
async function openReleasePage(id) {
  try {
    await invoke("open_release_page", { source: releaseSource, id });
  } catch (e) {
    toast(String(e), true);
  }
}

function cardMarkup(c) {
  // Four lines, top to bottom (#98): (1) the catalogue-no. + track-count match
  // key (one segmented badge, #124), (2) album artist, (3) album title, (4) the
  // rest (country · year · format). The cover fills the header's full height, so
  // it spans all four lines.
  const artist = c.artist ? `<span class="release-artist" title="${escapeHtml(c.artist)}">${escapeHtml(c.artist)}</span>` : "";
  const meta = candidateMeta(c);
  const metaLine = meta ? `<span class="release-meta">${escapeHtml(meta)}</span>` : "";
  return `
    <article class="release-card" data-id="${escapeHtml(c.id)}" aria-expanded="false">
      <div class="release-header-row">
        <button class="release-head" type="button">
          <span class="release-cover">${mediaBadgeMarkup(c)}</span>
          <span class="release-info">
            <span class="release-line1">${releaseBadge(c)}</span>
            ${artist}
            <span class="release-title" title="${escapeHtml(c.title)}">${escapeHtml(c.title)}</span>
            ${metaLine}
          </span>
          <span class="release-caret" aria-hidden="true">${ico("chevron-right")}</span>
        </button>
        <button class="release-import icon" type="button" data-act="import" title="Import this release into the selected files" aria-label="Import this release into the selected files"><svg class="ico"><use href="#i-import"/></svg></button>
      </div>
      <div class="release-tracklist"></div>
    </article>`;
}

function tileMarkup(c) {
  const artist = c.artist ? `<span class="tile-artist">${escapeHtml(c.artist)}</span>` : "";
  // Same information as a list card: the catalogue-no. + track-count match key
  // (one segmented badge, #124) · artist (bold) · album title · country/year/format.
  return `
    <article class="release-tile" data-id="${escapeHtml(c.id)}">
      <div class="tile-cover"></div>
      <div class="tile-info">
        <div class="tile-top">${releaseBadge(c)}</div>
        ${artist}
        <span class="release-title" title="${escapeHtml(c.title)}">${escapeHtml(c.title)}</span>
        <span class="muted">${escapeHtml(candidateMeta(c))}</span>
      </div>
    </article>`;
}

// Show the layout-appropriate cover for a candidate, fetching + caching it once.
// List cards use the small thumb (56px); grid tiles use the larger cover image so
// they don't look upscaled. Cached data URIs are reused, so toggling layout is
// instant and never re-hits Discogs.
async function applyImage(c) {
  const kind = releaseLayout === "grid" ? "cover" : "thumb";
  const url = kind === "cover" ? c.cover_url || c.thumb_url : c.thumb_url || c.cover_url;
  if (!url) return;
  const cached = imageCache.get(c.id) || {};
  let dataUri = cached[kind];
  if (!dataUri) {
    const token = await providerToken(releaseSource);
    try {
      const img = await invoke("provider_fetch_image", { source: releaseSource, token, url });
      dataUri = `data:${img.mime};base64,${img.data_base64}`;
      cached[kind] = dataUri;
      imageCache.set(c.id, cached);
    } catch (e) {
      return; // leave the striped placeholder
    }
  }
  const cover = coverElOf(c.id);
  if (cover) {
    // Drop in the art without wiping the media badge (#98) that shares the well.
    cover.querySelector("img")?.remove();
    cover.insertAdjacentHTML("afterbegin", `<img alt="" src="${dataUri}" />`);
  }
}

// Fetch each release once, in the background, to fill the track/disc count on
// every card up front. A small pool fetches several at a time (the commands are
// async/off-main-thread) so counts appear quickly without bursting past Discogs'
// 60/min limit; the backend still honours 429/Retry-After. Cached, so expanding
// a card and toggling layout are then instant.
const PREFETCH_CONCURRENCY = 4;

async function prefetchReleaseCounts(items, gen) {
  const token = await providerToken(releaseSource);
  const queue = (items || releaseCandidates).filter((c) => !releaseCache.has(c.id));
  if (queue.length === 0) return;
  // The sweep is interruptible background work — show Stop while it runs (#108).
  prefetching = true;
  updateLoadMoreUi();
  async function worker() {
    while (queue.length) {
      // A newer search, or Stop (#96), bumps the generation → bail.
      if (gen !== undefined && gen !== searchGen) return;
      const c = queue.shift();
      if (!c || releaseCache.has(c.id)) continue;
      try {
        releaseCache.set(c.id, await invoke("provider_fetch_release", { source: releaseSource, token, releaseId: c.id }));
        const pill = countPillOf(c.id);
        if (pill) pill.textContent = countLabel(c.id);
        updateMediaBadge(c); // fill the badge's ×N now that disc count is known
      } catch (e) {
        /* skip this one; the card just keeps its dash */
      }
    }
  }
  await Promise.all(Array.from({ length: PREFETCH_CONCURRENCY }, worker));
  // Only the sweep for the current generation owns the flag (Stop / a newer
  // search may have moved on already).
  if (gen === undefined || gen === searchGen) {
    prefetching = false;
    updateLoadMoreUi();
  }
}

// CSS.escape isn't guaranteed in every webview; ids are numeric strings anyway.
function cssEscape(s) {
  return String(s).replace(/["\\]/g, "\\$&");
}

// Expand/collapse a card; on first expand, fetch the release + render its
// tracklist and pull the full cover for embedding.
async function toggleCard(card) {
  const id = card.dataset.id;
  const expanded = card.getAttribute("aria-expanded") === "true";
  card.setAttribute("aria-expanded", expanded ? "false" : "true");
  card.querySelector(".release-caret").innerHTML = ico(expanded ? "chevron-right" : "caret-down");
  if (expanded) {
    expandedIds.delete(id);
    return;
  }
  expandedIds.add(id);
  const body = card.querySelector(".release-tracklist");
  if (body.dataset.loaded === "1") return;
  // Skeleton while the release fetch is in flight (inert stripe motif).
  body.innerHTML = `<div style="padding: 8px 10px">
      <div class="skeleton-line w-80"></div>
      <div class="skeleton-line w-60"></div>
      <div class="skeleton-line w-40"></div>
    </div>`;
  const token = await providerToken(releaseSource);
  try {
    let release = releaseCache.get(id);
    if (!release) {
      release = await invoke("provider_fetch_release", { source: releaseSource, token, releaseId: id });
      releaseCache.set(id, release);
    }
    renderTracklist(card, release);
    body.dataset.loaded = "1";
    const tkPill = countPillOf(id); if (tkPill) tkPill.textContent = countLabel(id);
    loadFullCover(id, release.cover_image_url, card);
  } catch (e) {
    body.innerHTML = "";
    body.dataset.loaded = "";
    expandedIds.delete(id);
    toast(String(e), true);
  }
}

// ---- length match against the selection (#188) ----
// A release card says what each track should run to; the table already knows
// what the selected files really run to. The difference between the two is what
// tells a CD rip filed under a vinyl catalogue number from the vinyl itself —
// and it has to be readable BEFORE an import, because after one the wrong
// lengths are already written as tags.
//
// Both sides are in memory (the release carries its durations, every row of the
// table carries the file's), so this is arithmetic, not I/O, and it can follow
// the selection as it changes.

// Within this many seconds two lengths are the same recording; up to `NEAR` a
// plausible other master or fade-out; past that a different recording. Nothing
// is paired at all beyond `PAIR_LIMIT` — a number that far out invites reading
// meaning into a coincidence.
const MATCH_SECS = 2;
const NEAR_SECS = 10;
const PAIR_LIMIT_SECS = 120;

// Pair each release track with the selected file closest to it in length, one
// file to one track, closest pairs claimed first. Deliberately NOT the import's
// positional pairing: three files of a five-track release line up against
// tracks 1-3 there, which is exactly the case this is meant to see through.
// Returns the map track index -> { delta (file minus track), path, secs }, and
// how many selected files could take part at all — an empty map means "nothing
// to compare" or "nothing close enough", and the tally says different things
// about the two.
function durationPairs(release) {
  const files = [];
  for (const path of selectedPaths()) {
    const t = trackAt(path);
    if (t && t.duration_secs) files.push({ path, secs: t.duration_secs });
  }
  const byTrack = new Map();
  if (files.length === 0) return { pairs: byTrack, files: 0 };
  const pairs = [];
  release.tracks.forEach((t, i) => {
    if (!t.duration_secs) return;
    files.forEach((f, j) => {
      const delta = f.secs - t.duration_secs;
      if (Math.abs(delta) <= PAIR_LIMIT_SECS) pairs.push({ i, j, delta });
    });
  });
  pairs.sort((a, b) => Math.abs(a.delta) - Math.abs(b.delta));
  const takenFiles = new Set();
  for (const p of pairs) {
    if (byTrack.has(p.i) || takenFiles.has(p.j)) continue;
    byTrack.set(p.i, { delta: p.delta, path: files[p.j].path, secs: files[p.j].secs });
    takenFiles.add(p.j);
  }
  return { pairs: byTrack, files: files.length };
}

function deltaBand(delta) {
  const d = Math.abs(delta);
  if (d <= MATCH_SECS) return "match";
  if (d <= NEAR_SECS) return "near";
  return "off";
}

// "+2s" / "-1:14" — seconds while they read as seconds, m:ss beyond a minute.
function deltaLabel(delta) {
  const d = Math.abs(delta);
  const sign = delta > 0 ? "+" : delta < 0 ? "-" : "";
  return `${sign}${d < 60 ? `${d}s` : fmtTime(d)}`;
}

// The duration cell of one track row: the release's own time, plus the
// difference to the file paired with it when there is one.
function durationCell(track, pair) {
  const base = track.duration_secs ? fmtTime(track.duration_secs) : "—";
  if (!pair) return base;
  const title = `${fileName(pair.path)} runs ${fmtTime(pair.secs)}`;
  return `${base}<span class="tk-delta ${deltaBand(pair.delta)}" title="${escapeHtml(title)}">${deltaLabel(
    pair.delta,
  )}</span>`;
}

// Re-derive the pairing for one rendered tracklist and write it into the rows.
// Rows are rendered in `release.tracks` order, so the index is the row.
function applyDurationDeltas(card, release) {
  const { pairs, files } = durationPairs(release);
  card.querySelectorAll(".release-tracklist tbody td.tk-dur").forEach((cell, i) => {
    cell.innerHTML = durationCell(release.tracks[i], pairs.get(i));
  });
  const fit = card.querySelector(".tk-fit");
  if (!fit) return;
  // The tally answers the question the rows only imply — is this the edition? —
  // and says nothing at all when there was nothing to compare, rather than
  // reporting a miss the user never asked for.
  const stated = release.tracks.filter((t) => t.duration_secs).length;
  const hits = [...pairs.values()].filter((p) => deltaBand(p.delta) === "match").length;
  const comparable = files > 0 && stated > 0;
  fit.textContent = !comparable ? "" : hits ? `${hits} of ${stated} lengths match` : "no lengths match";
  fit.classList.toggle("hit", comparable && hits > 0);
  fit.classList.toggle("miss", comparable && hits === 0);
}

// Follow the selection: the deltas are about the files the user has picked, so
// they go stale the moment that changes (app.js calls this from paintSelection).
function refreshReleaseMatches() {
  for (const id of expandedIds) {
    const card = cardEl(id);
    const release = releaseCache.get(id);
    if (card && release && card.querySelector(".release-tracklist")?.dataset.loaded === "1") {
      applyDurationDeltas(card, release);
    }
  }
}

function renderTracklist(card, release) {
  const rows = release.tracks
    .map((t, i) => {
      // Show the per-track artist only when it differs from the album artist —
      // otherwise it's noise on every row; it truncates before the title.
      const differs = t.artist && t.artist !== release.artist;
      const artistEl = differs ? `<span class="tk-a">${escapeHtml(t.artist)}</span>` : "";
      return `
      <tr>
        <td class="tk-lead"><span class="tk-lead-inner"><input type="checkbox" checked data-i="${i}" /><span class="tk-num">${escapeHtml(t.position)}</span></span></td>
        <td class="tk-track"><span class="tk-track-inner"><span class="tk-t" title="${escapeHtml(t.title)}">${escapeHtml(t.title)}</span>${artistEl}</span></td>
        <td class="tk-dur">${t.duration_secs ? fmtTime(t.duration_secs) : "—"}</td>
      </tr>`;
    })
    .join("");
  // Label / catalogue-number picker (#90): a release can list several pairs
  // (even from one label); the user picks the single one to write. One pair (or
  // none) needs no picker — importRelease falls back to the first.
  const labels = release.labels || [];
  const labelPicker =
    labels.length > 1
      ? `<div class="tracklist-label"><label>Label · cat#
          <select class="label-picker" title="Which label and catalogue number to write">${labels
            .map(
              (l, i) =>
                `<option value="${i}">${escapeHtml(l.name)}${l.catalog_number ? " — " + escapeHtml(l.catalog_number) : ""}</option>`,
            )
            .join("")}</select></label></div>`
      : "";
  // Cover controls (#102): resolution + image count (from the release JSON's
  // images, dimensions when the provider states them), and save-to-disk actions.
  const images = release.images || [];
  const primary = images[0];
  const res = primary && primary.width && primary.height ? `${primary.width}×${primary.height}` : "";
  // The resolution describes the PRIMARY image specifically — provider.rs orders
  // `images` primary-first and `cover_image_url` is that first entry, so it's the
  // exact file "Save as folder.jpg" writes and "Embed cover" embeds. It used to
  // sit in the row as "600×594 · 12 images", where it read as a property of the
  // whole set; it now hangs off the action it actually qualifies (and the button
  // tooltip), which also frees the row of a text block.
  const resNote = res ? `<span class="tk-menu-note">${res}</span>` : "";
  const artTitle =
    `Save this release's artwork next to the selected tracks` +
    (res ? ` — front cover ${res}` : "") +
    (images.length > 1 ? ` · ${images.length} images` : "");
  const artCount = images.length > 1 ? `<span class="tk-art-count">${images.length}</span>` : "";
  // Saving artwork is one call with a boolean (saveReleaseImages), so it reads as
  // ONE control rather than two competing buttons: a plain button when the
  // release carries a single image, a split button whose caret offers "all N"
  // when there are more. The count stays in the menu item and the adjacent
  // "N images" readout instead of being stamped on the button face.
  const saveBtn = !images.length
    ? ""
    : images.length === 1
      ? `<button class="btn-sm io tk-art-btn" data-act="save-cover" aria-label="Save the cover next to the selected tracks as folder.jpg" title="${escapeHtml(artTitle)}">${ico("image")}</button>`
      : `<div class="col-picker tk-save">
           <button class="btn-sm io tk-art-btn" data-act="save-menu" aria-label="Save this release's artwork to disk" title="${escapeHtml(artTitle)}">${ico("image")}${artCount}${ico("caret-down")}</button>
           <div class="col-menu tk-save-menu" hidden>
             <button type="button" class="col-menu-row tk-menu-item" data-act="save-cover">Save as folder.jpg${resNote}</button>
             <button type="button" class="col-menu-row tk-menu-item" data-act="save-all">Save all ${images.length} images</button>
           </div>
         </div>`;
  card.querySelector(".release-tracklist").innerHTML = `
    <div class="tracklist-actions">
      <button class="btn-sm" data-act="automatch" title="Reorder the selected files to line up with this tracklist">Auto-match</button>
      <button class="btn-sm" data-act="embed" title="Embed this release's cover into the selected files">Embed cover</button>
      ${saveBtn}
    </div>
    ${labelPicker}
    <div class="tracklist-scroll"><table>
      <thead><tr class="tk-head">
        <th class="tk-lead"><label class="tk-selall" title="Select all tracks / none"><input type="checkbox" class="tk-selall-box" aria-label="Select all tracks" /></label></th>
        <th class="tk-selcount muted" colspan="2"><span class="tk-fit" title="How many of this release's lengths the selected files account for"></span><span class="tk-tally"></span></th>
      </tr></thead>
      <tbody>${rows}</tbody></table></div>`;
  // The import action moved to a header icon button (shown once loaded + expanded).
  card.classList.add("tracklist-loaded");
  updateTracklistCount(card);
  // The rows above carry the release's own durations; the difference to the
  // selected files is written in afterwards, by the one path that also keeps it
  // current as the selection changes (#188).
  applyDurationDeltas(card, release);
}

function updateTracklistCount(card) {
  const boxes = [...card.querySelectorAll(".release-tracklist tbody .tk-lead input")];
  const on = boxes.filter((b) => b.checked).length;
  const label = card.querySelector(".tk-tally");
  if (label) label.textContent = `${on} / ${boxes.length} selected`;
  // Mirror the tally onto the master checkbox that replaced the Enable/Disable
  // all pair — same tri-state contract as the file table's #select-all, so the
  // control shows the current scope instead of just offering two commands.
  const master = card.querySelector(".tk-selall-box");
  if (master) {
    master.checked = boxes.length > 0 && on === boxes.length;
    master.indeterminate = on > 0 && on < boxes.length;
  }
}

// The enabled tracks of a card, shaped for import / auto-align.
function enabledTracksOf(card) {
  const release = releaseCache.get(card.dataset.id);
  return [...card.querySelectorAll(".release-tracklist tbody .tk-lead input:checked")].map((cb) => {
    const t = release.tracks[Number(cb.dataset.i)];
    return {
      position: t.position,
      // The disc the release puts this track on (#146), when it says.
      disc: t.disc ?? null,
      artist: t.artist || release.artist,
      title: t.title,
      duration_secs: t.duration_secs ?? null,
      isrc: t.isrc ?? null,
      // Tempo and key (#162) — stated only by a source that measures them.
      bpm: t.bpm ?? null,
      key: t.key ?? null,
    };
  });
}

// Fetch the full-size cover once (for embedding) and upgrade the card thumbnail.
async function loadFullCover(id, url, card) {
  if (!url || coverCache.has(id)) return;
  const token = await providerToken(releaseSource);
  try {
    const cover = await invoke("provider_fetch_image", { source: releaseSource, token, url });
    coverCache.set(id, cover);
    const coverEl = card.querySelector(".release-cover");
    if (coverEl) {
      // Swap in the full-res cover without wiping the media badge (#98).
      coverEl.querySelector("img")?.remove();
      coverEl.insertAdjacentHTML("afterbegin", `<img alt="" src="data:${cover.mime};base64,${cover.data_base64}" />`);
    }
  } catch (e) {
    /* embedding just won't be available for this card */
  }
}

async function autoMatchToRelease(card) {
  const paths = selectedPaths();
  const release = releaseCache.get(card.dataset.id);
  if (!release || paths.length === 0) {
    toast("Select the tracks to match against first", true);
    return;
  }
  const releaseTracks = release.tracks.map((t) => ({
    position: t.position,
    artist: t.artist || release.artist,
    title: t.title,
    duration_secs: t.duration_secs ?? null,
    isrc: t.isrc ?? null,
  }));
  try {
    // Each entry is { track, by_isrc } or null (#54).
    const aligned = await invoke("auto_align", { paths, tracks: releaseTracks });
    // The import pairs the i-th ENABLED track with the i-th file, so a match is
    // only usable if the ticks agree with it (#185). Tick exactly the tracks
    // that matched, untick the rest, and put the files in the order of the
    // tracks they matched — then the dense pairing is right by construction,
    // including when the folder holds fewer files than the release has tracks.
    // Without this, a file matching track 5 of 5 in a folder of three had
    // nowhere to go and was silently demoted to filler.
    const matchedPairs = [];
    const unmatched = [];
    paths.forEach((path, i) => {
      const k = aligned[i] ? aligned[i].track : null;
      if (k === null || k === undefined) unmatched.push(path);
      else matchedPairs.push({ path, track: k });
    });
    const hits = aligned.filter((m) => m);
    const matched = hits.length;
    if (matched) {
      matchedPairs.sort((a, b) => a.track - b.track);
      const wanted = new Set(matchedPairs.map((p) => p.track));
      card
        .querySelectorAll(".release-tracklist tbody .tk-lead input")
        .forEach((box) => (box.checked = wanted.has(Number(box.dataset.i))));
      updateTracklistCount(card);
      // Matched files first, in release order; anything unmatched keeps its own
      // order after them, where the untouched tail of the tracklist is.
      const order = [...matchedPairs.map((p) => p.path), ...unmatched];
      const byPath = new Map(tracks.map((t) => [t.path, t]));
      const selected = new Set(paths);
      let next = 0;
      setTracks(tracks.map((t) => (selected.has(t.path) ? byPath.get(order[next++]) : t)));
      setSortKey(null);
      hooks.renderTracks();
    }
    const byIsrc = hits.filter((m) => m.by_isrc).length;
    // Surface *why* — an ISRC match is exact, worth calling out (#54).
    const isrcNote = byIsrc ? ` (${byIsrc} exact by ISRC)` : "";
    // Say when the release carries tracks this folder doesn't, so the ticks
    // that just went off are not a surprise.
    const dropped = release.tracks.length - matched;
    const droppedNote = matched && dropped > 0 ? `, ${plural(dropped, "release track", "release tracks")} left out` : "";
    toast(
      matched
        ? `Matched ${matched}/${plural(paths.length, "file", "files")}${isrcNote} — reordered to line up${droppedNote}`
        : "No confident matches — leaving the order alone",
      matched === 0,
    );
  } catch (e) {
    toast(String(e), true);
  }
}

async function embedCoverFrom(card) {
  const cover = coverCache.get(card.dataset.id);
  if (!cover) {
    toast("This release has no cover to embed", true);
    return;
  }
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast("Select the tracks to embed the cover into first", true);
    return;
  }
  try {
    setPreviewPlan(await invoke("preview_cover_embed", { paths, cover }));
    setPreviewSource("cover");
    hooks.renderPreview(previewPlan);
    toast(
      previewPlan.changes.length
        ? `Previewing cover on ${plural(previewPlan.changes.length, "file", "files")} — click Apply`
        : "Selected files already have this cover",
    );
  } catch (e) {
    toast(String(e), true);
  }
}

// Save a release's image(s) to disk next to the selected tracks (#102). `all`
// saves every image (primary -> folder.jpg, then cover.jpg, cover-1.jpg…);
// otherwise just the primary. If the backend reports existing files, confirm
// before overwriting.
async function saveReleaseImages(card, all) {
  const id = card.dataset.id;
  const release = releaseCache.get(id);
  const images = (release && release.images) || [];
  if (!images.length) {
    toast("This release has no images to save", true);
    return;
  }
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast("Select the tracks to save the images next to first", true);
    return;
  }
  const urls = all ? images.map((i) => i.url) : [images[0].url];
  const token = await providerToken(releaseSource);
  const args = { source: releaseSource, token, path: paths[0], urls, overwrite: false };
  try {
    let res = await invoke("save_release_images", args);
    if (res.conflicts && res.conflicts.length) {
      const ok = await confirmDialog(
        `${res.conflicts.join(", ")} already exist${res.conflicts.length === 1 ? "s" : ""} in that folder. Overwrite?`,
        "Overwrite",
      );
      if (!ok) return;
      res = await invoke("save_release_images", { ...args, overwrite: true });
    }
    toast(`Saved ${plural(res.written.length, "image", "images")} next to the tracks`);
  } catch (e) {
    toast(String(e), true);
  }
}

async function importRelease(card) {
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast("Select the tracks to import onto first", true);
    return;
  }
  const release = releaseCache.get(card.dataset.id);
  // Prefer Discogs "styles" over the coarse "genres" for the genre tag (#26),
  // joined with "/" to match the user's library convention.
  const genreValues = release.styles.length ? release.styles : release.genres;
  // The chosen label / catalogue-number pair (#90): the picker's selection, or
  // the first pair when there's no picker (0 or 1 label).
  const labels = release.labels || [];
  const picker = card.querySelector(".label-picker");
  const chosen = labels[picker ? Number(picker.value) : 0];
  const selection = {
    album: release.title,
    album_artist: release.artist,
    year: release.year ? String(release.year) : null,
    genre: genreValues.join("/") || null,
    tracks: enabledTracksOf(card),
    // Store the release id so the table can group by release (#20).
    release_id: release.id || null,
    source: releaseSource,
    label: chosen ? chosen.name : null,
    catalog_number: chosen ? chosen.catalog_number || null : null,
    country: release.country || null,
    // Total tracks on the release (album-level), so a file reads as N/total.
    track_total: release.tracks && release.tracks.length ? String(release.tracks.length) : null,
    // Discs in the set (album-level), so a file reads as N/total (#146).
    disc_total: release.disc_total ? String(release.disc_total) : null,
    url: release.url || null,
    // Physical medium → drives the vinyl side view (#106).
    media_type: mediaTagValue(release.format),
  };
  try {
    const plan = await invoke("preview_import", {
      paths,
      selection,
      vinylSidesToDisc: el("import-vinyl-disc").checked,
    });
    // Merge into the pending-edits buffer; a field the user already edited by
    // hand wins (we don't overwrite an existing entry).
    let merged = 0;
    for (const change of plan.changes) {
      if (!edits.has(change.path)) edits.set(change.path, new Map());
      const fields = edits.get(change.path);
      for (const tc of change.tag_changes) {
        if (!fields.has(tc.field)) {
          fields.set(tc.field, tc.new ?? "");
          merged += 1;
        }
      }
      if (fields.size === 0) edits.delete(change.path);
    }
    // No plain render here: `previewEdits` stages the plan and repaints the
    // table as a diff a moment later, so rendering first only builds a few
    // thousand rows to throw them away (#186).
    refreshFieldEditor();
    await hooks.previewEdits();
    toast(
      merged
        ? `Merged ${plural(merged, "field change", "field changes")} from Discogs into pending edits`
        : "Nothing new to import from this release",
    );
  } catch (e) {
    toast(String(e), true);
  }
}

// ---- wire up ----
// The one search/stop button toggles by state (#111): stop while a search is
// working, otherwise start one. Enter always starts a search.
el("discogs-search").addEventListener("click", () => {
  if (searchBusy()) stopLoading();
  else discogsSearch();
});
el("discogs-query").addEventListener("keydown", (e) => e.key === "Enter" && discogsSearch());
// Typing switches the preset back to manual so a stale label doesn't mislead.
el("discogs-query").addEventListener("input", () => {
  el("query-preset").value = "manual";
});
el("query-preset").addEventListener("change", applyQueryPreset);
el("load-more").addEventListener("click", loadMoreResults);
el("search-per-page").addEventListener("change", (e) => {
  const v = parseInt(e.target.value, 10);
  try {
    localStorage.setItem(PERPAGE_STORAGE_KEY, String(v));
  } catch (err) {
    /* localStorage unavailable — preference just won't persist */
  }
  // Re-run from page 1 at the new page size if we already have results.
  if (releaseCandidates.length) discogsSearch();
});

// Media-type filter (#103): re-run the search from page 1 when it changes and
// there's already a query in play.
el("search-format").addEventListener("change", () => {
  if (releaseCandidates.length || el("discogs-query").value.trim()) discogsSearch();
});

// List/Grid layout toggle.
el("release-layout").addEventListener("click", (e) => {
  const btn = e.target.closest(".seg-btn");
  if (!btn || btn.classList.contains("active")) return;
  releaseLayout = btn.dataset.layout;
  el("release-layout").querySelectorAll(".seg-btn").forEach((b) => b.classList.toggle("active", b === btn));
  renderReleaseList();
});

// One delegated handler for every card interaction (they're re-rendered often).
el("release-list").addEventListener("click", (e) => {
  // The catalogue chip opens the release's provider page (#92) in both layouts;
  // caught before the tile/card handlers so it never also expands/switches.
  if (e.target.closest(".rb-catno")) {
    const host = e.target.closest("[data-id]");
    if (host) openReleasePage(host.dataset.id);
    return;
  }
  const tile = e.target.closest(".release-tile");
  if (tile) {
    // Grid tile → back to list layout, expanded on that release.
    releaseLayout = "list";
    el("release-layout").querySelectorAll(".seg-btn").forEach((b) => b.classList.toggle("active", b.dataset.layout === "list"));
    renderReleaseList();
    const card = el("release-list").querySelector(`.release-card[data-id="${cssEscape(tile.dataset.id)}"]`);
    if (card) toggleCard(card);
    return;
  }
  const card = e.target.closest(".release-card");
  if (!card) return;
  const act = e.target.closest("[data-act]")?.dataset.act;
  if (act === "save-menu") {
    // Split-button caret: toggle this card's save menu, closing any other.
    const menu = card.querySelector(".tk-save-menu");
    e.stopPropagation();
    document.querySelectorAll(".tk-save-menu").forEach((m) => {
      if (m !== menu) m.hidden = true;
    });
    if (menu) menu.hidden = !menu.hidden;
  } else if (act === "automatch") {
    autoMatchToRelease(card);
  } else if (act === "embed") {
    embedCoverFrom(card);
  } else if (act === "save-cover" || act === "save-all") {
    // Reached either from the plain single-image button or a menu item; dismiss
    // the menu so it doesn't hang open over the toast.
    const menu = card.querySelector(".tk-save-menu");
    if (menu) menu.hidden = true;
    saveReleaseImages(card, act === "save-all");
  } else if (act === "import") {
    importRelease(card);
  } else if (e.target.closest(".release-head")) {
    toggleCard(card);
  }
});

// Live "N / M selected" as track checkboxes toggle, plus the master checkbox
// that replaced the Enable all / Disable all pair: it drives every row from one
// control and re-derives its own tri-state from the tally.
el("release-list").addEventListener("change", (e) => {
  const card = e.target.closest(".release-card");
  if (!card) return;
  if (e.target.matches(".tk-selall-box")) {
    const on = e.target.checked;
    card.querySelectorAll(".release-tracklist tbody .tk-lead input").forEach((cb) => (cb.checked = on));
    updateTracklistCount(card);
  } else if (e.target.matches(".release-tracklist tbody .tk-lead input")) {
    updateTracklistCount(card);
  }
});

// Outside-click closes an open save menu, matching the Columns/Presets popovers.
document.addEventListener("click", (e) => {
  document.querySelectorAll(".tk-save-menu:not([hidden])").forEach((menu) => {
    if (!menu.contains(e.target) && !e.target.closest('[data-act="save-menu"]')) menu.hidden = true;
  });
});

export { loadSavedToken, refreshReleaseMatches, searchBusy, searchPerPage, stopLoading };
