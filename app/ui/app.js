"use strict";

// Bridge to the Rust command layer. Inside the Tauri webview this is the real
// IPC; in a plain browser (used to develop/verify the UI) it falls back to a
// small in-memory mock so the interface can be exercised without the native
// shell.
const TAURI = window.__TAURI__ ? window.__TAURI__.core : null;

async function invoke(cmd, args) {
  if (TAURI) return TAURI.invoke(cmd, args);
  return mockInvoke(cmd, args);
}

// ---- state ----
let tracks = [];
let previewPlan = null;
// What the current previewPlan came from, so apply() knows whether to clear
// pending tag edits ("edits") or remap them across a rename ("rename").
let previewSource = null;
// path -> Map(field -> newValue): pending tag changes not yet applied. Both
// inline cell edits and Discogs import feed this one buffer, so they compose
// into a single preview/apply. A value of "" means "clear the field".
const edits = new Map();

// Fields shown as editable columns, in table order.
const EDIT_FIELDS = ["artist", "title", "album", "year"];

// View state (does not change what's on disk). Sorting reorders the `tracks`
// array itself so position-based mapping (rename masks, Discogs import) follows
// the visible order; filtering only hides rows.
let sortKey = null; // "file" | one of EDIT_FIELDS
let sortDir = 1; // 1 asc, -1 desc
let filterText = "";
// Grouping is purely a view concern (#20): "" | "folder" | "artist" | "album".
// It regroups rows visually but never reorders the `tracks` array, so the file
// order used by mapping (rename masks, Discogs import) is unaffected. Collapsed
// group keys persist across renders.
let groupBy = "";
const collapsedGroups = new Set();

// ---- elements ----
const el = (id) => document.getElementById(id);
const rootInput = el("root");
const tracksBody = el("tracks-body");
const tracksEmpty = el("tracks-empty");
const trackCount = el("track-count");
const previewTable = el("preview-table");
const previewBody = el("preview-body");
const previewEmpty = el("preview-empty");
const applyBtn = el("apply");
const previewBtn = el("preview");
const previewEditsBtn = el("preview-edits");
const undoBtn = el("undo");
const selectAll = el("select-all");
const coverOpenBtn = el("cover-open");
const coverExportBtn = el("cover-export");
const coverFileInput = el("cover-file");
const discogsOpenBtn = el("discogs-open");
const discogsModal = el("discogs-modal");
const discogsResults = el("discogs-results");
const discogsEmpty = el("discogs-empty");
const playerBar = el("player");
const plToggle = el("pl-toggle");
const plStop = el("pl-stop");
const plTitle = el("pl-title");
const plSeek = el("pl-seek");
const plTime = el("pl-time");
// Playback runs in the native (rodio) backend; the UI mirrors its polled
// status. `playingPath` is the track the backend reports as current, `plPaused`
// its pause state, `plDuration` the current track's length (for the seek math).
let playingPath = null;
let plPaused = false;
let plDuration = 0;
// True while the user is dragging the seek slider, so status polls don't fight
// the drag.
let plSeeking = false;
// Poll timer handle (one interval once a library is open).
let plPollTimer = null;

const releaseTracksBody = el("release-tracks");
const releaseCoverImg = el("release-cover");
const coverEmbedBtn = el("cover-embed");
let currentRelease = null;
// The fetched cover of the currently open release, as a CoverArtDto ({ mime,
// data_base64 }), or null if the release has no image / it failed to load.
let currentReleaseCover = null;

// ---- helpers ----
function toast(message, isError) {
  const t = el("toast");
  t.textContent = message;
  t.classList.toggle("error", !!isError);
  t.hidden = false;
  clearTimeout(toast._timer);
  toast._timer = setTimeout(() => (t.hidden = true), 3200);
}

function fileName(path) {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1];
}

function tag(track, key) {
  return track.tags[key] || "";
}

// Paths of the checked rows, in `tracks` (mapping) order — NOT the visual/DOM
// order. This keeps position-based mapping (rename masks, Discogs import) tied
// to the real file order even when the view is grouped (#20). Identical to the
// visual order when ungrouped.
function selectedPaths() {
  const checked = new Set(
    [...tracksBody.querySelectorAll(".sel input:checked")].map((cb) => cb.dataset.path),
  );
  return tracks.filter((t) => checked.has(t.path)).map((t) => t.path);
}

function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    '"': "&quot;",
    "'": "&#39;",
  })[c]);
}

// ---- rendering ----
// Renders the table, overlaying any pending edits on top of the on-disk
// values (edited cells shown and marked dirty). Does NOT clear `edits` — call
// `resetEdits()` for that when loading fresh disk state.
function sortValue(track, key) {
  return (key === "file" ? fileName(track.path) : track.tags[key] || "").toLowerCase();
}

function matchesFilter(track) {
  if (!filterText) return true;
  const hay = [
    fileName(track.path),
    track.tags.artist,
    track.tags.title,
    track.tags.album,
    track.tags.year,
  ]
    .join(" ")
    .toLowerCase();
  return hay.includes(filterText);
}

function updateSortIndicators() {
  document.querySelectorAll("th.sortable").forEach((th) => {
    const ind = th.querySelector(".sort-ind");
    ind.textContent = th.dataset.sort === sortKey ? (sortDir > 0 ? "▲" : "▼") : "";
  });
}

// The grouping-key value for a track under the active `groupBy`.
function groupKeyOf(track) {
  switch (groupBy) {
    case "folder": {
      const i = Math.max(track.path.lastIndexOf("/"), track.path.lastIndexOf("\\"));
      return i >= 0 ? track.path.slice(0, i) : "";
    }
    case "artist":
      return track.tags.artist || "";
    case "album":
      return track.tags.album || "";
    default:
      return "";
  }
}

// Human label for a group header ("(no artist)" etc.; folder shows its name).
function groupLabel(key) {
  if (key === "") {
    return groupBy === "folder" ? "(no folder)" : `(no ${groupBy})`;
  }
  return groupBy === "folder" ? fileName(key) : key;
}

// Build one track row and append it to the body. `groupKey` (when grouping)
// tags the row so its group header can collapse it.
function appendTrackRow(track, groupKey) {
  const pending = edits.get(track.path);
  const tr = document.createElement("tr");
  tr.dataset.path = track.path;
  if (groupKey !== null) {
    tr.dataset.group = groupKey;
    if (collapsedGroups.has(groupKey)) tr.classList.add("hidden-row");
  }
  if (track.path === playingPath) tr.classList.add("playing");
  const playGlyph = track.path === playingPath && !plPaused ? "❚❚" : "▶";
  tr.innerHTML = `
      <td class="sel"><input type="checkbox" checked data-path="${escapeHtml(track.path)}" /></td>
      <td class="play"><button class="play-btn" data-path="${escapeHtml(track.path)}" title="Preview">${playGlyph}</button></td>
      <td class="mono file" title="${escapeHtml(track.path)}">${escapeHtml(fileName(track.path))}</td>`;
  for (const field of EDIT_FIELDS) {
    const original = tag(track, field);
    const edited = pending && pending.has(field);
    const value = edited ? pending.get(field) : original;
    const td = document.createElement("td");
    td.className = "editable";
    td.contentEditable = "true";
    td.spellcheck = false;
    td.dataset.path = track.path;
    td.dataset.field = field;
    td.dataset.original = original;
    td.textContent = value;
    if (edited && value !== original) td.classList.add("dirty");
    tr.appendChild(td);
  }
  tracksBody.appendChild(tr);
}

