// The RENAMER mode (#143 split it out of app.js; unified in #382).
//
// One mask over the selection: a `/` or `\` in it restructures folders
// wherever the file already is, the same way renaming it in place always
// has — no second pattern to keep in sync with the first. Reorganizing is an
// on/off extra on top: move or copy the same result to a different folder
// instead of leaving files where they sit. Both directions produce an
// ordinary plan, so preview, apply and undo work exactly like every other
// change.
import { el, toast } from "./dom.js";
import { t, tn } from "./i18n.js";
import { invoke } from "./invoke.js";
import { onChainChanged, runChainOverPlan } from "./chains.js";
import { hooks } from "./hooks.js";
import {
  previewPlan,
  previewSource,
  selectedPaths,
  setPreviewPlan,
  setPreviewSource,
} from "./state.js";

// The destination, mode, and whether reorganizing is even on are working
// state for the panel, not backend settings — and re-picking the same
// library folder every session is exactly the friction this feature exists
// to remove.
const MOVE_DEST_STORAGE_KEY = "tagrex.moveDestination";
const MOVE_MODE_STORAGE_KEY = "tagrex.moveMode";
const MOVE_PRUNE_STORAGE_KEY = "tagrex.movePrune";
const REORGANIZE_STORAGE_KEY = "tagrex.reorganizeEnabled";

let moveMode = "move";
let reorganize = false;

function renamePlan(paths) {
  return invoke("preview_rename", { mask: el("mask").value, paths });
}

function movePlan(paths) {
  return invoke("preview_move", {
    mask: el("mask").value,
    paths,
    destination: el("move-dest").value || null,
    copy: moveMode === "copy",
    pruneEmptyDirs: el("move-prune").checked,
  });
}

async function preview() {
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast(reorganize ? "Select the tracks to move first" : "Select at least one track", true);
    return;
  }
  try {
    // RENAMER's own chain runs on the names this just produced (#237) — where a
    // space becoming an underscore is the usual wish, and exactly the wish that
    // must not reach the tags FROM NAME reads.
    const plan = await (reorganize ? movePlan(paths) : renamePlan(paths));
    setPreviewPlan(await runChainOverPlan(plan, "renamer"));
    setPreviewSource("rename");
    hooks.renderPreview(previewPlan);
    if (reorganize) {
      const copy = moveMode === "copy";
      toast(
        previewPlan.changes.length
          ? t(copy ? "toast.previewingCopy" : "toast.previewingMove", {
              files: tn("unit.file", previewPlan.changes.length),
            })
          : t("toast.nothingToMove"),
        previewPlan.changes.length === 0
      );
    }
  } catch (e) {
    toast(String(e), true);
  }
}

// ---- reorganize on/off (#153 destination + copy, unified in #382) ----

function updatePreviewLabel() {
  el("preview").textContent = !reorganize
    ? t("renamer.previewRename")
    : moveMode === "copy"
      ? t("renamer.previewCopy")
      : t("renamer.previewMove");
}

function setReorganize(on) {
  reorganize = on;
  el("reorganize-toggle").checked = on;
  el("reorganize-options").hidden = !on;
  updatePreviewLabel();
  writeStored(REORGANIZE_STORAGE_KEY, on ? "1" : "");
  scheduleMaskExample();
}

// Move or copy. Pruning only means anything for a move — a copy empties
// nothing — so the checkbox goes inert rather than quietly doing nothing.
function setMoveMode(mode) {
  moveMode = mode;
  el("move-mode")
    .querySelectorAll(".seg-btn")
    .forEach((b) => b.classList.toggle("active", b.dataset.moveMode === mode));
  const prune = el("move-prune");
  prune.disabled = mode === "copy";
  prune.closest(".rule-flag").classList.toggle("off", mode === "copy");
  updatePreviewLabel();
  writeStored(MOVE_MODE_STORAGE_KEY, mode);
  scheduleMaskExample();
}

function writeStored(key, value) {
  try {
    localStorage.setItem(key, value);
  } catch (e) {
    /* localStorage unavailable — preference just won't persist */
  }
}

