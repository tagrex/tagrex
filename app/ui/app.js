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
const undoBtn = el("undo");
const selectAll = el("select-all");

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
  return [...tracksBody.querySelectorAll("input:checked")].map((cb) => cb.dataset.path);
}

// ---- rendering ----
function renderTracks() {
  tracksBody.innerHTML = "";
  trackCount.textContent = tracks.length ? `(${tracks.length})` : "";
  tracksEmpty.hidden = tracks.length > 0;

  for (const track of tracks) {
    const tr = document.createElement("tr");
    tr.innerHTML = `
      <td class="sel"><input type="checkbox" checked data-path="${escapeHtml(track.path)}" /></td>
      <td class="mono" title="${escapeHtml(track.path)}">${escapeHtml(fileName(track.path))}</td>
      <td>${escapeHtml(tag(track, "artist"))}</td>
      <td>${escapeHtml(tag(track, "title"))}</td>
      <td>${escapeHtml(tag(track, "album"))}</td>
      <td>${escapeHtml(tag(track, "year"))}</td>`;
    tracksBody.appendChild(tr);
  }
  previewBtn.disabled = tracks.length === 0;
  applyBtn.disabled = true;
  previewTable.hidden = true;
  previewEmpty.hidden = true;
}

function renderPreview(plan) {
  previewBody.innerHTML = "";
  const changes = plan.changes;
  if (changes.length === 0) {
    previewTable.hidden = true;
    previewEmpty.hidden = false;
    previewEmpty.textContent = "No files would change with this mask.";
    applyBtn.disabled = true;
    return;
  }
  for (const change of changes) {
    const tr = document.createElement("tr");
    tr.innerHTML = `
      <td class="mono old">${escapeHtml(fileName(change.path))}</td>
      <td class="arrow">→</td>
      <td class="mono new">${escapeHtml(fileName(change.rename_to || change.path))}</td>`;
    previewBody.appendChild(tr);
  }
  previewTable.hidden = false;
  previewEmpty.hidden = true;
  applyBtn.disabled = false;
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

async function refreshHistory() {
  try {
    const batches = await invoke("history", {});
    undoBtn.disabled = batches.length === 0;
    undoBtn.textContent = batches.length ? `Undo (${batches.length})` : "Undo last";
  } catch (e) {
    // history is best-effort in the toolbar
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

async function apply() {
  if (!previewPlan || previewPlan.changes.length === 0) return;
  try {
    await invoke("apply_plan", { plan: previewPlan });
    toast(`Applied ${previewPlan.changes.length} rename(s)`);
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

// ---- wire up ----
el("open").addEventListener("click", openLibrary);
previewBtn.addEventListener("click", preview);
applyBtn.addEventListener("click", apply);
undoBtn.addEventListener("click", undo);
rootInput.addEventListener("keydown", (e) => e.key === "Enter" && openLibrary());
selectAll.addEventListener("change", () => {
  tracksBody
    .querySelectorAll("input[type=checkbox]")
    .forEach((cb) => (cb.checked = selectAll.checked));
});

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
  switch (cmd) {
    case "open_library":
      return Promise.resolve();
    case "list_tracks":
      return Promise.resolve(s.tracks);
    case "preview_rename": {
      const changes = args.paths
        .map((p) => {
          const t = s.tracks.find((x) => x.path === p);
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
    case "apply_plan":
      for (const c of args.plan.changes) {
        const t = s.tracks.find((x) => x.path === c.path);
        if (t) t.path = c.rename_to;
      }
      s.history.unshift({ id: s.history.length + 1, description: args.plan.description, applied_at: 0 });
      return Promise.resolve({ id: s.history.length, description: args.plan.description, applied_at: 0 });
    case "history":
      return Promise.resolve(s.history);
    case "undo":
      s.history.shift();
      return Promise.resolve();
    default:
      return Promise.reject(`unknown command: ${cmd}`);
  }
}
