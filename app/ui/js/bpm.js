// BPM detection (#536): measure the tempo of the selected files from the audio
// and stage it in the BPM field, through the ordinary preview/apply/undo path.
//
// The measuring takes seconds a file, so it runs in the backend while the window
// stays usable. The toolbar button is one toggling slot, like the online search
// (#111): it starts a run, and while one is going it is a Stop that ends it at
// the current file, staging what was measured before. Progress goes in the
// status bar's busy pill, the one undo uses.
//
// Settings can switch the whole thing off (#538), which hides the button; the
// shortcut presses the button, so it goes quiet with it.
import { el, toast } from "./dom.js";
import { invoke } from "./invoke.js";
import { t, tn } from "./i18n.js";
import { hooks } from "./hooks.js";
import { applyColumns } from "./columns.js";
import {
  previewPlan,
  selectedPaths,
  setPreviewPlan,
  setPreviewSource,
  visibleColumns,
} from "./state.js";

const EVENT = "tagrex://bpm-progress";
const button = el("detect-bpm");

let running = false;
let stopping = false;

// Title and label come from here rather than from data-i18n attributes: they
// change with the run, and a language switch would otherwise put the idle text
// back on a Stop button (#287).
function paintButton() {
  button.classList.toggle("busy", running);
  button.disabled = stopping;
  button.title = running ? t("toolbar.detectBpmStop") : t("toolbar.detectBpmTitle");
  button.setAttribute("aria-label", running ? t("toolbar.detectBpmStop") : t("toolbar.detectBpm"));
}

function paintProgress(done, total) {
  el("busy-fill").style.width = total ? `${Math.round((done / total) * 100)}%` : "0%";
  el("busy-text").textContent = stopping
    ? t("bpm.stopping")
    : t("bpm.progress", { done: Math.min(done + 1, total), total });
}

// The toast after a run: what was staged, then what could not be measured.
function summary(result, total) {
  const staged = result.plan.changes.length;
  const parts = [];
  if (result.cancelled) parts.push(t("toast.bpm.stopped", { done: result.analysed, total }));
  parts.push(
    staged ? t("toast.bpm.staged", { files: tn("unit.file", staged) }) : t("toast.bpm.unchanged")
  );
  if (result.too_long) parts.push(tn("toast.bpm.tooLong", result.too_long));
  if (result.no_tempo) parts.push(tn("toast.bpm.noTempo", result.no_tempo));
  if (result.failed.length) parts.push(tn("toast.bpm.failed", result.failed.length));
  return parts.join(" · ");
}

async function detectBpm() {
  if (running) {
    stopping = true;
    paintButton();
    el("busy-text").textContent = t("bpm.stopping");
    invoke("cancel_detect_bpm", {});
    return;
  }
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast(t("toast.bpm.select"), true);
    return;
  }
  running = true;
  stopping = false;
  paintButton();
  paintProgress(0, paths.length);
  el("busy-pill").hidden = false;
  const events = window.__TAURI__ && window.__TAURI__.event;
  const unlisten = events
    ? await events.listen(EVENT, (e) => paintProgress(e.payload.done, e.payload.total))
    : null;
  try {
    const result = await invoke("detect_bpm", { paths });
    // A run that measured nothing new leaves whatever is staged alone.
    if (result.plan.changes.length || !previewPlan) {
      // The diff shows a change in the column of its field, and a BPM column
      // the table does not have would leave the preview with nothing in it. It
      // stays afterwards, like any column picked in the menu.
      if (result.plan.changes.length && !visibleColumns.includes("bpm")) {
        applyColumns([...visibleColumns, "bpm"]);
      }
      setPreviewPlan(result.plan);
      setPreviewSource("bpm");
      hooks.renderPreview(previewPlan);
    }
    toast(summary(result, paths.length), result.failed.length > 0 && !result.plan.changes.length);
  } catch (e) {
    toast(String(e), true);
  } finally {
    if (unlisten) unlisten();
    running = false;
    stopping = false;
    el("busy-pill").hidden = true;
    paintButton();
  }
}

export function retranslateBpm() {
  paintButton();
}

export function setBpmEnabled(on) {
  button.hidden = !on;
}

button.addEventListener("click", detectBpm);
paintButton();
invoke("load_settings", {})
  .then((settings) => setBpmEnabled(!settings || settings.bpm_detection !== false))
  .catch(() => {});