// A collapsible group header row spanning the table width.
function appendGroupHeader(key, count) {
  const collapsed = collapsedGroups.has(key);
  const tr = document.createElement("tr");
  tr.className = "group-head" + (collapsed ? " collapsed" : "");
  tr.dataset.group = key;
  tr.innerHTML = `<td class="group-cell" colspan="7">
      <span class="group-caret">${collapsed ? "▶" : "▼"}</span>
      <span class="group-label">${escapeHtml(groupLabel(key))}</span>
      <span class="group-count muted">${count}</span>
    </td>`;
  tracksBody.appendChild(tr);
}

function renderTracks() {
  tracksBody.innerHTML = "";
  updateSortIndicators();

  const visible = tracks.filter(matchesFilter);
  trackCount.textContent = tracks.length
    ? filterText
      ? `(${visible.length}/${tracks.length})`
      : `(${tracks.length})`
    : "";
  tracksEmpty.hidden = tracks.length > 0;

  if (groupBy) {
    // Groups in first-appearance order over the (mapping-ordered) track list,
    // so grouping never reorders the underlying files.
    const order = [];
    const byKey = new Map();
    for (const track of visible) {
      const key = groupKeyOf(track);
      if (!byKey.has(key)) {
        byKey.set(key, []);
        order.push(key);
      }
      byKey.get(key).push(track);
    }
    for (const key of order) {
      appendGroupHeader(key, byKey.get(key).length);
      for (const track of byKey.get(key)) appendTrackRow(track, key);
    }
  } else {
    for (const track of visible) appendTrackRow(track, null);
  }

  // Expand/Collapse all only make sense while grouped (#32).
  el("expand-all").hidden = !groupBy;
  el("collapse-all").hidden = !groupBy;

  previewBtn.disabled = tracks.length === 0;
  discogsOpenBtn.disabled = tracks.length === 0;
  coverOpenBtn.disabled = tracks.length === 0;
  coverExportBtn.disabled = tracks.length === 0;
  el("export-open").disabled = tracks.length === 0;
  el("fields-open").disabled = tracks.length === 0;
  el("move-open").disabled = tracks.length === 0;
  updateEditsButton();
  applyBtn.disabled = true;
  previewTable.hidden = true;
  previewEmpty.hidden = true;
}

function resetEdits() {
  edits.clear();
  updateEditsButton();
}

// Move pending edits from old paths to new paths after a rename is applied, so
// tag edits survive a rename instead of being orphaned by the path change.
function remapEditsAfterRename(plan) {
  for (const change of plan.changes) {
    if (change.rename_to && edits.has(change.path)) {
      edits.set(change.rename_to, edits.get(change.path));
      edits.delete(change.path);
    }
  }
}

function onCellEdit(td) {
  const { path, field, original } = td.dataset;
  const value = td.textContent.trim();
  if (value === original) {
    td.classList.remove("dirty");
    if (edits.has(path)) {
      edits.get(path).delete(field);
      if (edits.get(path).size === 0) edits.delete(path);
    }
  } else {
    td.classList.add("dirty");
    if (!edits.has(path)) edits.set(path, new Map());
    edits.get(path).set(field, value);
  }
  updateEditsButton();
}

function updateEditsButton() {
  previewEditsBtn.disabled = edits.size === 0;
}

function renderPreview(plan) {
  previewBody.innerHTML = "";
  const changes = plan.changes;
  if (changes.length === 0) {
    previewTable.hidden = true;
    previewEmpty.hidden = false;
    previewEmpty.textContent = "Nothing would change.";
    applyBtn.disabled = true;
    return;
  }
  for (const change of changes) {
    if (change.rename_to) {
      addPreviewRow(fileName(change.path), fileName(change.rename_to));
    }
    for (const tc of change.tag_changes) {
      const label = `${fileName(change.path)} · ${tc.field}`;
      addPreviewRow(`${label}: ${tc.old || "∅"}`, tc.new || "∅");
    }
    if (change.cover_change) {
      addCoverPreviewRow(change);
    }
  }
  previewTable.hidden = false;
  previewEmpty.hidden = true;
  applyBtn.disabled = false;
}

function addPreviewRow(oldText, newText) {
  const tr = document.createElement("tr");
  tr.innerHTML = `
    <td class="mono old">${escapeHtml(oldText)}</td>
    <td class="arrow">→</td>
    <td class="mono new">${escapeHtml(newText)}</td>`;
  previewBody.appendChild(tr);
}

function coverThumb(cover) {
  return cover ? `<img class="cover-thumb" src="data:${cover.mime};base64,${cover.data_base64}" />` : "∅";
}

function addCoverPreviewRow(change) {
  const cc = change.cover_change;
  const tr = document.createElement("tr");
  tr.innerHTML = `
    <td class="mono old">${escapeHtml(fileName(change.path))} · cover: ${coverThumb(cc.old)}</td>
    <td class="arrow">→</td>
    <td class="new">${coverThumb(cc.new)}</td>`;
  previewBody.appendChild(tr);
}

async function refreshHistory() {
  try {
    const batches = await invoke("history", {});
    undoBtn.disabled = batches.length === 0;
    undoBtn.textContent = batches.length ? `Undo (${batches.length})` : "Undo last";
  } catch (e) {
    /* history is best-effort in the toolbar */
  }
}

// ---- actions ----
async function openLibrary() {
  const root = rootInput.value.trim();
  if (!root) {
    toast("Enter a library path first", true);
    return;
  }
  try {
    await invoke("open_library", { root });
    tracks = await invoke("list_tracks", {});
    previewPlan = null;
    resetEdits();
    sortKey = null;
    sortDir = 1;
    filterText = "";
    el("filter").value = "";
    groupBy = "";
    collapsedGroups.clear();
    el("group-by").value = "";
    renderTracks();
    showPlayerBar();
    await refreshHistory();
    toast(`Opened ${root} — ${tracks.length} tracks`);
  } catch (e) {
    toast(String(e), true);
  }
}

async function preview() {
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast("Select at least one track", true);
    return;
  }
  try {
    previewPlan = await invoke("preview_rename", { mask: el("mask").value, paths });
    previewSource = "rename";
    renderPreview(previewPlan);
  } catch (e) {
    toast(String(e), true);
  }
}

async function previewEdits() {
  const list = [];
  for (const [path, fields] of edits) {
    for (const [field, value] of fields) {
      list.push({ path, field, value });
    }
  }
  if (list.length === 0) {
    previewEmpty.hidden = false;
    previewEmpty.textContent = "No pending edits.";
    previewTable.hidden = true;
    applyBtn.disabled = true;
    return;
  }
  try {
    previewPlan = await invoke("preview_tag_edits", { edits: list });
    previewSource = "edits";
    renderPreview(previewPlan);
  } catch (e) {
    toast(String(e), true);
  }
}

