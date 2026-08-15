// Cover art (#41, #102, #133, #143 split it out of app.js).
//
// The cover well in EDITOR — what the selection currently carries, embedding a
// picked or dropped image, exporting one next to the files, and removing it.
// Every change goes through the ordinary preview/apply/undo path; only the
// export bypasses it, because it writes a new file and touches no track.
//
// A file can carry several images, each saying what it depicts (#56). The well
// stays about "the cover" — the front one, which is what a picked, dropped or
// fetched image means — and the strip below it is the rest of the set, editable
// only when every selected file carries the same one. Every edit there sends the
// WHOLE set, which is also what the plan stores and what undo writes back.
import { el, escapeHtml, ico, plural, toast } from "./dom.js";
import { invoke } from "./invoke.js";
import { hooks } from "./hooks.js";
import { enablePointerReorder } from "./reorder.js";
import { previewPlan, selectedPaths, setPreviewPlan, setPreviewSource } from "./state.js";

const coverWell = el("cover-well");
const coverSetBox = el("cover-set");
const coverFileInput = el("cover-file");

// What an image can say it depicts. The backend's storage keys; anything it
// doesn't know reads back as "other", so this list is the whole menu.
const COVER_KINDS = [
  ["front", "Front cover"],
  ["back", "Back cover"],
  ["media", "Disc / media"],
  ["leaflet", "Leaflet"],
  ["artist", "Artist"],
  ["logo", "Label logo"],
  ["icon", "Icon"],
  ["other", "Other"],
];

// The set the strip is editing — the one every selected file carries. Replaced
// on every refresh, so it is never stale against the disk.
let coverSet = [];
// Whether the next picked file joins the set rather than becoming the front
// cover. Set by "Add image…", cleared as soon as the picker answers.
let pickAppends = false;

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
  const appending = pickAppends;
  pickAppends = false;
  if (!file) return;
  if (appending) await addCoverFile(file);
  else await embedCoverFile(file);
}

// Read an image File into a cover DTO ({ mime, data_base64 }).
async function readImageFile(file) {
  const dataUrl = await new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result);
    reader.onerror = reject;
    reader.readAsDataURL(file);
  });
  // dataUrl is "data:<mime>;base64,<data>"
  const comma = dataUrl.indexOf(",");
  return {
    mime: dataUrl.slice(5, dataUrl.indexOf(";")),
    data_base64: dataUrl.slice(comma + 1),
  };
}

// Read an image File, base64-encode it, and preview embedding it as the front
// cover of the selection. Used by the file picker and the well's HTML5 drop
// (browser-dev). In the packaged app a dropped cover arrives as a path instead
// — see embedCoverFromPath (#133).
async function embedCoverFile(file) {
  if (!file.type.startsWith("image/")) {
    toast("Drop an image file", true);
    return;
  }
  if (selectedPaths().length === 0) {
    toast("Select the tracks to embed the cover into first", true);
    return;
  }
  await embedCoverDto(await readImageFile(file));
}

// Add a picked image to the set rather than replacing the front cover (#56).
// New images join as "other": the front one is what the well is for, and
// guessing a type from a file name would be worse than asking.
async function addCoverFile(file) {
  if (!file.type.startsWith("image/")) {
    toast("Pick an image file", true);
    return;
  }
  const image = await readImageFile(file);
  await previewCoverSet([...coverSet, { ...image, kind: "other", description: "" }]);
}

// Stage a whole image set onto the selection (#56) — the one call behind add,
// remove, reorder and retype.
async function previewCoverSet(covers) {
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast("Select the tracks whose images to edit first", true);
    return;
  }
  try {
    setPreviewPlan(await invoke("preview_cover_set", { paths, covers }));
    setPreviewSource("cover");
    hooks.renderPreview(previewPlan);
    toast(
      previewPlan.changes.length
        ? `Previewing ${plural(covers.length, "image", "images")} on ${plural(previewPlan.changes.length, "file", "files")}`
        : "Selected files already carry exactly these images"
    );
  } catch (e) {
    toast(String(e), true);
  }
}

