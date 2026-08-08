// The RENAMER mode (#143 split it out of app.js).
//
// Two masks over the selection: one renders each file's new name, the other a
// folder path under the library root. Both produce an ordinary plan, so the
// rename and the move preview, apply and undo exactly like every other change.
import { el, toast } from "./dom.js";
import { invoke } from "./invoke.js";
import { hooks } from "./hooks.js";
import { previewPlan, selectedPaths, setPreviewPlan, setPreviewSource } from "./state.js";

// Rename by mask (#37): render each selected file's new name from its tags.
async function previewRename() {
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast("Select at least one track", true);
    return;
  }
  try {
    setPreviewPlan(await invoke("preview_rename", { mask: el("mask").value, paths }));
    setPreviewSource("rename");
    hooks.renderPreview(previewPlan);
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
    setPreviewPlan(await invoke("preview_move", { mask: el("move-mask").value, paths }));
    setPreviewSource("rename");
    hooks.renderPreview(previewPlan);
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

// ---- wire up ----
el("preview").addEventListener("click", previewRename);
el("move-preview").addEventListener("click", previewMove);
