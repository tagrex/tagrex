"use strict";

import { TAURI, invoke } from "./js/invoke.js";
import { el, toast, fileName, escapeHtml, ico, confirmDialog, placeFloating, plural } from "./js/dom.js";
import { enablePointerReorder } from "./js/reorder.js";
import { refreshExporter, setExportKind } from "./js/exporters.js";
import { hooks } from "./js/hooks.js";
import { vinylPositionOf } from "./js/vinyl.js";
import "./js/dragdrop.js";
import "./js/tablegestures.js";
import "./js/renamer.js";
import { previewTagsFromName, refreshNameProbe, scheduleNameProbe } from "./js/fromname.js";
import { refreshDeduplicator, runDuplicateScan } from "./js/dedup.js";
import {
  chooseCover,
  embedCoverFile,
  embedCoverFromPath,
  embedExternalCover,
  exportCover,
  onCoverChosen,
  previewClearTags,
  previewCoverRemove,
  refreshCoverWell,
} from "./js/cover.js";
import {
  allColumnKeys,
  columnLabel,
  columnWidth,
  customColumnOf,
  customColumnValue,
  invalidateCustomColumns,
  loadColumnWidths,
  loadColumns,
  loadCustomColumns,
  maybeAutofit,
  refreshCustomColumnCells,
  populateGroupMenu,
  renderColumnsMenu,
  renderTableHead,
  setGroupBy,
} from "./js/columns.js";
import { dropGroupKey, folderGroupLabel, groupKeyOf, groupLabel } from "./js/grouping.js";
import { loadSavedToken, refreshReleaseMatches, searchBusy, searchPerPage, stopLoading } from "./js/online.js";
import { closeTransformPopover, refreshGenerator } from "./js/generator.js";
import { initActionGroups, initBuiltinGroups } from "./js/chain.js";
import { EXTENDED_FIELDS, KNOWN_CUSTOM_LABELS, VIRTUAL_COLUMNS } from "./js/fields.js";
import { initPlaceholderReference } from "./js/placeholders.js";
import { isFieldLocked, loadFieldLocks, pushFieldLocks } from "./js/locks.js";
import {
  cellSuggestKey,
  endCellSuggest,
  openCellSuggest,
  refreshCellSuggest,
} from "./js/suggest.js";
import {
  currentFieldValue,
  refreshFieldEditor,
  openAddField,
  closeAddField,
  validateFieldValue,
  addCustomField,
  applyFieldEditor,
} from "./js/editor.js";

// Hand the table-side refreshers to the panels that were split out (#143); see
// js/hooks.js for why this seam exists and when it goes away.
Object.assign(hooks, {
  renderTracks,
  renderPreview,
  previewEdits,
  refreshCoverWell,
  openDrop,
  updateSortIndicators,
  mountMeasureRows,
  renderTableHead,
  navigablePaths: () => navPaths,
});
import { openSettings, cancelSettings, updateSettingsDot } from "./js/settings.js";
import {
  valueFont,
  applyValueFont,
  checkboxColEnabled,
  applyCheckboxCol,
  regexModeEnabled,
  caseSensitiveEnabled,
  saveFilterMode,
  tableFontPx,
  applyTableFont,
  tracklistFontPx,
  applyTracklistFont,
  badgeFont,
  applyBadgeFont,
  groupByPref,
  saveGroupBy,
} from "./js/prefs.js";
import {
  fmtTime,
  showPlayerBar,
  playTrack,
  isPlayingPath,
} from "./js/player.js";
import {
  tracks, setTracks, reindexTracks,
  previewPlan, setPreviewPlan,
  previewSource, setPreviewSource,
  pendingImportCover, setPendingImportCover,
  diffByPath, setDiffByPath,
  applySelection, setApplySelection,
  edits, selection, selectedPaths, tag, trackAt, trackByPath,
  DEFAULT_COLUMNS, visibleColumns, setVisibleColumns,
  columnWidths, setColumnWidths,
  sortKey, setSortKey, sortDir, setSortDir,
  manualOrder, setManualOrder,
  filterText, setFilterText,
  filterRegex, setFilterRegex,
  filterCase, setFilterCase,
  filterError, setFilterError,
  filterQuery, setFilterQuery,
  groupBy, setGroupByValue, collapsedGroups,
  dropFolders, setDropFolders,
  sessionRoot, setSessionRoot,
  currentMode, setCurrentMode,
  activeRowPath, setActiveRowPath,
  savedSettings, setSavedSettings,
  actionGroups, setActionGroups,
  builtinGroups, setBuiltinGroups,
} from "./js/state.js";

// ---- elements ----
const rootInput = el("root");
const tableWrap = el("files-view");
const tracksBody = el("tracks-body");
const tracksEmpty = el("tracks-empty");
const applyBtn = el("diff-apply");
const previewBtn = el("preview");
const previewEditsBtn = el("preview-edits");
const undoBtn = el("undo");
const selectAll = el("select-all");
const statusSel = el("status-sel");

// ---- helpers ----

// ---- rendering ----
// Renders the table, overlaying any pending edits on top of the on-disk
// values (edited cells shown and marked dirty). Does NOT clear `edits` — call
// `resetEdits()` for that when loading fresh disk state.
// The text of a read-only derived column — the vinyl position (#106) or the
// playing time (#172) — or null when the column is an ordinary tag. One place,
// so the row renderer, the diff renderer, sorting and filtering cannot drift on
// what a derived cell says.
function virtualCellText(track, key, pending) {
  if (key === "position") return vinylPositionOf(track, pending);
  if (key === "length") return track.duration_secs == null ? "" : fmtTime(track.duration_secs);
  if (key === "tagtypes") return tagBlockSummary(track);
  return null;
}

// The Tag types cell (#47): every block the file carries, the one being read
// first. That order is the point — the rest are the file's other answers, and
// seeing them is how "the edit didn't take" becomes "you are reading the other
// block".
function tagBlockSummary(track) {
  const blocks = track.tag_blocks || [];
  if (!blocks.length) return "";
  const read = blocks.filter((b) => b.read_from).map((b) => b.label);
  const rest = blocks.filter((b) => !b.read_from).map((b) => b.label);
  return [...read, ...rest].join(" + ");
}

function sortValue(track, key) {
  // Length sorts on the seconds, not on "9:59" vs "10:01" as text.
  if (key === "length") return String(track.duration_secs ?? -1).padStart(9, "0");
  // Position sorts by (disc, track) numerically, so A1, A2, … B1 order holds
  // (and plain CD tracks sort numerically too) instead of by the display string.
  if (key === "position") {
    const disc = parseInt(track.tags.disc || "0", 10) || 0;
    const trk = parseInt(track.tags.track || "0", 10) || 0;
    return String(disc).padStart(4, "0") + String(trk).padStart(5, "0");
  }
  // Files carrying a second block sort together, ahead of the plain ones —
  // which is the way you would go looking for them (#47).
  if (key === "tagtypes") {
    const blocks = track.tag_blocks || [];
    return String(9 - Math.min(blocks.length, 9)) + tagBlockSummary(track).toLowerCase();
  }
  // A mask column sorts on what it renders, which is what the eye compares.
  if (customColumnOf(key)) return customColumnValue(key, track.path).toLowerCase();
  return (key === "file" ? fileName(track.path) : track.tags[key] || "").toLowerCase();
}

// The display value of one column for a track — the same source the columns and
// sort use, so a field-scoped filter (`position:B1`) sees what the eye sees.
function fieldValue(track, key) {
  if (key === "file") return fileName(track.path);
  const derived = virtualCellText(track, key, edits.get(track.path));
  if (derived !== null) return derived;
  if (customColumnOf(key)) return customColumnValue(key, track.path);
  return track.tags[key] || "";
}

// Re-derive the parsed filter from the raw text + mode flags (#44). Runs on any
// change to either, so `matchesFilter` stays a cheap per-row test. A leading
// `field:` scopes the query to one column when the prefix names a known one;
// otherwise the colon is treated as part of the query. In regex mode the pattern
// is compiled once here, inside a try/catch, so an invalid pattern is flagged
// (`filterError`) and can never throw from the render loop.
function recompileFilter() {
  setFilterError(false);
  let raw = filterText;
  let scope = null;
  const m = raw.match(/^([A-Za-z0-9_]+):(.*)$/);
  if (m) {
    const key = m[1].toLowerCase();
    if (allColumnKeys().includes(key)) {
      scope = key;
      raw = m[2];
    }
  }
  let re = null;
  if (filterRegex && raw) {
    try {
      re = new RegExp(raw, filterCase ? "" : "i");
    } catch (e) {
      setFilterError(true);
    }
  }
  setFilterQuery({ scope, needle: raw, re });
}

function matchesFilter(track) {
  const { scope, needle, re } = filterQuery;
  if (!needle) return true;
  // A broken regex filters nothing (the box shows the error) so the UI never
  // hangs or empties out mid-keystroke.
  if (filterRegex && !re) return true;
  // Scope to one column, or search the file name plus every tag value so a
  // column the user added (#43) is filterable too.
  const hay = scope
    ? fieldValue(track, scope)
    : [fileName(track.path), ...Object.values(track.tags)].join(" ");
  if (re) return re.test(hay);
  return filterCase ? hay.includes(needle) : hay.toLowerCase().includes(needle.toLowerCase());
}

function updateSortIndicators() {
  document.querySelectorAll("th.sortable").forEach((th) => {
    const ind = th.querySelector(".sort-ind");
    const sorted = th.dataset.sort === sortKey;
    ind.innerHTML = sorted ? ico(sortDir > 0 ? "tri-up" : "tri-down") : "";
    // A hand-made order (a drag, a match) leaves the sort in force for
    // everything it didn't touch (#198). The indicator fades rather than
    // vanishing, so the column still says what orders the list while admitting
    // that some rows are where they were put.
    ind.classList.toggle("manual", sorted && manualOrder);
    ind.title = sorted && manualOrder ? "Some files were reordered by hand" : "";
  });
}

// ---- filter mode + saved presets (#44) ----
// Reflect the current mode flags onto the two in-box toggles and the input's
// error state, and re-derive the parsed query. Call after any change to the
// filter text or flags, before renderTracks.
function syncFilterControls() {
  recompileFilter();
  const reBtn = el("filter-regex");
  const caseBtn = el("filter-case");
  reBtn.classList.toggle("on", filterRegex);
  reBtn.setAttribute("aria-pressed", String(filterRegex));
  caseBtn.classList.toggle("on", filterCase);
  caseBtn.setAttribute("aria-pressed", String(filterCase));
  const box = el("filter");
  box.classList.toggle("filter-bad", filterError);
  box.title = filterError ? "Invalid regular expression" : "";
}

// A named preset captures the full view query: filter text + mode flags, the
// sort column/direction, and the grouping — everything the user tuned by hand.
function loadPresets() {
  try {
    const saved = JSON.parse(localStorage.getItem(PRESETS_STORAGE_KEY));
    return Array.isArray(saved) ? saved.filter((p) => p && typeof p.name === "string") : [];
  } catch (e) {
    return [];
  }
}

function savePresets(list) {
  try {
    localStorage.setItem(PRESETS_STORAGE_KEY, JSON.stringify(list));
  } catch (e) {
    /* localStorage unavailable — presets just won't persist */
  }
}

// Store the current view under `name` (replacing a same-named one).
function saveCurrentPreset(name) {
  name = name.trim();
  if (!name) return;
  const list = loadPresets().filter((p) => p.name !== name);
  list.push({
    name,
    filter: filterText,
    regex: filterRegex,
    caseSensitive: filterCase,
    sortKey,
    sortDir,
    group: groupBy,
  });
  list.sort((a, b) => a.name.localeCompare(b.name));
  savePresets(list);
  renderPresetsMenu();
}

// Re-apply a saved preset: restore the filter text/flags, sort, and grouping,
// then reflect them onto every control and repaint.
function applyPreset(p) {
  setFilterText(p.filter || "");
  setFilterRegex(!!p.regex);
  setFilterCase(!!p.caseSensitive);
  saveFilterMode();
  el("filter").value = filterText;
  syncFilterControls();

  setGroupBy(p.group || "", { rerender: false });

  if (p.sortKey) applySort(p.sortKey, p.sortDir === -1 ? -1 : 1);
  else renderTracks();
}

