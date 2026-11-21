// The DEDUPLICATOR mode (#40, #118, #143 split it out of app.js).
//
// A read-only scan: it groups the library by the chosen criterion and shows
// what it found behind a lock banner. Nothing here changes a file — deleting a
// duplicate is deliberately not offered.
import { el, escapeHtml, fileName, toast } from "./dom.js";
import { currentLang, t, tn } from "./i18n.js";
import { invoke } from "./invoke.js";
import { selectedPaths } from "./state.js";
import { beginActivity } from "./activity.js";

const AUDIO_EVENT = "tagrex://duplicate-progress";
let audioRunning = false;
let audioStopping = false;
// This scan's handle on the activity card (#843).
let activity = null;
// The last scan's outcome, drawn again on a language change or a return to the
// mode (#822): { criterion, groups, cancelled, failed }. Null before any scan.
let last = null;
let scanning = false;

// The Scan button is Stop while an identical-audio scan runs. Its data-i18n
// key is swapped along with the text, so a language change mid-run keeps the
// right label (#287).
function paintScanButton() {
  const button = el("dup-scan");
  const key = audioRunning ? "dedup.stop" : "dedup.scan";
  button.dataset.i18n = key;
  button.textContent = t(key);
  button.disabled = audioStopping;
}

function paintAudioProgress(stage, done, total) {
  activity.progress({
    title: t(stage === "full" ? "activity.dedupFull" : "activity.dedupQuick"),
    percent: total ? (done / total) * 100 : null,
    detail: total ? t("activity.countOf", { done, total }) : "",
  });
}

function stopAudioScan() {
  if (!audioRunning || audioStopping) return;
  audioStopping = true;
  paintScanButton();
  activity.stopping(t("dedup.stopping"));
  invoke("cancel_find_duplicates", {});
}

// Identical decoded audio (#821): decoding takes a while on a big library, so
// it reports progress on the activity card (#843) and can be stopped there or
// from the Scan button, keeping the groups confirmed so far.
async function runAudioScan() {
  audioRunning = true;
  audioStopping = false;
  paintScanButton();
  activity = beginActivity({ title: t("activity.dedupQuick"), stop: stopAudioScan });
  const events = window.__TAURI__ && window.__TAURI__.event;
  const unlisten = events
    ? await events.listen(AUDIO_EVENT, (e) => paintAudioProgress(e.payload.stage, e.payload.done, e.payload.total))
    : null;
  try {
    const result = await invoke("find_audio_duplicates", {});
    last = { criterion: "audio", groups: result.groups, cancelled: result.cancelled, failed: result.failed.length };
    scanning = false;
    paintResults();
  } catch (e) {
    el("dup-summary").textContent = "";
    toast(String(e), true);
  } finally {
    if (unlisten) unlisten();
    audioRunning = false;
    audioStopping = false;
    activity.finish();
    activity = null;
    paintScanButton();
  }
}

// What the identical-audio criterion costs, under the picker.
function paintCriterionHint() {
  el("dup-audio-hint").hidden = el("dup-criterion").value !== "audio";
}

async function runDuplicateScan() {
  if (audioRunning) {
    stopAudioScan();
    return;
  }
  const criterion = el("dup-criterion").value;
  el("dup-summary").textContent = t("dedup.scanning");
  scanning = true;
  if (criterion === "audio") {
    el("dup-results").innerHTML = "";
    try {
      await runAudioScan();
    } finally {
      scanning = false;
    }
    return;
  }
  el("dup-results").innerHTML = "";
  try {
    const groups = await invoke("find_duplicates", { criterion });
    last = { criterion, groups, cancelled: false, failed: 0 };
    scanning = false;
    paintResults();
  } catch (e) {
    el("dup-summary").textContent = "";
    toast(String(e), true);
  } finally {
    scanning = false;
  }
}

// The line a group is headed with (#822). Criteria that match on tag values
// show those values; the others are labelled here, in the interface language,
// from what every file in the group shares.
function groupLabel(group, criterion) {
  const first = group.files[0];
  switch (criterion) {
    case "hash":
      return t("dedup.key.identicalBytes");
    case "audio":
      return t("dedup.key.identicalAudio");
    case "size":
      return t("dedup.key.size", { size: first.size_bytes.toLocaleString(currentLang()) });
    case "duration":
      return mmss(first.duration_secs);
    default:
      return group.key;
  }
}

