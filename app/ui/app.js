"use strict";

import { TAURI, invoke } from "./js/invoke.js";
import { el, toast, fileName, escapeHtml, ico, confirmDialog } from "./js/dom.js";
import { enablePointerReorder } from "./js/reorder.js";
import { refreshExporter, setExportKind } from "./js/exporters.js";
import { hooks } from "./js/hooks.js";
import { allColumnKeys, columnLabel, populateGroupMenu, setGroupBy } from "./js/columns.js";
import { dropGroupKey, folderGroupLabel, groupKeyOf, groupLabel } from "./js/grouping.js";
import { loadSavedToken, searchBusy, searchPerPage, stopLoading } from "./js/online.js";
import {
  addTransformRule,
  initActionGroups,
  initBuiltinGroups,
  numberTracks,
  previewTransform,
  refreshGenerator,
  renderGroupsMenu,
  splitVinylSides,
} from "./js/generator.js";
import { EXTENDED_FIELDS, KNOWN_CUSTOM_LABELS, VIRTUAL_COLUMNS } from "./js/fields.js";
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
Object.assign(hooks, { renderTracks, renderPreview, previewEdits, refreshCoverWell });
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
  tracks, setTracks,
  previewPlan, setPreviewPlan,
  previewSource, setPreviewSource,
  diffByPath, setDiffByPath,
  applySelection, setApplySelection,
  edits, selection, selectedPaths, tag, trackByPath,
  DEFAULT_COLUMNS, visibleColumns, setVisibleColumns,
  columnWidths, setColumnWidths,
  sortKey, setSortKey, sortDir, setSortDir,
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

// Column persistence keys; the columns themselves live in the state module.
const COLUMNS_STORAGE_KEY = "tagrex.columns";
const COLUMN_WIDTHS_STORAGE_KEY = "tagrex.colWidths";
const COLUMN_MIN_WIDTH = 48;

// Sensible starting width for a column the user hasn't resized yet. The file
// name is the widest; short numeric/code fields start narrow.
function defaultColumnWidth(key) {
  if (key === "file") return 240;
  if (["year", "track", "tracktotal", "disc", "bpm", "key"].includes(key)) return 70;
  if (["artist", "title", "album", "albumartist", "composer"].includes(key)) return 160;
  return 130;
}

function columnWidth(key) {
  return columnWidths[key] || defaultColumnWidth(key);
}

// View state (does not change what's on disk). Sorting reorders the `tracks`
// array itself so position-based mapping (rename masks, Discogs import) follows
// the visible order; filtering only hides rows.
// Filtering (#44). `filterText` is the raw query as typed (case is significant
// in regex/case-sensitive mode, so it is NOT pre-lowercased). Regex and
// case-sensitivity are persisted display prefs. A field-scoped query
// (`artist:aphex`) narrows the match to one column. `filterQuery` is the parsed
// form (re-derived by recompileFilter whenever the text or flags change) so the
// per-row test stays cheap; `filterError` flags a regex that failed to compile.
// The persisted filter-mode flags are read here, at the same point in startup
// as before, and pushed into the state module.
setFilterRegex(regexModeEnabled());
setFilterCase(caseSensitiveEnabled());
const PRESETS_STORAGE_KEY = "tagrex.filterPresets";
setGroupByValue(groupByPref());
// The dropped directories of a file-set drag-and-drop (#127), or null for an
// ordinary library. When set, the table's "drop" grouping buckets each track
// under the dropped folder it came from, with loose files under "Files".
// The root of the currently open session (the opened/dropped folder, or a
// file-set's common ancestor). Folder-group headers show the path relative to
// it — starting with the root's own name — so nested folders read like a tree
// (#129), matching what a reference tagger shows.

// ---- elements ----
const rootInput = el("root");
const tracksBody = el("tracks-body");
const tracksEmpty = el("tracks-empty");
const applyBtn = el("diff-apply");
const previewBtn = el("preview");
const previewEditsBtn = el("preview-edits");
const undoBtn = el("undo");
const selectAll = el("select-all");
const coverWell = el("cover-well");
const coverFileInput = el("cover-file");
const statusSel = el("status-sel");

// ---- helpers ----

// ---- rendering ----
// Renders the table, overlaying any pending edits on top of the on-disk
// values (edited cells shown and marked dirty). Does NOT clear `edits` — call
// `resetEdits()` for that when loading fresh disk state.
function sortValue(track, key) {
  // Position sorts by (disc, track) numerically, so A1, A2, … B1 order holds
  // (and plain CD tracks sort numerically too) instead of by the display string.
  if (key === "position") {
    const disc = parseInt(track.tags.disc || "0", 10) || 0;
    const trk = parseInt(track.tags.track || "0", 10) || 0;
    return String(disc).padStart(4, "0") + String(trk).padStart(5, "0");
  }
  return (key === "file" ? fileName(track.path) : track.tags[key] || "").toLowerCase();
}

// The display value of one column for a track — the same source the columns and
// sort use, so a field-scoped filter (`position:B1`) sees what the eye sees.
function fieldValue(track, key) {
  if (key === "file") return fileName(track.path);
  if (key === "position") return vinylPositionOf(track, edits.get(track.path));
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
    ind.innerHTML =
      th.dataset.sort === sortKey ? ico(sortDir > 0 ? "tri-up" : "tri-down") : "";
  });
}

// ---- vinyl side notation view (#106) ----
// Whether a media-type value denotes a side-based medium (vinyl / cassette).
// Mirrors `is_side_medium` in the mask backend.
function isSideMedium(media) {
  const m = (media || "").toLowerCase();
  return ["vinyl", "lp", "shellac", "cassette", "tape", '"', "acetate"].some((k) => m.includes(k));
}

// The side letter for a disc on side-based media (disc 1 → A, …, 26 → Z), else
// null. Mirrors `side_letter_for` in the backend.
function sideLetterOf(media, disc) {
  if (!isSideMedium(media)) return null;
  const d = parseInt(disc, 10);
  return d >= 1 && d <= 26 ? String.fromCharCode(64 + d) : null;
}

// The reconstructed position shown in the Position column: vinyl/cassette get the
// side letter + track ("B" + "3" = "B3"); everything else just the track number.
// Reads pending edits first so it tracks staged changes to media/disc/track.
function vinylPositionOf(track, pending) {
  const get = (key) => (pending && pending.has(key) ? pending.get(key) : track.tags[key] || "");
  const letter = sideLetterOf(get("media"), get("disc"));
  const trackNo = get("track");
  return letter ? letter + trackNo : trackNo;
}

