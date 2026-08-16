// The table's columns and its grouping menu (#43, #143 split them out of
// app.js).
//
// Which columns exist, what they are called, and the popover that picks and
// reorders them; plus the grouping selector, which shares the same field list.
// The chosen columns and their widths live in the state module, since the table
// renderer reads them on every paint.
import { el, escapeHtml, ico, toast } from "./dom.js";
import { invoke } from "./invoke.js";
import { hooks } from "./hooks.js";
import { EXTENDED_FIELDS, VIRTUAL_COLUMNS } from "./fields.js";
import { placeholderToken } from "./placeholders.js";
import { groupByPref, saveGroupBy } from "./prefs.js";
import { enablePointerReorder } from "./reorder.js";
import {
  DEFAULT_COLUMNS,
  collapsedGroups,
  columnWidths,
  diffByPath,
  groupBy,
  setColumnWidths,
  setGroupByValue,
  setVisibleColumns,
  tracks,
  visibleColumns,
} from "./state.js";

// Column persistence keys and the narrowest a column may be dragged.
//
// These live HERE, with the code that uses them, and are not re-declared
// anywhere else. They were left behind in app.js by the #143 split while all
// their users moved out, which made every reference below an undefined
// identifier — silently, because the try/catch around each localStorage call
// swallowed the ReferenceError. The whole column configuration stopped
// persisting and header-grip resizing stopped working (#155).
const COLUMNS_STORAGE_KEY = "tagrex.columns";
const COLUMN_WIDTHS_STORAGE_KEY = "tagrex.colWidths";
const COLUMN_MIN_WIDTH = 48;

// ---- configurable columns (#43) ----
// "file" plus every modeled tag field, the pool the user picks columns from.
function allColumnKeys() {
  return [
    "file",
    ...EXTENDED_FIELDS.map(([key]) => key),
    ...VIRTUAL_COLUMNS.map(([key]) => key),
    ...customColumns.map((c) => customColumnKey(c.id)),
  ];
}

function columnLabel(key) {
  if (key === "file") return "File";
  const custom = customColumnOf(key);
  if (custom) return custom.name;
  const found =
    EXTENDED_FIELDS.find(([k]) => k === key) || VIRTUAL_COLUMNS.find(([k]) => k === key);
  return found ? found[1] : key;
}

// Build the "Group by" options (#43): the fixed groupings (Folder, Release id)
// plus every modeled tag field, so any column groups the table like the built-in
// ones. "By drop" is set only by a file-set drag-and-drop, so it's a hidden
// option. Also validates the persisted choice against the built list — an old or
// unknown key falls back to Folder (never the drop grouping, which is transient).
// Keys promoted above the fold in the group menu — the ones actually reached
// for. Everything else stays available below a separator, in field order.
const GROUP_COMMON = ["", "folder", "release", "artist", "album", "albumartist"];

// NB: distinct from groupLabel() below, which names a group HEADER (a folder or
// tag value). This one names a grouping KEY for the menu/tooltip.
function groupKeyLabel(value) {
  if (value === "") return "None";
  if (value === "folder") return "Folder";
  if (value === "release") return "Release id";
  if (value === "drop") return "By drop";
  const field = EXTENDED_FIELDS.find(([key]) => key === value);
  return field ? field[1] : value;
}

function populateGroupMenu() {
  const menu = el("group-menu");
  const rest = EXTENDED_FIELDS.filter(([key]) => !GROUP_COMMON.includes(key));
  const row = (value) =>
    `<button type="button" class="col-menu-row tk-menu-item group-opt" data-group="${escapeHtml(value)}">${escapeHtml(groupKeyLabel(value))}</button>`;
  menu.innerHTML =
    GROUP_COMMON.map(row).join("") +
    `<div class="col-menu-sep"></div>` +
    rest.map(([key]) => row(key)).join("");
  // "drop" is set by the drag-drop flow, never picked here, so it isn't listed;
  // any other unknown persisted value falls back to Folder.
  const selectable = new Set([...GROUP_COMMON, ...EXTENDED_FIELDS.map(([k]) => k)]);
  if (groupBy !== "drop" && !selectable.has(groupBy)) {
    setGroupByValue("folder");
    saveGroupBy(groupBy);
  }
  syncGroupButton();
}