// Embed an image dropped onto the cover well in the packaged app (#133): the
// drag-drop event gives a path, which the backend reads into a cover DTO.
async function embedCoverFromPath(path) {
  if (selectedPaths().length === 0) {
    toast("Select the tracks to embed the cover into first", true);
    return;
  }
  try {
    const cover = await invoke("read_cover_image", { path });
    await embedCoverDto(cover);
  } catch (e) {
    toast(String(e), true);
  }
}

// Preview embedding a cover DTO ({ mime, data_base64 }) as the front cover of
// the current selection — the shared tail of every cover-embed source (picker,
// HTML5 drop, native drop, and the release card).
async function embedCoverDto(cover) {
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
        ? `Previewing cover on ${plural(previewPlan.changes.length, "file", "files")}`
        : "Selected files already have this cover"
    );
  } catch (e) {
    toast(String(e), true);
  }
}

// Preview removing the embedded cover from the selection, through the normal
// preview/apply/undo path.
async function previewCoverRemove() {
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast("Select the tracks whose cover to remove first", true);
    return;
  }
  try {
    setPreviewPlan(await invoke("preview_cover_remove", { paths }));
    setPreviewSource("cover");
    hooks.renderPreview(previewPlan);
    toast(
      previewPlan.changes.length
        ? `Previewing cover removal on ${plural(previewPlan.changes.length, "file", "files")}`
        : "None of the selected files have a cover"
    );
  } catch (e) {
    toast(String(e), true);
  }
}

// Preview wiping every text tag from the selection for a fresh start (#94),
// through the normal preview/apply/undo path. The cover and DJ cue points are
// kept; the diff is the review, and undo reverses it.
async function previewClearTags() {
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast("Select the tracks whose tags to clear first", true);
    return;
  }
  try {
    setPreviewPlan(await invoke("preview_clear_tags", { paths }));
    setPreviewSource("clear");
    hooks.renderPreview(previewPlan);
    toast(
      previewPlan.changes.length
        ? `Previewing tag clear on ${plural(previewPlan.changes.length, "file", "files")}`
        : "None of the selected files have tags to clear"
    );
  } catch (e) {
    toast(String(e), true);
  }
}

// ---- cover well (#editor design pass) ----
// A thumbnail + state + actions that replaces the two bare Embed/Export buttons.
// Reflects the selection's cover state: none / one shared / mixed.
async function refreshCoverWell() {
  const paths = selectedPaths();
  if (paths.length === 0) {
    coverWell.className = "cover-well empty";
    coverWell.innerHTML = `<div class="cover-thumb inert"></div>
      <div class="cover-body"><div class="cover-title">No selection</div>
      <div class="cover-hint">Select tracks to edit their cover.</div></div>`;
    coverSetBox.hidden = true;
    coverSet = [];
    return;
  }
  try {
    const summary = await invoke("read_cover_summary", { paths });
    renderCoverWell(summary);
    renderCoverSet(summary);
    await showExternalCoverAction(paths);
  } catch (e) {
    toast(String(e), true);
  }
}

// The external cover (folder.jpg/cover.jpg next to the tracks), when present —
// offered as a one-click embed under the well (#41).
let externalCover = null;

async function showExternalCoverAction(paths) {
  externalCover = null;
  let found;
  try {
    found = await invoke("read_external_cover", { paths });
  } catch (e) {
    return; // best-effort; the well already rendered
  }
  if (!found) return;
  externalCover = found;
  // Join the action row so it flows with Replace…/Remove/Export as an equal
  // button (#134), not a full-width slab. Falls back to the body in the no-cover
  // state, which has no action row.
  const host = coverWell.querySelector(".cover-actions") || coverWell.querySelector(".cover-body");
  if (!host) return;
  const note = document.createElement("button");
  note.className = "btn cover-external";
  note.dataset.cover = "external";
  note.textContent = "Use folder image";
  note.title = "Embed the cover.jpg / folder.jpg sitting next to these tracks";
  host.appendChild(note);
}

