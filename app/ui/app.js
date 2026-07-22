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

// The set of selected file paths — the single source of truth for what every
// mode operates on. Kept here (not in the DOM) so a re-render (sort, reorder,
// auto-match, staging edits) never silently wipes or widens the selection.
const selection = new Set();

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
const statusSel = el("status-sel");
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

// Selected paths in `tracks` (mapping) order — NOT the visual/DOM order. This
// keeps position-based mapping (rename masks, Discogs import) tied to the real
// file order even when the view is grouped (#20). Reads the `selection` set, so
// it survives re-renders.
function selectedPaths() {
  return tracks.filter((t) => selection.has(t.path)).map((t) => t.path);
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
  // Checkbox + row highlight both reflect the `selection` set (source of truth),
  // so re-rendering never changes what's selected.
  const isSel = selection.has(track.path);
  if (isSel) tr.classList.add("selected");
  const playGlyph = track.path === playingPath && !plPaused ? "❚❚" : "▶";
  tr.innerHTML = `
      <td class="sel"><input type="checkbox" ${isSel ? "checked" : ""} data-path="${escapeHtml(track.path)}" /></td>
      <td class="play"><button class="play-btn" data-path="${escapeHtml(track.path)}" title="Preview">${playGlyph}</button></td>
      <td class="file" title="${escapeHtml(track.path)}">${escapeHtml(fileName(track.path))}</td>`;
  for (const field of EDIT_FIELDS) {
    const original = tag(track, field);
    const edited = pending && pending.has(field);
    const value = edited ? pending.get(field) : original;
    const td = document.createElement("td");
    td.className = "editable";
    // Not editable until double-clicked (single click selects the row).
    td.contentEditable = "false";
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
  updateEditsButton();
  syncSelectionUI();
  refreshRoving();
}

// Selection count in the status bar ("N/M selected"). Uses the checked-row
// count directly; total size/duration are deferred (#27 notes → their own issue).
function updateStatus() {
  const total = tracks.length;
  const selected = selectedPaths().length;
  statusSel.textContent = total ? `${selected}/${total} selected` : "";
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

// Switch the files column between the table ("files") and the change-plan diff
// ("preview"). The Preview tab is only reachable while a plan is staged.
function showView(which) {
  const preview = which === "preview";
  el("files-view").hidden = preview;
  el("preview-view").hidden = !preview;
  el("view-files").classList.toggle("active", !preview);
  el("view-preview").classList.toggle("active", preview);
  el("view-preview").disabled = !preview && !previewPlan;
}

function discardPreview() {
  // A preview built from the pending-edits buffer (inline edits + Discogs
  // import) owns that buffer, so discarding it must also drop those staged
  // values and repaint the table; other previews just drop the plan.
  const wasEdits = previewSource === "edits";
  previewPlan = null;
  previewSource = null;
  el("view-preview").disabled = true;
  if (wasEdits) {
    resetEdits();
    renderTracks();
  }
  showView("files");
}

function renderPreview(plan) {
  previewBody.innerHTML = "";
  el("view-preview").disabled = false;
  showView("preview");
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
    // Everything selected by default; the set (not the DOM) holds it.
    selection.clear();
    for (const t of tracks) selection.add(t.path);
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
    el("view-preview").disabled = true;
    showView("files");
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
    el("view-preview").disabled = true;
    showView("files");
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
    el("view-preview").disabled = true;
    showView("files");
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

// ---- transformations (#34) ----
// An ordered chain of cleanup rules applied to tags or filenames. The rules
// live here only for the length of the dialog; naming and saving chains is
// tracked separately (#57).
let transformRules = [];

// Refresh the GENERATOR panel for the current selection (called on entering the
// mode). The rule chain persists across mode switches within a session.
function refreshGenerator() {
  const count = selectedPaths().length;
  el("transform-count").textContent = count ? `— ${count} file(s)` : "";
  renderTransformRules();
}

function addTransformRule() {
  const kind = el("transform-kind").value;
  transformRules.push({
    kind,
    from: "",
    to: "",
    regex: false,
    whole_word: false,
    case_sensitive: false,
    style: kind === "case" ? "title" : "",
  });
  renderTransformRules();
}

function renderTransformRules() {
  const body = el("transform-rules");
  body.innerHTML = "";
  el("transform-empty").hidden = transformRules.length > 0;

  transformRules.forEach((rule, index) => {
    const tr = document.createElement("tr");
    const cell = document.createElement("td");
    cell.className = "rule-cell";

    const header = document.createElement("div");
    header.className = "rule-head";
    header.innerHTML = `<span class="rule-index">${index + 1}</span>`;

    const label = document.createElement("span");
    label.textContent =
      rule.kind === "replace"
        ? "Find and replace"
        : rule.kind === "case"
          ? "Change case"
          : "Remove diacritics";
    header.appendChild(label);

    const spacer = document.createElement("div");
    spacer.className = "spacer";
    header.appendChild(spacer);

    // Order matters — case conversion before or after an acronym fix gives
    // different results — so rules can be moved.
    const up = document.createElement("button");
    up.className = "icon";
    up.textContent = "↑";
    up.title = "Move up";
    up.disabled = index === 0;
    up.addEventListener("click", () => {
      [transformRules[index - 1], transformRules[index]] = [
        transformRules[index],
        transformRules[index - 1],
      ];
      renderTransformRules();
    });
    const down = document.createElement("button");
    down.className = "icon";
    down.textContent = "↓";
    down.title = "Move down";
    down.disabled = index === transformRules.length - 1;
    down.addEventListener("click", () => {
      [transformRules[index + 1], transformRules[index]] = [
        transformRules[index],
        transformRules[index + 1],
      ];
      renderTransformRules();
    });
    const remove = document.createElement("button");
    remove.className = "icon";
    remove.textContent = "✕";
    remove.title = "Remove rule";
    remove.addEventListener("click", () => {
      transformRules.splice(index, 1);
      renderTransformRules();
    });
    header.append(up, down, remove);
    cell.appendChild(header);

    if (rule.kind === "replace") {
      const row = document.createElement("div");
      row.className = "rule-body";
      const from = document.createElement("input");
      from.type = "text";
      from.placeholder = "find";
      from.value = rule.from;
      from.spellcheck = false;
      from.addEventListener("input", () => (rule.from = from.value));
      const to = document.createElement("input");
      to.type = "text";
      to.placeholder = "replace with";
      to.value = rule.to;
      to.spellcheck = false;
      to.addEventListener("input", () => (rule.to = to.value));
      row.append(from, to);

      for (const [key, text, hint] of [
        ["regex", "regex", "Treat the pattern as a regular expression"],
        ["whole_word", "whole word", "Only match complete words"],
        ["case_sensitive", "match case", "Distinguish upper and lower case"],
      ]) {
        const label = document.createElement("label");
        label.className = "rule-flag muted";
        label.title = hint;
        const box = document.createElement("input");
        box.type = "checkbox";
        box.checked = rule[key];
        box.addEventListener("change", () => (rule[key] = box.checked));
        label.append(box, document.createTextNode(text));
        row.appendChild(label);
      }
      cell.appendChild(row);
    } else if (rule.kind === "case") {
      const row = document.createElement("div");
      row.className = "rule-body";
      const style = document.createElement("select");
      for (const [value, text] of [
        ["title", "Title Case"],
        ["lower", "lower case"],
        ["upper", "UPPER CASE"],
        ["sentence", "Sentence case"],
      ]) {
        const option = document.createElement("option");
        option.value = value;
        option.textContent = text;
        style.appendChild(option);
      }
      style.value = rule.style;
      style.addEventListener("change", () => (rule.style = style.value));
      row.appendChild(style);
      const note = document.createElement("span");
      note.className = "muted";
      note.textContent = "Known acronyms and roman numerals keep their casing.";
      row.appendChild(note);
      cell.appendChild(row);
    }

    tr.appendChild(cell);
    body.appendChild(tr);
  });
}

async function previewTransform() {
  const paths = selectedPaths();
  if (transformRules.length === 0) {
    toast("Add at least one rule", true);
    return;
  }
  try {
    previewPlan = await invoke("preview_transform", {
      paths,
      rules: transformRules,
      scope: el("transform-scope").value,
    });
    // A filename transform is a rename; a tag transform is an edit. Either way
    // it applies through the normal preview/apply/undo path.
    previewSource = el("transform-scope").value === "filename" ? "rename" : "transform";
    renderPreview(previewPlan);
    toast(
      previewPlan.changes.length
        ? `Previewing ${previewPlan.changes.length} file(s) — click Apply`
        : "These rules change nothing on the selection",
      previewPlan.changes.length === 0
    );
  } catch (e) {
    toast(String(e), true);
  }
}

// ---- reorganize into folders (#37) ----
// Builds the plan and shows it in the usual preview view, so the move is
// applied (and undone) through exactly the same path as a rename.
async function previewMove() {
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast("Select the tracks to move first", true);
    return;
  }
  try {
    previewPlan = await invoke("preview_move", { mask: el("move-mask").value, paths });
    previewSource = "rename";
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

// Refresh the field-editor section of the TAGGER panel for the current
// selection (called on entering the mode). Staged-but-unapplied field changes
// are dropped on refresh — they only make sense against the selection they were
// typed for.
function refreshFieldEditor() {
  const paths = selectedPaths();
  stagedFields = new Map();
  el("fields-count").textContent = paths.length ? `— ${paths.length} file(s)` : "";
  el("fields-new-name").value = "";
  el("fields-new-value").value = "";
  renderFieldEditor(paths);
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
  if (paths.length === 0) {
    toast("Select the tracks to edit first", true);
    return;
  }
  if (stagedFields.size === 0) {
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
  renderTracks();
  refreshFieldEditor();
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

// Refresh the EXPORTER panel for the current selection (called on entering the
// mode). Only resets the file name when it's empty, so a name the user typed
// survives a mode switch.
function refreshExporter() {
  const count = selectedPaths().length;
  el("export-count").textContent = count ? `— ${count} track(s)` : "";
  if (!el("export-name").value) syncExportKind();
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
  if (paths.length === 0) {
    toast("Select the tracks to export first", true);
    return;
  }
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
    toast(`Exported ${paths.length} track(s) to ${fileName(written)}`);
  } catch (e) {
    toast(String(e), true);
  }
}

// ---- Discogs import (release picker cards, #27 step 2) ----
// Each search hit is a card; expanding it lazily fetches the release (tracklist)
// and its cover. Import / auto-match / embed-cover are per-card and route
// through the same preview/apply/undo path as before.
let releaseCandidates = []; // last search results (CandidateDto[])
let releaseLayout = "list"; // "list" | "grid"
const releaseCache = new Map(); // releaseId -> fetched ReleaseDto (with tracks)
const coverCache = new Map(); // releaseId -> CoverArtDto (full cover, for embed)
// Fetched images as data URIs, so re-rendering (layout toggle) never re-fetches.
const imageCache = new Map(); // releaseId -> { thumb?, cover? }
const expandedIds = new Set(); // cards currently expanded — survive a re-render

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
    releaseCandidates = await invoke("search_discogs", { token, query: { album: query } });
    releaseCache.clear();
    coverCache.clear();
    imageCache.clear();
    expandedIds.clear();
    renderReleaseList();
    prefetchReleaseCounts(); // fill track/disc counts up front, in the background
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
  return discs > 1 ? `${release.tracks.length} tracks · ${discs} discs` : `${release.tracks.length} tracks`;
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

function renderReleaseList() {
  const list = releaseList();
  list.innerHTML = "";
  el("release-toolbar").hidden = releaseCandidates.length === 0;
  el("release-count").textContent = String(releaseCandidates.length);
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
      card.querySelector(".release-caret").textContent = "▾";
      renderTracklist(card, releaseCache.get(c.id));
      card.querySelector(".release-tracklist").dataset.loaded = "1";
    }
  }
}

function cardMarkup(c) {
  const catno = c.catalog_number ? `<span class="catno">${escapeHtml(c.catalog_number)}</span>` : "";
  return `
    <article class="release-card" data-id="${escapeHtml(c.id)}" aria-expanded="false">
      <button class="release-head" type="button">
        <span class="release-cover"></span>
        <span class="release-info">
          <span class="release-title" title="${escapeHtml(c.title)}">${escapeHtml(c.artist ? c.artist + " — " : "")}${escapeHtml(c.title)}</span>
          <span class="release-meta">${catno}<span class="muted">${escapeHtml(candidateMeta(c))}</span></span>
        </span>
        <span class="release-caret" aria-hidden="true">▸</span>
      </button>
      <div class="release-facts">
        <span class="pill tk-count">${escapeHtml(countLabel(c.id))}</span>
        <button class="release-details" type="button">details…</button>
      </div>
      <div class="release-tracklist"></div>
    </article>`;
}

function tileMarkup(c) {
  const catno = c.catalog_number ? `<span class="catno">${escapeHtml(c.catalog_number)}</span>` : "";
  const artist = c.artist ? `<span class="tile-artist">${escapeHtml(c.artist)}</span>` : "";
  // Same information as a list card: catalogue no. · artist (bold) · album title ·
  // country/year/format · track (and disc) count.
  return `
    <article class="release-tile" data-id="${escapeHtml(c.id)}">
      <div class="tile-cover"></div>
      <div class="tile-info">
        <div class="tile-top">${catno}<span class="pill tk-count">${escapeHtml(countLabel(c.id))}</span></div>
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
    const token = el("discogs-token").value.trim();
    try {
      const img = await invoke("fetch_discogs_image", { token, url });
      dataUri = `data:${img.mime};base64,${img.data_base64}`;
      cached[kind] = dataUri;
      imageCache.set(c.id, cached);
    } catch (e) {
      return; // leave the striped placeholder
    }
  }
  const cover = coverElOf(c.id);
  if (cover) cover.innerHTML = `<img alt="" src="${dataUri}" />`;
}

// Fetch each release once, in the background, to fill the track/disc count on
// every card up front. A small pool fetches several at a time (the commands are
// async/off-main-thread) so counts appear quickly without bursting past Discogs'
// 60/min limit; the backend still honours 429/Retry-After. Cached, so expanding
// a card and toggling layout are then instant.
const PREFETCH_CONCURRENCY = 4;

async function prefetchReleaseCounts() {
  const token = el("discogs-token").value.trim();
  const batch = releaseCandidates;
  const queue = batch.filter((c) => !releaseCache.has(c.id));
  async function worker() {
    while (queue.length) {
      if (releaseCandidates !== batch) return; // a newer search superseded this
      const c = queue.shift();
      if (!c || releaseCache.has(c.id)) continue;
      try {
        releaseCache.set(c.id, await invoke("fetch_discogs_release", { token, releaseId: c.id }));
        const pill = countPillOf(c.id);
        if (pill) pill.textContent = countLabel(c.id);
      } catch (e) {
        /* skip this one; the card just keeps its dash */
      }
    }
  }
  await Promise.all(Array.from({ length: PREFETCH_CONCURRENCY }, worker));
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
  card.querySelector(".release-caret").textContent = expanded ? "▸" : "▾";
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
  const token = el("discogs-token").value.trim();
  try {
    let release = releaseCache.get(id);
    if (!release) {
      release = await invoke("fetch_discogs_release", { token, releaseId: id });
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

function renderTracklist(card, release) {
  const rows = release.tracks
    .map(
      (t, i) => `
      <tr>
        <td class="sel"><input type="checkbox" checked data-i="${i}" /></td>
        <td class="tk-num">${escapeHtml(t.position)}</td>
        <td class="tk-title" title="${escapeHtml(t.title)}">${escapeHtml(t.title)}</td>
        <td class="tk-artist">${escapeHtml(t.artist || release.artist)}</td>
        <td class="tk-dur">${t.duration_secs ? fmtTime(t.duration_secs) : "—"}</td>
      </tr>`,
    )
    .join("");
  card.querySelector(".release-tracklist").innerHTML = `
    <div class="tracklist-actions">
      <button class="btn-sm" data-act="enable-all">Enable all</button>
      <button class="btn-sm" data-act="disable-all">Disable all</button>
      <button class="btn-sm" data-act="automatch" title="Reorder the selected files to line up with this tracklist">Auto-match</button>
      <button class="btn-sm" data-act="embed" title="Embed this release's cover into the selected files">Embed cover</button>
      <span class="muted tk-selcount" style="margin-left:auto"></span>
    </div>
    <table><tbody>${rows}</tbody></table>
    <div class="tracklist-apply"><button class="primary" data-act="import">Import to selected files</button></div>`;
  updateTracklistCount(card);
}

function updateTracklistCount(card) {
  const boxes = [...card.querySelectorAll(".release-tracklist .sel input")];
  const on = boxes.filter((b) => b.checked).length;
  const label = card.querySelector(".tk-selcount");
  if (label) label.textContent = `${on} / ${boxes.length} selected`;
}

// The enabled tracks of a card, shaped for import / auto-align.
function enabledTracksOf(card) {
  const release = releaseCache.get(card.dataset.id);
  return [...card.querySelectorAll(".release-tracklist .sel input:checked")].map((cb) => {
    const t = release.tracks[Number(cb.dataset.i)];
    return {
      position: t.position,
      artist: t.artist || release.artist,
      title: t.title,
      duration_secs: t.duration_secs ?? null,
    };
  });
}

// Fetch the full-size cover once (for embedding) and upgrade the card thumbnail.
async function loadFullCover(id, url, card) {
  if (!url || coverCache.has(id)) return;
  const token = el("discogs-token").value.trim();
  try {
    const cover = await invoke("fetch_discogs_image", { token, url });
    coverCache.set(id, cover);
    const coverEl = card.querySelector(".release-cover");
    if (coverEl) coverEl.innerHTML = `<img alt="" src="data:${cover.mime};base64,${cover.data_base64}" />`;
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
  }));
  try {
    const aligned = await invoke("auto_align", { paths, tracks: releaseTracks });
    const ranked = paths.map((path, i) => ({
      path,
      key: aligned[i] === null || aligned[i] === undefined ? Number.MAX_SAFE_INTEGER : aligned[i],
    }));
    ranked.sort((a, b) => a.key - b.key);
    const byPath = new Map(tracks.map((t) => [t.path, t]));
    const selected = new Set(paths);
    let next = 0;
    tracks = tracks.map((t) => (selected.has(t.path) ? byPath.get(ranked[next++].path) : t));
    sortKey = null;
    renderTracks();
    const matched = aligned.filter((i) => i !== null && i !== undefined).length;
    toast(
      matched
        ? `Matched ${matched}/${paths.length} file(s) — reordered to line up`
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
    previewPlan = await invoke("preview_cover_embed", { paths, cover });
    previewSource = "cover";
    renderPreview(previewPlan);
    toast(
      previewPlan.changes.length
        ? `Previewing cover on ${previewPlan.changes.length} file(s) — click Apply`
        : "Selected files already have this cover",
    );
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
  const selection = {
    album: release.title,
    album_artist: release.artist,
    year: release.year ? String(release.year) : null,
    genre: genreValues.join("/") || null,
    tracks: enabledTracksOf(card),
  };
  try {
    const plan = await invoke("preview_import", { paths, selection });
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
    renderTracks();
    refreshFieldEditor();
    await previewEdits();
    toast(
      merged
        ? `Merged ${merged} field change(s) from Discogs into pending edits`
        : "Nothing new to import from this release",
    );
  } catch (e) {
    toast(String(e), true);
  }
}

// ---- mode tabs ----
// The active mode's panel is the only one shown; entering a mode refreshes its
// panel against the current selection. The table (subject) never changes — only
// the right-hand panel (verb) swaps.
const MODE_REFRESH = {
  renamer: () => {},
  tagger: refreshTagger,
  generator: refreshGenerator,
  exporter: refreshExporter,
};
let currentMode = "renamer";

function setMode(name) {
  currentMode = name;
  document.querySelectorAll(".mode-tab").forEach((tab) => {
    tab.classList.toggle("active", tab.dataset.mode === name);
  });
  document.querySelectorAll(".mode-panel").forEach((panel) => {
    panel.hidden = panel.id !== `panel-${name}`;
  });
  // Uncollapse when a tab is clicked, so switching modes always reveals the panel.
  document.body.classList.remove("panel-collapsed");
  (MODE_REFRESH[name] || (() => {}))();
}

// Refresh the TAGGER field grid for the selection. The Discogs card list
// persists across mode switches (a search isn't thrown away when you leave).
function refreshTagger() {
  refreshFieldEditor();
}

document.querySelectorAll(".mode-tab").forEach((tab) => {
  tab.addEventListener("click", () => setMode(tab.dataset.mode));
});

// Collapse/expand the mode panel to give the table the full width.
el("panel-toggle").addEventListener("click", () => {
  document.body.classList.toggle("panel-collapsed");
});

// ---- view tabs (Files | Preview) ----
el("view-files").addEventListener("click", () => showView("files"));
el("view-preview").addEventListener("click", () => {
  if (previewPlan) showView("preview");
});
el("discard").addEventListener("click", discardPreview);

// ---- wire up ----
el("open").addEventListener("click", openLibrary);
el("browse").addEventListener("click", browseForFolder);
previewBtn.addEventListener("click", preview);
previewEditsBtn.addEventListener("click", previewEdits);
applyBtn.addEventListener("click", apply);
undoBtn.addEventListener("click", undo);
coverOpenBtn.addEventListener("click", chooseCover);
coverExportBtn.addEventListener("click", exportCover);
el("transform-add").addEventListener("click", addTransformRule);
el("transform-preview").addEventListener("click", previewTransform);
el("move-preview").addEventListener("click", previewMove);
el("fields-add").addEventListener("click", addCustomField);
el("fields-apply").addEventListener("click", applyFieldEditor);
el("export-kind").addEventListener("change", syncExportKind);
el("export-run").addEventListener("click", runExport);
coverFileInput.addEventListener("change", onCoverChosen);
el("discogs-search").addEventListener("click", discogsSearch);
el("discogs-query").addEventListener("keydown", (e) => e.key === "Enter" && discogsSearch());

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
  if (act === "enable-all" || act === "disable-all") {
    card.querySelectorAll(".release-tracklist .sel input").forEach((cb) => (cb.checked = act === "enable-all"));
    updateTracklistCount(card);
  } else if (act === "automatch") {
    autoMatchToRelease(card);
  } else if (act === "embed") {
    embedCoverFrom(card);
  } else if (act === "import") {
    importRelease(card);
  } else if (e.target.closest(".release-head") || e.target.closest(".release-details")) {
    toggleCard(card);
  }
});

// Live "N / M selected" as track checkboxes toggle.
el("release-list").addEventListener("change", (e) => {
  if (e.target.matches(".release-tracklist .sel input")) {
    updateTracklistCount(e.target.closest(".release-card"));
  }
});

// Open a native folder chooser (Tauri dialog plugin). The scanner recurses into
// subfolders, so picking a folder loads everything under it. Outside Tauri
// (browser dev) there's no native dialog — fall back to focusing the path field.
async function browseForFolder() {
  const dialog = window.__TAURI__ && window.__TAURI__.dialog;
  if (!dialog) {
    toast("Type a library path, then press Open");
    rootInput.focus();
    return;
  }
  try {
    const picked = await dialog.open({ directory: true, multiple: false });
    if (!picked) return; // user cancelled
    rootInput.value = picked;
    await openLibrary();
  } catch (e) {
    toast(String(e), true);
  }
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
  const on = selectAll.checked;
  for (const tr of dataRows()) {
    if (on) selection.add(tr.dataset.path);
    else selection.delete(tr.dataset.path);
  }
  syncSelectionUI();
});
// Direct checkbox clicks feed the selection set too.
tracksBody.addEventListener("change", (e) => {
  const cb = e.target.closest(".sel input[type=checkbox]");
  if (!cb) return;
  if (cb.checked) selection.add(cb.dataset.path);
  else selection.delete(cb.dataset.path);
  syncSelectionUI();
});

// ---- row selection ----
// The `selection` set is the source of truth (see its declaration). On top of
// checkboxes, clicking a row selects it the way a file list does (click = only
// this row, ⌘/Ctrl = toggle, Shift = range); double-clicking a group's name
// toggles that whole group. Editing a cell is a deliberate double-click, so the
// single click is free for selection.
let selAnchor = null; // path of the last row clicked, for Shift-range

function rowCheckbox(tr) {
  return tr.querySelector(".sel input[type=checkbox]");
}

// Data rows in DOM (visual) order, group headers excluded.
function dataRows() {
  return [...tracksBody.querySelectorAll("tr")].filter(
    (tr) => tr.dataset.path && !tr.classList.contains("group-head"),
  );
}

// Push the `selection` set onto the checkboxes + row highlight, set the
// select-all tri-state, and refresh the status count. Called after any change.
function syncSelectionUI() {
  const rows = dataRows();
  let checked = 0;
  for (const tr of rows) {
    const on = selection.has(tr.dataset.path);
    rowCheckbox(tr).checked = on;
    tr.classList.toggle("selected", on);
    if (on) checked += 1;
  }
  selectAll.checked = checked > 0 && checked === rows.length;
  selectAll.indeterminate = checked > 0 && checked < rows.length;
  updateStatus();
  // The TAGGER field grid shows the current selection's values, so keep it in
  // step as the selection changes while that mode is open.
  if (currentMode === "tagger") refreshFieldEditor();
}

function selectRow(tr, e) {
  const rows = dataRows();
  const path = tr.dataset.path;
  if (e.shiftKey && selAnchor) {
    const paths = rows.map((r) => r.dataset.path);
    let a = paths.indexOf(selAnchor);
    let b = paths.indexOf(path);
    if (a < 0) a = b;
    if (a > b) [a, b] = [b, a];
    selection.clear();
    for (let i = a; i <= b; i++) selection.add(paths[i]);
  } else if (e.metaKey || e.ctrlKey) {
    if (selection.has(path)) selection.delete(path);
    else selection.add(path);
    selAnchor = path;
  } else {
    selection.clear();
    selection.add(path);
    selAnchor = path;
  }
  syncSelectionUI();
  setActiveRow(tr, true); // clicking a row also makes it the keyboard-nav anchor
}

// Toggle a whole group's selection (a group-name double-click): if every row of
// the group is already selected, deselect them; otherwise select them all,
// leaving other groups' selection untouched.
function toggleGroupSelection(key) {
  const rows = dataRows().filter((tr) => tr.dataset.group === key);
  if (rows.length === 0) return;
  const allSelected = rows.every((tr) => selection.has(tr.dataset.path));
  for (const tr of rows) {
    if (allSelected) selection.delete(tr.dataset.path);
    else selection.add(tr.dataset.path);
  }
  syncSelectionUI();
}

// Enter edit mode on a cell and select its text (double-click, per the hint).
function beginCellEdit(cell) {
  cell.contentEditable = "true";
  cell.focus();
  const range = document.createRange();
  range.selectNodeContents(cell);
  const sel = window.getSelection();
  sel.removeAllRanges();
  sel.addRange(range);
}

tracksBody.addEventListener("click", (e) => {
  if (e.target.closest("td.play")) return; // play button handles itself
  if (e.target.closest("td.sel")) return; // checkbox toggle → change listener
  if (e.target.closest("tr.group-head")) return; // caret handles collapse
  const tr = e.target.closest("tr");
  if (!tr || !tr.dataset.path) return;
  const cell = e.target.closest("td.editable");
  if (cell && cell.isContentEditable) return; // mid-edit: don't reselect
  selectRow(tr, e);
});

tracksBody.addEventListener("dblclick", (e) => {
  const head = e.target.closest("tr.group-head");
  if (head) {
    // Caret double-click just toggles collapse (handled by the click listener);
    // double-clicking the name toggles the group's selection.
    if (!e.target.closest(".group-caret")) toggleGroupSelection(head.dataset.group);
    return;
  }
  const cell = e.target.closest("td.editable");
  if (cell) beginCellEdit(cell);
});

// contentEditable is turned off again when a cell loses focus (blur doesn't
// bubble, so listen in the capture phase).
tracksBody.addEventListener(
  "blur",
  (e) => {
    if (e.target.classList && e.target.classList.contains("editable")) {
      e.target.contentEditable = "false";
    }
  },
  true,
);

// ---- keyboard row navigation (roving tabindex) ----
// Exactly one data row is tabbable (tabindex 0); ↑/↓ move focus between visible
// rows and Space toggles the focused row's selection. This makes the row focus
// ring (states.css) reachable for a keyboard-heavy tool.
let activeRowPath = null;

// Visible data rows (group headers and collapsed rows excluded).
function navRows() {
  return dataRows().filter((tr) => !tr.classList.contains("hidden-row"));
}

// Keep exactly one row tabbable; called after every render.
function refreshRoving() {
  const rows = navRows();
  if (rows.length === 0) {
    activeRowPath = null;
    return;
  }
  if (!rows.some((r) => r.dataset.path === activeRowPath)) activeRowPath = rows[0].dataset.path;
  for (const r of dataRows()) r.tabIndex = r.dataset.path === activeRowPath ? 0 : -1;
}

function setActiveRow(tr, focus) {
  activeRowPath = tr ? tr.dataset.path : null;
  for (const r of dataRows()) r.tabIndex = r.dataset.path === activeRowPath ? 0 : -1;
  if (tr && focus) tr.focus();
}

tracksBody.addEventListener("keydown", (e) => {
  // Don't hijack keys while editing a cell or typing in a control.
  if (e.target.isContentEditable || e.target.matches("input, textarea, select")) return;
  const tr = e.target.closest("tr");
  if (!tr || !tr.dataset.path) return;
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    const rows = navRows();
    const i = rows.indexOf(tr);
    const next = rows[e.key === "ArrowDown" ? i + 1 : i - 1];
    if (next) setActiveRow(next, true);
  } else if (e.key === " ") {
    e.preventDefault(); // Space would otherwise scroll
    const path = tr.dataset.path;
    if (selection.has(path)) selection.delete(path);
    else selection.add(path);
    selAnchor = path;
    syncSelectionUI();
  }
});

// ---- resize the table / mode-panel split by dragging the divider ----
// Mouse events (not a native splitter) for the same WKWebView reason as the row
// reorder. The panel has a fixed flex-basis; dragging sets it in pixels.
(function initSplitter() {
  const splitter = el("col-splitter");
  const modeCol = document.querySelector(".mode-col");
  const workarea = document.querySelector(".workarea");
  let dragging = false;

  splitter.addEventListener("mousedown", (e) => {
    e.preventDefault();
    dragging = true;
    document.body.classList.add("resizing");
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  });

  function onMove(e) {
    if (!dragging) return;
    // Panel width = distance from the cursor to the right edge of the work area,
    // clamped so neither column collapses.
    const rect = workarea.getBoundingClientRect();
    const width = Math.min(Math.max(rect.right - e.clientX, 240), rect.width - 360);
    modeCol.style.flexBasis = `${Math.round(width)}px`;
  }

  function onUp() {
    dragging = false;
    document.body.classList.remove("resizing");
    document.removeEventListener("mousemove", onMove);
    document.removeEventListener("mouseup", onUp);
  }
})();

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
// Collapse/expand only via the caret at the start of the header, so a click on
// the group name is free to (double-)select the group instead.
tracksBody.addEventListener("click", (e) => {
  const caret = e.target.closest(".group-caret");
  if (!caret) return;
  const head = caret.closest("tr.group-head");
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
// Enter commits a cell instead of inserting a newline, and leaves edit mode.
tracksBody.addEventListener("keydown", (e) => {
  if (e.target.classList.contains("editable") && e.key === "Enter") {
    e.preventDefault();
    e.target.contentEditable = "false";
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
    case "preview_transform": {
      // Mirrors the backend closely enough to exercise the dialog: literal
      // replace plus title-casing, over tags or the file name.
      const applyRules = (value) => {
        let out = value;
        for (const rule of args.rules) {
          if (rule.kind === "replace" && rule.from) {
            out = out.split(rule.from).join(rule.to);
          } else if (rule.kind === "case" && rule.style === "title") {
            out = out.replace(/[\p{L}\p{N}']+/gu, (w) => w[0].toUpperCase() + w.slice(1).toLowerCase());
          } else if (rule.kind === "case" && rule.style === "lower") {
            out = out.toLowerCase();
          } else if (rule.kind === "case" && rule.style === "upper") {
            out = out.toUpperCase();
          }
        }
        return out;
      };
      const changes = args.paths
        .map((p) => {
          const t = findTrack(p);
          if (!t) return null;
          if (args.scope === "filename") {
            const dir = p.slice(0, p.lastIndexOf("/") + 1);
            const base = p.slice(p.lastIndexOf("/") + 1, p.lastIndexOf("."));
            const ext = p.slice(p.lastIndexOf("."));
            const renamed = applyRules(base);
            return renamed === base ? null : { path: p, rename_to: `${dir}${renamed}${ext}`, tag_changes: [] };
          }
          const tag_changes = [];
          for (const [field, value] of Object.entries(t.tags)) {
            if (args.scope !== "tags" && args.scope !== field) continue;
            const next = applyRules(value);
            if (next !== value) tag_changes.push({ field, old: value, new: next });
          }
          return tag_changes.length ? { path: p, rename_to: null, tag_changes } : null;
        })
        .filter(Boolean);
      return Promise.resolve({ description: "Transform", changes });
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
        { id: "316795", artist: "Various", title: "La Bush - Music From The Temple Of House", year: 1996, score: 1.0, thumb_url: "https://img/1t.jpg", cover_url: "https://img/1c.jpg", country: "Belgium", label: "Antler-Subway", format: "CD, Compilation, Mixed", catalog_number: "TOTH 006" },
        { id: "764414", artist: "Various", title: "La Bush Vol. 4", year: 1997, score: 0.9, thumb_url: "https://img/2t.jpg", cover_url: "https://img/2c.jpg", country: "Belgium", label: "Antler-Subway", format: "CD, Mixed", catalog_number: "TOTH 021" },
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
