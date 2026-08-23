// The DEDUPLICATOR mode (#40, #118, #143 split it out of app.js).
//
// A read-only scan: it groups the library by the chosen criterion and shows
// what it found behind a lock banner. Nothing here changes a file — deleting a
// duplicate is deliberately not offered.
import { el, escapeHtml, fileName, toast } from "./dom.js";
import { t, tn } from "./i18n.js";
import { invoke } from "./invoke.js";
import { selectedPaths } from "./state.js";

async function runDuplicateScan() {
  const criterion = el("dup-criterion").value;
  el("dup-summary").textContent = "Scanning…";
  el("dup-results").innerHTML = "";
  try {
    const groups = await invoke("find_duplicates", { criterion });
    renderDuplicates(groups);
  } catch (e) {
    el("dup-summary").textContent = "";
    toast(String(e), true);
  }
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
// persist across mode switches (#118).
function refreshDeduplicator() {
  const results = el("dup-results");
  if (!results.querySelector("table, .empty")) {
    results.innerHTML = `<p class="empty inert-panel">Pick a key in the panel and <b>Scan the library</b> to find duplicates.</p>`;
  }
}

function renderDuplicates(groups) {
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
      const head = `<tr class="dup-group"><td colspan="6"><span class="dup-badge">${g.files.length} copies</span><span class="dup-key">${escapeHtml(g.key)}</span></td></tr>`;
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
    <thead><tr><th>File</th><th>Artist</th><th>Title</th><th>Album</th><th class="num">Length</th><th class="num">Size · Rate</th></tr></thead>
    <tbody>${rows}</tbody></table>`;
}

export { refreshDeduplicator, runDuplicateScan };
