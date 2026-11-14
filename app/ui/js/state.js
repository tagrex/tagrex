// The state every part of the frontend shares (#143 split it out of app.js).
//
// Most of the UI's state belongs to exactly one feature and stays private to
// its module; what lives here is the handful of things several modules read:
// the open library, what is selected, the staged plan, and how the table is
// currently shown.
//
// Reassignable values are exported as live bindings plus a setter. An imported
// binding can be read from anywhere and always shows the current value, but it
// cannot be assigned to from another module — hence `setTracks(...)` rather
// than `tracks = ...`. Values that are only ever mutated in place (the
// selection set, the edit buffer) need no setter and are plain exports.

// The open library's tracks, in mapping order (never reordered by grouping),
// with a path index kept beside them (#184). Nothing should scan the array to
// find one file: on a library of a few thousand, the tag-field grid did exactly
// that once per selected path and again per field, and a single row click cost a
// quarter of a second.
export let tracks = [];
export let contentRevision = 0;
export function invalidateTrackContent() { contentRevision++; }
let trackIndex = new Map();
let trackOrder = new Map();
export function setTracks(value) {
  tracks = value;
  invalidateTrackContent();
  reindexTracks();
}

// Rebuild the index after the list is reordered in place (the drag reorder does
// that) — the entries are the same, but their order is what the table shows.
export function reindexTracks() {
  trackIndex = new Map();
  trackOrder = new Map();
  tracks.forEach((track, index) => {
    trackIndex.set(track.path, track);
    trackOrder.set(track.path, index);
  });
}

// The staged ChangePlan, and what produced it — apply() uses the source to
// decide whether to clear pending tag edits ("edits") or remap them across a
// rename ("rename").
export let previewPlan = null;
export function setPreviewPlan(value) {
  previewPlan = value;
}
// The release cover an import wants written alongside its tags (#207), waiting
// for the staging call that turns the pending edits into a plan. One plan means
// one Apply and one undo for what the user did as a single action; leaving the
// cover to its own batch is what made undoing an import bring back the tags and
// keep the artwork.
export let pendingImportCover = null;
export function setPendingImportCover(value) {
  pendingImportCover = value;
}

export let previewSource = null;
export function setPreviewSource(value) {
  previewSource = value;
}

// While a plan is staged the file table enters a DIFF-STATE in place (#117):
// `diffByPath` maps each changed path to its change so the row renderer can show
// the diff, and `applySelection` is the subset of those paths ticked to apply
// (the sel column's meaning switches from row-selection to apply-scope while
// diffing). Both are null/empty when not diffing.
export let diffByPath = null;
export function setDiffByPath(value) {
  diffByPath = value;
}
export let applySelection = new Set();
export function setApplySelection(value) {
  applySelection = value;
}

// path -> Map(field -> newValue): pending tag changes not yet applied. Both
// inline cell edits and Discogs import feed this one buffer, so they compose
// into a single preview/apply. A value of "" means "clear the field".
export const edits = new Map();

// The set of selected file paths — the single source of truth for what every
// mode operates on. Kept here (not in the DOM) so a re-render (sort, reorder,
// auto-match, staging edits) never silently wipes or widens the selection.
let selectionRevision = 0;
export function selectionVersion() { return selectionRevision; }
class SelectionSet extends Set {
  add(path) { if (!this.has(path)) selectionRevision++; return super.add(path); }
  delete(path) { const removed = super.delete(path); if (removed) selectionRevision++; return removed; }
  clear() { if (this.size) selectionRevision++; super.clear(); }
}
export const selection = new SelectionSet();
let selectedSnapshot = null;

// All async consumers bind both their source data and destination paths to this
// revision. A -> B -> A is still a new subject; a late response cannot revive it.
export function captureSelection() {
  return { paths: selectedPaths(), selectionRevision, contentRevision, library: tracks };
}
export function isSelectionCurrent(subject) {
  return !!subject && subject.selectionRevision === selectionRevision &&
    subject.contentRevision === contentRevision && subject.library === tracks;
}

// Selected paths in `tracks` (mapping) order — NOT the visual/DOM order. This
// keeps position-based mapping (rename masks, Discogs import) tied to the real
// file order even when the view is grouped (#20). Reads the `selection` set, so
// it survives re-renders.
export function selectedPaths() {
  if (selectedSnapshot?.revision === selectionRevision && selectedSnapshot.order === trackOrder) return selectedSnapshot.paths;
  const paths = orderedSelectedPaths();
  selectedSnapshot = { revision: selectionRevision, order: trackOrder, paths };
  return paths;
}
function orderedSelectedPaths() {
  if (selection.size === 0) return [];
  // Row clicks usually select a handful out of a large library. Resolve that
  // small set through the existing path index, retaining mapping order rather
  // than paying for a full-library scan in each selection-dependent panel.
  if (selection.size < tracks.length / 4) {
    return [...selection].filter((path) => trackIndex.has(path))
      .sort((a, b) => trackOrder.get(a) - trackOrder.get(b));
  }
  // A broad range/select-all is already O(library); a linear walk beats sorting.
  const paths = [];
  for (const track of tracks) if (selection.has(track.path)) paths.push(track.path);
  return paths;
}

