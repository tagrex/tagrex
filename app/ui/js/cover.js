import { adoptLiveCoverPlan } from "./live-preview.js";
import { mount } from "svelte";
import CoverArtWell from "../components/CoverArtWell.svelte";
import "../cover-art.css";
// Cover art (#41, #102, #133, #143 split it out of app.js).
//
// The cover well in EDITOR — what the selection currently carries, embedding a
// picked or dropped image, exporting one next to the files, and removing it.
// Every change goes through the ordinary preview/apply/undo path; only the
// export bypasses it, because it writes a new file and touches no track.
//
// This surface edits the main cover. Replacement uses the existing native
// front-cover operation, which preserves other embedded images.
import { el, toast } from "./dom.js";
import { t, tn } from "./i18n.js";
import { invoke } from "./invoke.js";
import { hooks } from "./hooks.js";
import { previewPlan, selectedPaths, captureSelection, isSelectionCurrent, setPreviewPlan, setPreviewSource, trackAt } from "./state.js";

const coverWell = el("cover-well");
const coverFileInput = el("cover-file");

let coverSubject = null;
let diskCoverSummary = null;
let lastRenderedCoverSummary = null;
let pickSubject = null;
let coverRequest = 0;
let coverReadsActive = 0;
let queuedCoverRead = null;
let coverViewModel = { selected: 0, loading: false, images: [], mixed: false, withCover: 0, external: false, action: () => {} };
const coverView = mount(CoverArtWell, { target: coverWell, props: { initial: coverViewModel } });
let nativePickActive = false;

// Keep at most two native reads outstanding; replace any waiting job with the
// newest subject. Freshness checks still protect reverse completion order.
async function readCurrentCoverSummary(subject) {
  if (coverReadsActive >= 2) {
    const proceed = await new Promise(resolve => {
      queuedCoverRead?.(false);
      queuedCoverRead = resolve;
    });
    if (!proceed || !isSelectionCurrent(subject)) return null;
  }
  if (!isSelectionCurrent(subject)) return null;
  coverReadsActive++;
  try { return await invoke("read_cover_summary", { paths: subject.paths }); }
  finally {
    coverReadsActive--;
    const next = queuedCoverRead;
    queuedCoverRead = null;
    next?.(true);
  }
}
// ---- cover art ----
async function chooseCover() {
  if (selectedPaths().length === 0) {
    toast(t("toast.cover.selectEmbed"), true);
    return;
  }
  if (nativePickActive) return;
  const subject = captureSelection();
  if (!isSelectionCurrent(subject)) return;
  pickSubject = subject;
  const dialog = window.__TAURI__?.dialog;
  if (!dialog?.open) {
    coverFileInput.value = "";
    coverFileInput.click();
    return;
  }
  // Native dialogs can start beside the selected audio files; browser file
  // inputs intentionally cannot choose a directory on the user's behalf.
  const first = subject.paths[0];
  const separator = Math.max(first.lastIndexOf("/"), first.lastIndexOf("\\"));
  const defaultPath = separator >= 0 ? first.slice(0, separator + 1) : undefined;
  nativePickActive = true;
  try {
    const chosen = await dialog.open({
      multiple: false, directory: false, defaultPath,
      filters: [{ name: t("editor.coverArt"), extensions: ["jpg", "jpeg", "png", "webp", "gif", "bmp", "avif"] }],
    });
    if (typeof chosen !== "string" || !isSelectionCurrent(subject)) return;
    const cover = await invoke("read_cover_image", { path: chosen });
    if (!isSelectionCurrent(subject)) return;
    await embedCoverDto(cover, subject);
  } catch (error) {
    if (isSelectionCurrent(subject)) toast(String(error), true);
  } finally { nativePickActive = false; }
}