function renderPresetsMenu() {
  const menu = el("presets-menu");
  menu.innerHTML = "";
  const list = loadPresets();
  if (!list.length) {
    const empty = document.createElement("div");
    empty.className = "col-menu-sep";
    empty.textContent = "No saved presets";
    menu.appendChild(empty);
  }
  for (const p of list) {
    const row = document.createElement("div");
    row.className = "col-menu-row preset-row";

    const apply = document.createElement("button");
    apply.type = "button";
    apply.className = "text-btn preset-apply";
    apply.textContent = p.name;
    apply.title = presetSummary(p);
    apply.addEventListener("click", () => {
      applyPreset(p);
      menu.hidden = true;
    });

    const del = document.createElement("button");
    del.type = "button";
    del.className = "preset-del";
    del.innerHTML = ico("close");
    del.title = `Delete “${p.name}”`;
    del.addEventListener("click", (e) => {
      e.stopPropagation();
      savePresets(loadPresets().filter((q) => q.name !== p.name));
      renderPresetsMenu();
    });

    row.append(apply, del);
    menu.appendChild(row);
  }

  // Save-current footer: name the current filter + sort and store it.
  const foot = document.createElement("div");
  foot.className = "col-menu-foot preset-save";
  const input = document.createElement("input");
  input.type = "text";
  input.placeholder = "Save current as…";
  input.spellcheck = false;
  input.className = "preset-name";
  const save = document.createElement("button");
  save.type = "button";
  save.className = "text-btn";
  save.textContent = "Save";
  const commit = () => {
    if (input.value.trim()) {
      saveCurrentPreset(input.value);
      input.value = "";
    }
  };
  save.addEventListener("click", commit);
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      commit();
    }
  });
  foot.append(input, save);
  menu.appendChild(foot);
}

// A human-readable one-liner for a preset's tooltip.
function presetSummary(p) {
  const bits = [];
  if (p.filter) bits.push(`filter “${p.filter}”${p.regex ? " (regex)" : ""}${p.caseSensitive ? " (Aa)" : ""}`);
  if (p.sortKey) bits.push(`sort ${columnLabel(p.sortKey)} ${p.sortDir === -1 ? "↓" : "↑"}`);
  if (p.group) bits.push(`group by ${p.group}`);
  return bits.length ? bits.join(" · ") : "empty view";
}

// Build one track row. `groupKey` (when grouping) tags the row so a click on it
// can tell which folder it belongs to. Returned rather than appended: the window
// renderer (#189) decides which rows exist at all, and a single row is also
// rebuilt in place when only its own contents changed (#186).
function buildTrackRow(track, groupKey) {
  const pending = edits.get(track.path);
  const tr = document.createElement("tr");
  tr.dataset.path = track.path;
  if (groupKey !== null) tr.dataset.group = groupKey;
  // An unreadable file (tags failed to parse) is shown but inert: it can't be
  // selected, played, or edited, and every mode's preview already skips it. It's
  // listed only so it never looks like the file vanished (#83).
  if (track.unreadable) {
    tr.classList.add("unreadable");
    tr.innerHTML = `
      <td class="sel"><input type="checkbox" disabled title="This file's tags couldn't be read" /></td>
      <td class="file" title="${escapeHtml(track.path)} — tags couldn't be read">${escapeHtml(fileName(track.path))}</td>
      <td class="unreadable-note" colspan="${visibleColumns.length - 1}">couldn't read tags — file left untouched</td>`;
    return tr;
  }
  // Diff-state (#117): a staged plan renders its change into this same table in
  // place — no separate Preview view. Staged rows show the new values (dirty)
  // and the sel column becomes the per-row apply scope; other rows recede.
  if (diffByPath) {
    fillDiffRow(tr, track);
    return tr;
  }
  if (isPlayingPath(track.path)) tr.classList.add("playing");
  // Checkbox + row highlight both reflect the `selection` set (source of truth),
  // so re-rendering never changes what's selected.
  const isSel = selection.has(track.path);
  if (isSel) tr.classList.add("selected");
  tr.innerHTML = `
      <td class="sel"><input type="checkbox" ${isSel ? "checked" : ""} data-path="${escapeHtml(track.path)}" /></td>
      <td class="file" title="${escapeHtml(track.path)}">${escapeHtml(fileName(track.path))}</td>`;
  for (const field of visibleColumns) {
    if (field === "file") continue; // rendered above (structural, always first)
    // A mask column (#150): computed, so read-only like Position — and it shows
    // what is on disk, since the backend renders it from the file.
    const custom = customColumnOf(field);
    if (custom) {
      const td = document.createElement("td");
      td.className = `position-cell align-${custom.align}`;
      td.textContent = customColumnValue(field, track.path);
      tr.appendChild(td);
      continue;
    }
    // A derived column (#106 Position, #172 Length): read-only, so it isn't
    // editable and carries no dirty state.
    const derived = virtualCellText(track, field, pending);
    if (derived !== null) {
      const td = document.createElement("td");
      td.className = "position-cell";
      td.textContent = derived;
      tr.appendChild(td);
      continue;
    }
    const original = tag(track, field);
    const edited = pending && pending.has(field);
    const value = edited ? pending.get(field) : original;
    const td = document.createElement("td");
    // A locked column keeps its cell shape and its dirty/error states, and
    // simply refuses to be edited (#48). Still marked `editable` so the diff and
    // the column machinery treat it like any other tag column — what `locked`
    // takes away is the double-click.
    td.className = "editable" + (isFieldLocked(field) ? " locked" : "");
    // Not editable until double-clicked (single click selects the row).
    // The "double-click to edit" hint is a self-managed tooltip (see cellTip
    // below), not a native title — same-text neighbours made the OS bubble
    // linger over the wrong cell.
    td.contentEditable = "false";
    td.spellcheck = false;
    td.dataset.path = track.path;
    td.dataset.field = field;
    td.dataset.original = original;
    td.textContent = value;
    if (edited && value !== original) td.classList.add("dirty");
    tr.appendChild(td);
  }
  return tr;
}

// One row of the in-table diff (#117). A staged row shows the plan's new values
// (dirty/error cells) and its sel checkbox is the per-row apply tick tracked in
// `applySelection`; an untouched row shows current values, inert. Reuses
// diffCell()/diffDir() from the old preview-table renderer.
function fillDiffRow(tr, track) {
  const change = diffByPath.get(track.path);
  const pending = edits.get(track.path);
  if (!change) {
    // Untouched row: current values, no apply tick, no editing/selection.
    tr.classList.add("untouched");
    tr.innerHTML =
      `<td class="sel"><input type="checkbox" disabled title="Not part of this change" /></td>` +
      `<td class="file" title="${escapeHtml(track.path)}">${escapeHtml(fileName(track.path))}</td>`;
    for (const field of visibleColumns) {
      if (field === "file") continue;
      const td = document.createElement("td");
      const derived = virtualCellText(track, field, pending);
      if (derived !== null) {
        td.className = "position-cell";
        td.textContent = derived;
      } else {
        td.textContent = tag(track, field);
      }
      tr.appendChild(td);
    }
    return;
  }
  tr.classList.add("staged");
  const ticked = applySelection.has(track.path);
  tr.innerHTML =
    `<td class="sel"><input type="checkbox" class="apply-tick" ${ticked ? "checked" : ""} data-path="${escapeHtml(track.path)}" title="Include this file in Apply" /></td>` +
    diffFileCellHtml(change, track);
  for (const field of visibleColumns) {
    if (field === "file") continue;
    const derived = virtualCellText(track, field, pending);
    if (derived !== null) {
      const td = document.createElement("td");
      td.className = "position-cell";
      td.textContent = derived;
      tr.appendChild(td);
      continue;
    }
    // A changed visible field shows the new value (dirty/error); an unchanged one
    // shows the current value. Fields changed outside visibleColumns still apply
    // — they just don't get a cell here (the action-bar count covers them).
    const cell = diffCell(change, field, track);
    const td = document.createElement("td");
    td.className = cell.cls;
    if (cell.title) td.title = cell.title;
    td.textContent = cell.text;
    if (cell.old) {
      const old = document.createElement("span");
      old.className = "cell-old";
      old.textContent = cell.old;
      td.appendChild(old);
    }
    tr.appendChild(td);
  }
}

// The File cell for a staged row: on a rename/move it shows the new name, the
// new relative folder beneath it (on a reorganize, #37), and the struck old name
// revealed under "Show old values". A non-rename change keeps the plain name.
function diffFileCellHtml(change, track) {
  if (!change.rename_to) {
    return `<td class="file" title="${escapeHtml(track.path)}">${escapeHtml(fileName(track.path))}</td>`;
  }
  const moved = diffDir(change.rename_to) !== diffDir(change.path);
  const pathLine = moved
    ? `<span class="fpath">${ico("corner")}${escapeHtml(diffDir(change.rename_to))}/</span>`
    : "";
  // Sidecars travelling with this rename/move (#58): a count badge, each pair
  // named in the tooltip so the plan is honest about what else will move.
  const sc = change.sidecar_renames && change.sidecar_renames.length ? change.sidecar_renames : null;
  const scBadge = sc
    ? `<span class="fsidecars" title="${escapeHtml(
        sc.map(([from, to]) => `${fileName(from)}  →  ${fileName(to)}`).join("\n")
      )}">+${sc.length} sidecar${sc.length === 1 ? "" : "s"}</span>`
    : "";
  return (
    `<td class="file dirty" title="${escapeHtml(change.path)}  →  ${escapeHtml(change.rename_to)}">` +
    `<span class="fcell"><span class="fname">${escapeHtml(fileName(change.rename_to))}</span>${pathLine}${scBadge}` +
    `<span class="cell-old">${escapeHtml(fileName(change.path))}</span></span></td>`
  );
}

// A collapsible group header row spanning the table width.
function buildGroupHeader(key, count) {
  const collapsed = collapsedGroups.has(key);
  const tr = document.createElement("tr");
  tr.className = "group-head" + (collapsed ? " collapsed" : "");
  tr.dataset.group = key;
  tr.innerHTML = `<td class="group-cell" colspan="${2 + visibleColumns.length}">
      <span class="group-caret">${collapsed ? ico("chevron-right") : ico("caret-down")}</span>
      <span class="group-label">${escapeHtml(groupLabel(key))}</span>
      <span class="group-count muted">· ${count} ${count === 1 ? "file" : "files"}</span>
    </td>`;
  return tr;
}

// ---- the windowed table (#189) ----
// The table used to keep one DOM row per file, so a library of a few thousand
// paid for rows nobody could see: every render, every restyle, and every walk
// over the rows was priced against the library rather than against the window.
//
// What the table shows is now a MODEL — an ordered list of group headers and
// tracks — of which only the slice the viewport covers is built as rows, held in
// place by a spacer row above and below. Everything that used to read the rows
// (select-all, ranges, group selection, keyboard navigation, the group toggle)
// reads the model; everything that paints rows works on the rendered slice.
//
// The rule to keep: the DOM is a view of the model, never the source of truth
// for what the table contains. `selection` was already a set for exactly this
// reason (#20) — this extends the same discipline to order, grouping and counts.

// The rendered list: group headers plus the tracks of expanded groups, in order.
let viewItems = [];
// Selectable paths in visual order, INCLUDING files inside collapsed groups —
// collapsing a folder hides its rows, it doesn't take its files out of scope
// (select-all and Shift-ranges covered them when they were hidden rows too).
let viewPaths = [];
let viewPathIndex = new Map(); // path -> index in viewPaths
// The paths the keyboard walks: what is actually on screen, so ↑/↓ skip a
// collapsed folder rather than stepping through it invisibly.
let navPaths = [];
let navPathIndex = new Map();
let groupPaths = new Map(); // group key -> its paths, in order
let viewGroups = []; // group keys in order
let itemIndexByPath = new Map(); // path -> index in viewItems (rendered items only)
// Per-item height in px once measured; null until then, when the per-kind
// estimate stands in. Rows are uniform in practice, but a staged rename is two
// lines tall, so heights are learned rather than assumed.
let itemHeights = [];
let itemTops = []; // prefix sums, itemTops[i] = top of item i (length n + 1)
let rowHeightEstimate = 18;
let groupHeightEstimate = 22;
// The sticky header's height, re-read once per model rebuild: it sits inside the
// scroller, so the furthest the list can be scrolled counts it too.
let headHeight = 0;
// The rows currently in the DOM, by path — the replacement for querying the
// table, and the set every painter iterates.
let renderedRows = new Map();
let renderedFirst = 0;
let renderedLast = -1;
// Rows built above and below the viewport, so a small scroll shows real rows
// rather than a gap waiting to be filled.
const OVERSCAN_ROWS = 10;