// Reflect the current key onto the button + the menu's checkmarks. The button
// tints whenever grouping is on, so "am I grouped?" is answerable at a glance.
function syncGroupButton() {
  const btn = el("group-btn");
  const label = `Group by: ${groupKeyLabel(groupBy)}`;
  btn.title = label;
  btn.setAttribute("aria-label", label);
  btn.classList.toggle("active", groupBy !== "");
  el("group-menu")
    .querySelectorAll(".group-opt")
    .forEach((b) => b.classList.toggle("checked", b.dataset.group === groupBy));
}

// One way in for every grouping change (menu pick, preset apply, library open),
// so the button, the menu and the persisted pref can't drift apart.
function setGroupBy(value, { persist = true, rerender = true } = {}) {
  setGroupByValue(value);
  if (persist) saveGroupBy(groupBy);
  collapsedGroups.clear();
  syncGroupButton();
  if (rerender) hooks.renderTracks();
}

// Sensible starting width for a column the user hasn't resized yet. The file
// name is the widest; short numeric/code fields start narrow.
function defaultColumnWidth(key) {
  if (key === "file") return 240;
  if (["year", "track", "tracktotal", "disc", "bpm", "key", "length"].includes(key)) return 70;
  if (["artist", "title", "album", "albumartist", "composer"].includes(key)) return 160;
  return 130;
}

function columnWidth(key) {
  return columnWidths[key] || defaultColumnWidth(key);
}

// ---- mask-defined columns (#150) ----
//
// A column whose value is a mask rendered per row, so a derived value — "artist
// — title", a catalogue number beside the year, the bitrate — can be a column
// without being a tag.
//
// Rendering happens in Rust: the mask engine is one grammar serving both
// directions (architecture.md), and a renderer reimplemented here would be a
// second one drifting from it. A round-trip per cell per repaint is impossible,
// so the whole column is rendered in one call and cached against the paths,
// refreshed when the pattern or the track list changes.
const CUSTOM_COLUMNS_STORAGE_KEY = "tagrex.customColumns";
const CUSTOM_PREFIX = "custom:";

// [{ id, name, pattern, align }]. `id` is only ever generated here, so it stays
// a safe suffix for the column key.
let customColumns = [];
// id -> Map<path, rendered>. Dropped wholesale when a definition changes.
const customValues = new Map();

function loadCustomColumns() {
  const saved = readStoredJson(CUSTOM_COLUMNS_STORAGE_KEY);
  if (!Array.isArray(saved)) return;
  customColumns = saved
    .filter((c) => c && typeof c.id === "string" && typeof c.pattern === "string")
    .map((c) => ({
      id: c.id,
      name: typeof c.name === "string" && c.name ? c.name : c.pattern,
      pattern: c.pattern,
      align: ["left", "center", "right"].includes(c.align) ? c.align : "left",
    }));
}

function saveCustomColumns() {
  writeStored(CUSTOM_COLUMNS_STORAGE_KEY, JSON.stringify(customColumns));
}

function customColumnKey(id) {
  return CUSTOM_PREFIX + id;
}

function customColumnOf(key) {
  if (!key.startsWith(CUSTOM_PREFIX)) return null;
  const id = key.slice(CUSTOM_PREFIX.length);
  return customColumns.find((c) => c.id === id) || null;
}

// The rendered value for one row, or "" until the batch answers.
function customColumnValue(key, path) {
  const column = customColumnOf(key);
  if (!column) return "";
  return customValues.get(column.id)?.get(path) ?? "";
}