// Draw the last scan's outcome in the current language.
function paintResults() {
  if (!last) return;
  if (last.cancelled && !last.groups.length) {
    // Stopped before anything was confirmed: that is not a clean library.
    el("dup-summary").textContent = t("dedup.stoppedEarly");
    el("dup-results").innerHTML = `<p class="empty inert-panel">${t("dedup.pickKey")}</p>`;
    return;
  }
  renderDuplicates(last.groups, last.criterion);
  const notes = [];
  if (last.cancelled) notes.push(t("dedup.stopped"));
  if (last.failed) notes.push(tn("dedup.failed", last.failed));
  if (notes.length) el("dup-summary").textContent += ` · ${notes.join(" · ")}`;
}

function humanSize(bytes) {
  if (!bytes) return "";
  const units = ["B", "KB", "MB", "GB"];
  let n = bytes;
  let i = 0;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i += 1;
  }
  return `${n < 10 && i > 0 ? n.toFixed(1) : Math.round(n)} ${units[i]}`;
}

function mmss(secs) {
  if (!secs) return "";
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  return `${m}:${String(s).padStart(2, "0")}`;
}

// Entering DEDUPLICATOR with no scan yet shows a prompt; a prior scan's results
// persist across mode switches (#118), and are drawn again so a language
// changed meanwhile reaches them (#822).
function refreshDeduplicator() {
  if (scanning) return;
  if (last) {
    paintResults();
    return;
  }
  const results = el("dup-results");
  if (!results.querySelector("table, .empty")) {
    results.innerHTML = `<p class="empty inert-panel">${t("dedup.pickKey")}</p>`;
  }
}

function renderDuplicates(groups, criterion) {
  const results = el("dup-results");
  const fileCount = groups.reduce((n, g) => n + g.files.length, 0);
  el("dup-summary").textContent = groups.length
    ? `${tn("unit.set", groups.length)} · ${tn("unit.file", fileCount)}`
    : t("dedup.none");
  if (!groups.length) {
    results.innerHTML = `<p class="empty inert-panel">${escapeHtml(t("dedup.clean"))}</p>`;
    return;
  }
  // Same .files table shell as the main view, so the read-only result set reads
  // as the workspace in a grouped state (design A7). Group rows carry an
  // "N copies" badge + the matched key.
  const rows = groups
    .map((g) => {
      const head = `<tr class="dup-group"><td colspan="6"><span class="dup-badge">${escapeHtml(tn("dedup.copies", g.files.length))}</span><span class="dup-key">${escapeHtml(groupLabel(g, criterion))}</span></td></tr>`;
      const files = g.files
        .map(
          (f) => `<tr>
            <td class="file" title="${escapeHtml(f.path)}">${escapeHtml(fileName(f.path))}</td>
            <td>${escapeHtml(f.artist)}</td>
            <td>${escapeHtml(f.title)}</td>
            <td>${escapeHtml(f.album)}</td>
            <td class="num">${mmss(f.duration_secs)}</td>
            <td class="num dup-note">${humanSize(f.size_bytes)}${f.bitrate_kbps ? ` · ${f.bitrate_kbps}k` : ""}</td>
          </tr>`,
        )
        .join("");
      return head + files;
    })
    .join("");
  results.innerHTML = `<table class="files dup-results-table">
    <thead><tr><th>${escapeHtml(t("column.file"))}</th><th>${escapeHtml(t("field.artist"))}</th><th>${escapeHtml(t("field.title"))}</th><th>${escapeHtml(t("field.album"))}</th><th class="num">${escapeHtml(t("field.length"))}</th><th class="num">${escapeHtml(t("dedup.sizeRate"))}</th></tr></thead>
    <tbody>${rows}</tbody></table>`;
}

el("dup-criterion").addEventListener("change", paintCriterionHint);
paintCriterionHint();

export { refreshDeduplicator, runDuplicateScan };
