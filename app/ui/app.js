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
    tr.innerHTML = `
      <td class="sel"><input type="checkbox" checked data-path="${escapeHtml(track.path)}" /></td>
      <td class="mono" title="${escapeHtml(track.path)}">${escapeHtml(fileName(track.path))}</td>`;
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

// ---- wire up ----
el("open").addEventListener("click", openLibrary);
previewBtn.addEventListener("click", preview);
previewEditsBtn.addEventListener("click", previewEdits);
applyBtn.addEventListener("click", apply);
undoBtn.addEventListener("click", undo);
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
    default:
      return Promise.reject(`unknown command: ${cmd}`);
  }
}