// Render every visible custom column over `paths`, one call each. Skipped
// entirely when no custom column is shown, which is the normal case.
async function refreshCustomColumns(paths) {
  const shown = visibleColumns.map(customColumnOf).filter(Boolean);
  if (!shown.length || !paths.length) return false;
  let changed = false;
  for (const column of shown) {
    const cached = customValues.get(column.id);
    // Nothing to do when the cache already covers exactly these paths.
    if (cached && cached.size === paths.length && paths.every((p) => cached.has(p))) continue;
    try {
      const rendered = await invoke("render_column", { pattern: column.pattern, paths });
      customValues.set(column.id, new Map(paths.map((p, i) => [p, rendered[i] ?? ""])));
      changed = true;
    } catch (e) {
      // A pattern the parser refuses (a typo, or half-typed) leaves the column
      // blank rather than blocking the paint or spamming a toast per repaint.
      customValues.set(column.id, new Map());
      changed = true;
    }
  }
  return changed;
}

// A definition changed: its cached values are meaningless now.
function invalidateCustomColumn(id) {
  customValues.delete(id);
}

// The library was re-read, so every cached value is suspect. After an apply the
// paths are often IDENTICAL while the tags behind them are not, which is the one
// case the "cache already covers these paths" check cannot catch (#150).
function invalidateCustomColumns() {
  customValues.clear();
}

// Render whatever the visible mask columns need for the current track list and
// repaint if anything arrived. The one call site for "make the columns current".
async function refreshCustomColumnCells() {
  if (await refreshCustomColumns(tracks.map((t) => t.path))) hooks.renderTracks();
}

function upsertCustomColumn(column) {
  const at = customColumns.findIndex((c) => c.id === column.id);
  if (at >= 0) customColumns[at] = column;
  else customColumns.push(column);
  invalidateCustomColumn(column.id);
  saveCustomColumns();
}

function removeCustomColumn(id) {
  customColumns = customColumns.filter((c) => c.id !== id);
  invalidateCustomColumn(id);
  saveCustomColumns();
  applyColumns(visibleColumns.filter((k) => k !== customColumnKey(id)));
  renderColumnsMenu();
}

// ---- the mask-column editor (#150) ----
//
// One modal serving both "add" and "edit": which it is depends on whether it was
// opened with an existing column. Ids are minted here and never come from
// storage input, so a column key stays a safe string.
let editingColumn = null;
let editingAlign = "left";

function setColumnAlign(align) {
  editingAlign = align;
  el("colmask-align")
    .querySelectorAll(".seg-btn")
    .forEach((b) => b.classList.toggle("active", b.dataset.align === align));
}

function openColumnEditor(column) {
  editingColumn = column;
  el("colmask-title").textContent = column ? "Edit column" : "Add column";
  el("colmask-ok").textContent = column ? "Save" : "Add column";
  el("colmask-name").value = column ? column.name : "";
  el("colmask-pattern").value = column ? column.pattern : "";
  setColumnAlign(column ? column.align : "left");
  el("colmask-modal").hidden = false;
  el("colmask-pattern").focus();
}

function closeColumnEditor() {
  el("colmask-modal").hidden = true;
  editingColumn = null;
}

async function commitColumnEditor() {
  const pattern = el("colmask-pattern").value.trim();
  if (!pattern) {
    toast("A column needs a pattern", true);
    return;
  }
  // Check the pattern before storing it, so a typo is reported once, here,
  // rather than silently blanking the column on every repaint.
  try {
    await invoke("render_column", { pattern, paths: [] });
  } catch (e) {
    toast(String(e), true);
    return;
  }
  const column = {
    id: editingColumn ? editingColumn.id : `c${Date.now().toString(36)}`,
    // An unnamed column is labelled by its own pattern, which beats "Column 3".
    name: el("colmask-name").value.trim() || pattern,
    pattern,
    align: editingAlign,
  };
  const isNew = !editingColumn;
  upsertCustomColumn(column);
  closeColumnEditor();
  // A new column is shown straight away — it was just asked for.
  if (isNew) applyColumns([...visibleColumns, customColumnKey(column.id)]);
  else {
    renderTableHead();
    hooks.renderTracks();
  }
  renderColumnsMenu();
  await refreshCustomColumnCells();
}