// Rebuild the sortable column headers from `visibleColumns` (the sel + play
// headers are static so #select-all and its listener survive).
function renderTableHead() {
  const row = el("tracks").querySelector("thead tr");
  row.querySelectorAll("th.sortable").forEach((th) => th.remove());
  for (const key of visibleColumns) {
    const th = document.createElement("th");
    th.dataset.sort = key;
    th.className = "sortable";
    th.style.width = `${columnWidth(key)}px`;
    // A drag grip on the right edge resizes the column; a label span keeps the
    // header text clipping (ellipsis) independent of the grip.
    th.innerHTML =
      `<span class="th-label">${escapeHtml(columnLabel(key))}<span class="sort-ind"></span></span>` +
      `<span class="col-resize" data-key="${escapeHtml(key)}"></span>`;
    row.appendChild(th);
  }
  updateSortIndicators();
}

function saveColumns() {
  try {
    localStorage.setItem(COLUMNS_STORAGE_KEY, JSON.stringify(visibleColumns));
  } catch (e) {
    /* localStorage unavailable — columns just won't persist */
  }
}

function saveColumnWidths() {
  try {
    localStorage.setItem(COLUMN_WIDTHS_STORAGE_KEY, JSON.stringify(columnWidths));
  } catch (e) {
    /* localStorage unavailable — widths just won't persist */
  }
}

// Load saved widths; keep only known keys with a sane positive number.
function loadColumnWidths() {
  try {
    const saved = JSON.parse(localStorage.getItem(COLUMN_WIDTHS_STORAGE_KEY));
    if (saved && typeof saved === "object") {
      const known = new Set(allColumnKeys());
      for (const [key, w] of Object.entries(saved)) {
        if (known.has(key) && Number.isFinite(w) && w >= COLUMN_MIN_WIDTH) {
          columnWidths[key] = Math.round(w);
        }
      }
    }
  } catch (e) {
    /* keep defaults */
  }
}

// Load the saved column choice; drop unknown keys and force "file" first.
function loadColumns() {
  try {
    const saved = JSON.parse(localStorage.getItem(COLUMNS_STORAGE_KEY));
    if (Array.isArray(saved) && saved.length) {
      const known = new Set(allColumnKeys());
      const cols = saved.filter((k) => known.has(k) && k !== "file");
      cols.unshift("file");
      if (cols.length > 1) setVisibleColumns(cols);
    }
  } catch (e) {
    /* keep defaults */
  }
}

// Apply a new column set: persist, rebuild the header, repaint rows.
function applyColumns(cols) {
  const deduped = [...new Set(cols)].filter((k) => k !== "file");
  setVisibleColumns(["file", ...deduped]);
  saveColumns();
  renderTableHead();
  renderTracks();
}

// Reset columns to the default set, visibility, and widths (#91).
function resetColumns() {
  setColumnWidths({});
  try {
    localStorage.removeItem(COLUMN_WIDTHS_STORAGE_KEY);
  } catch (e) {
    /* nothing persisted to clear */
  }
  applyColumns(DEFAULT_COLUMNS.slice()); // persists + rebuilds head/rows
  renderColumnsMenu();
}


// ---- column picker popover (#43) ----

function renderColumnsMenu() {
  const menu = el("columns-menu");
  menu.innerHTML = "";
  for (const key of visibleColumns) menu.appendChild(colMenuRow(key, true));
  const hidden = allColumnKeys().filter((k) => !visibleColumns.includes(k));
  if (hidden.length) {
    const sep = document.createElement("div");
    sep.className = "col-menu-sep";
    sep.textContent = "Hidden";
    menu.appendChild(sep);
    for (const key of hidden) menu.appendChild(colMenuRow(key, false));
  }
  // Reset-to-default footer (#91): default set, order, visibility, and widths.
  const foot = document.createElement("div");
  foot.className = "col-menu-foot";
  const reset = document.createElement("button");
  reset.type = "button";
  reset.className = "text-btn";
  reset.textContent = "Reset to default";
  reset.title = "File · Artist · Title · Album · Year, default widths";
  reset.addEventListener("click", resetColumns);
  foot.appendChild(reset);
  menu.appendChild(foot);
}