// The file table's columns are user-configurable (#43): "file" (always first,
// structural) followed by any modeled tag field, in a persisted order.
// Selection and artwork precede that logical list in the DOM. Keep this shared
// offset for row/header spans, grid coordinates and virtual measurement rows.
export const TABLE_DATA_COLUMN_OFFSET = 2;
export const ARTWORK_COLUMN_WIDTH = 24;
// The columns a fresh library starts with (#173). A chosen set is persisted and
// wins over this, so changing it never rearranges anyone's table.
export const DEFAULT_COLUMNS = ["file", "artist", "title", "album", "year", "length"];
export let visibleColumns = DEFAULT_COLUMNS.slice();
export function setVisibleColumns(value) {
  visibleColumns = value;
}

// Per-column pixel widths, keyed by column key, persisted across sessions (#76:
// resizable columns). A missing key falls back to `defaultColumnWidth`. The
// table is `table-layout: fixed`, so a header-cell width governs its column.
export let columnWidths = {};
export function setColumnWidths(value) {
  columnWidths = value;
}

// Sort: which column and which direction (1 asc, -1 desc). A view concern, like
// grouping — it never reorders `tracks`.
export let sortKey = null;
export function setSortKey(value) {
  sortKey = value;
}
export let sortDir = 1;
export function setSortDir(value) {
  sortDir = value;
}

// Some files were put in an order by hand — a drag, or a match against a release
// (#198). Kept apart from the sort rather than expressed by dropping it: the
// sort is also what holds the FOLDERS in place, so clearing it moved every
// group in the library to release three files. The flag only says the sorted
// column no longer describes every row, which the header shows.
export let manualOrder = false;
export function setManualOrder(value) {
  manualOrder = value;
}

// The filter box (#44). `filterQuery` is the compiled form (re-derived by
// recompileFilter whenever the text or flags change) so the per-row test stays
// cheap; `filterError` flags a regex that failed to compile.
export let filterText = "";
export function setFilterText(value) {
  filterText = value;
}
export let filterRegex = false;
export function setFilterRegex(value) {
  filterRegex = value;
}
export let filterCase = false;
export function setFilterCase(value) {
  filterCase = value;
}
export let filterError = false;
export function setFilterError(value) {
  filterError = value;
}
export let filterQuery = { scope: null, needle: "", re: null };
export function setFilterQuery(value) {
  filterQuery = value;
}

// Grouping is purely a view concern (#20): "" | "folder" | "artist" | "album".
// It regroups rows visually but never reorders the `tracks` array, so the file
// order used by mapping (rename masks, Discogs import) is unaffected. Collapsed
// group keys persist across renders.
export let groupBy = "folder";
export function setGroupByValue(value) {
  groupBy = value;
}
export const collapsedGroups = new Set();

// The dropped directories of a file-set drag-and-drop (#127), or null for an
// ordinary library. When set, the table's "drop" grouping buckets each track
// under the dropped folder it came from, with loose files under "Files".
export let dropFolders = null;
export function setDropFolders(value) {
  dropFolders = value;
}

// The root of the currently open session (the opened/dropped folder, or a
// file-set's common ancestor). Folder-group headers show the path relative to
// it — starting with the root's own name — so nested folders read like a tree
// (#129), matching what a reference tagger shows.
export let sessionRoot = "";
export function setSessionRoot(value) {
  sessionRoot = value;
}

// The open mode tab, and the row keyboard navigation treats as current.
export let currentMode = "tagger";
export function setCurrentMode(value) {
  currentMode = value;
}
export let activeRowPath = null;
export function setActiveRowPath(value) {
  activeRowPath = value;
}

// The last full SettingsDto read from the backend. `save_settings` overwrites
// settings.json with whatever DTO it is given, so every partial writer spreads
// this snapshot rather than sending its own fields alone.
export let savedSettings = {};
export function setSavedSettings(value) {
  savedSettings = value;
}

// Saved action groups (#57) and the shipped preset library (#137).
export let actionGroups = [];
export function setActionGroups(value) {
  actionGroups = value;
}

// Saved mask presets (#360): one shared pool the user drops into any mask input.
export let maskPresets = [];
export function setMaskPresets(value) {
  maskPresets = value;
}
export let builtinGroups = [];
export function setBuiltinGroups(value) {
  builtinGroups = value;
}

// One tag value off a track, empty string when absent — the reader every render
// path uses, so a missing field is never `undefined` in the DOM.
export function tag(track, key) {
  return track.tags[key] || "";
}

// The open library indexed by path. Held rather than built per call (#184):
// every caller wanting one file used to pay for a whole `Map` — or, worse, a
// linear scan — and those calls sit inside loops over the selection.
export function trackByPath() {
  return trackIndex;
}

/// One file by path, or undefined.
export function trackAt(path) {
  return trackIndex.get(path);
}