// ---- fit a column to its content (#151) ----
//
// A library of long album titles leaves every column truncated until each one is
// dragged wider by hand, which is a lot of dragging for something the browser can
// work out.
//
// Measuring is delegated to the browser rather than reimplemented: the fixed
// layout and the explicit widths come off for one synchronous read, the natural
// widths are taken, and everything goes back. That is exactly what "fit to
// content" means, it costs one reflow for the whole table rather than a
// measurement per cell, and it needs no second copy of what each cell renders —
// nested markup, badges and the two-line file cell are all accounted for because
// the browser is laying them out for real.
const COLUMN_MAX_FIT_WIDTH = 480;
const AUTOFIT_STORAGE_KEY = "tagrex.autofitColumns";

// Natural width per key, in one pass. Reads happen after the class is on and
// before anything is written back, so the browser flushes layout once.
function measureColumns(keys) {
  const table = el("tracks");
  const headers = [...table.querySelectorAll("thead th.sortable")];
  const saved = headers.map((th) => th.style.width);
  for (const th of headers) th.style.width = "";
  // The table only holds the rows the window shows (#189), so the widest value
  // in a column usually has no row to be measured. The renderer lends us one for
  // the duration of the read — the browser still does the measuring, which is
  // what makes this honest about badges, two-line cells and the value font.
  const unmountMeasureRows = hooks.mountMeasureRows(keys);
  table.classList.add("measuring");
  const natural = new Map();
  for (const th of headers) {
    if (keys.includes(th.dataset.sort)) {
      natural.set(th.dataset.sort, th.getBoundingClientRect().width);
    }
  }
  table.classList.remove("measuring");
  unmountMeasureRows();
  headers.forEach((th, index) => (th.style.width = saved[index]));
  return natural;
}

// Fit `keys` and persist the result — a fitted width is an ordinary width, no
// different from a dragged one, so it is stored, restored and resizable after.
//
// Clamped at both ends: never narrower than a draggable column, and never so
// wide that one pathological value (a 300-character comment) pushes the table
// into horizontal scrolling on its own.
function fitColumns(keys) {
  const natural = measureColumns(keys);
  if (!natural.size) return;
  for (const [key, width] of natural) {
    const clamped = Math.min(Math.max(width, COLUMN_MIN_WIDTH), COLUMN_MAX_FIT_WIDTH);
    columnWidths[key] = Math.round(clamped);
  }
  saveColumnWidths();
  renderTableHead();
}

function fitColumn(key) {
  fitColumns([key]);
}

function fitAllColumns() {
  fitColumns(visibleColumns.slice());
}

// Sticky mode: re-fit whenever what the table shows changes.
function autofitEnabled() {
  return readStored(AUTOFIT_STORAGE_KEY) === "1";
}

function setAutofit(on) {
  writeStored(AUTOFIT_STORAGE_KEY, on ? "1" : "0");
  if (on) fitAllColumns();
}

// What the last automatic fit was computed for. Re-fitting on every paint would
// be wasteful and visibly jumpy — the table repaints on every row click, and a
// selection change moves no text. So a fit is redone only when something that
// can change a cell's contents changes: the rows, the columns, the diff state
// (which reveals old values under the new ones) or the value font.
let lastAutofitSignature = null;

function autofitSignature() {
  return [
    tracks.length,
    visibleColumns.join(","),
    diffByPath ? diffByPath.size : 0,
    document.body.className,
    getComputedStyle(document.body).getPropertyValue("--table-font-size"),
  ].join("|");
}