function itemHeightAt(i) {
  const measured = itemHeights[i];
  if (measured != null) return measured;
  return viewItems[i].kind === "group" ? groupHeightEstimate : rowHeightEstimate;
}

function recomputeItemTops() {
  itemTops = new Array(viewItems.length + 1);
  let top = 0;
  for (let i = 0; i < viewItems.length; i++) {
    itemTops[i] = top;
    top += itemHeightAt(i);
  }
  itemTops[viewItems.length] = top;
}

// First item at or after `offset` px, by binary search over the prefix sums.
function itemIndexAt(offset) {
  let lo = 0;
  let hi = viewItems.length - 1;
  let found = viewItems.length - 1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (itemTops[mid + 1] > offset) {
      found = mid;
      hi = mid - 1;
    } else {
      lo = mid + 1;
    }
  }
  return Math.max(0, found);
}

// Rebuild the model from the track list, the filter, the grouping and the staged
// plan. No DOM work at all — the window render is what touches the table.
function buildViewModel() {
  // A staged change always stays visible so the whole plan can be reviewed and
  // scoped, even if the current filter would otherwise hide it (#117).
  const visible = tracks.filter((t) => (diffByPath && diffByPath.has(t.path)) || matchesFilter(t));
  viewItems = [];
  viewPaths = [];
  navPaths = [];
  groupPaths = new Map();
  viewGroups = [];
  const pushTrack = (track, groupKey, shown) => {
    if (shown) viewItems.push({ kind: "track", track, groupKey });
    if (track.unreadable) return; // inert: never selectable, never navigable
    viewPaths.push(track.path);
    if (shown) navPaths.push(track.path);
  };
  if (groupBy) {
    // Groups in first-appearance order over the (mapping-ordered) track list,
    // so grouping never reorders the underlying files.
    const order = [];
    const byKey = new Map();
    for (const track of visible) {
      const key = groupKeyOf(track);
      if (!byKey.has(key)) {
        byKey.set(key, []);
        order.push(key);
      }
      byKey.get(key).push(track);
    }
    // Group order (#196): with a column sort active, first appearance means "the
    // folder whose first file sorts first", which reads as random. So the
    // folders take their own order, in the sort's direction. With nothing
    // sorted, first appearance is the scan order — what opening a folder implies.
    if (sortKey) {
      order.sort(
        (a, b) => groupLabel(a).localeCompare(groupLabel(b), undefined, { numeric: true }) * sortDir,
      );
    }
    for (const key of order) {
      viewGroups.push(key);
      viewItems.push({ kind: "group", key, count: byKey.get(key).length });
      const shown = !collapsedGroups.has(key);
      const paths = [];
      for (const track of byKey.get(key)) {
        pushTrack(track, key, shown);
        if (!track.unreadable) paths.push(track.path);
      }
      groupPaths.set(key, paths);
    }
  } else {
    for (const track of visible) pushTrack(track, null, true);
  }
  viewPathIndex = new Map(viewPaths.map((p, i) => [p, i]));
  navPathIndex = new Map(navPaths.map((p, i) => [p, i]));
  itemIndexByPath = new Map();
  viewItems.forEach((item, i) => {
    if (item.kind === "track") itemIndexByPath.set(item.track.path, i);
  });
  itemHeights = new Array(viewItems.length).fill(null);
  recomputeItemTops();
}

// A spacer stands in for the rows outside the window, so the table is as tall as
// the whole list and the scrollbar means what it says.
function spacerRow(height) {
  const tr = document.createElement("tr");
  tr.className = "spacer";
  tr.setAttribute("aria-hidden", "true");
  const td = document.createElement("td");
  td.colSpan = 2 + visibleColumns.length;
  td.style.height = `${Math.max(0, Math.round(height))}px`;
  tr.appendChild(td);
  return tr;
}

function setSpacerHeights() {
  const top = tracksBody.firstElementChild;
  const bottom = tracksBody.lastElementChild;
  if (top && top.classList.contains("spacer")) {
    top.firstElementChild.style.height = `${Math.max(0, Math.round(itemTops[renderedFirst] || 0))}px`;
  }
  if (bottom && bottom.classList.contains("spacer")) {
    const below = itemTops[viewItems.length] - itemTops[Math.min(renderedLast + 1, viewItems.length)];
    bottom.firstElementChild.style.height = `${Math.max(0, Math.round(below))}px`;
  }
}

// Learn the real heights of the rows just built. A row that turns out taller
// than its estimate (a staged rename is two lines) moves everything below it, so
// the scroll offset is corrected by however much the rows ABOVE the window
// changed — the first rendered row then stays exactly where the eye left it.
function measureRenderedRows() {
  // Reading a height forces the browser to lay the table out, so ask only when
  // the window holds a row whose height is still a guess. Scrolling back over
  // ground already covered costs nothing.
  let unmeasured = false;
  for (let i = renderedFirst; i <= renderedLast; i++) {
    if (itemHeights[i] == null) {
      unmeasured = true;
      break;
    }
  }
  if (!unmeasured) return;
  let changed = false;
  let index = renderedFirst;
  for (const tr of tracksBody.children) {
    if (tr.classList.contains("spacer")) continue;
    const height = tr.offsetHeight;
    if (!height) continue;
    const item = viewItems[index];
    if (item) {
      if (item.kind === "group") groupHeightEstimate = height;
      else rowHeightEstimate = height;
      if (itemHeights[index] !== height) {
        itemHeights[index] = height;
        changed = true;
      }
    }
    index += 1;
  }
  if (!changed) return;
  const before = itemTops[renderedFirst];
  recomputeItemTops();
  const shift = itemTops[renderedFirst] - before;
  if (shift) tableWrap.scrollTop += shift;
  setSpacerHeights();
}

// One row of the model, ready to insert.
function buildItemRow(i) {
  const item = viewItems[i];
  if (item.kind === "group") return buildGroupHeader(item.key, item.count);
  const tr = buildTrackRow(item.track, item.groupKey);
  tr.tabIndex = item.track.path === activeRowPath ? 0 : -1;
  renderedRows.set(item.track.path, tr);
  return tr;
}

function dropRow(tr) {
  if (tr.dataset.path) renderedRows.delete(tr.dataset.path);
  tr.remove();
}

// Build the rows the viewport covers (plus the overscan). Cheap enough to run on
// every scroll event: it is bounded by the window, not by the library — and a
// scroll that moves the window by a row or two only builds those rows, because
// an overlapping window is trimmed and extended rather than rebuilt.
function renderWindow({ force = false } = {}) {
  // Before the table is shown for the first time it has no height yet; a nominal
  // one keeps the first render from being a single row.
  const viewportHeight = tableWrap.clientHeight || 600;
  // A rebuilt model can leave the scroller past the end of what is now a much
  // shorter list — a filter narrowing to a handful, a collapse-all. The browser
  // will clamp the offset when it next lays the table out, so clamp it here
  // first: cutting the window for a position that is about to be corrected
  // leaves the rows built somewhere the eye isn't.
  const maxTop = Math.max(0, headHeight + itemTops[viewItems.length] - viewportHeight);
  if (tableWrap.scrollTop > maxTop) tableWrap.scrollTop = maxTop;
  const viewportTop = tableWrap.scrollTop;
  const first = Math.max(0, itemIndexAt(viewportTop) - OVERSCAN_ROWS);
  const last = Math.min(viewItems.length - 1, itemIndexAt(viewportTop + viewportHeight) + OVERSCAN_ROWS);
  if (!force && first === renderedFirst && last === renderedLast) return;
  const rowCount = renderedLast - renderedFirst + 1;
  const intact = rowCount > 0 && tracksBody.children.length === rowCount + 2;
  const overlaps = intact && first <= renderedLast && last >= renderedFirst;
  if (!force && overlaps) {
    const topSpacer = tracksBody.firstElementChild;
    const bottomSpacer = tracksBody.lastElementChild;
    while (renderedFirst < first) {
      dropRow(topSpacer.nextElementSibling);
      renderedFirst += 1;
    }
    while (renderedLast > last) {
      dropRow(bottomSpacer.previousElementSibling);
      renderedLast -= 1;
    }
    while (renderedFirst > first) {
      renderedFirst -= 1;
      topSpacer.after(buildItemRow(renderedFirst));
    }
    while (renderedLast < last) {
      renderedLast += 1;
      bottomSpacer.before(buildItemRow(renderedLast));
    }
    setSpacerHeights();
    measureRenderedRows();
    return;
  }
  renderedFirst = first;
  renderedLast = last;
  renderedRows = new Map();
  const frag = document.createDocumentFragment();
  frag.appendChild(spacerRow(itemTops[first] || 0));
  for (let i = first; i <= last; i++) frag.appendChild(buildItemRow(i));
  frag.appendChild(spacerRow(itemTops[viewItems.length] - itemTops[Math.min(last + 1, viewItems.length)]));
  tracksBody.replaceChildren(frag);
  // The rows were built from the current selection, so that is what the DOM
  // shows (#184: the painter only touches rows whose state moved).
  paintedSelection = new Set(selection);
  measureRenderedRows();
}

// The row for a path, when it is on screen. Off-window paths have no row — that
// is the point — so every caller has to tolerate `undefined`.
function renderedRow(path) {
  return renderedRows.get(path);
}

// Fit-to-content (#151) against a windowed table. Measuring what is rendered
// would fit a column to whatever happens to be on screen, and the widest value
// is usually elsewhere — so the measurer borrows a few real rows per column,
// the longest values in the list, and the browser lays those out for real.
// Collapsed folders are excluded for the same reason they were when their rows
// were `display: none`: a hidden value never set a column's width.
const MEASURE_CANDIDATES = 3;

function mountMeasureRows(keys) {
  const wanted = new Map();
  for (const key of keys) {
    // Longest first, by character count — a stand-in for width good enough to
    // pick the handful of candidates the browser then measures properly.
    const best = [];
    for (const item of viewItems) {
      if (item.kind !== "track") continue;
      const length = fieldValue(item.track, key).length;
      if (best.length < MEASURE_CANDIDATES) {
        best.push({ length, item });
        best.sort((a, b) => a.length - b.length);
      } else if (length > best[0].length) {
        best[0] = { length, item };
        best.sort((a, b) => a.length - b.length);
      }
    }
    for (const { item } of best) wanted.set(item.track.path, item);
  }
  const borrowed = [];
  for (const [path, item] of wanted) {
    if (renderedRows.has(path)) continue; // already in the table, already measured
    const tr = buildTrackRow(item.track, item.groupKey);
    tracksBody.appendChild(tr);
    borrowed.push(tr);
  }
  return () => borrowed.forEach((tr) => tr.remove());
}

tableWrap.addEventListener("scroll", () => renderWindow(), { passive: true });
// The window is a function of the viewport, so anything that resizes it — the
// OS window, the mode-panel splitter, showing the table for the first time —
// has to re-cut it.
new ResizeObserver(() => renderWindow({ force: true })).observe(tableWrap);

function renderTracks() {
  updateSortIndicators();
  tracksEmpty.hidden = tracks.length > 0;
  headHeight = el("tracks").tHead?.offsetHeight || headHeight;
  buildViewModel();
  renderWindow({ force: true });

  // The group toggle only makes sense while grouped (#32).
  el("toggle-groups").hidden = !groupBy;
  syncGroupToggle();

  previewBtn.disabled = tracks.length === 0;
  updateEditsButton();
  // While diffing the sel column is the apply scope, not the selection, so the
  // selection UI must not repaint the checkboxes — refresh the action bar instead.
  if (diffByPath) updateDiffBar();
  else syncSelectionUI();
  refreshRoving();
  // Sticky autofit (#151): re-fit only when what the cells show has actually
  // changed — this runs on every paint, including a plain row click.
  maybeAutofit();
}

// Selection count in the status bar ("N/M selected"). Uses the checked-row
// count directly; total size/duration are deferred (#27 notes → their own issue).
// The status bar is the single home for the table's counts (#126): the total
// file count (or `visible/total` when a filter is active, replacing the old
// `Files (N)` toolbar label) plus the selection count.
function updateStatus() {
  const total = tracks.length;
  if (!total) {
    statusSel.textContent = "";
    return;
  }
  const selected = selectedPaths().length;
  const noun = total === 1 ? "file" : "files";
  const files = filterText
    ? `${tracks.filter(matchesFilter).length}/${total} ${noun}`
    : `${total} ${noun}`;
  statusSel.textContent = `${files} · ${selected} selected`;
}

