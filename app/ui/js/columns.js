// The table's columns and its grouping menu (#43, #143 split them out of
// app.js).
//
// Which columns exist, what they are called, and the popover that picks and
// reorders them; plus the grouping selector, which shares the same field list.
// The chosen columns and their widths live in the state module, since the table
// renderer reads them on every paint.
import { el, escapeHtml, ico } from "./dom.js";
import { hooks } from "./hooks.js";
import { EXTENDED_FIELDS, VIRTUAL_COLUMNS } from "./fields.js";
import { groupByPref, saveGroupBy } from "./prefs.js";
import { enablePointerReorder } from "./reorder.js";
import {
  DEFAULT_COLUMNS,
  collapsedGroups,
  columnWidths,
  groupBy,
  setColumnWidths,
  setGroupByValue,
  setVisibleColumns,
  visibleColumns,
} from "./state.js";

// ---- configurable columns (#43) ----
// "file" plus every modeled tag field, the pool the user picks columns from.
function allColumnKeys() {
  return ["file", ...EXTENDED_FIELDS.map(([key]) => key), ...VIRTUAL_COLUMNS.map(([key]) => key)];
}

function columnLabel(key) {
  if (key === "file") return "File";
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

export { allColumnKeys, columnLabel, populateGroupMenu, setGroupBy };