async function apply() {
  if (!previewPlan || previewPlan.changes.length === 0) return;
  const wasRename = previewSource === "rename";
  const wasEdits = previewSource === "edits";
  const appliedPlan = previewPlan;
  try {
    await invoke("apply_plan", { plan: appliedPlan });
    toast(`Applied changes to ${appliedPlan.changes.length} file(s)`);
    previewPlan = null;
    previewSource = null;
    if (wasRename) {
      remapEditsAfterRename(appliedPlan); // keep pending tag edits, new paths
    } else if (wasEdits) {
      resetEdits(); // tag edits are now on disk
    }
    // cover apply leaves the tag-edits buffer untouched (separate change kind)
    tracks = await invoke("list_tracks", {});
    renderTracks();
    await refreshHistory();
  } catch (e) {
    toast(String(e), true);
  }
}

async function undo() {
  try {
    const batches = await invoke("history", {});
    if (batches.length === 0) return;
    await invoke("undo", { batchId: batches[0].id });
    toast("Undid last batch");
    previewPlan = null;
    previewSource = null;
    resetEdits();
    tracks = await invoke("list_tracks", {});
    renderTracks();
    await refreshHistory();
  } catch (e) {
    toast(String(e), true);
  }
}

// ---- cover art ----
function chooseCover() {
  if (selectedPaths().length === 0) {
    toast("Select the tracks to embed the cover into first", true);
    return;
  }
  coverFileInput.value = ""; // allow re-picking the same file
  coverFileInput.click();
}

async function onCoverChosen() {
  const file = coverFileInput.files[0];
  if (!file) return;
  const dataUrl = await new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result);
    reader.onerror = reject;
    reader.readAsDataURL(file);
  });
  // dataUrl is "data:<mime>;base64,<data>"
  const comma = dataUrl.indexOf(",");
  const mime = dataUrl.slice(5, dataUrl.indexOf(";"));
  const data_base64 = dataUrl.slice(comma + 1);
  try {
    previewPlan = await invoke("preview_cover_embed", {
      paths: selectedPaths(),
      cover: { mime, data_base64 },
    });
    previewSource = "cover";
    renderPreview(previewPlan);
    toast(
      previewPlan.changes.length
        ? `Previewing cover on ${previewPlan.changes.length} file(s)`
        : "Selected files already have this cover"
    );
  } catch (e) {
    toast(String(e), true);
  }
}

// Export the embedded cover of the selected files to disk (cover.<ext> next to
// each file). Read-only for the audio: no preview/apply, it just writes.
async function exportCover() {
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast("Select the tracks whose cover to export first", true);
    return;
  }
  try {
    const result = await invoke("export_cover", { paths, basename: "cover" });
    const wrote = result.written.length;
    const skipped = result.skipped_no_cover.length;
    if (wrote === 0) {
      toast(
        skipped ? "None of the selected files have an embedded cover" : "Nothing to export",
        true
      );
      return;
    }
    const skipNote = skipped ? ` (${skipped} without a cover skipped)` : "";
    toast(`Exported ${wrote} cover file(s)${skipNote}`);
  } catch (e) {
    toast(String(e), true);
  }
}

// ---- preview player ----
// Playback is native (rodio backend, #30): the UI sends commands and polls the
// backend's status. Gapless + auto-advance happen in the backend, which keeps
// the current + next track queued in one sink; the UI just feeds the next track
// whenever the current one changes.