function resetEdits() {
  edits.clear();
  updateEditsButton();
}

// Move pending edits from old paths to new paths after a rename is applied, so
// tag edits survive a rename instead of being orphaned by the path change.
function remapEditsAfterRename(plan) {
  for (const change of plan.changes) {
    if (change.rename_to && edits.has(change.path)) {
      edits.set(change.rename_to, edits.get(change.path));
      edits.delete(change.path);
    }
  }
}

function onCellEdit(td) {
  const { path, field, original } = td.dataset;
  const value = td.textContent.trim();
  const unstage = () => {
    if (edits.has(path)) {
      edits.get(path).delete(field);
      if (edits.get(path).size === 0) edits.delete(path);
    }
  };
  // Typed fields (year / track / disc / bpm) are validated with the same rule
  // the EDITOR form and the backend use. An invalid value lights up the cell's
  // error state and is never staged, so an apply can't try to write it (#76).
  if (!validateFieldValue(field, value).ok) {
    td.classList.add("error");
    td.classList.remove("dirty");
    unstage();
    updateEditsButton();
    return;
  }
  td.classList.remove("error");
  if (value === original) {
    td.classList.remove("dirty");
    unstage();
  } else {
    td.classList.add("dirty");
    if (!edits.has(path)) edits.set(path, new Map());
    edits.get(path).set(field, value);
  }
  updateEditsButton();
}

function updateEditsButton() {
  previewEditsBtn.disabled = edits.size === 0;
}

// Reveal the file table. The Preview view is gone (#117): a staged plan now
// renders into this same table via the diff-state, and the duplicate scan is the
// DEDUPLICATOR mode driven from setMode (#118). With no sibling views left, the
// old `Files` tab is now a plain label (#121); this just toggles the file table.
function showView(which) {
  el("files-view").hidden = which !== "files";
}

// ---- duplicate finder (#40): a read-only library scan, grouped ----
function discardPreview() {
  // A preview built from the pending-edits buffer (inline edits + Discogs
  // import) owns that buffer, so discarding it must also drop those staged
  // values; other previews just drop the plan. exitDiffState() repaints the
  // table back to its normal (non-diff) state either way.
  const wasEdits = previewSource === "edits";
  if (wasEdits) resetEdits();
  exitDiffState();
}

// Path -> track lookup, so the diff can show the current value of a file's
// unchanged main/extra columns. Rebuilt per render from the live `tracks`.

// Stage `previewPlan` into the file table's diff-state (#117). Named renderPreview
// still because every mutating mode funnels its plan here; there is no separate
// Preview view any more — the same #tracks table shows the change in place with a
// floating Apply/Discard bar. An empty plan just leaves the table untouched.
function renderPreview(plan) {
  if (!plan || plan.changes.length === 0) {
    // A plan a lock emptied would otherwise land as "nothing to change", which
    // is the one message it must not give: nothing changed BECAUSE of the lock,
    // and the difference is the whole point of having one (#48). Said here
    // rather than in each caller — every mode's plan comes through this.
    const skips = (plan && plan.locked_skipped) || [];
    if (skips.length) {
      const parts = skips.map((s) => `${diffLabel(s.field)} on ${plural(s.files, "file", "files")}`);
      toast(`Nothing left to change — locked: ${parts.join(", ")}`);
    }
    exitDiffState();
    return;
  }
  enterDiffState();
}

// Entering and leaving the diff-state is a re-render of the WINDOW (#189), not
// of the library: the staged rows the viewport holds are rebuilt, everything
// else recedes by a rule hanging off the body-level `diffing` class, and a
// staged file the filter would otherwise hide is in the model because the model
// builder keeps it (#117). This is what #186 was reaching for by patching rows
// one at a time; with the table windowed, the plain renderer is already that
// cheap and the patching layer went away with it.

// Enter the in-table diff-state: build the path->change map, tick every changed
// file for apply by default, show the change in the table, and float the bar.
function enterDiffState() {
  setDiffByPath(new Map(previewPlan.changes.map((c) => [c.path, c])));
  setApplySelection(new Set(diffByPath.keys()));
  document.body.classList.add("diffing");
  el("diff-show-old").checked = false;
  el("tracks").classList.remove("show-old");
  showView("files"); // never diff over the dedup view
  renderTracks();
  el("ab-plan").textContent = previewPlan.description ? ` · ${previewPlan.description}` : "";
  renderLockedSkips();
  el("diff-actionbar").hidden = false;
  updateDiffBar();
  // The rule chain acts on the staged plan while one is staged (#142), and its
  // wording says so — so it has to hear about entering and leaving this state.
  refreshGenerator();
}

// What a lock kept out of the staged plan (#48).
//
// A change that silently did not happen is the one thing a lock must never be,
// so the bar names the fields and how many files each would have touched. It
// reads as a note rather than an error: nothing went wrong, the plan did
// exactly what the locks said it could.
function renderLockedSkips() {
  const note = el("ab-locked");
  const skips = (previewPlan && previewPlan.locked_skipped) || [];
  note.hidden = skips.length === 0;
  if (!skips.length) return;
  const parts = skips.map((s) => `${diffLabel(s.field)} on ${plural(s.files, "file", "files")}`);
  note.textContent = ` · locked: ${parts.join(", ")}`;
  note.title = `${parts.length === 1 ? "This field is" : "These fields are"} locked, so the plan left ${parts.length === 1 ? "it" : "them"} alone.`;
}

// Leave the diff-state: drop the plan + apply scope and put the plain rows back.
function exitDiffState() {
  setPreviewPlan(null);
  setPreviewSource(null);
  setDiffByPath(null);
  setApplySelection(new Set());
  document.body.classList.remove("diffing");
  el("tracks").classList.remove("show-old");
  el("diff-actionbar").hidden = true;
  renderTracks();
  refreshGenerator();
}

// Sync the floating action bar (apply count + enabled) and the header select-all
// tri-state to `applySelection`. Every changed file starts ticked; unticking
// some narrows what a single Apply writes (#81).
function updateDiffBar() {
  if (!diffByPath) return;
  const staged = diffByPath.size;
  const n = applySelection.size;
  el("ab-count").textContent = String(n);
  applyBtn.disabled = n === 0;
  selectAll.checked = n > 0 && n === staged;
  selectAll.indeterminate = n > 0 && n < staged;
}

/* ---- per-cell diff helpers (#80, reused by the in-table diff-state #117) ----
   The Preview view's mirror table is gone; the file table now shows the diff in
   place (fillDiffRow). These helpers still compute the state + text of one
   changed cell, so they are shared by that renderer. */

// Main columns (mirror the file table, minus play).
const DIFF_MAIN_COLS = ["file", "artist", "title", "album", "year"];
const DIFF_LABELS = {
  file: "File", artist: "Artist", title: "Title", album: "Album", year: "Year",
  albumartist: "Album Artist", track: "Track", tracktotal: "Track Total",
  disc: "Disc", media: "Media", genre: "Genre", composer: "Composer", publisher: "Publisher",
  catalognumber: "Catalogue #", bpm: "BPM", isrc: "ISRC", key: "Key", url: "URL", comment: "Comment",
};

function diffLabel(field) {
  return DIFF_LABELS[field] || (field.startsWith("custom:") ? field.slice(7) : field);
}

function diffDir(path) {
  const i = Math.max(String(path).lastIndexOf("/"), String(path).lastIndexOf("\\"));
  return i >= 0 ? path.slice(0, i) : "";
}

// The state + text of one field cell for one file: unchanged | dirty | error |
// cleared. `track` supplies the current value for an unchanged cell.
function diffCell(change, field, track) {
  const tc = (change.tag_changes || []).find((t) => t.field === field);
  const current = track ? (track.tags[field] || "") : "";
  if (!tc) return { text: current, cls: "unchanged" + (current ? "" : " empty") };
  const nv = tc.new == null ? "" : String(tc.new);
  const ov = tc.old == null ? "" : String(tc.old);
  if (tc.invalid) {
    return { text: nv, cls: "error", title: `Invalid ${diffLabel(field)}: “${nv}” — keeping “${ov || "∅"}”`, old: ov };
  }
  if (nv === "") return { text: "", cls: "dirty cleared", title: `Cleared (was “${ov}”)`, old: ov };
  return { text: nv, cls: "dirty", title: `was “${ov || "∅"}”`, old: ov };
}

async function refreshHistory() {
  try {
    const batches = await invoke("history", {});
    undoBtn.disabled = batches.length === 0;
    // Undo is icon-only now (#115) — reflect the batch count in the tooltip /
    // aria-label instead of overwriting the SVG with text.
    const label = batches.length
      ? `Undo the last applied batch (${batches.length} available)`
      : "Undo the last applied batch";
    undoBtn.title = label;
    undoBtn.setAttribute("aria-label", label);
  } catch (e) {
    /* history is best-effort in the toolbar */
  }
}



// A path as a file manager hands it over (#179). Finder's "Copy as Pathname"
// wraps anything containing a space in single quotes, a shell copy uses double
// ones, and a path dragged into a terminal comes back with its spaces escaped.
// Pasted verbatim, none of those name a folder — and what followed was an empty
// table rather than a word about why.
function cleanPastedPath(raw) {
  let path = (raw || "").trim();
  // A matching pair only: a quote on one side alone is part of the name.
  const quote = path[0];
  if ((quote === "'" || quote === '"') && path.endsWith(quote) && path.length > 1) {
    path = path.slice(1, -1);
  }
  return path.replace(/\\(.)/g, "$1").trim();
}


// ---- recently opened libraries (#180) ----
//
// A display preference like the column set, so localStorage rather than
// settings.json. Most recent first, no duplicates, and only real libraries: a
// drop of loose files opens a session rooted at their common ancestor, which is
// not a folder anyone chose to open.
const RECENT_ROOTS_KEY = "tagrex.recentRoots";
const RECENT_ROOTS_MAX = 8;

function recentRoots() {
  try {
    const saved = JSON.parse(localStorage.getItem(RECENT_ROOTS_KEY) || "[]");
    return Array.isArray(saved) ? saved.filter((p) => typeof p === "string" && p) : [];
  } catch (e) {
    return [];
  }
}

function writeRecentRoots(list) {
  try {
    localStorage.setItem(RECENT_ROOTS_KEY, JSON.stringify(list.slice(0, RECENT_ROOTS_MAX)));
  } catch (e) {
    /* storage unavailable — the list just won't persist */
  }
  el("root-recent").hidden = list.length === 0;
}

// Reopening a folder moves it up rather than adding a second row.
function rememberRoot(path) {
  if (!path) return;
  writeRecentRoots([path, ...recentRoots().filter((p) => p !== path)]);
}

// A folder that has since moved or been unmounted stops being offered.
function forgetRoot(path) {
  writeRecentRoots(recentRoots().filter((p) => p !== path));
}

function closeRecentMenu() {
  el("root-recent-menu").hidden = true;
}

function toggleRecentMenu() {
  const menu = el("root-recent-menu");
  if (!menu.hidden) {
    closeRecentMenu();
    return;
  }
  const list = recentRoots();
  menu.innerHTML = "";
  for (const path of list) {
    const parts = path.split(/[\\/]/).filter(Boolean);
    const name = parts.pop() || path;
    const row = document.createElement("button");
    row.type = "button";
    row.className = "col-menu-row tk-menu-item recent-row";
    // The tail is what tells two of these apart; the head is the tooltip's job.
    row.innerHTML =
      `<span class="path-parent">${escapeHtml(parts.length ? `${parts.length > 1 ? "…/" : "/"}${parts[parts.length - 1]}/` : "")}</span>` +
      `<span class="path-name">${escapeHtml(name)}</span>`;
    row.title = path;
    row.addEventListener("click", () => {
      closeRecentMenu();
      rootInput.value = path;
      openLibrary();
    });
    menu.appendChild(row);
  }
  menu.hidden = false;
  // `floating` is what neutralizes .col-menu's own absolute placement so the
  // inline offsets from placeFloating are the ones that count (#160).
  menu.classList.add("floating");
  placeFloating(menu, el("root-box"), { align: "left" });
}

// ---- the library indicator and its one button (#177) ----
//
// `openedRoot` is what is actually open; the input is what the user is saying.
// The button's role follows the difference between them, NOT focus: pasting a
// path and reaching for the button blurs the field, and a focus-driven toggle
// would turn Open back into Browse just in time for the click to open a folder
// dialog instead of the pasted path.
let openedRoot = "";