// Embed the external cover file (folder.jpg/cover.jpg) into the selection (#41),
// through the same preview/apply/undo path as any other cover.
async function embedExternalCover() {
  if (!externalCover) return;
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast("Select the tracks to embed the cover into first", true);
    return;
  }
  try {
    setPreviewPlan(await invoke("preview_cover_embed", { paths, cover: externalCover }));
    setPreviewSource("cover");
    hooks.renderPreview(previewPlan);
    toast(
      previewPlan.changes.length
        ? `Previewing folder image on ${plural(previewPlan.changes.length, "file", "files")} — click Apply`
        : "Selected files already have this cover",
    );
  } catch (e) {
    toast(String(e), true);
  }
}

function coverThumbImg(cover, cls) {
  return `<img class="cover-thumb${cls ? " " + cls : ""}" alt="front cover" src="data:${cover.mime};base64,${cover.data_base64}" />`;
}

function renderCoverWell(summary) {
  const { total, with_cover, distinct, samples } = summary;
  const n = total;
  const drop = `<div class="cover-drop-cue">Drop image to embed in ${plural(n, "file", "files")}</div>`;

  if (with_cover === 0) {
    // No cover anywhere — the well itself is the click/drop target.
    coverWell.className = "cover-well empty";
    coverWell.innerHTML = `<div class="cover-thumb inert"></div>
      <div class="cover-body">
        <div class="cover-title">No cover</div>
        ${drop}
        <div class="cover-hint"><b>Embed cover…</b><br>or drag an image here</div>
      </div>`;
    return;
  }

  if (!distinct && samples.length === 1) {
    // One cover shared across the whole selection.
    coverWell.className = "cover-well";
    coverWell.innerHTML = `${coverThumbImg(samples[0])}
      <div class="cover-body">
        <div class="cover-title">Front cover</div>
        <div class="cover-meta">shared across ${plural(n, "file", "files")}</div>
        ${drop}
        <div class="cover-actions">
          <button class="btn" data-cover="replace">Replace…</button>
          <button class="btn" data-cover="remove">Remove</button>
          <button class="btn" data-cover="export">Export</button>
        </div>
      </div>`;
    return;
  }

  // Mixed — files carry different covers (or some have none). A small fan, never
  // implying one shared image.
  const fan = samples.map((c) => coverThumbImg(c)).join("");
  coverWell.className = "cover-well";
  coverWell.innerHTML = `<div class="cover-stack">${fan || '<div class="cover-thumb inert"></div>'}</div>
    <div class="cover-body">
      <div class="cover-title">Multiple covers</div>
      <div class="cover-meta">${with_cover}/${n} with a cover</div>
      ${drop}
      <div class="cover-actions">
        <button class="btn" data-cover="replace">Set one…</button>
        <button class="btn" data-cover="remove">Remove all</button>
        <button class="btn" data-cover="export">Export</button>
      </div>
    </div>`;
}