function colMenuRow(key, visible) {
  const isFile = key === "file";
  const row = document.createElement("div");
  row.className = "col-menu-row";
  row.dataset.key = key;

  const grip = document.createElement("span");
  grip.className = "col-grip";
  grip.innerHTML = ico("grip");
  if (visible && !isFile) {
    grip.title = "Drag to reorder";
    enablePointerReorder(grip, row, el("columns-menu"), ".col-menu-row", (dragged, target, below) => {
      if (target === "file" || !visibleColumns.includes(target)) return;
      const order = visibleColumns.filter((k) => k !== "file");
      order.splice(order.indexOf(dragged), 1);
      let to = order.indexOf(target);
      if (below) to += 1;
      order.splice(to, 0, dragged);
      applyColumns(order);
      renderColumnsMenu();
    });
  } else {
    grip.style.visibility = "hidden";
  }

  const box = document.createElement("input");
  box.type = "checkbox";
  box.checked = visible;
  box.disabled = isFile; // file is structural — always shown, first
  box.addEventListener("change", () => {
    applyColumns(box.checked ? [...visibleColumns, key] : visibleColumns.filter((k) => k !== key));
    renderColumnsMenu();
  });

  const label = document.createElement("span");
  label.className = "col-menu-label";
  label.textContent = columnLabel(key) + (isFile ? " (always shown)" : "");

  row.append(grip, box, label);
  return row;
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

// Build one track row and append it to the body. `groupKey` (when grouping)
// tags the row so its group header can collapse it.
function appendTrackRow(track, groupKey) {
  const pending = edits.get(track.path);
  const tr = document.createElement("tr");
  tr.dataset.path = track.path;
  if (groupKey !== null) {
    tr.dataset.group = groupKey;
    if (collapsedGroups.has(groupKey)) tr.classList.add("hidden-row");
  }
  // An unreadable file (tags failed to parse) is shown but inert: it can't be
  // selected, played, or edited, and every mode's preview already skips it. It's
  // listed only so it never looks like the file vanished (#83).
  if (track.unreadable) {
    tr.classList.add("unreadable");
    tr.innerHTML = `
      <td class="sel"><input type="checkbox" disabled title="This file's tags couldn't be read" /></td>
      <td class="file" title="${escapeHtml(track.path)} — tags couldn't be read">${escapeHtml(fileName(track.path))}</td>
      <td class="unreadable-note" colspan="${visibleColumns.length - 1}">couldn't read tags — file left untouched</td>`;
    tracksBody.appendChild(tr);
    return;
  }
  // Diff-state (#117): a staged plan renders its change into this same table in
  // place — no separate Preview view. Staged rows show the new values (dirty)
  // and the sel column becomes the per-row apply scope; other rows recede.
  if (diffByPath) {
    fillDiffRow(tr, track);
    tracksBody.appendChild(tr);
    return;
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
    // Position (#106): a derived, read-only view of the vinyl side notation —
    // not a tag, so it isn't editable and carries no dirty state.
    if (field === "position") {
      const td = document.createElement("td");
      td.className = "position-cell";
      td.textContent = vinylPositionOf(track, pending);
      tr.appendChild(td);
      continue;
    }
    const original = tag(track, field);
    const edited = pending && pending.has(field);
    const value = edited ? pending.get(field) : original;
    const td = document.createElement("td");
    td.className = "editable";
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
  tracksBody.appendChild(tr);
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
      if (field === "position") {
        td.className = "position-cell";
        td.textContent = vinylPositionOf(track, pending);
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
    if (field === "position") {
      const td = document.createElement("td");
      td.className = "position-cell";
      td.textContent = vinylPositionOf(track, pending);
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
function appendGroupHeader(key, count) {
  const collapsed = collapsedGroups.has(key);
  const tr = document.createElement("tr");
  tr.className = "group-head" + (collapsed ? " collapsed" : "");
  tr.dataset.group = key;
  tr.innerHTML = `<td class="group-cell" colspan="${2 + visibleColumns.length}">
      <span class="group-caret">${collapsed ? ico("chevron-right") : ico("caret-down")}</span>
      <span class="group-label">${escapeHtml(groupLabel(key))}</span>
      <span class="group-count muted">· ${count} ${count === 1 ? "file" : "files"}</span>
    </td>`;
  tracksBody.appendChild(tr);
}

function renderTracks() {
  tracksBody.innerHTML = "";
  updateSortIndicators();

  // A staged change always stays visible so the whole plan can be reviewed and
  // scoped, even if the current filter would otherwise hide it (#117).
  const visible = tracks.filter(
    (t) => (diffByPath && diffByPath.has(t.path)) || matchesFilter(t),
  );
  tracksEmpty.hidden = tracks.length > 0;

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
    for (const key of order) {
      appendGroupHeader(key, byKey.get(key).length);
      for (const track of byKey.get(key)) appendTrackRow(track, key);
    }
  } else {
    for (const track of visible) appendTrackRow(track, null);
  }

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
    ? `${groups.length} set(s) · ${fileCount} files`
    : "No duplicates found";
  if (!groups.length) {
    results.innerHTML = `<p class="empty inert-panel">Nothing matched — the library looks clean by this criterion.</p>`;
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
    exitDiffState();
    return;
  }
  enterDiffState();
}

// Enter the in-table diff-state: build the path->change map, tick every changed
// file for apply by default, repaint the table as a diff, and float the bar.
function enterDiffState() {
  setDiffByPath(new Map(previewPlan.changes.map((c) => [c.path, c])));
  setApplySelection(new Set(diffByPath.keys()));
  document.body.classList.add("diffing");
  el("diff-show-old").checked = false;
  el("tracks").classList.remove("show-old");
  showView("files"); // never diff over the dedup view
  renderTracks();
  el("ab-plan").textContent = previewPlan.description ? ` · ${previewPlan.description}` : "";
  el("diff-actionbar").hidden = false;
  updateDiffBar();
}

// Leave the diff-state: drop the plan + apply scope and repaint the plain table.
function exitDiffState() {
  setPreviewPlan(null);
  setPreviewSource(null);
  setDiffByPath(null);
  setApplySelection(new Set());
  document.body.classList.remove("diffing");
  el("tracks").classList.remove("show-old");
  el("diff-actionbar").hidden = true;
  renderTracks();
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

// ---- actions ----
async function openLibrary() {
  const root = rootInput.value.trim();
  if (!root) {
    toast("Enter a library path first", true);
    return;
  }
  try {
    await invoke("open_library", { root });
    setDropFolders(null); // a typed/browsed open is a plain library, not a drop
    await afterOpen(root);
  } catch (e) {
    toast(String(e), true);
  }
}

// Shared tail of every "open a session" path (Open, Browse, or a drag-and-drop):
// pull the track list and reset all per-session view state. `label` is what the
// success toast names. When `dropFolders` is set (a file-set drop, #127) the
// table defaults to drop-origin grouping; otherwise the saved group pref.
async function afterOpen(label) {
  setSessionRoot(label); // the opened/dropped root — folder group labels hang off it
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
  setFilterText("");
  el("filter").value = "";
  syncFilterControls(); // clears the parsed query + any regex-error state
  setGroupBy(dropFolders ? "drop" : groupByPref(), { persist: false, rerender: false });
  renderTracks();
  showView("files");
  showPlayerBar();
  await refreshHistory();
  toast(`Opened ${label} — ${tracks.length} tracks`);
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
    await afterOpen(result.root);
  } catch (e) {
    toast(String(e), true);
  }
}

async function preview() {
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast("Select at least one track", true);
    return;
  }
  try {
    setPreviewPlan(await invoke("preview_rename", { mask: el("mask").value, paths }));
    setPreviewSource("rename");
    renderPreview(previewPlan);
  } catch (e) {
    toast(String(e), true);
  }
}

// FROM NAME's pattern and its replacement table (#141). Both are the user's
// working state for the panel, not backend settings, so they live in
// localStorage beside the other persisted panel prefs — a replacement table
// retyped every session would defeat the point of having one.
const FROM_NAME_MASK_STORAGE_KEY = "tagrex.fromNameMask";
const FROM_NAME_REPL_STORAGE_KEY = "tagrex.fromNameReplacements";
// Separators are what names carry instead of spaces, so the table starts with
// the one everybody needs; it's an ordinary row and can be deleted.
const DEFAULT_REPLACEMENTS = [{ from: "_", to: " " }];
let nameReplacements = DEFAULT_REPLACEMENTS.slice();

function loadFromNamePrefs() {
  try {
    const mask = localStorage.getItem(FROM_NAME_MASK_STORAGE_KEY);
    if (mask) el("from-name-mask").value = mask;
    const stored = localStorage.getItem(FROM_NAME_REPL_STORAGE_KEY);
    if (stored) {
      const rows = JSON.parse(stored);
      // An empty table is a real choice (the user deleted every row), so only
      // a malformed value falls back to the default.
      if (Array.isArray(rows)) {
        nameReplacements = rows.map((r) => ({ from: String(r.from || ""), to: String(r.to || "") }));
      }
    }
  } catch (e) {
    /* unreadable or unavailable — fall back to the defaults */
  }
  renderReplacements();
}

function saveFromNamePrefs() {
  try {
    localStorage.setItem(FROM_NAME_MASK_STORAGE_KEY, el("from-name-mask").value);
    localStorage.setItem(FROM_NAME_REPL_STORAGE_KEY, JSON.stringify(nameReplacements));
  } catch (e) {
    /* localStorage unavailable — preference just won't persist */
  }
}

// One row per replacement, rebuilt whenever the list changes. Typing in a row
// updates the list in place (no re-render, or the caret would jump).
function renderReplacements() {
  const box = el("repl-rows");
  box.innerHTML = nameReplacements
    .map(
      (row, i) => `
      <div class="repl-row" data-index="${i}">
        <input class="repl-from" type="text" placeholder="find" spellcheck="false" value="${escapeHtml(row.from)}" />
        <span class="repl-arrow" aria-hidden="true">→</span>
        <input class="repl-to" type="text" placeholder="replace with" spellcheck="false" value="${escapeHtml(row.to)}" />
        <button class="icon repl-del" type="button" title="Remove" aria-label="Remove"><svg class="ico"><use href="#i-close"/></svg></button>
      </div>`,
    )
    .join("");
  if (!nameReplacements.length) {
    box.innerHTML = `<div class="repl-empty muted">Values go into the tags exactly as the name spells them.</div>`;
  }
}

// FROM NAME (#139): read each selected file's own name back into tags — the
// extract direction of the mask RENAMER renders with. The result is an ordinary
// tag plan, so it lands in the same in-table diff and applies/undoes like any
// other change.
async function previewTagsFromName() {
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast("Select at least one track", true);
    return;
  }
  try {
    const plan = await invoke("preview_tags_from_name", {
      mask: el("from-name-mask").value,
      paths,
      replacements: nameReplacements,
    });
    // Distinguish "nothing to do" from a silent no-op: with this feature an
    // empty plan usually means the pattern doesn't fit the names.
    if (plan.changes.length === 0) {
      toast("No selected name gives this pattern anything new", true);
      return;
    }
    setPreviewPlan(plan);
    setPreviewSource("fromname");
    renderPreview(previewPlan);
  } catch (e) {
    toast(String(e), true);
  }
}

// The live read-out under the pattern box: what the mask pulls out of the first
// selected file, tags-on-disk irrelevant. Extraction stays in Rust — one
// grammar, one implementation (mask.rs) — so this is a per-keystroke round trip,
// debounced by its caller.
async function refreshNameProbe() {
  const box = el("from-name-probe");
  if (!box) return;
  const path = selectedPaths()[0];
  if (!path) {
    box.innerHTML = `<div class="probe-miss">Select a track to see what the pattern reads.</div>`;
    return;
  }
  try {
    const probe = await invoke("probe_tags_from_name", {
      mask: el("from-name-mask").value,
      path,
      replacements: nameReplacements,
    });
    const subject = `<div class="probe-subject">${escapeHtml(probe.subject)}</div>`;
    if (!probe.matched || probe.fields.length === 0) {
      box.innerHTML = `${subject}<div class="probe-miss">This name doesn't fit the pattern.</div>`;
      return;
    }
    const rows = probe.fields
      .map(
        ([field, value]) =>
          `<div class="probe-row"><span class="probe-field">${escapeHtml(columnLabel(field))}</span><span class="probe-value">${escapeHtml(value)}</span></div>`,
      )
      .join("");
    box.innerHTML = subject + rows;
  } catch (e) {
    // A pattern that can't parse at all — say so where the result would be.
    box.innerHTML = `<div class="probe-miss">${escapeHtml(String(e))}</div>`;
  }
}

let nameProbeTimer = null;
function scheduleNameProbe() {
  if (el("subtab-filename").hidden) return;
  clearTimeout(nameProbeTimer);
  nameProbeTimer = setTimeout(refreshNameProbe, 180);
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
    setPreviewPlan(await invoke("preview_tag_edits", { edits: list }));
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
    toast(`Applied changes to ${appliedPlan.changes.length} file(s)`);
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
    // exitDiffState() drops the plan + apply scope and repaints the plain table.
    exitDiffState();
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
    // exitDiffState() also clears previewPlan/previewSource and repaints.
    exitDiffState();
    await refreshHistory();
  } catch (e) {
    toast(String(e), true);
  }
}

// ---- cover art ----
function chooseCover() {
  if (selectedPaths().length === 0) {
    toast("Select the tracks to embed the cover into first", true);
    return;
  }
  coverFileInput.value = ""; // allow re-picking the same file
  coverFileInput.click();
}

async function onCoverChosen() {
  const file = coverFileInput.files[0];
  if (file) await embedCoverFile(file);
}

// Read an image File, base64-encode it, and preview embedding it as the front
// cover of the selection. Used by the file picker and the well's HTML5 drop
// (browser-dev). In the packaged app a dropped cover arrives as a path instead
// — see embedCoverFromPath (#133).
async function embedCoverFile(file) {
  if (!file.type.startsWith("image/")) {
    toast("Drop an image file", true);
    return;
  }
  if (selectedPaths().length === 0) {
    toast("Select the tracks to embed the cover into first", true);
    return;
  }
  const dataUrl = await new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result);
    reader.onerror = reject;
    reader.readAsDataURL(file);
  });
  // dataUrl is "data:<mime>;base64,<data>"
  const comma = dataUrl.indexOf(",");
  const mime = dataUrl.slice(5, dataUrl.indexOf(";"));
  const data_base64 = dataUrl.slice(comma + 1);
  await embedCoverDto({ mime, data_base64 });
}