// Called at the end of every table paint.
function maybeAutofit() {
  if (!autofitEnabled()) return;
  const signature = autofitSignature();
  if (signature === lastAutofitSignature) return;
  lastAutofitSignature = signature;
  fitAllColumns();
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
const PRESETS_STORAGE_KEY = "tagrex.filterPresets";
setGroupByValue(groupByPref());
// The dropped directories of a file-set drag-and-drop (#127), or null for an
// ordinary library. When set, the table's "drop" grouping buckets each track
// under the dropped folder it came from, with loose files under "Files".
// The root of the currently open session (the opened/dropped folder, or a
// file-set's common ancestor). Folder-group headers show the path relative to
// it — starting with the root's own name — so nested folders read like a tree
// (#129), matching what a reference tagger shows.

function renderTableHead() {
  const row = el("tracks").querySelector("thead tr");
  row.querySelectorAll("th.sortable").forEach((th) => th.remove());
  for (const key of visibleColumns) {
    const th = document.createElement("th");
    th.dataset.sort = key;
    th.className = "sortable";
    th.style.width = `${columnWidth(key)}px`;
    // Name the mask placeholder that addresses this column (#148). The column
    // header is where someone is already looking when they wonder what to write
    // for a field, and the spelling is not always guessable — Catalogue # is
    // %catalognumber%. Columns no placeholder addresses (File, Position) get
    // their label alone.
    const custom = customColumnOf(key);
    if (custom) {
      // A mask column names its pattern instead of a placeholder — the pattern
      // IS the answer to "where does this value come from?" (#150). An unnamed
      // column is labelled by its pattern already, so don't print it twice.
      th.title =
        custom.name === custom.pattern ? custom.pattern : `${custom.name} · ${custom.pattern}`;
      th.classList.add(`align-${custom.align}`);
    } else {
      // The Length column is addressed by a file placeholder rather than a tag
      // one, and it is spelled with the leading underscore the technical
      // placeholders use (#147) — so the header still answers "what do I write
      // for this?" (#172).
      const token = placeholderToken(key === "length" ? "_length" : key);
      th.title = token ? `${columnLabel(key)} · ${token}` : columnLabel(key);
    }
    // A drag grip on the right edge resizes the column; a label span keeps the
    // header text clipping (ellipsis) independent of the grip.
    th.innerHTML =
      `<span class="th-label">${escapeHtml(columnLabel(key))}<span class="sort-ind"></span></span>` +
      `<span class="col-resize" data-key="${escapeHtml(key)}"></span>`;
    // Drag the header itself to reorder (#89) — the direct version of the
    // Columns popover's grips, over the same order model. File is structural and
    // stays first, so it is not a handle and not a target.
    th.dataset.key = key;
    if (key !== "file") {
      enablePointerReorder(th, th, row, "th.sortable", moveColumn, { axis: "x" });
    }
    row.appendChild(th);
  }
  hooks.updateSortIndicators();
}

// The two localStorage calls, wrapped as narrowly as the failure they tolerate:
// storage being unavailable or full. Everything else stays outside the try, so a
// mistake in the surrounding code throws where it can be seen instead of being
// mistaken for "storage wasn't there" — which is exactly how #155 hid for so long.
function readStored(key) {
  try {
    return localStorage.getItem(key);
  } catch (e) {
    return null; // storage unavailable — fall back to defaults
  }
}

function writeStored(key, value) {
  try {
    localStorage.setItem(key, value);
  } catch (e) {
    /* storage unavailable or full — the preference just won't persist */
  }
}

function saveColumns() {
  writeStored(COLUMNS_STORAGE_KEY, JSON.stringify(visibleColumns));
}

function saveColumnWidths() {
  writeStored(COLUMN_WIDTHS_STORAGE_KEY, JSON.stringify(columnWidths));
}

// Parse stored JSON, treating malformed content as absent — a hand-edited or
// half-written value is data, not a bug in this file.
function readStoredJson(key) {
  const raw = readStored(key);
  if (raw === null) return null;
  try {
    return JSON.parse(raw);
  } catch (e) {
    return null;
  }
}

// Load saved widths; keep only known keys with a sane positive number.
function loadColumnWidths() {
  const saved = readStoredJson(COLUMN_WIDTHS_STORAGE_KEY);
  if (!saved || typeof saved !== "object") return;
  const known = new Set(allColumnKeys());
  for (const [key, w] of Object.entries(saved)) {
    if (known.has(key) && Number.isFinite(w) && w >= COLUMN_MIN_WIDTH) {
      columnWidths[key] = Math.round(w);
    }
  }
}

// Load the saved column choice; drop unknown keys and force "file" first.
function loadColumns() {
  const saved = readStoredJson(COLUMNS_STORAGE_KEY);
  if (!Array.isArray(saved) || !saved.length) return;
  const known = new Set(allColumnKeys());
  const cols = saved.filter((k) => known.has(k) && k !== "file");
  cols.unshift("file");
  if (cols.length > 1) setVisibleColumns(cols);
}

// Apply a new column set: persist, rebuild the header, repaint rows.
// Move one column to where it was dropped, for both gestures that reorder them
// (#43 in the popover, #89 on the header itself): `past` means the drop landed
// on the far half of the target, so the dragged column goes after it.
//
// File is structural — always shown, always first — so it is neither dragged nor
// dropped onto, and the order worked on here leaves it out entirely;
// `applyColumns` puts it back in front.
function moveColumn(dragged, target, past) {
  if (target === "file" || !visibleColumns.includes(target)) return;
  const order = visibleColumns.filter((k) => k !== "file");
  order.splice(order.indexOf(dragged), 1);
  let to = order.indexOf(target);
  if (past) to += 1;
  order.splice(to, 0, dragged);
  applyColumns(order);
  renderColumnsMenu();
}

function applyColumns(cols) {
  const deduped = [...new Set(cols)].filter((k) => k !== "file");
  setVisibleColumns(["file", ...deduped]);
  saveColumns();
  renderTableHead();
  hooks.renderTracks();
}

// Reset columns to the default set, visibility, and widths (#91).
function resetColumns() {
  // Turn the sticky fit off first: leaving it on would re-fit immediately and
  // the defaults would never be seen (#151).
  writeStored(AUTOFIT_STORAGE_KEY, "0");
  setColumnWidths({});
  try {
    localStorage.removeItem(COLUMN_WIDTHS_STORAGE_KEY);
  } catch (e) {
    /* storage unavailable — nothing was persisted to clear */
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
  // Everything that is not a column goes into one block that stays put while
  // the list scrolls above it (#174). With a dozen columns picked, these used to
  // sit below the fold with nothing to say they were there.
  const actions = document.createElement("div");
  actions.className = "col-menu-actions";

  // Defining a new mask column (#150) belongs with the list of columns, since
  // that is where someone goes looking for one that isn't there.
  const addFoot = document.createElement("div");
  addFoot.className = "col-menu-foot";
  const add = document.createElement("button");
  add.type = "button";
  add.className = "text-btn";
  add.textContent = "Add column…";
  add.title = "A column whose value is a mask, rendered per row";
  add.addEventListener("click", () => {
    menu.hidden = true;
    openColumnEditor(null);
  });
  addFoot.appendChild(add);
  actions.appendChild(addFoot);

  // Fit-to-content (#151), above the reset row: it acts on the widths, which is
  // what the rows above configure, while Reset throws the whole set away.
  const fitFoot = document.createElement("div");
  fitFoot.className = "col-menu-foot col-fit-foot";
  const fit = document.createElement("button");
  fit.type = "button";
  fit.className = "text-btn";
  fit.textContent = "Fit to content";
  fit.title = "Size every column to its widest value (double-click a column edge for one)";
  fit.addEventListener("click", () => {
    menu.hidden = true;
    fitAllColumns();
  });
  fitFoot.appendChild(fit);

  // ...and the sticky version of the same thing, as a row rather than a button:
  // it is a mode that stays on, not an action.
  const autoRow = document.createElement("label");
  autoRow.className = "col-menu-row col-autofit";
  const autoBox = document.createElement("input");
  autoBox.type = "checkbox";
  autoBox.checked = autofitEnabled();
  autoBox.addEventListener("change", () => setAutofit(autoBox.checked));
  const autoLabel = document.createElement("span");
  autoLabel.className = "col-menu-label";
  autoLabel.textContent = "Autofit by content";
  autoRow.title = "Keep columns sized to their content as the table changes";
  autoRow.append(autoBox, autoLabel);
  fitFoot.appendChild(autoRow);
  actions.appendChild(fitFoot);

  // Reset-to-default footer (#91): default set, order, visibility, and widths.
  const foot = document.createElement("div");
  foot.className = "col-menu-foot";
  const reset = document.createElement("button");
  reset.type = "button";
  reset.className = "text-btn";
  reset.textContent = "Reset to default";
  reset.title = "File · Artist · Title · Album · Year · Length, default widths";
  reset.addEventListener("click", resetColumns);
  foot.appendChild(reset);
  actions.appendChild(foot);
  menu.appendChild(actions);
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
    enablePointerReorder(grip, row, el("columns-menu"), ".col-menu-row", moveColumn);
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
    // A mask column being shown for the first time has nothing cached yet (#150).
    if (box.checked && customColumnOf(key)) refreshCustomColumnCells();
  });

  const label = document.createElement("span");
  label.className = "col-menu-label";
  label.textContent = columnLabel(key) + (isFile ? " (always shown)" : "");

  row.append(grip, box, label);

  // A mask column is defined by the user, so it can be edited and deleted from
  // the same row that shows it (#150). A tag column has nothing to edit.
  const custom = customColumnOf(key);
  if (custom) {
    row.title = custom.pattern;

    const edit = document.createElement("button");
    edit.type = "button";
    edit.className = "icon col-row-btn";
    edit.innerHTML = ico("rename");
    edit.title = `Edit — ${custom.pattern}`;
    edit.setAttribute("aria-label", `Edit column ${custom.name}`);
    edit.addEventListener("click", (e) => {
      e.preventDefault();
      e.stopPropagation();
      openColumnEditor(custom);
    });
    const remove = document.createElement("button");
    remove.type = "button";
    remove.className = "icon col-row-btn";
    remove.innerHTML = ico("close");
    remove.title = "Remove this column";
    remove.setAttribute("aria-label", `Remove column ${custom.name}`);
    remove.addEventListener("click", (e) => {
      e.preventDefault();
      e.stopPropagation();
      removeCustomColumn(custom.id);
    });
    row.append(edit, remove);
  }
  return row;
}

export {
  COLUMN_MIN_WIDTH,
  allColumnKeys,
  applyColumns,
  customColumnOf,
  customColumnValue,
  invalidateCustomColumns,
  loadCustomColumns,
  refreshCustomColumnCells,
  autofitEnabled,
  fitAllColumns,
  fitColumn,
  maybeAutofit,
  columnLabel,
  columnWidth,
  defaultColumnWidth,
  loadColumnWidths,
  loadColumns,
  populateGroupMenu,
  renderColumnsMenu,
  renderTableHead,
  saveColumnWidths,
  setGroupBy,
};

// ---- wire up the mask-column editor (#150) ----
el("colmask-align").addEventListener("click", (e) => {
  const btn = e.target.closest(".seg-btn");
  if (btn) setColumnAlign(btn.dataset.align);
});
el("colmask-cancel").addEventListener("click", closeColumnEditor);
el("colmask-ok").addEventListener("click", commitColumnEditor);
el("colmask-modal").addEventListener("click", (e) => {
  if (e.target === el("colmask-modal")) closeColumnEditor();
});
el("colmask-pattern").addEventListener("keydown", (e) => {
  if (e.key === "Enter") {
    e.preventDefault();
    commitColumnEditor();
  }
});
