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
import { activeRuleCount, onChainChanged, runChainOverPlan } from "./chains.js";
import { hooks } from "./hooks.js";
import {
  previewPlan,
  previewSource,
  savedSettings,
  selectedPaths,
  setPreviewPlan,
  setPreviewSource,
  setSavedSettings,
} from "./state.js";

// The destination, mode, and whether reorganizing is even on are working
// state for the panel, not backend settings — and re-picking the same
// library folder every session is exactly the friction this feature exists
// to remove.
const MOVE_DEST_STORAGE_KEY = "tagrex.moveDestination";
const MOVE_MODE_STORAGE_KEY = "tagrex.moveMode";
const MOVE_PRUNE_STORAGE_KEY = "tagrex.movePrune";
const REORGANIZE_STORAGE_KEY = "tagrex.reorganizeEnabled";
// The mask itself (#402): the pattern last typed or picked, the way FROM NAME
// keeps its own (#141) — a relaunch used to put the markup's default back.
const MASK_STORAGE_KEY = "tagrex.renameMask";

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
  const key = !reorganize
    ? "renamer.previewRename"
    : moveMode === "copy"
      ? "renamer.previewCopy"
      : "renamer.previewMove";
  // The button's data-i18n follows the label: the static-text pass at startup
  // (and on every language change) rewrites it from that key, so a fixed
  // "previewRename" there overwrote "Preview move" right after it was set.
  const btn = el("preview");
  btn.dataset.i18n = key;
  btn.textContent = t(key);
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

// ---- what travels with a track (#405) ----
// Same-named files and a folder's leftovers are backend settings (they change
// what a plan carries), set here because this is where that plan is made. The
// panel writes them straight to settings.json, the same file Settings saves.
async function setCarry(key, on) {
  setSavedSettings({ ...savedSettings, [key]: on });
  try {
    await invoke("save_settings", { settings: savedSettings });
  } catch (e) {
    toast(String(e), true);
    return;
  }
  scheduleMaskExample();
  // A staged rename was built with the old setting; build it again.
  if (previewSource === "rename" && previewPlan?.changes.length) preview();
}

// Show the saved values once settings.json is loaded (app.js calls this).
function syncCarryToggles() {
  el("carry-sidecars").checked = savedSettings.carry_sidecars !== false;
  el("carry-extras").checked = savedSettings.carry_folder_extras !== false;
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
    // Through the same chain Preview runs (#383), or the example and the plan
    // disagree — the example said "The X Factor - Desert Rain" while the plan,
    // lower-cased and underscored by the wand's rules, said something else.
    const plan = await (reorganize ? movePlan([path]) : renamePlan([path]));
    const out = await runChainOverPlan(plan, "renamer");
    const change = out.changes[0];
    const to = change?.rename_to;
    box.textContent = to ? maskPart(to, change.rename_root) : t("renamer.exampleUnchanged");
    box.classList.toggle("muted", !to);
    // The ticked presets' rules run on every rename without being on this panel,
    // so the example says so — otherwise its result reads as the mask's alone.
    const rules = activeRuleCount("renamer");
    if (rules) {
      const note = document.createElement("span");
      note.className = "mask-example-chain";
      note.textContent = ` · ${t("renamer.exampleChain", { rules: tn("unit.rule", rules) })}`;
      box.append(note);
    }
  } catch (e) {
    box.textContent = String(e);
    box.classList.add("muted");
  }
}

// Exactly what the mask produced (#384): the target below the folder it was
// rendered under — the file's own folder, or the destination — the way an
// inline example in a tagger usually reads. No root to split at (an older plan)
// falls back to the last few segments.
function maskPart(fullPath, root) {
  if (root) {
    const base = root.replace(/[\\/]+$/, "");
    if (fullPath.startsWith(base) && /[\\/]/.test(fullPath[base.length] || "")) {
      return fullPath.slice(base.length + 1);
    }
  }
  const sep = fullPath.includes("\\") && !fullPath.includes("/") ? "\\" : "/";
  return fullPath.split(/[\\/]/).filter(Boolean).slice(-3).join(sep);
}

let maskExampleTimer = null;
function scheduleMaskExample() {
  if (el("panel-renamer").hidden) return;
  clearTimeout(maskExampleTimer);
  maskExampleTimer = setTimeout(refreshMaskExample, 180);
}

// ---- wire up ----
el("preview").addEventListener("click", preview);
el("mask").addEventListener("input", () => {
  writeStored(MASK_STORAGE_KEY, el("mask").value);
  scheduleMaskExample();
});
el("reorganize-toggle").addEventListener("change", (e) => setReorganize(e.target.checked));
el("move-dest-pick").addEventListener("click", pickDestination);
el("move-dest-clear").addEventListener("click", () => setDestination(""));
el("move-mode").addEventListener("click", (e) => {
  const btn = e.target.closest(".seg-btn");
  if (btn) setMoveMode(btn.dataset.moveMode);
});
el("carry-sidecars").addEventListener("change", (e) => setCarry("carry_sidecars", e.target.checked));
el("carry-extras").addEventListener("change", (e) => setCarry("carry_folder_extras", e.target.checked));
el("move-prune").addEventListener("change", () => {
  writeStored(MOVE_PRUNE_STORAGE_KEY, el("move-prune").checked ? "1" : "");
  scheduleMaskExample();
});

// Last session's mask (#402), destination, mode and whether reorganizing was on
// (#153, #382). An empty stored mask is not restored: the markup's default is a
// better start than a blank field.
el("mask").value = readStored(MASK_STORAGE_KEY) || el("mask").value;
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

export { scheduleMaskExample, syncCarryToggles };