// Embed an image dropped onto the cover well in the packaged app (#133): the
// drag-drop event gives a path, which the backend reads into a cover DTO.
async function embedCoverFromPath(path) {
  if (selectedPaths().length === 0) {
    toast("Select the tracks to embed the cover into first", true);
    return;
  }
  try {
    const cover = await invoke("read_cover_image", { path });
    await embedCoverDto(cover);
  } catch (e) {
    toast(String(e), true);
  }
}

// Preview embedding a cover DTO ({ mime, data_base64 }) as the front cover of
// the current selection — the shared tail of every cover-embed source (picker,
// HTML5 drop, native drop, and the release card).
async function embedCoverDto(cover) {
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast("Select the tracks to embed the cover into first", true);
    return;
  }
  try {
    setPreviewPlan(await invoke("preview_cover_embed", { paths, cover }));
    setPreviewSource("cover");
    renderPreview(previewPlan);
    toast(
      previewPlan.changes.length
        ? `Previewing cover on ${previewPlan.changes.length} file(s)`
        : "Selected files already have this cover"
    );
  } catch (e) {
    toast(String(e), true);
  }
}

// Preview removing the embedded cover from the selection, through the normal
// preview/apply/undo path.
async function previewCoverRemove() {
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast("Select the tracks whose cover to remove first", true);
    return;
  }
  try {
    setPreviewPlan(await invoke("preview_cover_remove", { paths }));
    setPreviewSource("cover");
    renderPreview(previewPlan);
    toast(
      previewPlan.changes.length
        ? `Previewing cover removal on ${previewPlan.changes.length} file(s)`
        : "None of the selected files have a cover"
    );
  } catch (e) {
    toast(String(e), true);
  }
}

