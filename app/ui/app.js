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
// path -> { field -> newValue } for cells edited but not yet applied
const edits = new Map();

// Fields shown as editable columns, in table order.
const EDIT_FIELDS = ["artist", "title", "album", "year"];

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
const discogsOpenBtn = el("discogs-open");
const discogsModal = el("discogs-modal");
const discogsResults = el("discogs-results");
const discogsEmpty = el("discogs-empty");
const releaseTracksBody = el("release-tracks");
let currentRelease = null;

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

function selectedPaths() {
  return [...tracksBody.querySelectorAll(".sel input:checked")].map((cb) => cb.dataset.path);
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
function renderTracks() {
  tracksBody.innerHTML = "";
  edits.clear();
  updateEditsButton();
  trackCount.textContent = tracks.length ? `(${tracks.length})` : "";
  tracksEmpty.hidden = tracks.length > 0;

  for (const track of tracks) {
    const tr = document.createElement("tr");
    tr.dataset.path = track.path;
    tr.innerHTML = `
      <td class="sel"><input type="checkbox" checked data-path="${escapeHtml(track.path)}" /></td>
      <td class="mono file" title="${escapeHtml(track.path)}">${escapeHtml(fileName(track.path))}</td>`;
    for (const field of EDIT_FIELDS) {
      const td = document.createElement("td");
      td.className = "editable";
      td.contentEditable = "true";
      td.spellcheck = false;
      td.dataset.path = track.path;
      td.dataset.field = field;
      td.dataset.original = tag(track, field);
      td.textContent = tag(track, field);
      tr.appendChild(td);
    }
    tracksBody.appendChild(tr);
  }
  previewBtn.disabled = tracks.length === 0;
  discogsOpenBtn.disabled = tracks.length === 0;
  applyBtn.disabled = true;
  previewTable.hidden = true;
  previewEmpty.hidden = true;
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
    renderTracks();
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
  if (list.length === 0) return;
  try {
    previewPlan = await invoke("preview_tag_edits", { edits: list });
    renderPreview(previewPlan);
  } catch (e) {
    toast(String(e), true);
  }
}

async function apply() {
  if (!previewPlan || previewPlan.changes.length === 0) return;
  try {
    await invoke("apply_plan", { plan: previewPlan });
    const n = previewPlan.changes.length;
    toast(`Applied changes to ${n} file(s)`);
    previewPlan = null;
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
    tracks = await invoke("list_tracks", {});
    renderTracks();
    await refreshHistory();
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
      <td title="${escapeHtml(t.title)}">${escapeHtml(t.title)}</td>`;
    releaseTracksBody.appendChild(tr);
  });
}

function enabledReleaseTracks() {
  return [...releaseTracksBody.querySelectorAll(".sel input:checked")].map((cb) => {
    const t = currentRelease.tracks[Number(cb.dataset.i)];
    return { position: t.position, artist: t.artist || currentRelease.artist, title: t.title };
  });
}

async function applyImport() {
  const paths = selectedPaths();
  const tracks = enabledReleaseTracks();
  const selection = {
    album: currentRelease.title,
    album_artist: currentRelease.artist,
    year: currentRelease.year ? String(currentRelease.year) : null,
    genre: currentRelease.genres[0] || null,
    tracks,
  };
  try {
    previewPlan = await invoke("preview_import", { paths, selection });
    closeDiscogs();
    renderPreview(previewPlan);
    toast(
      previewPlan.changes.length
        ? `Previewing import onto ${previewPlan.changes.length} file(s)`
        : "Nothing to change from this release"
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
discogsOpenBtn.addEventListener("click", openDiscogs);
el("discogs-close").addEventListener("click", closeDiscogs);
el("discogs-search").addEventListener("click", discogsSearch);
el("discogs-query").addEventListener("keydown", (e) => e.key === "Enter" && discogsSearch());
el("discogs-back").addEventListener("click", () => showDiscogsView("search"));
el("import-apply").addEventListener("click", applyImport);
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
  if (!row || row.dataset.path === dragPath) return;
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
  renderTracks();
}
rootInput.addEventListener("keydown", (e) => e.key === "Enter" && openLibrary());
selectAll.addEventListener("change", () => {
  tracksBody
    .querySelectorAll(".sel input[type=checkbox]")
    .forEach((cb) => (cb.checked = selectAll.checked));
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
        tracks: [
          { position: "1", artist: "The X Factor", title: "Desert Rain" },
          { position: "2", artist: "Wish Mountain", title: "Radio" },
          { position: "3", artist: "West Coast Connection", title: "Voodoo Rhythm" },
        ],
      });
    case "preview_import": {
      const changes = args.paths.map((p, i) => {
        const t = findTrack(p);
        const rt = args.selection.tracks[i];
        const tag_changes = [
          { field: "album", old: t ? t.tags.album || null : null, new: args.selection.album },
        ];
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
