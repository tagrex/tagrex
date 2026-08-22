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
// "" writes one playlist for the whole selection (what it always did); "folder"
// and "album" write one apiece and turn the name field into a mask (#62).
let exportSplit = "";

// Default name mask per grouping, and the labels the name field takes on.
const SPLIT_DEFAULTS = {
  folder: "%foldername%.m3u",
  album: "%album%.m3u",
};

// Refresh the EXPORTER panel for the current selection (called on entering the
// mode). Reflects the current format; only fills the file name when it's empty,
// so a name the user typed survives a mode switch.
function refreshExporter() {
  const count = selectedPaths().length;
  el("export-count").textContent = count ? `— ${plural(count, "track", "tracks")}` : "";
  reflectExportKind();
  if (!el("export-name").value) el("export-name").value = exportName();
}

// The default for the current format and grouping: a file name, or a mask that
// names one file per group.
function exportName() {
  if (exportKind === "playlist" && exportSplit) return SPLIT_DEFAULTS[exportSplit];
  return EXPORT_DEFAULTS[exportKind];
}

// Mirror the current format onto the segmented control, the swapping hint, and
// the conditional Mask row — without touching the file name.
function reflectExportKind() {
  el("export-kind")
    .querySelectorAll(".seg-btn")
    .forEach((b) => b.classList.toggle("active", b.dataset.fmt === exportKind));
  el("export-mask-row").classList.toggle("show", exportKind === "report");
  el("export-split-row").classList.toggle("show", exportKind === "playlist");
  // Splitting turns the file name into a mask rendered per group, so the field
  // says which of the two it is rather than leaving the difference implicit.
  const splitting = exportKind === "playlist" && exportSplit;
  el("export-name-label").textContent = splitting ? "Name mask" : "File name";
  el("export-hint").innerHTML = splitting
    ? "One <b>.m3u</b> per " + exportSplit + ", named by the mask below."
    : EXPORT_HINTS[exportKind];
}

// Switch format (from the segmented control): reflect it and reset the file name
// to the new kind's default.
function setExportKind(kind) {
  exportKind = kind;
  reflectExportKind();
  el("export-name").value = exportName();
}

// Switch grouping (from the select): same deal, the name field follows.
function setExportSplit(split) {
  exportSplit = split;
  reflectExportKind();
  el("export-name").value = exportName();
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
    if (kind === "playlist" && exportSplit) {
      const files = await invoke("export_playlists", {
        paths,
        grouping: exportSplit,
        nameMask: outName,
      });
      toast(`Exported ${plural(files.length, "playlist", "playlists")}`);
      return;
    }
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
el("export-split").addEventListener("change", (e) => setExportSplit(e.target.value));

export { refreshExporter, setExportKind };