// Preview wiping every text tag from the selection for a fresh start (#94),
// through the normal preview/apply/undo path. The cover and DJ cue points are
// kept; the diff is the review, and undo reverses it.
async function previewClearTags() {
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast("Select the tracks whose tags to clear first", true);
    return;
  }
  try {
    setPreviewPlan(await invoke("preview_clear_tags", { paths }));
    setPreviewSource("clear");
    renderPreview(previewPlan);
    toast(
      previewPlan.changes.length
        ? `Previewing tag clear on ${previewPlan.changes.length} file(s)`
        : "None of the selected files have tags to clear"
    );
  } catch (e) {
    toast(String(e), true);
  }
}

// ---- cover well (#editor design pass) ----
// A thumbnail + state + actions that replaces the two bare Embed/Export buttons.
// Reflects the selection's cover state: none / one shared / mixed.
async function refreshCoverWell() {
  const paths = selectedPaths();
  if (paths.length === 0) {
    coverWell.className = "cover-well empty";
    coverWell.innerHTML = `<div class="cover-thumb inert"></div>
      <div class="cover-body"><div class="cover-title">No selection</div>
      <div class="cover-hint">Select tracks to edit their cover.</div></div>`;
    return;
  }
  try {
    const summary = await invoke("read_cover_summary", { paths });
    renderCoverWell(summary);
    await showExternalCoverAction(paths);
  } catch (e) {
    toast(String(e), true);
  }
}

// The external cover (folder.jpg/cover.jpg next to the tracks), when present —
// offered as a one-click embed under the well (#41).
let externalCover = null;

async function showExternalCoverAction(paths) {
  externalCover = null;
  let found;
  try {
    found = await invoke("read_external_cover", { paths });
  } catch (e) {
    return; // best-effort; the well already rendered
  }
  if (!found) return;
  externalCover = found;
  // Join the action row so it flows with Replace…/Remove/Export as an equal
  // button (#134), not a full-width slab. Falls back to the body in the no-cover
  // state, which has no action row.
  const host = coverWell.querySelector(".cover-actions") || coverWell.querySelector(".cover-body");
  if (!host) return;
  const note = document.createElement("button");
  note.className = "btn cover-external";
  note.dataset.cover = "external";
  note.textContent = "Use folder image";
  note.title = "Embed the cover.jpg / folder.jpg sitting next to these tracks";
  host.appendChild(note);
}

// Embed the external cover file (folder.jpg/cover.jpg) into the selection (#41),
// through the same preview/apply/undo path as any other cover.
async function embedExternalCover() {
  if (!externalCover) return;
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast("Select the tracks to embed the cover into first", true);
    return;
  }
  try {
    setPreviewPlan(await invoke("preview_cover_embed", { paths, cover: externalCover }));
    setPreviewSource("cover");
    renderPreview(previewPlan);
    toast(
      previewPlan.changes.length
        ? `Previewing folder image on ${previewPlan.changes.length} file(s) — click Apply`
        : "Selected files already have this cover",
    );
  } catch (e) {
    toast(String(e), true);
  }
}

function coverThumbImg(cover, cls) {
  return `<img class="cover-thumb${cls ? " " + cls : ""}" alt="front cover" src="data:${cover.mime};base64,${cover.data_base64}" />`;
}

function renderCoverWell(summary) {
  const { total, with_cover, distinct, samples } = summary;
  const n = total;
  const drop = `<div class="cover-drop-cue">Drop image to embed in ${n} file(s)</div>`;

  if (with_cover === 0) {
    // No cover anywhere — the well itself is the click/drop target.
    coverWell.className = "cover-well empty";
    coverWell.innerHTML = `<div class="cover-thumb inert"></div>
      <div class="cover-body">
        <div class="cover-title">No cover</div>
        ${drop}
        <div class="cover-hint"><b>Embed cover…</b><br>or drag an image here</div>
      </div>`;
    return;
  }

  if (!distinct && samples.length === 1) {
    // One cover shared across the whole selection.
    coverWell.className = "cover-well";
    coverWell.innerHTML = `${coverThumbImg(samples[0])}
      <div class="cover-body">
        <div class="cover-title">Front cover</div>
        <div class="cover-meta">shared across ${n} file(s)</div>
        ${drop}
        <div class="cover-actions">
          <button class="btn" data-cover="replace">Replace…</button>
          <button class="btn" data-cover="remove">Remove</button>
          <button class="btn" data-cover="export">Export</button>
        </div>
      </div>`;
    return;
  }

  // Mixed — files carry different covers (or some have none). A small fan, never
  // implying one shared image.
  const fan = samples.map((c) => coverThumbImg(c)).join("");
  coverWell.className = "cover-well";
  coverWell.innerHTML = `<div class="cover-stack">${fan || '<div class="cover-thumb inert"></div>'}</div>
    <div class="cover-body">
      <div class="cover-title">Multiple covers</div>
      <div class="cover-meta">${with_cover}/${n} with a cover</div>
      ${drop}
      <div class="cover-actions">
        <button class="btn" data-cover="replace">Set one…</button>
        <button class="btn" data-cover="remove">Remove all</button>
        <button class="btn" data-cover="export">Export</button>
      </div>
    </div>`;
}