function readStored(key) {
  try {
    return localStorage.getItem(key);
  } catch (e) {
    return null;
  }
}

// The native folder chooser, the same plugin the library Browse button uses.
// Outside Tauri there is none, so browser-dev gets a typed path instead — it is
// the only way to exercise the flow there.
async function pickDestination() {
  const dialog = window.__TAURI__ && window.__TAURI__.dialog;
  if (!dialog) {
    const typed = prompt("Destination folder (browser dev only)");
    if (typed) setDestination(typed);
    return;
  }
  try {
    const picked = await dialog.open({ directory: true, multiple: false });
    if (!picked) return; // user cancelled
    setDestination(picked);
  } catch (e) {
    toast(String(e), true);
  }
}

function setDestination(path) {
  el("move-dest").value = path || "";
  writeStored(MOVE_DEST_STORAGE_KEY, path || "");
  scheduleMaskExample();
}

// ---- live single-file example (#382) ----
// The read-out under the mask: what it renders for the first selected file,
// the same idea as FROM NAME's own probe (extraction rather than rendering,
// but the same "show, don't make me click Preview" point) — a per-keystroke
// round trip, debounced.
async function refreshMaskExample() {
  const box = el("mask-example");
  if (!box) return;
  const path = selectedPaths()[0];
  if (!path) {
    box.textContent = "";
    box.classList.remove("muted");
    return;
  }
  try {
    const plan = await (reorganize ? movePlan([path]) : renamePlan([path]));
    const to = plan.changes[0]?.rename_to;
    if (to) {
      box.textContent = shortenExample(to);
      box.classList.remove("muted");
    } else {
      box.textContent = t("renamer.exampleUnchanged");
      box.classList.add("muted");
    }
  } catch (e) {
    box.textContent = String(e);
    box.classList.add("muted");
  }
}

// The last couple of path segments read as "what this does" without the
// noise of a full absolute path — what TagScanner's own inline example goes
// for too.
function shortenExample(fullPath) {
  const sep = fullPath.includes("\\") && !fullPath.includes("/") ? "\\" : "/";
  const parts = fullPath.split(/[\\/]/).filter(Boolean);
  return parts.slice(-3).join(sep);
}

let maskExampleTimer = null;
function scheduleMaskExample() {
  if (el("panel-renamer").hidden) return;
  clearTimeout(maskExampleTimer);
  maskExampleTimer = setTimeout(refreshMaskExample, 180);
}

// ---- wire up ----
el("preview").addEventListener("click", preview);
el("mask").addEventListener("input", scheduleMaskExample);
el("reorganize-toggle").addEventListener("change", (e) => setReorganize(e.target.checked));
el("move-dest-pick").addEventListener("click", pickDestination);
el("move-dest-clear").addEventListener("click", () => setDestination(""));
el("move-mode").addEventListener("click", (e) => {
  const btn = e.target.closest(".seg-btn");
  if (btn) setMoveMode(btn.dataset.moveMode);
});
el("move-prune").addEventListener("change", () => {
  writeStored(MOVE_PRUNE_STORAGE_KEY, el("move-prune").checked ? "1" : "");
  scheduleMaskExample();
});

// Last session's destination, mode and whether reorganizing was on (#153, #382).
el("move-dest").value = readStored(MOVE_DEST_STORAGE_KEY) || "";
el("move-prune").checked = !!readStored(MOVE_PRUNE_STORAGE_KEY);
setMoveMode(readStored(MOVE_MODE_STORAGE_KEY) === "copy" ? "copy" : "move");
setReorganize(!!readStored(REORGANIZE_STORAGE_KEY));

// RENAMER has no read-out under its pattern beyond the single-file example: the
// staged diff IS the full example, so a chain change redoes the preview that
// produced it (#248) and refreshes the example the same way.
onChainChanged("renamer", () => {
  scheduleMaskExample();
  if (previewSource === "rename" && previewPlan?.changes.length) preview();
});

export { scheduleMaskExample };