function libraryIsDirty() {
  const typed = rootInput.value.trim();
  return typed !== "" && typed !== openedRoot;
}

// Name the open folder: the folder itself in full, its parent path dimmed and
// elided from the left (the tail is the half that answers "which one?"), the
// whole path in the tooltip.
function renderRootDisplay() {
  const display = el("root-display");
  const parent = display.querySelector(".path-parent");
  const name = display.querySelector(".path-name");
  if (!openedRoot) {
    parent.textContent = "";
    name.textContent = "No folder open";
    display.classList.add("empty");
    display.title = "Choose a folder to open";
    return;
  }
  const parts = openedRoot.split(/[\\/]/).filter(Boolean);
  name.textContent = parts.pop() || openedRoot;
  // Only the folder it sits in, with a leading ellipsis for everything above:
  // "…/Temp music/" says where you are, "/Users/oleg…" says nothing, and that
  // is what a plain end-elided path gives you. The whole path is the tooltip.
  parent.textContent = parts.length
    ? `${parts.length > 1 ? "…/" : "/"}${parts[parts.length - 1]}/`
    : "";
  display.classList.remove("empty");
  display.title = openedRoot;
  fitRootDisplay();
}

// The parent segment is worth showing whole or not at all (#182): shaved down to
// `…/Temp m…` it is noise, and a second ellipsis in a line this short reads as
// damage. CSS can only shave, so the choice is made here — measure, and drop the
// segment when the line overflows, leaving the name the whole width.
function fitRootDisplay() {
  const display = el("root-display");
  const parent = display.querySelector(".path-parent");
  const name = display.querySelector(".path-name");
  if (!parent.textContent) return;
  parent.hidden = false;
  // Not `scrollWidth > clientWidth` on the button: the name clamps itself with
  // an ellipsis, so the content always "fits" and the overflow never shows up
  // there. What matters is the width the name WANTS — its own scrollWidth —
  // next to the parent it would have to share with.
  const style = getComputedStyle(display);
  const inner =
    display.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
  if (parent.offsetWidth + name.scrollWidth > inner) parent.hidden = true;
}

// Browse… normally, Open while the field says something else. Label, tooltip and
// accessible name move together, as the search/stop button's do (#111).
function updateLibAction() {
  const dirty = libraryIsDirty();
  const btn = el("lib-action");
  el("lib-action-label").textContent = dirty ? "Open" : "Browse…";
  btn.classList.toggle("opening", dirty);
  btn.title = dirty ? "Open the path you typed" : "Choose a folder to open";
  btn.setAttribute("aria-label", btn.title);
}

// Show the input over the indicator, with the current path selected so a paste
// replaces it outright.
function editRootPath() {
  // The BOX, not the button inside it (#181): the border and the recent-folders
  // caret belong to the indicator and have to leave with it, and the inner
  // button carries a `display` of its own that would outrank `hidden` anyway.
  closeRecentMenu();
  el("root-box").hidden = true;
  rootInput.hidden = false;
  rootInput.value = openedRoot;
  rootInput.focus();
  rootInput.select();
  updateLibAction();
}

// Back to the indicator. Only when the field agrees with what is open — while it
// says something else, it has to stay visible, or the Open button would be
// offering to open something the user can no longer see.
function leaveRootEdit() {
  if (libraryIsDirty()) return;
  rootInput.hidden = true;
  el("root-box").hidden = false;
  renderRootDisplay();
  updateLibAction();
}

// ---- actions ----
async function openLibrary() {
  const root = cleanPastedPath(rootInput.value);
  if (!root) {
    toast("Enter a library path first", true);
    return;
  }
  // Show what is actually about to be opened, not what was pasted.
  rootInput.value = root;
  try {
    await invoke("open_library", { root });
    setDropFolders(null); // a typed/browsed open is a plain library, not a drop
    rememberRoot(root);
    await afterOpen(root);
  } catch (e) {
    // A recent folder that no longer opens stops being offered (#180): a list
    // that keeps suggesting something unopenable is worse than a short one.
    if (recentRoots().includes(root)) {
      forgetRoot(root);
      toast(`${root} is no longer there — removed from recent folders`, true);
      return;
    }
    toast(String(e), true);
  }
}

// Shared tail of every "open a session" path (Open, Browse, or a drag-and-drop):
// pull the track list and reset all per-session view state. `label` is what the
// success toast names. When `dropFolders` is set (a file-set drop, #127) the
// table defaults to drop-origin grouping; otherwise the saved group pref.
async function afterOpen(label) {
  setSessionRoot(label); // the opened/dropped root — folder group labels hang off it
  // What is open is now what the field says, so the button falls back to
  // Browse… and the indicator takes over from the input (#177).
  openedRoot = label;
  rootInput.value = label;
  leaveRootEdit();
  // Opening builds a new backend session with nothing locked, and the locks
  // belong to the person rather than to the folder they happen to have open —
  // so they are handed to it again (#48).
  await pushFieldLocks();
  setTracks(await invoke("list_tracks", {}));
  // Only the first readable track is selected on open (#128), so an operation
  // never silently hits the whole library — the user picks what to work on
  // (a row, a range, a whole folder via its group header, or the select-all
  // box). The set (not the DOM) holds the selection. Unreadable placeholders
  // (#83) can't be operated on, so the first readable one is chosen.
  selection.clear();
  const firstReadable = tracks.find((t) => !t.unreadable);
  if (firstReadable) selection.add(firstReadable.path);
  // Opening a session drops any staged plan and leaves the diff-state.
  setPreviewPlan(null);
  setPreviewSource(null);
  setDiffByPath(null);
  setApplySelection(new Set());
  document.body.classList.remove("diffing");
  el("diff-actionbar").hidden = true;
  resetEdits();
  setSortKey(null);
  setSortDir(1);
  setManualOrder(false);
  setFilterText("");
  el("filter").value = "";
  syncFilterControls(); // clears the parsed query + any regex-error state
  setGroupBy(dropFolders ? "drop" : groupByPref(), { persist: false, rerender: false });
  renderTracks();
  // Mask columns (#150) are rendered by the backend, so they arrive a beat after
  // the rows do. The table paints immediately with the cells blank and repaints
  // once the values land — a visible column is never worth blocking the open on.
  refreshCustomColumnCells();
  showView("files");
  showPlayerBar();
  await refreshHistory();
  toast(`Opened ${label} — ${plural(tracks.length, "track", "tracks")}`);
}

// Open a drag-and-drop of `paths` (#127). The backend resolves a lone folder to
// a library and anything else to a file-set; `dropFolders` (the dropped dirs)
// drives the table's drop-origin grouping, and is null for library mode.
async function openDrop(paths) {
  if (!paths || !paths.length) return;
  try {
    const result = await invoke("open_drop", { paths });
    setDropFolders(result.mode === "files" ? result.folders || [] : null);
    rootInput.value = result.root;
    // A dropped FOLDER is a library and worth remembering (#180); a drop of
    // loose files is rooted at their common ancestor, which nobody chose.
    if (result.mode !== "files") rememberRoot(result.root);
    await afterOpen(result.root);
  } catch (e) {
    toast(String(e), true);
  }
}


async function previewEdits() {
  const list = [];
  for (const [path, fields] of edits) {
    for (const [field, value] of fields) {
      list.push({ path, field, value });
    }
  }
  if (list.length === 0) {
    toast("No pending edits to preview", true);
    return;
  }
  try {
    // The cover rides along exactly once: whether it is written at all is the
    // backend's decision (the import-cover setting, per file), but a second
    // staging must not silently re-offer it.
    const cover = pendingImportCover;
    setPendingImportCover(null);
    setPreviewPlan(await invoke("preview_tag_edits", { edits: list, cover }));
    setPreviewSource("edits");
    renderPreview(previewPlan);
  } catch (e) {
    toast(String(e), true);
  }
}

async function apply() {
  if (!previewPlan || previewPlan.changes.length === 0) return;
  const wasRename = previewSource === "rename";
  const wasEdits = previewSource === "edits";
  // Only the ticked rows are applied (#81). The apply scope is the sel-column
  // tick set (`applySelection`) while diffing. The plan the backend gets — and
  // undo journals — is exactly this subset.
  const appliedPlan = {
    ...previewPlan,
    changes: previewPlan.changes.filter((c) => applySelection.has(c.path)),
  };
  if (appliedPlan.changes.length === 0) {
    toast("Tick at least one row to apply", true);
    return;
  }
  const appliedPaths = new Set(appliedPlan.changes.map((c) => c.path));
  try {
    await invoke("apply_plan", { plan: appliedPlan });
    toast(`Applied changes to ${plural(appliedPlan.changes.length, "file", "files")}`);
    if (wasRename) {
      remapEditsAfterRename(appliedPlan); // keep pending tag edits, new paths
    } else if (wasEdits) {
      // Drop only the applied files' edits; unticked files keep their staged
      // edits so a follow-up apply can still write them.
      for (const path of appliedPaths) edits.delete(path);
      updateEditsButton();
    }
    // cover apply leaves the tag-edits buffer untouched (separate change kind)
    setTracks(await invoke("list_tracks", {}));
    sortTracks(); // the list came back in scan order (#196)
    // Mask columns render from disk, and an apply is exactly when what is on
    // disk changed under unchanged paths (#150).
    invalidateCustomColumns();
    // exitDiffState() drops the plan + apply scope and repaints the table.
    exitDiffState();
    refreshCustomColumnCells();
    await refreshHistory();
  } catch (e) {
    toast(String(e), true);
  }
}

async function undo() {
  try {
    const batches = await invoke("history", {});
    if (batches.length === 0) return;
    await invoke("undo", { batchId: batches[0].id });
    toast("Undid last batch");
    resetEdits();
    setTracks(await invoke("list_tracks", {}));
    sortTracks(); // the list came back in scan order (#196)
    invalidateCustomColumns();
    // exitDiffState() also clears previewPlan/previewSource and repaints.
    exitDiffState();
    refreshCustomColumnCells();
    await refreshHistory();
  } catch (e) {
    toast(String(e), true);
  }
}



// ---- mode tabs ----
// The active mode's panel is the only one shown; entering a mode refreshes its
// panel against the current selection. The table (subject) never changes — only
// the right-hand panel (verb) swaps.
const MODE_REFRESH = {
  renamer: () => {},
  tagger: refreshTagger,
  generator: refreshGenerator,
  exporter: refreshExporter,
  deduplicator: refreshDeduplicator,
};
// TAGGER is the primary tool, so it's the default tab (see index.html). Keep
// this in sync with the tab/panel marked active/visible there.

function setMode(name) {
  setCurrentMode(name);
  // The Transform popover borrows the GENERATOR panel's chain block (#149);
  // give it back before any mode is shown, or GENERATOR opens without it.
  closeTransformPopover();
  // In GENERATOR the chain is already on screen, and the button would pull it
  // out of the panel it is sitting in — so there is nothing for it to do there.
  el("transform-btn").hidden = name === "generator";
  document.querySelectorAll(".mode-tab").forEach((tab) => {
    tab.classList.toggle("active", tab.dataset.mode === name);
  });
  document.querySelectorAll(".mode-panel").forEach((panel) => {
    panel.hidden = panel.id !== `panel-${name}`;
  });
  // DEDUPLICATOR (#118) is the one mode that takes over the main area: its
  // read-only results replace the file table (and the Files/Preview strip), the
  // way its controls sit in the right panel like every other mode. Leaving it
  // restores the normal table view.
  const dedup = name === "deduplicator";
  document.body.classList.toggle("mode-deduplicator", dedup);
  if (dedup) {
    // The read-only scan owns the main area; a staged plan's diff-state stays
    // intact underneath — its floating bar just hides until a normal mode returns.
    el("files-view").hidden = true;
    el("duplicates-view").hidden = false;
    el("diff-actionbar").hidden = true;
  } else {
    el("duplicates-view").hidden = true;
    showView("files");
    if (diffByPath) el("diff-actionbar").hidden = false;
  }
  // Uncollapse when a tab is clicked, so switching modes always reveals the panel.
  document.body.classList.remove("panel-collapsed");
  (MODE_REFRESH[name] || (() => {}))();
}

// Refresh the TAGGER field grid for the selection. The Discogs card list
// persists across mode switches (a search isn't thrown away when you leave).
function refreshTagger() {
  refreshFieldEditor();
  scheduleNameProbe(); // no-op unless FROM NAME is the open sub-tab
}