// Export the embedded cover of the selected files to disk (cover.<ext> next to
// each file). Read-only for the audio: no preview/apply, it just writes.
async function exportCover() {
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast("Select the tracks whose cover to export first", true);
    return;
  }
  try {
    const result = await invoke("export_cover", { paths, basename: "cover" });
    const wrote = result.written.length;
    const skipped = result.skipped_no_cover.length;
    if (wrote === 0) {
      toast(
        skipped ? "None of the selected files have an embedded cover" : "Nothing to export",
        true
      );
      return;
    }
    const skipNote = skipped ? ` (${skipped} without a cover skipped)` : "";
    toast(`Exported ${wrote} cover file(s)${skipNote}`);
  } catch (e) {
    toast(String(e), true);
  }
}

// ---- reorganize into folders (#37) ----
// Builds the plan and shows it in the usual preview view, so the move is
// applied (and undone) through exactly the same path as a rename.
async function previewMove() {
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast("Select the tracks to move first", true);
    return;
  }
  try {
    setPreviewPlan(await invoke("preview_move", { mask: el("move-mask").value, paths }));
    setPreviewSource("rename");
    renderPreview(previewPlan);
    toast(
      previewPlan.changes.length
        ? `Previewing move of ${previewPlan.changes.length} file(s) — click Apply`
        : "Nothing to move (check the pattern's tags are set)",
      previewPlan.changes.length === 0
    );
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
el("open").addEventListener("click", openLibrary);
el("browse").addEventListener("click", browseForFolder);
// FROM NAME's pattern + replacement table come back from the last session (#141).
loadFromNamePrefs();

// ---- drag-and-drop onto the window to open folders/files (#127) ----
// Tauri v2 intercepts OS file drops (dragDropEnabled) and re-emits them as
// window events carrying absolute paths, so we listen for those rather than
// HTML5 file DnD (which the webview suppresses). Enter/over/leave toggle the
// drop-cue overlay; the drop hands the paths to the backend resolver.
function showDropCue(on) {
  document.body.classList.toggle("drag-active", on);
}

function isImagePath(p) {
  return /\.(jpe?g|png|webp|gif|bmp|tiff?|avif|heic)$/i.test(p);
}

(function initWindowDrop() {
  const event = window.__TAURI__ && window.__TAURI__.event;
  if (event) {
    event.listen("tauri://drag-enter", () => showDropCue(true));
    event.listen("tauri://drag-over", () => showDropCue(true));
    event.listen("tauri://drag-leave", () => showDropCue(false));
    event.listen("tauri://drag-drop", (e) => {
      showDropCue(false);
      const paths = (e && e.payload && e.payload.paths) || [];
      // A single dropped image has only one meaning — embed it as the cover of
      // the selection (#133). No position hit-test: an image can't be "opened"
      // as a library, so this is unambiguous and doesn't depend on fragile
      // physical/logical-pixel coordinate conversion. Everything else opens.
      if (paths.length === 1 && isImagePath(paths[0])) {
        embedCoverFromPath(paths[0]);
        return;
      }
      openDrop(paths);
    });
    return;
  }
  // Browser dev (no native shell): the OS can't hand us real paths, but wiring
  // HTML5 DnD still lets the overlay and open flow be exercised against the
  // mock. Drops on the cover well keep their own handler.
  window.addEventListener("dragover", (e) => {
    if (e.target.closest("#cover-well")) return;
    e.preventDefault();
    showDropCue(true);
  });
  window.addEventListener("dragleave", (e) => {
    if (e.relatedTarget === null) showDropCue(false);
  });
  window.addEventListener("drop", (e) => {
    if (e.target.closest("#cover-well")) return;
    e.preventDefault();
    showDropCue(false);
    openDrop(Array.from(e.dataTransfer.files).map((f) => f.name));
  });
})();
previewBtn.addEventListener("click", preview);
previewEditsBtn.addEventListener("click", previewEdits);
applyBtn.addEventListener("click", apply);
undoBtn.addEventListener("click", undo);
// Cover well: action buttons (delegated) + drag-and-drop embed.
coverWell.addEventListener("click", (e) => {
  const btn = e.target.closest("[data-cover]");
  const act = btn ? btn.dataset.cover : coverWell.classList.contains("empty") ? "replace" : null;
  if (act === "replace") chooseCover();
  else if (act === "remove") previewCoverRemove();
  else if (act === "export") exportCover();
  else if (act === "external") embedExternalCover();
});
coverWell.addEventListener("dragover", (e) => {
  e.preventDefault();
  coverWell.classList.add("dragover");
});
coverWell.addEventListener("dragleave", (e) => {
  if (!coverWell.contains(e.relatedTarget)) coverWell.classList.remove("dragover");
});
coverWell.addEventListener("drop", (e) => {
  e.preventDefault();
  coverWell.classList.remove("dragover");
  const file = e.dataTransfer.files[0];
  if (file) embedCoverFile(file);
});
el("transform-add").addEventListener("click", addTransformRule);
el("transform-preview").addEventListener("click", previewTransform);
el("autonum-run").addEventListener("click", numberTracks);
el("vinyl-split").addEventListener("click", splitVinylSides);
// Rule reorder is wired per-card in renderTransformRules via enablePointerReorder
// (grip drag), with ↑/↓ as the fallback — no container-level HTML5 DnD (#88).
el("move-preview").addEventListener("click", previewMove);

// FROM NAME (#139): the probe follows the pattern as it's typed.
el("from-name-preview").addEventListener("click", previewTagsFromName);
el("from-name-mask").addEventListener("input", () => {
  saveFromNamePrefs();
  scheduleNameProbe();
});

// The replacement table (#141). Typing edits the row in place — re-rendering on
// every keystroke would move the caret — and the list is what both the probe
// and the preview send.
el("repl-rows").addEventListener("input", (e) => {
  const row = e.target.closest(".repl-row");
  if (!row) return;
  const entry = nameReplacements[Number(row.dataset.index)];
  if (!entry) return;
  if (e.target.classList.contains("repl-from")) entry.from = e.target.value;
  if (e.target.classList.contains("repl-to")) entry.to = e.target.value;
  saveFromNamePrefs();
  scheduleNameProbe();
});
el("repl-rows").addEventListener("click", (e) => {
  const del = e.target.closest(".repl-del");
  if (!del) return;
  nameReplacements.splice(Number(del.closest(".repl-row").dataset.index), 1);
  renderReplacements();
  saveFromNamePrefs();
  scheduleNameProbe();
});
el("repl-add").addEventListener("click", () => {
  nameReplacements.push({ from: "", to: "" });
  renderReplacements();
  el("repl-rows").querySelector(".repl-row:last-child .repl-from")?.focus();
});
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
coverFileInput.addEventListener("change", onCoverChosen);
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
    toast("Type a library path, then press Open");
    rootInput.focus();
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
  setSortKey(null); // manual order supersedes any column sort
  renderTracks();
}
rootInput.addEventListener("keydown", (e) => e.key === "Enter" && openLibrary());
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
  for (const tr of dataRows()) {
    if (on) selection.add(tr.dataset.path);
    else selection.delete(tr.dataset.path);
  }
  syncSelectionUI();
});
// Direct checkbox clicks feed the selection set — or, while diffing, the
// per-row apply scope (the sel column's meaning switches in diff-state, #117).
tracksBody.addEventListener("change", (e) => {
  const cb = e.target.closest(".sel input[type=checkbox]");
  if (!cb) return;
  if (diffByPath) {
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

// Data rows in DOM (visual) order — group headers and unreadable (inert)
// rows excluded, so selection/select-all never touches a file that can't be
// operated on.
function dataRows() {
  return [...tracksBody.querySelectorAll("tr")].filter(
    (tr) =>
      tr.dataset.path &&
      !tr.classList.contains("group-head") &&
      !tr.classList.contains("unreadable"),
  );
}

// Push the `selection` set onto the checkboxes + row highlight, set the
// select-all tri-state, and refresh the status count. Called after any change.
function syncSelectionUI() {
  const rows = dataRows();
  let checked = 0;
  for (const tr of rows) {
    const on = selection.has(tr.dataset.path);
    rowCheckbox(tr).checked = on;
    tr.classList.toggle("selected", on);
    if (on) checked += 1;
  }
  selectAll.checked = checked > 0 && checked === rows.length;
  selectAll.indeterminate = checked > 0 && checked < rows.length;
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
  }
}

// Selection-dependent counts in the GENERATOR/EXPORTER panel headings. Cheap
// text updates safe to run on every selection change (the panels stay in the
// DOM even while hidden); the full panel refresh still happens on mode entry.
function updatePanelCounts() {
  const count = selectedPaths().length;
  el("transform-count").textContent = count ? `— ${count} file(s)` : "";
  el("autonum-count").textContent = count ? `— ${count} selected` : "";
  el("vinyl-count").textContent = count ? `— ${count} selected` : "";
  el("export-count").textContent = count ? `— ${count} track(s)` : "";
}

function selectRow(tr, e) {
  if (tr.classList.contains("unreadable")) return; // inert — can't be selected
  const rows = dataRows();
  const path = tr.dataset.path;
  if (e.shiftKey && selAnchor) {
    const paths = rows.map((r) => r.dataset.path);
    let a = paths.indexOf(selAnchor);
    let b = paths.indexOf(path);
    if (a < 0) a = b;
    if (a > b) [a, b] = [b, a];
    selection.clear();
    for (let i = a; i <= b; i++) selection.add(paths[i]);
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
  const rows = dataRows();
  const paths = rows.map((r) => r.dataset.path);
  const idx = [];
  rows.forEach((r, i) => {
    if (r.dataset.group === key) idx.push(i);
  });
  if (idx.length === 0) return;
  const gStart = idx[0];
  const gEnd = idx[idx.length - 1];
  if (mode === "range") {
    let a = selAnchor ? paths.indexOf(selAnchor) : -1;
    if (a < 0) a = gStart;
    const lo = Math.min(a, gStart);
    const hi = Math.max(a, gEnd);
    selection.clear();
    for (let i = lo; i <= hi; i++) selection.add(paths[i]);
  } else if (mode === "add") {
    const allSelected = idx.every((i) => selection.has(paths[i]));
    for (const i of idx) {
      if (allSelected) selection.delete(paths[i]);
      else selection.add(paths[i]);
    }
    selAnchor = paths[gStart];
  } else {
    selection.clear();
    for (const i of idx) selection.add(paths[i]);
    selAnchor = paths[gStart];
  }
  syncSelectionUI();
}

// Enter edit mode on a cell and select its text (double-click, per the hint).
function beginCellEdit(cell) {
  cell.contentEditable = "true";
  cell.focus();
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
  if (cell) {
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

// Visible data rows (group headers and collapsed rows excluded).
function navRows() {
  return dataRows().filter((tr) => !tr.classList.contains("hidden-row"));
}

// Keep exactly one row tabbable; called after every render.
function refreshRoving() {
  const rows = navRows();
  if (rows.length === 0) {
    setActiveRowPath(null);
    return;
  }
  if (!rows.some((r) => r.dataset.path === activeRowPath)) setActiveRowPath(rows[0].dataset.path);
  for (const r of dataRows()) r.tabIndex = r.dataset.path === activeRowPath ? 0 : -1;
}

function setActiveRow(tr, focus) {
  setActiveRowPath(tr ? tr.dataset.path : null);
  for (const r of dataRows()) r.tabIndex = r.dataset.path === activeRowPath ? 0 : -1;
  if (tr && focus) tr.focus();
}

tracksBody.addEventListener("keydown", (e) => {
  // Don't hijack keys while editing a cell or typing in a control.
  if (e.target.isContentEditable || e.target.matches("input, textarea, select")) return;
  const tr = e.target.closest("tr");
  if (!tr || !tr.dataset.path) return;
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    const rows = navRows();
    const i = rows.indexOf(tr);
    const next = rows[e.key === "ArrowDown" ? i + 1 : i - 1];
    if (next) setActiveRow(next, true);
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

// ---- resize the table / mode-panel split by dragging the divider ----
// Mouse events (not a native splitter) for the same WKWebView reason as the row
// reorder. The panel has a fixed flex-basis; dragging sets it in pixels.
(function initSplitter() {
  const splitter = el("col-splitter");
  const modeCol = document.querySelector(".mode-col");
  const workarea = document.querySelector(".workarea");
  let dragging = false;

  splitter.addEventListener("mousedown", (e) => {
    e.preventDefault();
    dragging = true;
    document.body.classList.add("resizing");
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  });

  function onMove(e) {
    if (!dragging) return;
    // Panel width = distance from the cursor to the right edge of the work area,
    // clamped so neither column collapses.
    const rect = workarea.getBoundingClientRect();
    const width = Math.min(Math.max(rect.right - e.clientX, 240), rect.width - 360);
    modeCol.style.flexBasis = `${Math.round(width)}px`;
  }

  function onUp() {
    dragging = false;
    document.body.classList.remove("resizing");
    document.removeEventListener("mousemove", onMove);
    document.removeEventListener("mouseup", onUp);
  }

  // Keep the panel within the work area when the window shrinks (#109). The
  // splitter enforces this on drag, but without a resize clamp a panel that was
  // wide (or the default 480 on a narrow window) is pushed off the right edge,
  // clipping its toolbar. Mirror the splitter's clamp; only ever narrow.
  function clampPanel() {
    if (document.body.classList.contains("panel-collapsed")) return;
    const rect = workarea.getBoundingClientRect();
    if (rect.width === 0) return;
    const max = Math.max(240, rect.width - 360);
    if (modeCol.getBoundingClientRect().width > max) {
      modeCol.style.flexBasis = `${Math.round(max)}px`;
    }
  }
  window.addEventListener("resize", clampPanel);
  clampPanel(); // in case the initial window is narrower than the default panel
})();

// ---- resize a table column by dragging its header grip (#76) ----
// Delegated on the header (mousedown), because the sortable ths are rebuilt on
// every column change (#43). Dragging past a threshold suppresses the header's
// sort click. Same manual-mouse approach as the panel splitter (WKWebView).
(function initColumnResize() {
  const thead = el("tracks").querySelector("thead");
  let key = null;
  let startX = 0;
  let startWidth = 0;
  let moved = false;
  let th = null;

  thead.addEventListener("mousedown", (e) => {
    const grip = e.target.closest(".col-resize");
    if (!grip) return;
    e.preventDefault();
    e.stopPropagation(); // don't let the header treat this as a sort click
    key = grip.dataset.key;
    th = grip.closest("th");
    startX = e.clientX;
    startWidth = th.getBoundingClientRect().width;
    moved = false;
    document.body.classList.add("resizing-col");
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  });

  function onMove(e) {
    if (key === null) return;
    const width = Math.max(startWidth + (e.clientX - startX), COLUMN_MIN_WIDTH);
    if (Math.abs(e.clientX - startX) > 2) moved = true;
    columnWidths[key] = Math.round(width);
    if (th) th.style.width = `${columnWidths[key]}px`;
  }

  function onUp() {
    if (key === null) return;
    // A grip drag that never moved is a stray click — don't persist or block sort.
    if (moved) saveColumnWidths();
    key = null;
    th = null;
    document.body.classList.remove("resizing-col");
    document.removeEventListener("mousemove", onMove);
    document.removeEventListener("mouseup", onUp);
  }

  // Double-click a grip to reset that column to its default width.
  thead.addEventListener("dblclick", (e) => {
    const grip = e.target.closest(".col-resize");
    if (!grip) return;
    e.preventDefault();
    e.stopPropagation();
    delete columnWidths[grip.dataset.key];
    saveColumnWidths();
    renderTableHead();
  });
})();

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
  tracks.sort(
    (a, b) =>
      sortValue(a, key).localeCompare(sortValue(b, key), undefined, { numeric: true }) * sortDir,
  );
  renderTracks();
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

// Collapse/expand a group by clicking its header (no re-render, so selection
// and in-progress edits are preserved).
function toggleGroup(key) {
  const collapse = !collapsedGroups.has(key);
  if (collapse) collapsedGroups.add(key);
  else collapsedGroups.delete(key);
  tracksBody.querySelectorAll("tr").forEach((tr) => {
    if (tr.dataset.group !== key) return;
    if (tr.classList.contains("group-head")) {
      tr.classList.toggle("collapsed", collapse);
      const caret = tr.querySelector(".group-caret");
      if (caret) caret.innerHTML = ico(collapse ? "chevron-right" : "caret-down");
    } else {
      tr.classList.toggle("hidden-row", collapse);
    }
  });
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

// Expand/collapse every group at once (#32), reusing the same in-place update
// as individual headers so selection and in-progress edits survive.
function setAllGroupsCollapsed(collapse) {
  collapsedGroups.clear();
  if (collapse) {
    tracksBody
      .querySelectorAll("tr.group-head")
      .forEach((head) => collapsedGroups.add(head.dataset.group));
  }
  tracksBody.querySelectorAll("tr").forEach((tr) => {
    if (tr.dataset.group === undefined) return;
    if (tr.classList.contains("group-head")) {
      tr.classList.toggle("collapsed", collapse);
      const caret = tr.querySelector(".group-caret");
      if (caret) caret.innerHTML = ico(collapse ? "chevron-right" : "caret-down");
    } else {
      tr.classList.toggle("hidden-row", collapse);
    }
  });
}
// The two Expand all / Collapse all buttons collapsed into one state toggle:
// while any group is still open it offers "collapse", and once everything is
// shut it flips to "expand". Icon + tooltip are re-derived from the DOM rather
// than a remembered flag, so per-group clicks keep it honest.
function syncGroupToggle() {
  const btn = el("toggle-groups");
  if (!btn || btn.hidden) return;
  const heads = [...tracksBody.querySelectorAll("tr.group-head")];
  const anyOpen = heads.some((h) => !h.classList.contains("collapsed"));
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

el("filter").addEventListener("input", (e) => {
  // Keep the raw text — case matters in regex/case-sensitive mode; lowercasing
  // for a plain match happens in matchesFilter.
  setFilterText(e.target.value.trim());
  syncFilterControls();
  renderTracks();
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
// Action-groups popover (#57): same toggle + outside-click close as presets.
el("groups-btn").addEventListener("click", (e) => {
  e.stopPropagation();
  const menu = el("groups-menu");
  if (menu.hidden) renderGroupsMenu();
  menu.hidden = !menu.hidden;
});
document.addEventListener("click", (e) => {
  const menu = el("groups-menu");
  if (!menu.hidden && !menu.contains(e.target) && !el("groups-btn").contains(e.target)) {
    menu.hidden = true;
  }
});
// Track edits on any editable cell (event delegation).
tracksBody.addEventListener("input", (e) => {
  if (e.target.classList.contains("editable")) onCellEdit(e.target);
});
// Enter commits a cell instead of inserting a newline, and leaves edit mode.
tracksBody.addEventListener("keydown", (e) => {
  if (e.target.classList.contains("editable") && e.key === "Enter") {
    e.preventDefault();
    e.target.contentEditable = "false";
    e.target.blur();
  }
});

loadSavedToken();

// Apply the saved column choice (#43) and build the header before any library
// is opened.
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
