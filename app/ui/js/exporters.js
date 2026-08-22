// The exporters (#19, #143 split them out of app.js).
//
// Playlist, CUE, CSV, HTML, XML and the mask report. All read-only: they build a
// file in the opened library and deliberately bypass the change pipeline, since
// nothing about the tracks themselves is touched.
import { el, fileName, plural, toast } from "./dom.js";
import { invoke } from "./invoke.js";
import { selectedPaths } from "./state.js";

// Default output name per export kind; the user can override it. The backend
// only accepts a bare file name and writes into the opened library.
const EXPORT_DEFAULTS = {
  playlist: "playlist.m3u",
  cue: "tracks.cue",
  csv: "tags.csv",
  html: "tags.html",
  xml: "tags.xml",
  report: "report.txt",
};
// One-line "what it produces" hint per format, swapped under the segmented
// control (allows <b> emphasis, so set via innerHTML).
const EXPORT_HINTS = {
  playlist: "An <b>.m3u</b> playlist of the selected tracks, in table order.",
  cue: "A <b>.cue</b> sheet — one <b>FILE</b> per track, numbered in table order.",
  csv: "One <b>row per track</b> with the tag columns — opens in any spreadsheet.",
  html: "A self-contained <b>HTML table</b> of the tag columns — opens in any browser.",
  xml: "An <b>XML document</b> — one element per tag, for scripts and other tools.",
  report: "Each track rendered through the <b>mask</b> below, one line apiece.",
};
let exportKind = "playlist";

// Refresh the EXPORTER panel for the current selection (called on entering the
// mode). Reflects the current format; only fills the file name when it's empty,
// so a name the user typed survives a mode switch.
function refreshExporter() {
  const count = selectedPaths().length;
  el("export-count").textContent = count ? `— ${plural(count, "track", "tracks")}` : "";
  reflectExportKind();
  if (!el("export-name").value) el("export-name").value = EXPORT_DEFAULTS[exportKind];
}

// Mirror the current format onto the segmented control, the swapping hint, and
// the conditional Mask row — without touching the file name.
function reflectExportKind() {
  el("export-kind")
    .querySelectorAll(".seg-btn")
    .forEach((b) => b.classList.toggle("active", b.dataset.fmt === exportKind));
  el("export-mask-row").classList.toggle("show", exportKind === "report");
  el("export-hint").innerHTML = EXPORT_HINTS[exportKind];
}

// Switch format (from the segmented control): reflect it and reset the file name
// to the new kind's default.
function setExportKind(kind) {
  exportKind = kind;
  reflectExportKind();
  el("export-name").value = EXPORT_DEFAULTS[kind];
}

async function runExport() {
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast("Select the tracks to export first", true);
    return;
  }
  const kind = exportKind;
  // Named `outName` so it doesn't shadow the `fileName()` helper used below.
  const outName = el("export-name").value.trim();
  try {
    let written;
    if (kind === "playlist") {
      written = await invoke("export_playlist", { paths, fileName: outName });
    } else if (kind === "cue") {
      written = await invoke("export_cue", { paths, fileName: outName });
    } else if (kind === "csv") {
      written = await invoke("export_csv", { paths, fileName: outName });
    } else if (kind === "html") {
      written = await invoke("export_html", { paths, fileName: outName });
    } else if (kind === "xml") {
      written = await invoke("export_xml", { paths, fileName: outName });
    } else {
      written = await invoke("export_report", {
        paths,
        mask: el("export-mask").value,
        fileName: outName,
      });
    }
    toast(`Exported ${plural(paths.length, "track", "tracks")} to ${fileName(written)}`);
  } catch (e) {
    toast(String(e), true);
  }
}

el("export-run").addEventListener("click", runExport);

export { refreshExporter, setExportKind };