async function onCoverChosen() {
  const file = coverFileInput.files[0];
  if (!file || !isSelectionCurrent(pickSubject)) return;
  await embedCoverFile(file, pickSubject);
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
async function embedCoverFile(file, subject = captureSelection()) {
  if (!file.type.startsWith("image/")) {
    toast(t("toast.cover.dropImage"), true);
    return;
  }
  if (selectedPaths().length === 0) {
    toast(t("toast.cover.selectEmbed"), true);
    return;
  }
  await embedCoverDto(await readImageFile(file), subject);
}

// Embed an image dropped onto the cover well in the packaged app (#133): the
// drag-drop event gives a path, which the backend reads into a cover DTO.
async function embedCoverFromPath(path) {
  const subject = captureSelection();
  if (selectedPaths().length === 0) {
    toast(t("toast.cover.selectEmbed"), true);
    return;
  }
  try {
    const cover = await invoke("read_cover_image", { path });
    await embedCoverDto(cover, subject);
  } catch (e) {
    toast(String(e), true);
  }
}

// Preview embedding a cover DTO ({ mime, data_base64 }) as the front cover of
// the current selection — the shared tail of every cover-embed source (picker,
// HTML5 drop, native drop, and the release card).
async function embedCoverDto(cover, subject = captureSelection()) {
  if (!isSelectionCurrent(subject)) return;
  const paths = subject.paths;
  if (paths.length === 0) {
    toast(t("toast.cover.selectEmbed"), true);
    return;
  }
  try {
    const plan = await invoke("preview_cover_embed", { paths, cover });
    if (!isSelectionCurrent(subject)) return;
    adoptLiveCoverPlan(plan, paths);
    toast(
      plan.changes.length
        ? t("toast.cover.preview", { files: tn("unit.file", plan.changes.length) })
        : t("toast.cover.sameCover")
    );
  } catch (e) {
    toast(String(e), true);
  }
}

// Preview removing the embedded cover from the selection, through the normal
// preview/apply/undo path.
async function previewCoverRemove() {
  const subject = captureSelection();
  const paths = subject.paths;
  if (paths.length === 0) {
    toast(t("toast.cover.selectRemove"), true);
    return;
  }
  try {
    const pending = previewPlan;
    const plan = await invoke("preview_cover_remove", { paths });
    // Keep the first pending entry, matching find() even for duplicate paths.
    // These indexes belong only to this response; no plan cache can go stale.
    const changed = new Set(plan.changes.map(change => change.path));
    let pendingByPath;
    for (const path of paths) {
      if (changed.has(path)) continue;
      // Small selections keep the cheap early-exit search. Build the bulk
      // index only if a missing native change actually needs pending artwork.
      let incoming;
      if (paths.length <= 4) {
        incoming = pending?.changes?.find(change => change.path === path)?.cover_change;
      } else {
        if (!pendingByPath) {
          pendingByPath = new Map();
          for (const change of pending?.changes || []) if (!pendingByPath.has(change.path)) pendingByPath.set(change.path, change);
        }
        incoming = pendingByPath.get(path)?.cover_change;
      }
      if (incoming?.new?.length) {
        plan.changes.push({path,rename_to:null,tag_changes:[],cover_change:{old:incoming.old || [],new:[]},sidecar_renames:[],block_changes:[],copy:false,rename_root:null,creates_folder:false,folder_extras:[]});
        changed.add(path);
      }
    }
    if (!isSelectionCurrent(subject)) return;
    adoptLiveCoverPlan(plan, paths);
    toast(
      plan.changes.length
        ? t("toast.cover.previewRemoval", { files: tn("unit.file", plan.changes.length) })
        : t("toast.cover.noneHaveCover")
    );
  } catch (e) {
    toast(String(e), true);
  }
}

// Preview wiping every text tag from the selection for a fresh start (#94),
// through the normal preview/apply/undo path. The cover and DJ cue points are
// kept; the diff is the review, and undo reverses it.
async function previewClearTags() {
  const subject = captureSelection();
  const paths = subject.paths;
  if (paths.length === 0) {
    toast(t("toast.cover.selectClear"), true);
    return;
  }
  try {
    const plan = await invoke("preview_clear_tags", { paths });
    if (!isSelectionCurrent(subject)) return;
    setPreviewPlan(plan);
    setPreviewSource("clear");
    hooks.renderPreview(previewPlan);
    toast(
      previewPlan.changes.length
        ? t("toast.cover.previewClear", { files: tn("unit.file", previewPlan.changes.length) })
        : t("toast.cover.nothingToClear")
    );
  } catch (e) {
    toast(String(e), true);
  }
}

// ---- cover well (#editor design pass) ----
// A thumbnail + state + actions that replaces the two bare Embed/Export buttons.
// Reflects the selection's cover state: none / one shared / mixed.
async function refreshCoverWell() {
  const subject = captureSelection();
  const request = ++coverRequest;
  const paths = subject.paths;
  coverSubject = null;
  diskCoverSummary = null;
  lastRenderedCoverSummary = null;
  externalCover = null;
  coverViewModel = { selected: paths.length, loading: true, images: [], mixed: false, withCover: 0, external: false, action: () => {} };
  coverView.update(coverViewModel);
  coverWell.setAttribute("aria-busy", "true");
  if (paths.length === 0) {
    coverWell.className = "cover-well empty";
    coverView.update({ ...coverViewModel, loading: false });
    coverWell.removeAttribute("aria-busy");
    return;
  }
  try {
    const summary = await readCurrentCoverSummary(subject);
    if (!summary || request !== coverRequest || !isSelectionCurrent(subject)) return;
    coverSubject = subject;
    diskCoverSummary = summary;
    renderPendingCoverSummary();
    coverWell.removeAttribute("aria-busy");
    await showExternalCoverAction(subject, request);
  } catch (e) {
    if (request !== coverRequest || !isSelectionCurrent(subject)) return;
    coverWell.removeAttribute("aria-busy");
    coverView.update({ ...coverViewModel, loading: false });
    toast(String(e), true);
  }
}

// The external cover (folder.jpg/cover.jpg next to the tracks), when present —
// offered as a one-click embed under the well (#41).
let externalCover = null;

async function showExternalCoverAction(subject, request) {
  const paths = subject.paths;
  externalCover = null;
  let found;
  try {
    found = await invoke("read_external_cover", { paths });
  } catch (e) {
    return; // best-effort; the well already rendered
  }
  if (!found || request !== coverRequest || !isSelectionCurrent(subject)) return;
  externalCover = { image: found, subject };
  coverViewModel = { ...coverViewModel, external: true };
  coverView.update(coverViewModel, false);
}

// Embed the external cover file (folder.jpg/cover.jpg) into the selection (#41),
// through the same preview/apply/undo path as any other cover.
async function embedExternalCover() {
  const current = externalCover;
  if (!current || !isSelectionCurrent(current.subject) || current.subject !== coverSubject) return;
  await embedCoverDto(current.image, current.subject);
}

// Project pending artwork over the cached disk summary. Typing tag values
// never causes another artwork read or resets an unchanged lightbox/image.
function renderPendingCoverSummary() {
  if (!diskCoverSummary || !isSelectionCurrent(coverSubject)) return;
  let selected;
  const changes = [];
  for (const change of previewPlan?.changes || []) {
    if (!change.cover_change) continue;
    selected ??= new Set(coverSubject.paths);
    if (selected.has(change.path)) changes.push(change);
  }
  let summary = {...diskCoverSummary, pendingArtwork: changes.length > 0};
  if (changes.length) {
    let count = summary.with_cover;
    for (const change of changes) count += Number(change.cover_change.new.length > 0) - Number(change.cover_change.old.length > 0);
    const sets = changes.map(change => change.cover_change.new);
    const images = sets.map(set => set.find(image => image.kind === 'front') || set[0]).filter(Boolean);
    const untouched = coverSubject.paths.length - changes.length;
    if (untouched && summary.with_cover) images.push(...summary.samples);
    const first = images[0];
    const distinct = images.some(image => image.data_base64 !== first?.data_base64 || image.mime !== first?.mime) || (count > 0 && count < summary.total);
    summary = {...summary, with_cover:Math.max(0,Math.min(summary.total,count)), distinct, samples:first ? [first] : []};
  }
  const previous = lastRenderedCoverSummary;
  const sameImages = previous && previous.samples.length === summary.samples.length && previous.samples.every((image,index) => {
    const next = summary.samples[index];
    return image.mime === next.mime && image.data_base64 === next.data_base64 && image.description === next.description;
  });
  if (previous && previous.total === summary.total && previous.with_cover === summary.with_cover && previous.distinct === summary.distinct && previous.pendingArtwork === summary.pendingArtwork && sameImages) return;
  lastRenderedCoverSummary = summary;
  renderCoverWell(summary);
}
document.addEventListener('tagrex:transform-scope', renderPendingCoverSummary);

function renderCoverWell(summary) {
  const subject = coverSubject;
  coverWell.className = "cover-well" + (summary.with_cover ? "" : " empty");
  coverViewModel = {
    selected: summary.total, loading: false, mixed: !!summary.distinct,
    withCover: summary.with_cover, pendingArtwork:!!summary.pendingArtwork, canExport:(diskCoverSummary?.with_cover || 0) > 0, external: !!externalCover && isSelectionCurrent(externalCover.subject),
    images: summary.samples.slice(0, 1),
    action(name) {
      if (!isSelectionCurrent(subject) || subject !== coverSubject) return;
      if (name === "replace") chooseCover();
      else if (name === "remove") previewCoverRemove();
      else if (name === "export") exportCover();
      else if (name === "external") embedExternalCover();
    },
  };
  coverView.update(coverViewModel);
}

// Export the embedded cover of the selected files to disk (cover.<ext> next to
// each file). Read-only for the audio: no preview/apply, it just writes.
async function exportCover() {
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast(t("toast.cover.selectExport"), true);
    return;
  }
  try {
    const result = await invoke("export_cover", { paths, basename: "cover" });
    const wrote = result.written.length;
    const skipped = result.skipped_no_cover.length;
    if (wrote === 0) {
      toast(
        t(skipped ? "toast.cover.noneEmbedded" : "toast.cover.nothingToExport"),
        true
      );
      return;
    }
    const skipNote = skipped ? " " + t("toast.cover.skippedNote", { skipped }) : "";
    toast(t("toast.cover.exported", { files: tn("unit.coverFile", wrote) }) + skipNote);
  } catch (e) {
    toast(String(e), true);
  }
}

// ---- wire up ----
coverFileInput.addEventListener("change", onCoverChosen);
// Cover well: action buttons (delegated) + drag-and-drop embed.
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
  embedCoverFile,
  embedCoverFromPath,
  embedExternalCover,
  exportCover,
  onCoverChosen,
  previewClearTags,
  previewCoverRemove,
  refreshCoverWell,
};