document.querySelectorAll(".mode-tab").forEach((tab) => {
  tab.addEventListener("click", () => setMode(tab.dataset.mode));
});

// Collapse/expand the mode panel to give the table the full width.
el("panel-toggle").addEventListener("click", () => {
  document.body.classList.toggle("panel-collapsed");
});

// Drop the mode-tab labels to icon-only when the labelled tabs would overflow the
// mode bar (#116). Measured with labels shown (compact removed first), so the
// natural width is the yardstick — no oscillation. Keeps a fifth/longer mode
// (DEDUPLICATOR) from ever truncating the bar.
function updateCompactTabs() {
  const bar = document.querySelector(".modebar");
  const tabs = document.querySelector(".mode-tabs");
  if (!bar || !tabs) return;
  document.body.classList.remove("compact-tabs");
  // Measure against the viewport, not bar.clientWidth — the bar stretches to its
  // content when it overflows, which would hide the overflow from the check.
  const toggle = el("panel-toggle");
  const avail = document.documentElement.clientWidth - (toggle ? toggle.offsetWidth : 0) - 48;
  if (tabs.scrollWidth > avail) document.body.classList.add("compact-tabs");
}
// The mode bar stretches to its content when it overflows, so observing it never
// fires on a viewport shrink — track the viewport (the root element) instead, plus
// the window resize event, and run once at startup.
window.addEventListener("resize", updateCompactTabs);
if (window.ResizeObserver) {
  new ResizeObserver(updateCompactTabs).observe(document.documentElement);
}
updateCompactTabs();

// Right-click handling (#132). The webview's native menu (Reload / Inspect
// Element, plus a wall of macOS text services) looks out of place, so it's
// always suppressed. Over a text input or an editing tag cell we show our own
// minimal Cut/Copy/Paste/Select-All menu instead; elsewhere right-click does
// nothing. On macOS a Ctrl-click is an OS-level right-click, so this also keeps
// it from popping a menu (the additive-select modifier there is ⌘).
const editCtx = el("edit-ctx");
let ctxField = null; // the input / editing cell the menu acts on

function hideEditCtx() {
  editCtx.hidden = true;
  ctxField = null;
}

function showEditCtx(x, y, field) {
  ctxField = field;
  editCtx.hidden = false;
  // Measure once visible, then clamp so the menu never spills off-screen.
  const r = editCtx.getBoundingClientRect();
  const pad = 8;
  editCtx.style.left = Math.max(pad, Math.min(x, window.innerWidth - r.width - pad)) + "px";
  editCtx.style.top = Math.max(pad, Math.min(y, window.innerHeight - r.height - pad)) + "px";
}

document.addEventListener("contextmenu", (e) => {
  e.preventDefault(); // never show the webview's native menu
  const field = e.target.closest('input, textarea, [contenteditable="true"]');
  if (field) showEditCtx(e.clientX, e.clientY, field);
  else hideEditCtx();
});

// Paste at the caret. Read the clipboard through the Tauri plugin — reading it
// with navigator.clipboard.readText() makes WKWebView pop a system "Paste"
// permission pill (#132). In browser-dev there's no plugin, so fall back to the
// web API there. The insert goes through execCommand so it lands at the caret
// and feeds the cell's normal edit handling.
async function pasteIntoField(field) {
  let text = "";
  try {
    if (TAURI) {
      const res = await TAURI.invoke("plugin:clipboard-manager|read_text");
      text = typeof res === "string" ? res : (res && res.plainText && res.plainText.text) || "";
    } else if (navigator.clipboard && navigator.clipboard.readText) {
      text = (await navigator.clipboard.readText()) || "";
    }
  } catch (_) {
    text = "";
  }
  if (field && field.focus) field.focus();
  if (text) document.execCommand("insertText", false, text);
}

// Act on mousedown (fires before the field loses its selection) and keep the
// field focused so execCommand targets it.
editCtx.addEventListener("mousedown", (e) => {
  const item = e.target.closest(".ctx-item");
  if (!item) return;
  e.preventDefault();
  const field = ctxField;
  if (field && field.focus) field.focus();
  const cmd = item.dataset.cmd;
  if (cmd === "cut") document.execCommand("cut");
  else if (cmd === "copy") document.execCommand("copy");
  else if (cmd === "selectall") document.execCommand("selectAll");
  else if (cmd === "paste") pasteIntoField(field);
  hideEditCtx();
});

// Dismiss on any outside interaction.
document.addEventListener("mousedown", (e) => {
  if (!editCtx.hidden && !e.target.closest("#edit-ctx")) hideEditCtx();
});
window.addEventListener("scroll", hideEditCtx, true);
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") hideEditCtx();
});

// ---- diff-state action bar (#117) ----
el("dup-scan").addEventListener("click", runDuplicateScan);
el("diff-discard").addEventListener("click", discardPreview);
// "Show old values" (#80 Q1): reveal the struck-through old value under each
// changed cell. A density toggle over the default single-line (new-only) diff.
el("diff-show-old").addEventListener("change", (e) => {
  el("tracks").classList.toggle("show-old", e.target.checked);
});

// ---- wire up ----
// One button, two roles (#177): what it does is what its label says.
el("lib-action").addEventListener("click", () =>
  libraryIsDirty() ? openLibrary() : browseForFolder()
);
el("root-display").addEventListener("click", editRootPath);
el("root-recent").addEventListener("click", (e) => {
  e.stopPropagation();
  toggleRecentMenu();
});
document.addEventListener("click", (e) => {
  const menu = el("root-recent-menu");
  if (!menu.hidden && !menu.contains(e.target)) closeRecentMenu();
});

previewEditsBtn.addEventListener("click", previewEdits);
applyBtn.addEventListener("click", apply);
undoBtn.addEventListener("click", undo);
el("fields-add").addEventListener("click", addCustomField);
el("fields-add-toggle").addEventListener("click", openAddField);
el("fields-add-cancel").addEventListener("click", closeAddField);
// Enter commits the field; Escape collapses the row (#114).
for (const id of ["fields-new-name", "fields-new-value"]) {
  el(id).addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      addCustomField();
    } else if (e.key === "Escape") {
      e.preventDefault();
      closeAddField();
    }
  });
}
el("fields-apply").addEventListener("click", applyFieldEditor);
el("fields-clear").addEventListener("click", previewClearTags);
// Same flow from the table toolbar, so clearing doesn't require opening EDITOR.
el("clear-tags").addEventListener("click", previewClearTags);
el("export-kind").addEventListener("click", (e) => {
  const btn = e.target.closest("[data-fmt]");
  if (btn) setExportKind(btn.dataset.fmt);
});
// TAGGER sub-tabs: ONLINE (a metadata source), EDITOR (tag fields + cover),
// FROM NAME (the file's own name, #139) — three ways to the same outcome.
function setSubtab(name) {
  document.querySelectorAll(".subtab").forEach((t) => t.classList.toggle("active", t.dataset.subtab === name));
  document.querySelectorAll(".subtab-panel").forEach((p) => {
    p.hidden = p.id !== `subtab-${name}`;
  });
  // The probe is only computed while it's on screen; opening the tab is when it
  // has to catch up with the current selection and pattern.
  if (name === "filename") refreshNameProbe();
}
document.querySelectorAll(".subtab").forEach((tab) => {
  tab.addEventListener("click", () => setSubtab(tab.dataset.subtab));
});


document.addEventListener("keydown", (e) => {
  if (e.key !== "Escape") return;
  // Settings sheet takes precedence.
  if (!el("settings").hidden) {
    cancelSettings();
    return;
  }
  // Esc interrupts an in-progress search sweep (#108) — e.g. the wanted release
  // is already visible. Don't hijack Esc while editing a cell.
  const editing = document.activeElement && document.activeElement.isContentEditable;
  if (!editing && searchBusy()) {
    stopLoading();
    e.preventDefault();
  }
});

// Open a native folder chooser (Tauri dialog plugin). The scanner recurses into
// subfolders, so picking a folder loads everything under it. Outside Tauri
// (browser dev) there's no native dialog — fall back to focusing the path field.
async function browseForFolder() {
  const dialog = window.__TAURI__ && window.__TAURI__.dialog;
  if (!dialog) {
    // Browser development has no native chooser: open the field instead, which
    // is the only way in there.
    toast("Type a library path, then press Open");
    editRootPath();
    return;
  }
  try {
    const picked = await dialog.open({ directory: true, multiple: false });
    if (!picked) return; // user cancelled
    rootInput.value = picked;
    await openLibrary();
  } catch (e) {
    toast(String(e), true);
  }
}

// ---- reorder files by dragging the File cell ----
// Implemented with mouse events (not HTML5 drag-and-drop) because WKWebView,
// the macOS Tauri webview, doesn't drive native DnD reliably. Mouse events
// behave identically everywhere.
let dragPath = null;
// Where the dragged row would land: { path: hovered row, below: true if it
// drops after that row (cursor in its lower half), false if before it }.
let dropInfo = null;

function rowUnder(clientX, clientY) {
  // Not named `el`: that is the imported element lookup now (#143).
  const under = document.elementFromPoint(clientX, clientY);
  return under && under.closest("#tracks-body tr");
}

function clearDropMarkers() {
  tracksBody.querySelectorAll("tr").forEach((tr) => tr.classList.remove("drop-above", "drop-below"));
}

tracksBody.addEventListener("mousedown", (e) => {
  if (diffByPath) return; // no manual reorder while reviewing a staged diff
  const cell = e.target.closest("td.file");
  if (!cell) return;
  e.preventDefault(); // don't start a text selection
  dragPath = cell.closest("tr").dataset.path;
  cell.closest("tr").classList.add("dragging");
  document.addEventListener("mousemove", onDragMove);
  document.addEventListener("mouseup", onDragUp);
});

function onDragMove(e) {
  clearDropMarkers();
  dropInfo = null;
  const row = rowUnder(e.clientX, e.clientY);
  if (!row || !row.dataset.path || row.dataset.path === dragPath) return; // skip group headers
  const rect = row.getBoundingClientRect();
  const below = e.clientY > rect.top + rect.height / 2;
  row.classList.add(below ? "drop-below" : "drop-above");
  dropInfo = { path: row.dataset.path, below };
}

function onDragUp() {
  document.removeEventListener("mousemove", onDragMove);
  document.removeEventListener("mouseup", onDragUp);
  clearDropMarkers();
  tracksBody.querySelectorAll("tr").forEach((tr) => tr.classList.remove("dragging"));
  const source = dragPath;
  const drop = dropInfo;
  dragPath = null;
  dropInfo = null;
  if (!source || !drop) return;

  const from = tracks.findIndex((t) => t.path === source);
  if (from < 0) return;
  const [moved] = tracks.splice(from, 1);
  const targetIndex = tracks.findIndex((t) => t.path === drop.path);
  if (targetIndex < 0) {
    tracks.splice(from, 0, moved); // target vanished; put it back
    return;
  }
  tracks.splice(drop.below ? targetIndex + 1 : targetIndex, 0, moved);
  reindexTracks(); // the list is the same files in a new order (#184)
  // The dragged row supersedes the sort for itself, not for the library (#198):
  // clearing the sort key also released the folder order, so moving one file
  // moved every group.
  setManualOrder(true);
  renderTracks();
}
rootInput.addEventListener("input", updateLibAction);
rootInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter") openLibrary();
  // Escape abandons the edit: back to what is open, and out of the field.
  if (e.key === "Escape") {
    rootInput.value = openedRoot;
    leaveRootEdit();
  }
});
// Leaving the field returns the indicator, unless what was typed is still
// unopened — that has to stay visible while the button offers to open it.
rootInput.addEventListener("blur", leaveRootEdit);
renderRootDisplay();
updateLibAction();
// The bar narrows with the window, so what fits changes with it (#182).
window.addEventListener("resize", fitRootDisplay);
el("root-recent").hidden = recentRoots().length === 0;
selectAll.addEventListener("change", () => {
  const on = selectAll.checked;
  // While diffing the header box toggles the whole apply scope, not selection.
  if (diffByPath) {
    setApplySelection(new Set(on ? diffByPath.keys() : []));
    for (const tr of tracksBody.querySelectorAll("tr.staged")) {
      const cb = tr.querySelector(".apply-tick");
      if (cb) cb.checked = on;
    }
    updateDiffBar();
    return;
  }
  // Every file the table lists, not just the rows on screen (#189) — including
  // the ones inside a collapsed folder, which select-all covered when they were
  // hidden rows too.
  for (const path of viewPaths) {
    if (on) selection.add(path);
    else selection.delete(path);
  }
  syncSelectionUI();
});
// Direct checkbox clicks feed the selection set — or, while diffing, the
// per-row apply scope (the sel column's meaning switches in diff-state, #117).
tracksBody.addEventListener("change", (e) => {
  const cb = e.target.closest(".sel input[type=checkbox]");
  if (!cb) return;
  if (diffByPath) {
    // Only a staged row has an apply tick. A row outside the plan keeps its own
    // (inert) selection box while diffing (#186), so ignore it here.
    if (!diffByPath.has(cb.dataset.path)) return;
    if (cb.checked) applySelection.add(cb.dataset.path);
    else applySelection.delete(cb.dataset.path);
    updateDiffBar();
    return;
  }
  if (cb.checked) selection.add(cb.dataset.path);
  else selection.delete(cb.dataset.path);
  syncSelectionUI();
});