// ---- the image set (#56) ----
// One row per image: a thumbnail, what it depicts, a grip to reorder and a ✕.
// Only shown when every selected file carries the SAME set — with nothing
// shared there is no single set to edit, and the well's mixed state already
// says so. Each edit sends the whole revised set.
function renderCoverSet(summary) {
  coverSet = summary.distinct ? [] : summary.shared_set.map((image) => ({ ...image }));
  // Nothing to manage until there is at least one image; the well's own
  // "Embed cover…" is the way in, and Add image… appears once there is a set.
  if (summary.distinct || coverSet.length === 0) {
    coverSetBox.hidden = true;
    coverSetBox.innerHTML = "";
    return;
  }
  coverSetBox.hidden = false;
  coverSetBox.innerHTML = "";

  coverSet.forEach((image, index) => {
    const row = document.createElement("div");
    row.className = "cover-row";
    row.dataset.key = String(index);

    const grip = document.createElement("span");
    grip.className = "cover-grip";
    grip.innerHTML = ico("grip");
    grip.title = "Drag to reorder";
    // Order is what the file stores, and some players show the first picture
    // rather than the front-typed one. Pointer events, not HTML5 DnD (#88).
    enablePointerReorder(grip, row, coverSetBox, ".cover-row", (draggedKey, targetKey, below) => {
      const from = Number(draggedKey);
      let to = Number(targetKey);
      if (Number.isNaN(from) || Number.isNaN(to)) return;
      const next = coverSet.slice();
      const [moved] = next.splice(from, 1);
      if (from < to) to -= 1;
      if (below) to += 1;
      next.splice(Math.max(0, Math.min(next.length, to)), 0, moved);
      previewCoverSet(next);
    });

    const thumb = document.createElement("img");
    thumb.className = "cover-thumb cover-row-thumb";
    thumb.alt = image.description || "embedded image";
    thumb.src = `data:${image.mime};base64,${image.data_base64}`;

    const kind = document.createElement("select");
    kind.className = "cover-kind";
    kind.title = "What this image shows";
    for (const [value, label] of COVER_KINDS) {
      const option = document.createElement("option");
      option.value = value;
      option.textContent = label;
      option.selected = (image.kind || "front") === value;
      kind.appendChild(option);
    }
    kind.addEventListener("change", () => {
      const next = coverSet.map((c, at) => (at === index ? { ...c, kind: kind.value } : c));
      previewCoverSet(next);
    });

    const meta = document.createElement("span");
    meta.className = "cover-row-meta";
    meta.textContent = image.description || image.mime.replace("image/", "");

    const remove = document.createElement("button");
    remove.className = "icon cover-row-rm";
    remove.type = "button";
    remove.innerHTML = ico("close");
    remove.title = "Remove this image";
    remove.setAttribute("aria-label", "Remove this image");
    remove.addEventListener("click", () =>
      previewCoverSet(coverSet.filter((_, at) => at !== index))
    );

    row.append(grip, thumb, kind, meta, remove);
    coverSetBox.appendChild(row);
  });

  const add = document.createElement("button");
  add.className = "btn cover-add";
  add.type = "button";
  add.textContent = "Add image…";
  add.title = "Embed another image beside the cover — a back cover, the disc, a leaflet";
  add.addEventListener("click", () => {
    pickAppends = true;
    coverFileInput.value = ""; // allow re-picking the same file
    coverFileInput.click();
  });
  coverSetBox.appendChild(add);
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
    toast(`Exported ${plural(wrote, "cover file", "cover files")}${skipNote}`);
  } catch (e) {
    toast(String(e), true);
  }
}

// ---- wire up ----
coverFileInput.addEventListener("change", onCoverChosen);
// Cover well: action buttons (delegated) + drag-and-drop embed.
coverWell.addEventListener("click", (e) => {
  const btn = e.target.closest("[data-cover]");
  const act = btn ? btn.dataset.cover : coverWell.classList.contains("empty") ? "replace" : null;
  if (act === "replace") chooseCover();
  else if (act === "remove") previewCoverRemove();
  else if (act === "export") exportCover();
  else if (act === "external") embedExternalCover();
});
coverWell.addEventListener("dragover", (e) => {
  e.preventDefault();
  coverWell.classList.add("dragover");
});
coverWell.addEventListener("dragleave", (e) => {
  if (!coverWell.contains(e.relatedTarget)) coverWell.classList.remove("dragover");
});
coverWell.addEventListener("drop", (e) => {
  e.preventDefault();
  coverWell.classList.remove("dragover");
  const file = e.dataTransfer.files[0];
  if (file) embedCoverFile(file);
});

export {
  chooseCover,
  previewCoverSet,
  embedCoverFile,
  embedCoverFromPath,
  embedExternalCover,
  exportCover,
  onCoverChosen,
  previewClearTags,
  previewCoverRemove,
  refreshCoverWell,
};
