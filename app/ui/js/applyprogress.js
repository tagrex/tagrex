// Progress and Cancel for a running Apply (#437), undo (#448) and redo (#775).
//
// The backend emits `tagrex://apply-progress` while it works and stops an
// apply at the next file after `cancel_apply`. Since #843 all of it is shown
// on the activity card (js/activity.js): the stage, the share done, the file.
import { invoke } from "./invoke.js";
import { t } from "./i18n.js";
import { beginActivity } from "./activity.js";

const EVENT = "tagrex://apply-progress";

let unlisten = null;
let activity = null;

function baseName(path) {
  return String(path || "").split(/[\\/]/).pop();
}

async function listen(callback) {
  const events = window.__TAURI__ && window.__TAURI__.event;
  if (events) return events.listen(EVENT, (e) => callback(e.payload));
  // The browser mock has no event bus: it dispatches the same payloads on the
  // document, so the card can be checked without the native window.
  const listener = (e) => callback(e.detail);
  document.addEventListener(EVENT, listener);
  return () => document.removeEventListener(EVENT, listener);
}

function stopListening() {
  if (unlisten) unlisten();
  unlisten = null;
}

// An apply's events count the preflight reads first (one per file), then the
// write steps (#437). While `done` is below the file count the files are being
// checked, which writes nothing; after that each new path is the next file.
// A folder rename (#841) is one step for the whole folder: no file count.
export async function beginApplyProgress(plan, folderRename = false) {
  const files = folderRename ? 0 : plan.changes.length;
  const seen = new Set();
  let percent = 0;
  stopListening();
  activity = beginActivity({ title: t("activity.checking"), stop: cancelApply, foreground: true });
  const own = activity;
  unlisten = await listen(({ done, total, path }) => {
    if (activity !== own) return;
    const name = baseName(path);
    if (done < files && total > files) {
      own.progress({ title: t("activity.checking"), percent: null, detail: name });
      return;
    }
    seen.add(path);
    const steps = Math.max(1, total - files);
    percent = Math.max(percent, Math.min(99, ((done - files) / steps) * 100));
    own.progress({
      title: t("activity.writing"),
      percent,
      detail: files > 1 ? t("activity.fileOf", { n: Math.min(seen.size, files), total: files, file: name }) : name,
    });
  });
}

// The plan is written; the list is read again before the outcome shows.
export function finishingApplyProgress() {
  stopListening();
  if (activity) {
    activity.progress({ title: t("activity.finishing"), percent: 99, detail: t("activity.refreshing"), stoppable: false });
  }
}

/// `outcome` is "success", "cancelled", "error" or null (close at once).
export function endApplyProgress(outcome = null, title = "", detail = "") {
  stopListening();
  if (activity) activity.finish(outcome, title, detail);
  activity = null;
}

// An undo or a redo (`running` names which): no Stop, since a rollback
// stopped half-way would leave the batch partly undone (#448).
export async function beginUndoProgress(running = "undo.running") {
  stopListening();
  activity = beginActivity({ title: t(running), foreground: true });
  const own = activity;
  unlisten = await listen(({ done, total, path }) => {
    if (activity !== own) return;
    own.progress({ percent: total ? (done / total) * 100 : null, detail: baseName(path) });
  });
}

export function endUndoProgress(outcome = null, title = "", detail = "") {
  endApplyProgress(outcome, title, detail);
}

// The backend stops at the next file and rolls back; until it answers, Stop
// is spent and the card says so.
function cancelApply() {
  if (activity) activity.stopping(t("apply.cancelling"));
  invoke("cancel_apply", {});
}