// ---- row selection ----
// The `selection` set is the source of truth (see its declaration). On top of
// checkboxes, clicking a row selects it the way a file list does (click = only
// this row, ⌘/Ctrl = toggle, Shift = range); double-clicking a group's name
// toggles that whole group. Editing a cell is a deliberate double-click, so the
// single click is free for selection.
let selAnchor = null; // path of the last row clicked, for Shift-range

function rowCheckbox(tr) {
  return tr.querySelector(".sel input[type=checkbox]");
}

// Push the `selection` set onto the checkboxes + row highlight, set the
// select-all tri-state, and refresh the status count. Called after any change.
// What the DOM was last told about the selection, so a change can be applied to
// the rows it actually concerns (#184). Walking every row to toggle a class is
// most of what a row click cost on a library of a few thousand.
let paintedSelection = new Set();

function syncSelectionUI() {
  // Only the rows on screen can be painted; the ones off it are built from the
  // selection when they scroll in (#189). The tri-state still speaks for the
  // whole table, so its count comes from the model.
  for (const [path, tr] of renderedRows) {
    const on = selection.has(path);
    if (on !== paintedSelection.has(path)) {
      const cb = rowCheckbox(tr);
      if (cb) cb.checked = on;
      tr.classList.toggle("selected", on);
    }
  }
  paintedSelection = new Set(selection);
  let checked = 0;
  for (const path of viewPaths) if (selection.has(path)) checked += 1;
  selectAll.checked = checked > 0 && checked === viewPaths.length;
  selectAll.indeterminate = checked > 0 && checked < viewPaths.length;
  updateStatus();
  // Mode-panel headings show a selection count ("— N selected"); they otherwise
  // only refresh on mode entry, so keep them live as the selection changes.
  updatePanelCounts();
  // The TAGGER field grid shows the current selection's values, so keep it in
  // step as the selection changes while that mode is open — and so does the
  // FROM NAME probe, which reads the first selected file's name.
  if (currentMode === "tagger") {
    refreshFieldEditor();
    scheduleNameProbe();
    // An expanded release card compares its listed lengths against the selected
    // files (#188), so the deltas go stale the moment the selection moves.
    refreshReleaseMatches();
  }
}

// Selection-dependent counts in the GENERATOR/EXPORTER panel headings. Cheap
// text updates safe to run on every selection change (the panels stay in the
// DOM even while hidden); the full panel refresh still happens on mode entry.
function updatePanelCounts() {
  const count = selectedPaths().length;
  el("transform-count").textContent = count ? `— ${plural(count, "file", "files")}` : "";
  el("autonum-count").textContent = count ? `— ${count} selected` : "";
  el("vinyl-count").textContent = count ? `— ${count} selected` : "";
  el("export-count").textContent = count ? `— ${plural(count, "track", "tracks")}` : "";
}

function selectRow(tr, e) {
  if (tr.classList.contains("unreadable")) return; // inert — can't be selected
  const path = tr.dataset.path;
  if (e.shiftKey && selAnchor) {
    // The range runs over the model's order, so it spans files whose rows are
    // off screen or inside a collapsed folder (#189).
    let a = viewPathIndex.has(selAnchor) ? viewPathIndex.get(selAnchor) : -1;
    let b = viewPathIndex.get(path);
    if (a < 0) a = b;
    if (a > b) [a, b] = [b, a];
    selection.clear();
    for (let i = a; i <= b; i++) selection.add(viewPaths[i]);
  } else if (e.metaKey || e.ctrlKey) {
    if (selection.has(path)) selection.delete(path);
    else selection.add(path);
    selAnchor = path;
  } else {
    selection.clear();
    selection.add(path);
    selAnchor = path;
  }
  syncSelectionUI();
  setActiveRow(tr, true); // clicking a row also makes it the keyboard-nav anchor
}

// Which selection gesture a group-header click carries: Shift = extend a range,
// ⌘/Ctrl = add/toggle, otherwise replace. Mirrors row-click modifiers.
function groupSelectMode(e) {
  if (e.shiftKey) return "range";
  if (e.metaKey || e.ctrlKey) return "add";
  return "replace";
}

// Select a whole folder from its group header (#130, #131), by `mode`:
//  - "replace": the selection becomes exactly this folder's files.
//  - "add" (⌘/Ctrl): toggle this folder in/out, leaving other groups alone.
//  - "range" (Shift): extend from the anchor row through this whole folder,
//    selecting everything in between — the standard Shift-range behaviour.
// The group is expanded first so the choice is visible; replace/add drop the
// anchor on the folder's first file so a following Shift extends from there,
// while range keeps the existing anchor (as a range extension should).
function selectGroup(key, mode) {
  if (collapsedGroups.has(key)) toggleGroup(key); // reveal what's being selected
  const paths = groupPaths.get(key) || [];
  if (paths.length === 0) return;
  const gStart = viewPathIndex.get(paths[0]);
  const gEnd = viewPathIndex.get(paths[paths.length - 1]);
  if (mode === "range") {
    let a = selAnchor && viewPathIndex.has(selAnchor) ? viewPathIndex.get(selAnchor) : -1;
    if (a < 0) a = gStart;
    const lo = Math.min(a, gStart);
    const hi = Math.max(a, gEnd);
    selection.clear();
    for (let i = lo; i <= hi; i++) selection.add(viewPaths[i]);
  } else if (mode === "add") {
    const allSelected = paths.every((p) => selection.has(p));
    for (const p of paths) {
      if (allSelected) selection.delete(p);
      else selection.add(p);
    }
    selAnchor = paths[0];
  } else {
    selection.clear();
    for (const p of paths) selection.add(p);
    selAnchor = paths[0];
  }
  syncSelectionUI();
}

// Enter edit mode on a cell and select its text (double-click, per the hint).
function beginCellEdit(cell) {
  cell.contentEditable = "true";
  cell.focus();
  // Read the library's values for this column once per edit, not per keystroke
  // (#63). Nothing shows until something is typed.
  openCellSuggest(cell);
  const range = document.createRange();
  range.selectNodeContents(cell);
  const sel = window.getSelection();
  sel.removeAllRanges();
  sel.addRange(range);
}

tracksBody.addEventListener("click", (e) => {
  if (diffByPath) return; // rows are inert while a staged diff is under review
  if (e.target.closest("td.sel")) return; // checkbox toggle → change listener
  const groupHead = e.target.closest("tr.group-head");
  if (groupHead) {
    // The caret has its own collapse listener. A modified click (⌘/Ctrl add,
    // Shift range) selects the folder; a plain click does nothing, so plain
    // folder-select stays the double-click gesture and a stray click never
    // wipes the selection.
    const mode = groupSelectMode(e);
    if (!e.target.closest(".group-caret") && mode !== "replace") {
      selectGroup(groupHead.dataset.group, mode);
    }
    return;
  }
  const tr = e.target.closest("tr");
  if (!tr || !tr.dataset.path) return;
  const cell = e.target.closest("td.editable");
  if (cell && cell.isContentEditable) return; // mid-edit: don't reselect
  selectRow(tr, e);
});

tracksBody.addEventListener("dblclick", (e) => {
  if (diffByPath) return; // no editing/playing while reviewing a staged diff
  const head = e.target.closest("tr.group-head");
  if (head) {
    // Caret double-click just toggles collapse (handled by the click listener);
    // double-clicking the name selects the folder — plainly it replaces the
    // selection, ⌘/Ctrl adds it, Shift extends a range (#130, #131).
    if (!e.target.closest(".group-caret")) {
      selectGroup(head.dataset.group, groupSelectMode(e));
    }
    return;
  }
  // Double-clicking the file name plays the track (the per-row play button was
  // removed, #99 redesign); tag cells still double-click to edit.
  const fileCell = e.target.closest("td.file");
  if (fileCell) {
    const tr = e.target.closest("tr");
    if (tr && tr.dataset.path && !tr.classList.contains("unreadable")) {
      playTrack(tr.dataset.path);
    }
    return;
  }
  const cell = e.target.closest("td.editable");
  if (cell && !cell.classList.contains("locked")) {
    hideCellTip();
    beginCellEdit(cell);
  }
});

// contentEditable is turned off again when a cell loses focus (blur doesn't
// bubble, so listen in the capture phase).
tracksBody.addEventListener(
  "blur",
  (e) => {
    if (e.target.classList && e.target.classList.contains("editable")) {
      e.target.contentEditable = "false";
      endCellSuggest(); // the suggestion list belongs to the edit (#63)
    }
  },
  true,
);

// ---- editable-cell hover hint ----
// A self-managed tooltip rather than the native `title`: every editable cell
// carries the same text, so the OS bubble stayed anchored to the first cell when
// the pointer moved onto an identical neighbour. This one hides the instant the
// pointer leaves a cell, re-arms on the next, and never shows mid-edit.
const cellTip = document.createElement("div");
cellTip.className = "cell-tip";
cellTip.textContent = "Double-click to edit";
cellTip.hidden = true;
document.body.appendChild(cellTip);

let cellTipCell = null;
let cellTipTimer = 0;

function hideCellTip() {
  clearTimeout(cellTipTimer);
  cellTipTimer = 0;
  cellTipCell = null;
  cellTip.hidden = true;
}

function showCellTipAt(x, y) {
  cellTip.hidden = false;
  // Clamp to the viewport so the bubble never spills off-screen at the edges.
  const left = Math.min(x + 14, window.innerWidth - cellTip.offsetWidth - 6);
  const top = Math.min(y + 18, window.innerHeight - cellTip.offsetHeight - 6);
  cellTip.style.left = `${left}px`;
  cellTip.style.top = `${top}px`;
}

tracksBody.addEventListener("pointermove", (e) => {
  if (e.pointerType && e.pointerType !== "mouse") return; // no hover on touch
  const cell = e.target.closest("td.editable");
  if (!cell || cell.isContentEditable) {
    if (cellTipCell) hideCellTip();
    return;
  }
  if (cell !== cellTipCell) {
    // Onto a new cell: drop any showing/pending bubble and re-arm the delay.
    clearTimeout(cellTipTimer);
    cellTip.hidden = true;
    cellTipCell = cell;
    // A locked cell explains itself rather than offering an edit it will
    // refuse — and says where the lock can be taken off again (#48).
    cellTip.textContent = cell.classList.contains("locked")
      ? "Locked — unlock it in the EDITOR panel"
      : "Double-click to edit";
    const { clientX, clientY } = e;
    cellTipTimer = window.setTimeout(() => showCellTipAt(clientX, clientY), 350);
  }
});

tracksBody.addEventListener("pointerleave", hideCellTip);
tracksBody.addEventListener("pointerdown", hideCellTip);
// Scrolling moves the cells out from under a fixed-position bubble — drop it.
window.addEventListener("scroll", hideCellTip, true);

// ---- keyboard row navigation (roving tabindex) ----
// Exactly one data row is tabbable (tabindex 0); ↑/↓ move focus between visible
// rows and Space toggles the focused row's selection. This makes the row focus
// ring (states.css) reachable for a keyboard-heavy tool.

// Keep exactly one row tabbable; called after every render. The anchor is a
// PATH, not a row (#189): the row it names may be scrolled out of the window,
// and it has to survive that.
function refreshRoving() {
  if (navPaths.length === 0) {
    setActiveRowPath(null);
    return;
  }
  if (!navPathIndex.has(activeRowPath)) setActiveRowPath(navPaths[0]);
  for (const [path, tr] of renderedRows) tr.tabIndex = path === activeRowPath ? 0 : -1;
}

