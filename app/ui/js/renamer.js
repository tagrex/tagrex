// The RENAMER mode (#143 split it out of app.js).
//
// Two masks over the selection: one renders each file's new name, the other a
// folder path. Both produce an ordinary plan, so the rename and the move
// preview, apply and undo exactly like every other change.
//
// The reorganize half also carries where to file the tracks (#153): a folder
// outside the opened library, move or copy, and whether to clear up the folders
// a move empties. The destination is only ever what the user picked here —
// choosing it is what authorizes the backend to write there.
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

// The destination and mode are working state for the panel, not backend
// settings — and re-picking the same library folder every session is exactly
// the friction this feature exists to remove.
const MOVE_DEST_STORAGE_KEY = "tagrex.moveDestination";
const MOVE_MODE_STORAGE_KEY = "tagrex.moveMode";
const MOVE_PRUNE_STORAGE_KEY = "tagrex.movePrune";

let moveMode = "move";

// Rename by mask (#37): render each selected file's new name from its tags.
// Which of this panel's two previews was last run, so a chain change can redo
// the same one rather than guessing (#248). Both stage a plan whose source is
// "rename", which is all the shared state records.
let lastPreview = null;

async function previewRename() {
  lastPreview = previewRename;
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast("Select at least one track", true);
    return;
  }
  try {
    // RENAMER's own chain runs on the names this just produced (#237) — where a
    // space becoming an underscore is the usual wish, and exactly the wish that
    // must not reach the tags FROM NAME reads.
    const named = await invoke("preview_rename", { mask: el("mask").value, paths });
    setPreviewPlan(await runChainOverPlan(named, "renamer"));
    setPreviewSource("rename");
    hooks.renderPreview(previewPlan);
  } catch (e) {
    toast(String(e), true);
  }
}

// ---- reorganize into folders (#37, destination + copy in #153) ----
// Builds the plan and shows it in the usual preview view, so the move is
// applied (and undone) through exactly the same path as a rename.
async function previewMove() {
  lastPreview = previewMove;
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast("Select the tracks to move first", true);
    return;
  }
  const copy = moveMode === "copy";
  try {
    const moved = await invoke("preview_move", {
      mask: el("move-mask").value,
      paths,
      destination: el("move-dest").value || null,
      copy,
      pruneEmptyDirs: el("move-prune").checked,
    });
    setPreviewPlan(await runChainOverPlan(moved, "renamer"));
    setPreviewSource("rename");
    hooks.renderPreview(previewPlan);
    toast(
      previewPlan.changes.length
        ? t(copy ? "toast.previewingCopy" : "toast.previewingMove", {
            files: tn("unit.file", previewPlan.changes.length),
          })
        : t("toast.nothingToMove"),
      previewPlan.changes.length === 0
    );
  } catch (e) {
    toast(String(e), true);
  }
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
  el("move-preview").textContent = mode === "copy" ? "Preview copy" : "Preview move";
  writeStored(MOVE_MODE_STORAGE_KEY, mode);
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
}

// ---- wire up ----
el("preview").addEventListener("click", previewRename);
el("move-preview").addEventListener("click", previewMove);
el("move-dest-pick").addEventListener("click", pickDestination);
el("move-dest-clear").addEventListener("click", () => setDestination(""));
el("move-mode").addEventListener("click", (e) => {
  const btn = e.target.closest(".seg-btn");
  if (btn) setMoveMode(btn.dataset.moveMode);
});
el("move-prune").addEventListener("change", () =>
  writeStored(MOVE_PRUNE_STORAGE_KEY, el("move-prune").checked ? "1" : "")
);

// Last session's destination and mode (#153).
el("move-dest").value = readStored(MOVE_DEST_STORAGE_KEY) || "";
el("move-prune").checked = !!readStored(MOVE_PRUNE_STORAGE_KEY);
setMoveMode(readStored(MOVE_MODE_STORAGE_KEY) === "copy" ? "copy" : "move");

// RENAMER has no read-out under its pattern: the staged diff IS its example, so
// a chain change redoes the preview that produced it (#248).
onChainChanged("renamer", () => {
  if (previewSource === "rename" && previewPlan?.changes.length) lastPreview?.();
});
