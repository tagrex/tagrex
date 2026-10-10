// Progress and Cancel for a running Apply (#437).
//
// The backend emits `tagrex://apply-progress` while it writes and stops at the
// next file after `cancel_apply`. This only paints that in the diff bar; the
// bar shows nothing for the first moment, so a quick apply does not flash it.
import { el } from "./dom.js";
import { invoke } from "./invoke.js";
import { t } from "./i18n.js";

const EVENT = "tagrex://apply-progress";
const SHOW_AFTER_MS = 250;

let unlisten = null;
let showTimer = null;

function baseName(path) {
  return String(path || "").split(/[\\/]/).pop();
}

function paint(progress) {
  const total = progress.total || 0;
  el("ab-bar-fill").style.width = total ? `${Math.round((progress.done / total) * 100)}%` : "0%";
  if (!el("diff-cancel-apply").disabled) el("ab-file").textContent = baseName(progress.path);
}

export async function beginApplyProgress() {
  const bar = el("diff-actionbar");
  el("ab-bar-fill").style.width = "0%";
  el("ab-file").textContent = "";
  el("diff-cancel-apply").disabled = false;
  showTimer = setTimeout(() => bar.classList.add("applying"), SHOW_AFTER_MS);
  const events = window.__TAURI__ && window.__TAURI__.event;
  if (events) unlisten = await events.listen(EVENT, (e) => paint(e.payload));
}

export function endApplyProgress() {
  clearTimeout(showTimer);
  showTimer = null;
  if (unlisten) unlisten();
  unlisten = null;
  el("diff-actionbar").classList.remove("applying");
}

// The backend stops at the next file and rolls back; until it answers, the
// button is spent and the bar says so.
el("diff-cancel-apply").addEventListener("click", () => {
  el("diff-cancel-apply").disabled = true;
  el("ab-file").textContent = t("apply.cancelling");
  invoke("cancel_apply", {});
});