function fmtTime(seconds) {
  if (!isFinite(seconds) || seconds < 0) seconds = 0;
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${String(s).padStart(2, "0")}`;
}

function setPlayerControlsEnabled(on) {
  plToggle.disabled = !on;
  plStop.disabled = !on;
  plSeek.disabled = !on;
}

// Reveal the player bar (once a library is open) so its controls are always on
// screen, even with nothing loaded (#31), and start polling backend status.
function showPlayerBar() {
  playerBar.hidden = false;
  playerIdle();
  if (!plPollTimer) plPollTimer = setInterval(pollPlayerStatus, 300);
}

// Reset the UI to its idle, no-track state: controls disabled, placeholder
// title, zeroed time. The bar stays visible (#31). Used on stop, end of list,
// and when opening a library.
function playerIdle() {
  playingPath = null;
  plPaused = false;
  plDuration = 0;
  plTitle.textContent = "No track loaded";
  plTitle.title = "";
  plTime.textContent = "0:00 / 0:00";
  plSeek.value = "0";
  plToggle.textContent = "▶";
  playerBar.classList.add("idle");
  setPlayerControlsEnabled(false);
  markPlayingRow();
}

// The path of the next visible row after `path` in the current table order
// (respecting sort/filter/manual reorder — the DOM is the source of truth), or
// null if `path` is the last visible row.
function nextVisiblePath(path) {
  const rows = [...tracksBody.querySelectorAll("tr")];
  const i = rows.findIndex((r) => r.dataset.path === path);
  return i >= 0 && rows[i + 1] ? rows[i + 1].dataset.path : null;
}

// Start playing `path`. Clicking the already-current track toggles play/pause.
// Also primes the next visible track so the backend can play it gaplessly.
function playTrack(path) {
  if (path === playingPath) {
    togglePlay();
    return;
  }
  invoke("player_play", { path });
  const next = nextVisiblePath(path);
  if (next) invoke("player_set_next", { path: next });
  // Optimistic UI; the next poll confirms from the backend.
  playingPath = path;
  plPaused = false;
  plTitle.textContent = fileName(path);
  plTitle.title = path;
  playerBar.classList.remove("idle");
  setPlayerControlsEnabled(true);
  markPlayingRow();
}

function togglePlay() {
  if (!playingPath) return;
  plPaused = !plPaused;
  invoke(plPaused ? "player_pause" : "player_resume", {});
  markPlayingRow();
}

// Manual stop returns the bar to its idle state (still visible, #31).
function stopPlayback() {
  invoke("player_stop", {});
  playerIdle();
}

// Reflect the active track + play/pause state in the table without a full
// re-render (which would drop pending edits mid-typing).
function markPlayingRow() {
  tracksBody.querySelectorAll("tr").forEach((tr) => {
    const isPlaying = tr.dataset.path === playingPath;
    tr.classList.toggle("playing", isPlaying);
    const btn = tr.querySelector("td.play button");
    if (btn) btn.textContent = isPlaying && !plPaused ? "❚❚" : "▶";
  });
  plToggle.textContent = playingPath && !plPaused ? "❚❚" : "▶";
}

// Poll the backend and mirror its state. When the current track changes (a
// gapless transition, i.e. auto-advance #29), update the UI and feed the next
// track; when it wants a next track but none is queued, feed it too.
async function pollPlayerStatus() {
  let st;
  try {
    st = await invoke("player_status", {});
  } catch (e) {
    return;
  }
  const changed = st.path !== playingPath;
  playingPath = st.path;
  plPaused = st.is_paused;

  if (!st.path) {
    // Backend drained (end of list or stopped): go idle unless already idle.
    if (!playerBar.classList.contains("idle")) playerIdle();
    return;
  }

  if (changed) {
    plTitle.textContent = fileName(st.path);
    plTitle.title = st.path;
    playerBar.classList.remove("idle");
    setPlayerControlsEnabled(true);
    markPlayingRow();
  }
  // Keep the queue primed for gapless continuation.
  if (st.wants_next) {
    const next = nextVisiblePath(st.path);
    if (next) invoke("player_set_next", { path: next });
  }
  plDuration = st.duration_secs || 0;
  if (!plSeeking) {
    plSeek.value = plDuration
      ? String(Math.round((st.position_secs / plDuration) * 1000))
      : "0";
  }
  plTime.textContent = `${fmtTime(st.position_secs)} / ${fmtTime(plDuration)}`;
  plToggle.textContent = plPaused ? "▶" : "❚❚";
}

plToggle.addEventListener("click", togglePlay);
plStop.addEventListener("click", stopPlayback);
// While dragging, show the target time locally and suppress poll overrides;
// commit the seek to the backend on release.
plSeek.addEventListener("input", () => {
  plSeeking = true;
  const target = (Number(plSeek.value) / 1000) * plDuration;
  plTime.textContent = `${fmtTime(target)} / ${fmtTime(plDuration)}`;
});
plSeek.addEventListener("change", () => {
  const secs = (Number(plSeek.value) / 1000) * plDuration;
  invoke("player_seek", { secs });
  plSeeking = false;
});
// Play buttons live in dynamically-rendered rows — delegate.
tracksBody.addEventListener("click", (e) => {
  const btn = e.target.closest("td.play button");
  if (btn) playTrack(btn.dataset.path);
});

// ---- reorganize into folders (#37) ----
function openMove() {
  if (selectedPaths().length === 0) {
    toast("Select the tracks to move first", true);
    return;
  }
  el("move-modal").hidden = false;
}

function closeMove() {
  el("move-modal").hidden = true;
}

// Builds the plan and shows it in the usual preview panel, so the move is
// applied (and undone) through exactly the same path as a rename.
async function previewMove() {
  const paths = selectedPaths();
  try {
    previewPlan = await invoke("preview_move", { mask: el("move-mask").value, paths });
    previewSource = "rename";
    closeMove();
    renderPreview(previewPlan);
    toast(
      previewPlan.changes.length
        ? `Previewing move of ${previewPlan.changes.length} file(s) — click Apply`
        : "Nothing to move (check the pattern's tags are set)",
      previewPlan.changes.length === 0
    );
  } catch (e) {
    toast(String(e), true);
  }
}

// ---- extended field editor (#35) ----
// The table only edits four columns, but every field the model knows is already
// in `tracks[].tags` — this exposes the rest, including custom ones.
const EXTENDED_FIELDS = [
  ["artist", "Artist"],
  ["title", "Title"],
  ["album", "Album"],
  ["albumartist", "Album Artist"],
  ["track", "Track"],
  ["tracktotal", "Track Total"],
  ["disc", "Disc"],
  ["year", "Year"],
  ["genre", "Genre"],
  ["comment", "Comment"],
  ["composer", "Composer"],
  ["publisher", "Publisher"],
  ["bpm", "BPM"],
  ["isrc", "ISRC"],
  ["key", "Key"],
];

// Fields the user actually touched in the dialog, staged until they confirm.
let stagedFields = new Map();

// The value a file currently shows for a field: a pending edit if there is one,
// otherwise what's on disk.
function currentFieldValue(path, key) {
  const pending = edits.get(path);
  if (pending && pending.has(key)) return pending.get(key);
  const track = tracks.find((t) => t.path === path);
  return (track && track.tags[key]) || "";
}

function openFieldEditor() {
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast("Select the tracks to edit first", true);
    return;
  }
  stagedFields = new Map();
  el("fields-count").textContent = `${paths.length} file(s)`;
  el("fields-new-name").value = "";
  el("fields-new-value").value = "";
  renderFieldEditor(paths);
  el("fields-modal").hidden = false;
}

function closeFieldEditor() {
  el("fields-modal").hidden = true;
}

function renderFieldEditor(paths) {
  const body = el("fields-body");
  body.innerHTML = "";

  // Well-known fields plus any custom keys present anywhere in the selection —
  // both on disk and among pending edits, so a custom field staged a moment ago
  // is still listed when the dialog is reopened.
  const customs = new Set();
  for (const path of paths) {
    const track = tracks.find((t) => t.path === path);
    if (track) {
      for (const key of Object.keys(track.tags)) {
        if (key.startsWith("custom:")) customs.add(key);
      }
    }
    const pending = edits.get(path);
    if (pending) {
      for (const key of pending.keys()) {
        if (key.startsWith("custom:")) customs.add(key);
      }
    }
  }
  const rows = EXTENDED_FIELDS.concat(
    [...customs].sort().map((key) => [key, key.slice("custom:".length)])
  );

  for (const [key, label] of rows) {
    const values = new Set(paths.map((path) => currentFieldValue(path, key)));
    const shared = values.size === 1 ? [...values][0] : null;

    const tr = document.createElement("tr");
    const labelCell = document.createElement("td");
    labelCell.className = "field-label";
    labelCell.textContent = label;
    const valueCell = document.createElement("td");
    const input = document.createElement("input");
    input.type = "text";
    input.className = "field-input";
    input.dataset.key = key;
    input.spellcheck = false;
    if (shared === null) {
      // Differing values stay untouched unless the user types something.
      input.placeholder = "<multiple values>";
    } else {
      input.value = shared;
    }
    input.addEventListener("input", () => {
      stagedFields.set(key, input.value);
      input.classList.add("dirty");
    });
    valueCell.appendChild(input);
    tr.appendChild(labelCell);
    tr.appendChild(valueCell);
    body.appendChild(tr);
  }
}

// Add a custom field row; it stages immediately so an empty-valued custom field
// can still be created.
function addCustomField() {
  const name = el("fields-new-name").value.trim();
  if (!name) {
    toast("Name the custom field first", true);
    return;
  }
  const key = name.startsWith("custom:") ? name : `custom:${name}`;
  stagedFields.set(key, el("fields-new-value").value);
  toast(`Staged custom field "${name}" — press Stage changes to apply`);
  el("fields-new-name").value = "";
  el("fields-new-value").value = "";
}

// Push the staged fields into the shared pending-edits buffer for every
// selected file, then preview them alongside any other pending edits.
async function applyFieldEditor() {
  const paths = selectedPaths();
  if (stagedFields.size === 0) {
    closeFieldEditor();
    toast("No field changes to stage");
    return;
  }
  let changed = 0;
  for (const path of paths) {
    if (!edits.has(path)) edits.set(path, new Map());
    const fields = edits.get(path);
    for (const [key, value] of stagedFields) {
      // Skip no-ops so the preview stays honest.
      const track = tracks.find((t) => t.path === path);
      const onDisk = (track && track.tags[key]) || "";
      if (value === onDisk && !fields.has(key)) continue;
      fields.set(key, value);
      changed += 1;
    }
    if (fields.size === 0) edits.delete(path);
  }
  closeFieldEditor();
  renderTracks();
  await previewEdits();
  toast(
    changed
      ? `Staged ${stagedFields.size} field(s) across ${paths.length} file(s)`
      : "Nothing changed"
  );
}

// ---- exporters (#19) ----
// Default output name per export kind; the user can override it. The backend
// only accepts a bare file name and writes into the opened library.
const EXPORT_DEFAULTS = {
  playlist: "playlist.m3u",
  csv: "tags.csv",
  report: "report.txt",
};

function openExport() {
  const count = selectedPaths().length;
  if (count === 0) {
    toast("Select the tracks to export first", true);
    return;
  }
  el("export-count").textContent = `${count} track(s)`;
  syncExportKind();
  el("export-modal").hidden = false;
}

function closeExport() {
  el("export-modal").hidden = true;
}

// Show the mask field only for text reports, and reset the file name to the
// chosen kind's default.
function syncExportKind() {
  const kind = el("export-kind").value;
  el("export-mask-row").hidden = kind !== "report";
  el("export-name").value = EXPORT_DEFAULTS[kind];
}

async function runExport() {
  const paths = selectedPaths();
  const kind = el("export-kind").value;
  // Named `outName` so it doesn't shadow the `fileName()` helper used below.
  const outName = el("export-name").value.trim();
  try {
    let written;
    if (kind === "playlist") {
      written = await invoke("export_playlist", { paths, fileName: outName });
    } else if (kind === "csv") {
      written = await invoke("export_csv", { paths, fileName: outName });
    } else {
      written = await invoke("export_report", {
        paths,
        mask: el("export-mask").value,
        fileName: outName,
      });
    }
    closeExport();
    toast(`Exported ${paths.length} track(s) to ${fileName(written)}`);
  } catch (e) {
    toast(String(e), true);
  }
}

// ---- Discogs import ----
function openDiscogs() {
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast("Select the tracks to import onto first", true);
    return;
  }
  showDiscogsView("search");
  discogsModal.hidden = false;
  el("discogs-query").focus();
}

function closeDiscogs() {
  discogsModal.hidden = true;
}

async function discogsSearch() {
  const token = el("discogs-token").value.trim();
  const query = el("discogs-query").value.trim();
  if (!token) {
    toast("Enter your Discogs token", true);
    return;
  }
  // Remember the token locally so it's prefilled next time.
  invoke("save_discogs_token", { token }).catch(() => {});
  try {
    const candidates = await invoke("search_discogs", { token, query: { album: query } });
    renderCandidates(candidates);
  } catch (e) {
    toast(String(e), true);
  }
}

async function loadSavedToken() {
  try {
    const token = await invoke("saved_discogs_token", {});
    if (token) el("discogs-token").value = token;
  } catch (e) {
    /* no saved token yet */
  }
}

function renderCandidates(candidates) {
  discogsResults.innerHTML = "";
  discogsEmpty.hidden = candidates.length > 0;
  if (candidates.length === 0) {
    discogsEmpty.textContent = "No releases found.";
    return;
  }
  for (const c of candidates) {
    const tr = document.createElement("tr");
    const year = c.year ? ` · ${c.year}` : "";
    const artist = c.artist ? `${escapeHtml(c.artist)} — ` : "";
    tr.innerHTML = `<td>
        <div class="cand-title">${artist}${escapeHtml(c.title)}</div>
        <div class="cand-meta">Discogs #${escapeHtml(c.id)}${year}</div>
      </td>`;
    tr.addEventListener("click", () => openRelease(c.id));
    discogsResults.appendChild(tr);
  }
}

function showDiscogsView(which) {
  el("discogs-search-view").hidden = which !== "search";
  el("discogs-release-view").hidden = which !== "release";
}

async function openRelease(releaseId) {
  const token = el("discogs-token").value.trim();
  try {
    currentRelease = await invoke("fetch_discogs_release", { token, releaseId });
    renderReleaseTracks();
    const year = currentRelease.year ? ` (${currentRelease.year})` : "";
    el("release-title").textContent = `${currentRelease.artist} — ${currentRelease.title}${year}`;
    showDiscogsView("release");
    loadReleaseCover(token, currentRelease.cover_image_url);
  } catch (e) {
    toast(String(e), true);
  }
}

// Fetch the release's cover (bytes come through the backend, since the image
// URL needs Discogs auth headers the webview can't send) and show it. Failure
// is non-fatal: the tracklist import still works without a cover.
async function loadReleaseCover(token, url) {
  currentReleaseCover = null;
  releaseCoverImg.hidden = true;
  releaseCoverImg.removeAttribute("src");
  coverEmbedBtn.hidden = true;
  if (!url) return;
  try {
    const cover = await invoke("fetch_discogs_image", { token, url });
    currentReleaseCover = cover;
    releaseCoverImg.src = `data:${cover.mime};base64,${cover.data_base64}`;
    releaseCoverImg.hidden = false;
    coverEmbedBtn.hidden = false;
  } catch (e) {
    // Leave the cover hidden; the import flow is unaffected.
  }
}

// Embed the fetched release cover into the files selected in the main table,
// routing through the same preview/apply/undo path as a locally chosen image.
async function embedReleaseCover() {
  if (!currentReleaseCover) return;
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast("Select the tracks to embed the cover into first", true);
    return;
  }
  try {
    previewPlan = await invoke("preview_cover_embed", { paths, cover: currentReleaseCover });
    previewSource = "cover";
    closeDiscogs();
    renderPreview(previewPlan);
    toast(
      previewPlan.changes.length
        ? `Previewing cover on ${previewPlan.changes.length} file(s) — click Apply`
        : "Selected files already have this cover"
    );
  } catch (e) {
    toast(String(e), true);
  }
}

function renderReleaseTracks() {
  releaseTracksBody.innerHTML = "";
  currentRelease.tracks.forEach((t, i) => {
    const tr = document.createElement("tr");
    tr.innerHTML = `
      <td class="sel"><input type="checkbox" checked data-i="${i}" /></td>
      <td class="mono">${escapeHtml(t.position)}</td>
      <td>${escapeHtml(t.artist || currentRelease.artist)}</td>
      <td title="${escapeHtml(t.title)}">${escapeHtml(t.title)}</td>
      <td class="mono muted">${t.duration_secs ? fmtTime(t.duration_secs) : "—"}</td>`;
    releaseTracksBody.appendChild(tr);
  });
}

// Reorder the selected files so they line up with this release's tracklist,
// matching on title/artist instead of trusting the order they happen to be in
// (#53). Import maps the i-th enabled track onto the i-th file, so getting the
// file order right is what stops a whole album being tagged one title out.
async function autoMatchTracks() {
  const paths = selectedPaths();
  if (!currentRelease || paths.length === 0) {
    toast("Select the tracks to match against first", true);
    return;
  }
  const releaseTracks = currentRelease.tracks.map((t) => ({
    position: t.position,
    artist: t.artist || currentRelease.artist,
    title: t.title,
    duration_secs: t.duration_secs ?? null,
  }));
  try {
    const aligned = await invoke("auto_align", { paths, tracks: releaseTracks });
    // Unmatched files sort to the end rather than scrambling the rest.
    const ranked = paths.map((path, i) => ({
      path,
      key: aligned[i] === null || aligned[i] === undefined ? Number.MAX_SAFE_INTEGER : aligned[i],
    }));
    ranked.sort((a, b) => a.key - b.key);

    // Rewrite `tracks` in place: selected files take their new relative order,
    // unselected ones stay exactly where they are.
    const byPath = new Map(tracks.map((t) => [t.path, t]));
    const selected = new Set(paths);
    let next = 0;
    tracks = tracks.map((t) => (selected.has(t.path) ? byPath.get(ranked[next++].path) : t));
    sortKey = null; // a manual order supersedes any column sort
    renderTracks();

    const matched = aligned.filter((i) => i !== null && i !== undefined).length;
    toast(
      matched
        ? `Matched ${matched}/${paths.length} file(s) — reordered to line up`
        : "No confident matches — leaving the order alone",
      matched === 0
    );
  } catch (e) {
    toast(String(e), true);
  }
}

function enabledReleaseTracks() {
  return [...releaseTracksBody.querySelectorAll(".sel input:checked")].map((cb) => {
    const t = currentRelease.tracks[Number(cb.dataset.i)];
    return {
      position: t.position,
      artist: t.artist || currentRelease.artist,
      title: t.title,
      duration_secs: t.duration_secs ?? null,
    };
  });
}

async function applyImport() {
  const paths = selectedPaths();
  // Prefer Discogs "styles" (e.g. Trance/Tribal/Techno) over the coarse
  // "genres" (e.g. Electronic) for the genre tag — styles are closer to what a
  // genre tag usually means (#26). Fall back to genres when a release has no
  // styles. Multiple values are joined with "/" (no spaces), matching the
  // existing convention in the user's library.
  const genreValues = currentRelease.styles.length ? currentRelease.styles : currentRelease.genres;
  const selection = {
    album: currentRelease.title,
    album_artist: currentRelease.artist,
    year: currentRelease.year ? String(currentRelease.year) : null,
    genre: genreValues.join("/") || null,
    tracks: enabledReleaseTracks(),
  };
  try {
    const plan = await invoke("preview_import", { paths, selection });
    // Merge the import into the pending-edits buffer instead of replacing the
    // preview. A field the user already edited by hand wins (we don't
    // overwrite an existing entry), so manual edits aren't clobbered.
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
    closeDiscogs();
    renderTracks(); // imported values now show as pending edits in the table
    await previewEdits(); // one unified preview of all pending edits
    toast(
      merged
        ? `Merged ${merged} field change(s) from Discogs into pending edits`
        : "Nothing new to import from this release"
    );
  } catch (e) {
    toast(String(e), true);
  }
}

// ---- wire up ----
el("open").addEventListener("click", openLibrary);
previewBtn.addEventListener("click", preview);
previewEditsBtn.addEventListener("click", previewEdits);
applyBtn.addEventListener("click", apply);
undoBtn.addEventListener("click", undo);
coverOpenBtn.addEventListener("click", chooseCover);
coverExportBtn.addEventListener("click", exportCover);
el("move-open").addEventListener("click", openMove);
el("move-close").addEventListener("click", closeMove);
el("move-preview").addEventListener("click", previewMove);
el("move-modal").addEventListener("click", (e) => {
  if (e.target === el("move-modal")) closeMove();
});
el("fields-open").addEventListener("click", openFieldEditor);
el("fields-close").addEventListener("click", closeFieldEditor);
el("fields-add").addEventListener("click", addCustomField);
el("fields-apply").addEventListener("click", applyFieldEditor);
el("fields-modal").addEventListener("click", (e) => {
  if (e.target === el("fields-modal")) closeFieldEditor();
});
el("export-open").addEventListener("click", openExport);
el("export-close").addEventListener("click", closeExport);
el("export-kind").addEventListener("change", syncExportKind);
el("export-run").addEventListener("click", runExport);
el("export-modal").addEventListener("click", (e) => {
  if (e.target === el("export-modal")) closeExport();
});
coverFileInput.addEventListener("change", onCoverChosen);
discogsOpenBtn.addEventListener("click", openDiscogs);
el("discogs-close").addEventListener("click", closeDiscogs);
el("discogs-search").addEventListener("click", discogsSearch);
el("discogs-query").addEventListener("keydown", (e) => e.key === "Enter" && discogsSearch());
el("discogs-back").addEventListener("click", () => showDiscogsView("search"));
el("import-apply").addEventListener("click", applyImport);
coverEmbedBtn.addEventListener("click", embedReleaseCover);
el("auto-match").addEventListener("click", autoMatchTracks);
el("tracks-all").addEventListener("click", () => toggleReleaseTracks(true));
el("tracks-none").addEventListener("click", () => toggleReleaseTracks(false));
discogsModal.addEventListener("click", (e) => {
  if (e.target === discogsModal) closeDiscogs();
});

function toggleReleaseTracks(on) {
  releaseTracksBody.querySelectorAll(".sel input").forEach((cb) => (cb.checked = on));
}

// ---- reorder files by dragging the File cell ----
// Implemented with mouse events (not HTML5 drag-and-drop) because WKWebView,
// the macOS Tauri webview, doesn't drive native DnD reliably. Mouse events
// behave identically everywhere.
let dragPath = null;
// Where the dragged row would land: { path: hovered row, below: true if it
// drops after that row (cursor in its lower half), false if before it }.
let dropInfo = null;

function rowUnder(clientX, clientY) {
  const el = document.elementFromPoint(clientX, clientY);
  return el && el.closest("#tracks-body tr");
}

function clearDropMarkers() {
  tracksBody.querySelectorAll("tr").forEach((tr) => tr.classList.remove("drop-above", "drop-below"));
}

tracksBody.addEventListener("mousedown", (e) => {
  const cell = e.target.closest("td.file");
  if (!cell) return;
  e.preventDefault(); // don't start a text selection
  dragPath = cell.closest("tr").dataset.path;
  cell.closest("tr").classList.add("dragging");
  document.addEventListener("mousemove", onDragMove);
  document.addEventListener("mouseup", onDragUp);
});

function onDragMove(e) {
  clearDropMarkers();
  dropInfo = null;
  const row = rowUnder(e.clientX, e.clientY);
  if (!row || !row.dataset.path || row.dataset.path === dragPath) return; // skip group headers
  const rect = row.getBoundingClientRect();
  const below = e.clientY > rect.top + rect.height / 2;
  row.classList.add(below ? "drop-below" : "drop-above");
  dropInfo = { path: row.dataset.path, below };
}

function onDragUp() {
  document.removeEventListener("mousemove", onDragMove);
  document.removeEventListener("mouseup", onDragUp);
  clearDropMarkers();
  tracksBody.querySelectorAll("tr").forEach((tr) => tr.classList.remove("dragging"));
  const source = dragPath;
  const drop = dropInfo;
  dragPath = null;
  dropInfo = null;
  if (!source || !drop) return;

  const from = tracks.findIndex((t) => t.path === source);
  if (from < 0) return;
  const [moved] = tracks.splice(from, 1);
  const targetIndex = tracks.findIndex((t) => t.path === drop.path);
  if (targetIndex < 0) {
    tracks.splice(from, 0, moved); // target vanished; put it back
    return;
  }
  tracks.splice(drop.below ? targetIndex + 1 : targetIndex, 0, moved);
  sortKey = null; // manual order supersedes any column sort
  renderTracks();
}
rootInput.addEventListener("keydown", (e) => e.key === "Enter" && openLibrary());
selectAll.addEventListener("change", () => {
  tracksBody
    .querySelectorAll(".sel input[type=checkbox]")
    .forEach((cb) => (cb.checked = selectAll.checked));
});

// Sort by clicking a column header (toggles direction). Reorders `tracks`
// itself so position-based mapping follows the visible order.
function sortBy(key) {
  if (sortKey === key) sortDir = -sortDir;
  else {
    sortKey = key;
    sortDir = 1;
  }
  tracks.sort(
    (a, b) =>
      sortValue(a, key).localeCompare(sortValue(b, key), undefined, { numeric: true }) * sortDir,
  );
  renderTracks();
}

document.querySelectorAll("th.sortable").forEach((th) => {
  th.addEventListener("click", () => sortBy(th.dataset.sort));
});

// Grouping is a view overlay — changing it only re-renders, never reorders
// `tracks`. Collapsed state is per grouping, so reset it on change.
el("group-by").addEventListener("change", (e) => {
  groupBy = e.target.value;
  collapsedGroups.clear();
  renderTracks();
});

// Collapse/expand a group by clicking its header (no re-render, so selection
// and in-progress edits are preserved).
function toggleGroup(key) {
  const collapse = !collapsedGroups.has(key);
  if (collapse) collapsedGroups.add(key);
  else collapsedGroups.delete(key);
  tracksBody.querySelectorAll("tr").forEach((tr) => {
    if (tr.dataset.group !== key) return;
    if (tr.classList.contains("group-head")) {
      tr.classList.toggle("collapsed", collapse);
      const caret = tr.querySelector(".group-caret");
      if (caret) caret.textContent = collapse ? "▶" : "▼";
    } else {
      tr.classList.toggle("hidden-row", collapse);
    }
  });
}
tracksBody.addEventListener("click", (e) => {
  const head = e.target.closest("tr.group-head");
  if (head) toggleGroup(head.dataset.group);
});

// Expand/collapse every group at once (#32), reusing the same in-place update
// as individual headers so selection and in-progress edits survive.
function setAllGroupsCollapsed(collapse) {
  collapsedGroups.clear();
  if (collapse) {
    tracksBody
      .querySelectorAll("tr.group-head")
      .forEach((head) => collapsedGroups.add(head.dataset.group));
  }
  tracksBody.querySelectorAll("tr").forEach((tr) => {
    if (tr.dataset.group === undefined) return;
    if (tr.classList.contains("group-head")) {
      tr.classList.toggle("collapsed", collapse);
      const caret = tr.querySelector(".group-caret");
      if (caret) caret.textContent = collapse ? "▶" : "▼";
    } else {
      tr.classList.toggle("hidden-row", collapse);
    }
  });
}
el("expand-all").addEventListener("click", () => setAllGroupsCollapsed(false));
el("collapse-all").addEventListener("click", () => setAllGroupsCollapsed(true));

el("filter").addEventListener("input", (e) => {
  filterText = e.target.value.trim().toLowerCase();
  renderTracks();
});
// Track edits on any editable cell (event delegation).
tracksBody.addEventListener("input", (e) => {
  if (e.target.classList.contains("editable")) onCellEdit(e.target);
});
// Enter commits a cell instead of inserting a newline.
tracksBody.addEventListener("keydown", (e) => {
  if (e.target.classList.contains("editable") && e.key === "Enter") {
    e.preventDefault();
    e.target.blur();
  }
});

loadSavedToken();

// Browser-only fake of the native player: a wall-clock timer advances position,
// auto-advances to the queued `next` on end, and reports status — enough to
// exercise the polling/gapless-feed UI without the rodio backend. Uses a short
// fixed duration so transitions are quick to observe.
const mockPlayer = {
  current: null,
  next: null,
  duration: 600, // seconds (long, so tests aren't raced by natural track end)
  base: 0, // position at last (re)start
  started: 0, // Date.now() when the current run began
  pausedAt: 0, // Date.now() when paused, else 0
  position() {
    if (!this.current) return 0;
    const now = this.pausedAt || Date.now();
    return this.base + (now - this.started) / 1000;
  },
  restart(base = 0) {
    this.base = base;
    this.started = Date.now();
    this.pausedAt = 0;
  },
  status() {
    if (this.current) {
      // Advance across the (gapless) boundary when the current track ends.
      if (this.position() >= this.duration) {
        if (this.next) {
          this.current = this.next;
          this.next = null;
          this.restart(0);
        } else {
          this.current = null;
        }
      }
    }
    return {
      path: this.current,
      is_paused: !!this.pausedAt,
      position_secs: this.current ? Math.min(this.position(), this.duration) : 0,
      duration_secs: this.current ? this.duration : 0,
      wants_next: !!this.current && !this.next,
    };
  },
};

// ---- browser-only mock (no effect inside Tauri) ----
function mockInvoke(cmd, args) {
  mockInvoke.state = mockInvoke.state || {
    tracks: [
      { path: "/music/01 - the x factor - desert rain.mp3", format: "Mp3", tags: { artist: "The X Factor", title: "Desert Rain", album: "La Bush", year: "1996" } },
      { path: "/music/02 - wish mountain - radio.mp3", format: "Mp3", tags: { artist: "Wish Mountain", title: "Radio", album: "La Bush", year: "1996" } },
      { path: "/music/03 - u-hi - feel it.mp3", format: "Mp3", tags: { artist: "U-Hi?", title: "Feel It", album: "La Bush", year: "1996" } },
    ],
    history: [],
  };
  const s = mockInvoke.state;
  const findTrack = (p) => s.tracks.find((x) => x.path === p);
  switch (cmd) {
    case "open_library":
      return Promise.resolve();
    case "list_tracks":
      return Promise.resolve(s.tracks);
    case "preview_rename": {
      const changes = args.paths
        .map((p) => {
          const t = findTrack(p);
          if (!t) return null;
          const dir = p.slice(0, p.lastIndexOf("/") + 1);
          const ext = p.slice(p.lastIndexOf("."));
          const name = args.mask
            .replace("%artist%", t.tags.artist || "")
            .replace("%title%", t.tags.title || "")
            .replace("%album%", t.tags.album || "")
            .replace("%year%", t.tags.year || "");
          const rename_to = dir + name + ext;
          return rename_to === p ? null : { path: p, rename_to, tag_changes: [] };
        })
        .filter(Boolean);
      return Promise.resolve({ description: "Rename by mask", changes });
    }
    case "preview_move": {
      const changes = args.paths
        .map((p) => {
          const t = findTrack(p);
          if (!t) return null;
          const ext = p.slice(p.lastIndexOf("."));
          const rendered = args.mask
            .replace("%albumartist%", t.tags.albumartist || t.tags.artist || "")
            .replace("%artist%", t.tags.artist || "")
            .replace("%title%", t.tags.title || "")
            .replace("%album%", t.tags.album || "")
            .replace("%year%", t.tags.year || "")
            .replace("%track%", t.tags.track || "")
            .replace("%genre%", t.tags.genre || "");
          if (rendered.split("/").some((part) => !part.trim() || part === "..")) return null;
          return { path: p, rename_to: `/music/${rendered}${ext}`, tag_changes: [] };
        })
        .filter(Boolean);
      return Promise.resolve({ description: "Reorganize by mask", changes });
    }
    case "preview_tag_edits": {
      const byPath = {};
      for (const e of args.edits) {
        const t = findTrack(e.path);
        if (!t) continue;
        const old = t.tags[e.field] || null;
        const nv = e.value || null;
        if (old === nv) continue;
        (byPath[e.path] = byPath[e.path] || []).push({ field: e.field, old, new: nv });
      }
      const changes = Object.entries(byPath).map(([path, tag_changes]) => ({ path, rename_to: null, tag_changes }));
      return Promise.resolve({ description: "Edit tags", changes });
    }
    case "apply_plan":
      for (const c of args.plan.changes) {
        const t = findTrack(c.path);
        if (!t) continue;
        if (c.rename_to) t.path = c.rename_to;
        for (const tc of c.tag_changes) t.tags[tc.field] = tc.new || "";
      }
      s.history.unshift({ id: s.history.length + 1, description: args.plan.description, applied_at: 0 });
      return Promise.resolve({ id: s.history.length, description: args.plan.description, applied_at: 0 });
    case "history":
      return Promise.resolve(s.history);
    case "undo":
      s.history.shift();
      return Promise.resolve();
    case "preview_cover_embed": {
      const changes = args.paths.map((p) => ({
        path: p,
        rename_to: null,
        tag_changes: [],
        cover_change: { old: null, new: args.cover },
      }));
      return Promise.resolve({ description: "Embed cover art", changes });
    }
    case "export_cover": {
      // Pretend odd-indexed files have no cover so the skip path is exercised;
      // dedupe same-folder targets like the real backend does.
      const written = [];
      const seen = new Set();
      const skipped_no_cover = [];
      args.paths.forEach((p, i) => {
        if (i % 2 !== 0) {
          skipped_no_cover.push(p);
          return;
        }
        const dir = p.slice(0, p.lastIndexOf("/") + 1);
        const target = `${dir}${args.basename}.jpg`;
        if (!seen.has(target)) {
          seen.add(target);
          written.push(target);
        }
      });
      return Promise.resolve({ written, skipped_no_cover });
    }
    case "auto_align": {
      // Mock: match by exact title, mirroring what the backend does on real data.
      const titles = args.tracks.map((t) => t.title.toLowerCase());
      return Promise.resolve(
        args.paths.map((p) => {
          const t = findTrack(p);
          const i = t ? titles.indexOf((t.tags.title || "").toLowerCase()) : -1;
          return i >= 0 ? i : null;
        })
      );
    }
    case "export_playlist":
    case "export_csv":
    case "export_report":
      // The real backend writes into the library root and returns the path.
      return Promise.resolve(`/music/${args.fileName}`);
    case "player_play":
      mockPlayer.current = args.path;
      mockPlayer.next = null;
      mockPlayer.restart(0);
      return Promise.resolve();
    case "player_set_next":
      if (mockPlayer.current && !mockPlayer.next) mockPlayer.next = args.path;
      return Promise.resolve();
    case "player_pause":
      if (mockPlayer.current && !mockPlayer.pausedAt) mockPlayer.pausedAt = Date.now();
      return Promise.resolve();
    case "player_resume":
      if (mockPlayer.pausedAt) {
        mockPlayer.restart(mockPlayer.position());
      }
      return Promise.resolve();
    case "player_stop":
      mockPlayer.current = null;
      mockPlayer.next = null;
      mockPlayer.pausedAt = 0;
      return Promise.resolve();
    case "player_seek":
      if (mockPlayer.current) mockPlayer.restart(args.secs);
      return Promise.resolve();
    case "player_status":
      return Promise.resolve(mockPlayer.status());
    case "saved_discogs_token":
      return Promise.resolve("");
    case "save_discogs_token":
      return Promise.resolve();
    case "search_discogs":
      return Promise.resolve([
        { id: "316795", artist: "Various", title: "La Bush - Music From The Temple Of House", year: 1996, score: 1.0 },
        { id: "764414", artist: "Various", title: "La Bush Vol. 4", year: 1997, score: 0.9 },
      ]);
    case "fetch_discogs_release":
      return Promise.resolve({
        id: args.releaseId,
        artist: "Various",
        title: "La Bush - Music From The Temple Of House",
        year: 1996,
        genres: ["Electronic"],
        styles: ["Trance", "Tribal", "Techno"],
        tracks: [
          { position: "1", artist: "The X Factor", title: "Desert Rain", duration_secs: 278 },
          { position: "2", artist: "Wish Mountain", title: "Radio", duration_secs: 142 },
          { position: "3", artist: "West Coast Connection", title: "Voodoo Rhythm", duration_secs: 321 },
        ],
        cover_image_url: "https://img.discogs.com/mock/front.jpg",
      });
    case "fetch_discogs_image":
      // A tiny solid-color PNG so the release-view cover has something to show.
      return Promise.resolve({
        mime: "image/png",
        data_base64:
          "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
      });
    case "preview_import": {
      const changes = args.paths.map((p, i) => {
        const t = findTrack(p);
        const rt = args.selection.tracks[i];
        const tag_changes = [
          { field: "album", old: t ? t.tags.album || null : null, new: args.selection.album },
        ];
        if (args.selection.genre) {
          tag_changes.push({ field: "genre", old: t ? t.tags.genre || null : null, new: args.selection.genre });
        }
        if (rt) {
          tag_changes.push({ field: "title", old: t ? t.tags.title || null : null, new: rt.title });
          tag_changes.push({ field: "artist", old: t ? t.tags.artist || null : null, new: rt.artist });
          tag_changes.push({ field: "track", old: t ? t.tags.track || null : null, new: rt.position });
        }
        return { path: p, rename_to: null, tag_changes };
      });
      return Promise.resolve({ description: "Import Discogs release", changes });
    }
    default:
      return Promise.reject(`unknown command: ${cmd}`);
  }
}