// Scroll `path` into view when it is outside the window, so a row reached by
// keyboard exists to be focused. The sticky header sits over the top of the
// scroller, so a row arriving from above is placed below it rather than under it.
function scrollPathIntoView(path) {
  const index = itemIndexByPath.get(path);
  if (index === undefined) return;
  const headHeight = el("tracks").querySelector("thead")?.offsetHeight || 0;
  const top = itemTops[index];
  const bottom = top + itemHeightAt(index);
  const viewTop = tableWrap.scrollTop + headHeight;
  const viewBottom = tableWrap.scrollTop + tableWrap.clientHeight;
  if (top < viewTop) tableWrap.scrollTop = Math.max(0, top - headHeight);
  else if (bottom > viewBottom) tableWrap.scrollTop += bottom - viewBottom;
  renderWindow();
}

// Make `path` the keyboard anchor, and focus its row when asked — bringing it on
// screen first if the window doesn't hold it.
function setActiveRowByPath(path, focus) {
  setActiveRowPath(path);
  if (focus && path) scrollPathIntoView(path);
  for (const [p, tr] of renderedRows) tr.tabIndex = p === activeRowPath ? 0 : -1;
  if (focus && path) renderedRow(path)?.focus();
}

function setActiveRow(tr, focus) {
  setActiveRowByPath(tr ? tr.dataset.path : null, focus);
}

tracksBody.addEventListener("keydown", (e) => {
  // Don't hijack keys while editing a cell or typing in a control.
  if (e.target.isContentEditable || e.target.matches("input, textarea, select")) return;
  const tr = e.target.closest("tr");
  if (!tr || !tr.dataset.path) return;
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    const i = navPathIndex.get(tr.dataset.path);
    const next = i === undefined ? undefined : navPaths[e.key === "ArrowDown" ? i + 1 : i - 1];
    if (next) setActiveRowByPath(next, true);
  } else if (e.key === " ") {
    e.preventDefault(); // Space would otherwise scroll
    const path = tr.dataset.path;
    // While diffing, Space toggles the focused staged row's apply tick.
    if (diffByPath) {
      if (!diffByPath.has(path)) return; // untouched rows aren't tickable
      if (applySelection.has(path)) applySelection.delete(path);
      else applySelection.add(path);
      const cb = tr.querySelector(".apply-tick");
      if (cb) cb.checked = applySelection.has(path);
      updateDiffBar();
      return;
    }
    if (selection.has(path)) selection.delete(path);
    else selection.add(path);
    selAnchor = path;
    syncSelectionUI();
  }
});


// Sort by clicking a column header (toggles direction). Reorders `tracks`
// itself so position-based mapping follows the visible order.
function sortBy(key) {
  if (sortKey === key) applySort(key, -sortDir);
  else applySort(key, 1);
}

// Sort by `key` in an explicit direction (1 asc, -1 desc) — the toggle-free core
// shared by header clicks and preset restore (#44).
function applySort(key, dir) {
  setSortKey(key);
  setSortDir(dir);
  sortTracks();
  renderTracks();
}

// Put `tracks` in the active sort's order. Separate from applySort because a
// freshly read list has to be sorted again without re-deciding what the sort is
// (#196): an apply or an undo replaces the list with what is on disk, in scan
// order, while the header still claims a sort — so everything jumped, the folder
// being worked on included, and nothing said why.
function sortTracks() {
  if (!sortKey) return;
  // Whatever was placed by hand is placed by the sort now (#198) — a re-read
  // from disk is where a hand-made order ends, and so is a fresh sort.
  setManualOrder(false);
  tracks.sort(
    (a, b) =>
      sortValue(a, sortKey).localeCompare(sortValue(b, sortKey), undefined, { numeric: true }) *
      sortDir,
  );
}

// Sort clicks are delegated on the header so dynamically-built columns (#43)
// stay sortable.
el("tracks").querySelector("thead").addEventListener("click", (e) => {
  if (e.target.closest(".col-resize")) return; // grip click is a resize, not a sort
  const th = e.target.closest("th.sortable");
  if (th) sortBy(th.dataset.sort);
});

// Column picker popover (#43): toggle, drag-reorder, and outside-click close.
el("columns-btn").addEventListener("click", (e) => {
  e.stopPropagation();
  const menu = el("columns-menu");
  if (menu.hidden) renderColumnsMenu();
  menu.hidden = !menu.hidden;
});
document.addEventListener("click", (e) => {
  const menu = el("columns-menu");
  if (!menu.hidden && !menu.contains(e.target) && e.target !== el("columns-btn")) {
    menu.hidden = true;
  }
});
// Grouping is a view overlay — changing it only re-renders, never reorders
// `tracks`. Collapsed state is per grouping, so setGroupBy resets it.
el("group-btn").addEventListener("click", (e) => {
  e.stopPropagation();
  const menu = el("group-menu");
  menu.hidden = !menu.hidden;
});
el("group-menu").addEventListener("click", (e) => {
  const opt = e.target.closest(".group-opt");
  if (!opt) return;
  el("group-menu").hidden = true;
  setGroupBy(opt.dataset.group);
});
document.addEventListener("click", (e) => {
  const menu = el("group-menu");
  if (!menu.hidden && !menu.contains(e.target) && !e.target.closest("#group-btn")) menu.hidden = true;
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") el("group-menu").hidden = true;
});

// Collapse/expand a group by clicking its header. Which rows exist is a model
// question now (#189): a collapsed folder's tracks leave the rendered list
// entirely rather than being rows with `display: none`. The selection is a set,
// so it survives the rebuild untouched.
function toggleGroup(key) {
  const collapse = !collapsedGroups.has(key);
  if (collapse) collapsedGroups.add(key);
  else collapsedGroups.delete(key);
  buildViewModel();
  renderWindow({ force: true });
  syncGroupToggle();
}
// Collapse/expand only via the caret at the start of the header, so a click on
// the group name is free to (double-)select the group instead.
tracksBody.addEventListener("click", (e) => {
  const caret = e.target.closest(".group-caret");
  if (!caret) return;
  const head = caret.closest("tr.group-head");
  if (head) toggleGroup(head.dataset.group);
});

// Expand/collapse every group at once (#32), through the same model rebuild as
// an individual header.
function setAllGroupsCollapsed(collapse) {
  collapsedGroups.clear();
  if (collapse) for (const key of viewGroups) collapsedGroups.add(key);
  buildViewModel();
  renderWindow({ force: true });
}
// The two Expand all / Collapse all buttons collapsed into one state toggle:
// while any group is still open it offers "collapse", and once everything is
// shut it flips to "expand". Icon + tooltip are re-derived from the model rather
// than a remembered flag, so per-group clicks keep it honest.
function syncGroupToggle() {
  const btn = el("toggle-groups");
  if (!btn || btn.hidden) return;
  const anyOpen = viewGroups.some((key) => !collapsedGroups.has(key));
  const label = anyOpen ? "Collapse every group" : "Expand every group";
  btn.innerHTML = ico(anyOpen ? "collapse-all" : "expand-all");
  btn.title = label;
  btn.setAttribute("aria-label", label);
  btn.dataset.collapse = anyOpen ? "1" : "0";
}
el("toggle-groups").addEventListener("click", () => {
  setAllGroupsCollapsed(el("toggle-groups").dataset.collapse !== "0");
  syncGroupToggle();
});

// Typing re-renders the whole table, which on a large library is far slower
// than the typing (#184) — so the render waits for a pause. The controls follow
// the keystroke, so the box itself never feels laggy.
let filterTimer = null;
el("filter").addEventListener("input", (e) => {
  // Keep the raw text — case matters in regex/case-sensitive mode; lowercasing
  // for a plain match happens in matchesFilter.
  setFilterText(e.target.value.trim());
  syncFilterControls();
  clearTimeout(filterTimer);
  filterTimer = setTimeout(renderTracks, 120);
});
// Regex / case toggles (#44): flip the flag, persist, recompile, repaint.
el("filter-regex").addEventListener("click", () => {
  setFilterRegex(!filterRegex);
  saveFilterMode();
  syncFilterControls();
  renderTracks();
});
el("filter-case").addEventListener("click", () => {
  setFilterCase(!filterCase);
  saveFilterMode();
  syncFilterControls();
  renderTracks();
});
// Presets popover (#44): mirrors the columns picker — toggle + outside-click close.
el("presets-btn").addEventListener("click", (e) => {
  e.stopPropagation();
  const menu = el("presets-menu");
  if (menu.hidden) renderPresetsMenu();
  menu.hidden = !menu.hidden;
});
document.addEventListener("click", (e) => {
  const menu = el("presets-menu");
  if (!menu.hidden && !menu.contains(e.target) && e.target !== el("presets-btn")) {
    menu.hidden = true;
  }
});
// The action-groups popovers (#57) wire their own toggle and outside-click
// close, in chain.js, since there is one per rule chain (#144).
// Track edits on any editable cell (event delegation).
tracksBody.addEventListener("input", (e) => {
  if (!e.target.classList.contains("editable")) return;
  onCellEdit(e.target);
  refreshCellSuggest(e.target); // offer what the library already says (#63)
});
// Enter commits a cell instead of inserting a newline, and leaves edit mode.
tracksBody.addEventListener("keydown", (e) => {
  if (!e.target.classList.contains("editable")) return;
  // The suggestion list gets first refusal on the arrows, and on Enter/Escape
  // only once an arrow has stepped into it (#63) — so a cell nobody is reading
  // suggestions for still commits on Enter exactly as it always has.
  if (cellSuggestKey(e)) return;
  if (e.key === "Enter") {
    e.preventDefault();
    // Ending the edit ends the suggestions with it, said here rather than left
    // to the blur listener below: turning contentEditable off moves focus to
    // the body WITHOUT firing a blur on the cell, so the list would stay on
    // screen over a cell nobody is editing any more (#63).
    endCellSuggest();
    e.target.contentEditable = "false";
    e.target.blur();
  }
});

loadSavedToken();
// What the session locks (#48). Nothing is open yet at start-up, so this
// normally answers "nothing"; after a window reload with a library still open it
// is what stops the padlocks from disagreeing with the backend enforcing them.
loadFieldLocks();

// Apply the saved column choice (#43) and build the header before any library
// is opened.
// The persisted filter-mode flags (#44), read at start-up into shared state.
setFilterRegex(regexModeEnabled());
setFilterCase(caseSensitiveEnabled());
loadCustomColumns(); // before loadColumns: it validates keys against the pool
loadColumns();
loadColumnWidths();
renderTableHead();
applyValueFont(valueFont());
applyCheckboxCol(checkboxColEnabled());
// Publish the platform's scrollbar width so non-scrolling headers can reserve
// the same gutter their scrolling sibling does and the two stay flush. Overlay
// scrollbars (macOS default) measure 0, which is exactly right — nothing to
// reserve there.
(function measureScrollbarWidth() {
  const probe = document.createElement("div");
  probe.style.cssText = "position:absolute;top:-9999px;width:100px;height:100px;overflow:scroll;";
  document.body.appendChild(probe);
  const w = probe.offsetWidth - probe.clientWidth;
  probe.remove();
  document.documentElement.style.setProperty("--sb-w", `${w}px`);
})();

applyTableFont(tableFontPx());
applyTracklistFont(tracklistFontPx());
applyBadgeFont(badgeFont());
// Reflect saved defaults onto the grouping + search page-size selects (#108).
// Grouping options are built from the modeled fields (#43), then the saved
// choice is applied.
populateGroupMenu();
el("search-per-page").value = String(searchPerPage);
// Load saved action groups (#57) from settings.json into the Groups popover.
initActionGroups();
initBuiltinGroups();
// Reflect saved filter-mode flags (#44) onto the toggles.
syncFilterControls();
// Wire the placeholder reference (#148) and warm its catalogue, so the column
// headers can name their placeholder without an await per header. The head is
// already drawn by then, so it is redrawn once the names are available.
initPlaceholderReference().then(renderTableHead);

// ---- dev surface (#143) ----
// app.js is a module now, so nothing in it is global by accident. The
// browser-served verification path drives the UI by calling into it, so the few
// entry points it needs are exposed deliberately, in one place, instead of the
// whole file leaking into `window`. Harmless in the native app: it is a handle
// on functions the UI itself already calls.
window.tagrex = {
  invoke,
  openLibrary,
  setMode,
  setSubtab,
  selectedPaths,
  renderTracks,
  get tracks() {
    return tracks;
  },
  get selection() {
    return selection;
  },
  get previewPlan() {
    return previewPlan;
  },
};
