// BPM detection (#536): measure the tempo of the selected files from the audio
// and stage it in the BPM field, through the ordinary preview/apply/undo path.
//
// The measuring takes seconds a file, so it runs in the backend while the window
// stays usable. The toolbar button is one toggling slot, like the online search
// (#111): it starts a run, and while one is going it is a Stop that ends it at
// the current file, staging what was measured before. Progress goes on the
// activity card, the one apply and undo use (#843).
//
// Settings can switch the whole thing off (#538), which hides the button; the
// shortcut presses the button, so it goes quiet with it.
import { el, report, toast } from "./dom.js";
import { invoke } from "./invoke.js";
import { t, tn } from "./i18n.js";
import { hooks } from "./hooks.js";
import { beginActivity } from "./activity.js";
import {
  previewPlan,
  selectedPaths,
  setPreviewPlan,
  setPreviewSource,
} from "./state.js";

const EVENT = "tagrex://bpm-progress";
const button = el("detect-bpm");

let running = false;
let stopping = false;
// This run's handle on the activity card (#843).
let activity = null;

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
  activity.progress({
    percent: total ? (done / total) * 100 : null,
    detail: t("activity.countOf", { done: Math.min(done + 1, total), total }),
  });
}

// The report after a run (#558): what was staged as the headline, then a line
// for each other thing worth knowing. Stopping and skipped mixes are fine;
// no steady beat and files that would not decode need a look.
function reportRun(result, total) {
  const staged = result.plan.changes.length;
  report(staged ? t("toast.bpm.staged", { files: tn("unit.file", staged) }) : t("toast.bpm.unchanged"), [
    result.cancelled && { text: t("toast.bpm.stopped", { done: result.analysed, total }) },
    result.too_long && { text: tn("toast.bpm.tooLong", result.too_long) },
    result.no_tempo && { text: tn("toast.bpm.noTempo", result.no_tempo), problem: true },
    result.failed.length && { text: tn("toast.bpm.failed", result.failed.length), problem: true },
  ]);
}

// Stop the run at the current file, keeping what was measured. From the button
// and from Escape (#539).
export function stopBpm() {
  if (!running || stopping) return;
  stopping = true;
  paintButton();
  // Stopping (#558): the count freezes, the bar goes muted, Stop is spent.
  activity.stopping(t("bpm.stopping"));
  invoke("cancel_detect_bpm", {});
}

export function bpmRunning() {
  return running;
}

async function detectBpm() {
  if (running) {
    stopBpm();
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
  // Stop on the card, where the run is watched (#558), as well as on the
  // toolbar button that started it.
  activity = beginActivity({ title: t("activity.bpm"), stop: stopBpm });
  paintProgress(0, paths.length);
  const events = window.__TAURI__ && window.__TAURI__.event;
  const unlisten = events
    ? await events.listen(EVENT, (e) => paintProgress(e.payload.done, e.payload.total))
    : null;
  try {
    const result = await invoke("detect_bpm", { paths });
    // A run that measured nothing new leaves whatever is staged alone.
    if (result.plan.changes.length || !previewPlan) {
      setPreviewPlan(result.plan);
      setPreviewSource("bpm");
      hooks.renderPreview(previewPlan);
    }
    reportRun(result, paths.length);
  } catch (e) {
    toast(String(e), true);
  } finally {
    if (unlisten) unlisten();
    running = false;
    stopping = false;
    // The report toast says how it went; the card just goes.
    activity.finish();
    activity = null;
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
