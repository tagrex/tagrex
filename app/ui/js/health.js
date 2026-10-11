// The library health menu (#436): ready-made checks that narrow the table to the
// files that need a look. The checks themselves are in healthchecks.js; this is
// the menu, the one active check and the filter it contributes.
//
// A check is a view, not an edit. Fixing what it shows goes through the normal
// change plan, so nothing is written unseen.
import { el } from "./dom.js";
import { fieldLabel } from "./fields.js";
import { t } from "./i18n.js";
import { tracks } from "./state.js";
import { CHECKS, computeHealth } from "./healthchecks.js";

const GROUPS = ["missing", "suspect", "albums", "library", "files"];

let activeId = null;
let cache = { source: null, health: null };
let onChange = () => {};

// Recomputed when the track list is replaced (a re-read, an apply), not on every
// repaint, so a check that is on keeps tracking the files as they are fixed.
function health() {
  if (cache.source !== tracks) cache = { source: tracks, health: computeHealth(tracks) };
  return cache.health;
}

function checkLabel(check) {
  return check.field
    ? t("health.missing", { field: fieldLabel(check.field) })
    : t(`health.${check.id}`);
}

// Whether the active check lets this file through; true when none is active.
export function healthAllows(path) {
  if (!activeId) return true;
  return health().sets[activeId].has(path);
}

// The name of the active check, for the status line; null when none is on.
export function activeHealthLabel() {
  const check = CHECKS.find((candidate) => candidate.id === activeId);
  return check ? checkLabel(check) : null;
}

function syncButton() {
  const button = el("health-btn");
  button.classList.toggle("on", activeId !== null);
  const label = activeHealthLabel();
  button.title = label ? t("health.active", { check: label }) : t("health.title");
}

// A language switch: the button's tooltip names the active check (#476).
export function retranslateHealth() {
  syncButton();
}

// Drops the active check without repainting: for a caller that is about to.
export function clearHealth() {
  activeId = null;
  syncButton();
}

function choose(id) {
  activeId = activeId === id ? null : id;
  syncButton();
  el("health-menu").hidden = true;
  onChange();
}

function row(label, count, id) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "col-menu-row health-row";
  button.classList.toggle("on", id === activeId);
  button.classList.toggle("zero", count === 0);
  // aria-disabled, not `disabled`: the stock disabled button is drawn striped,
  // which on a whole menu of zero counts reads as noise rather than as "nothing
  // here".
  const empty = count === 0 && id !== activeId;
  if (empty) button.setAttribute("aria-disabled", "true");
  const name = document.createElement("span");
  name.className = "health-name";
  name.textContent = label;
  const number = document.createElement("span");
  number.className = "health-count";
  number.textContent = String(count);
  button.append(name, number);
  button.addEventListener("click", () => {
    if (!empty) choose(id);
  });
  return button;
}

// What the spelling check found, as the values themselves: no side is picked,
// the person sees which spellings sit side by side and how many files hold each.
const SPELLING_LINES = 4;

function appendSpellingDetails(menu, found) {
  if (!found.length) return;
  const box = document.createElement("div");
  box.className = "health-detail";
  for (const item of found.slice(0, SPELLING_LINES)) {
    const line = document.createElement("div");
    const forms = item.forms.map((form) => `${form.value} ×${form.count}`).join(" · ");
    line.textContent = `${fieldLabel(item.field)}: ${forms}`;
    box.appendChild(line);
  }
  if (found.length > SPELLING_LINES) {
    const more = document.createElement("div");
    more.textContent = t("health.spellingMore", { n: found.length - SPELLING_LINES });
    box.appendChild(more);
  }
  menu.appendChild(box);
}

function renderMenu() {
  const menu = el("health-menu");
  menu.innerHTML = "";
  if (!tracks.length) {
    const empty = document.createElement("div");
    empty.className = "col-menu-sep";
    empty.textContent = t("health.empty");
    menu.appendChild(empty);
    return;
  }
  const { counts, flagged, details } = health();
  if (activeId) {
    const clear = document.createElement("button");
    clear.type = "button";
    clear.className = "col-menu-row health-row health-clear";
    clear.textContent = t("health.clear");
    clear.addEventListener("click", () => choose(activeId));
    menu.appendChild(clear);
  }
  const summary = document.createElement("div");
  summary.className = "col-menu-sep";
  summary.textContent = flagged.size
    ? t("health.summary", { n: flagged.size, total: tracks.length })
    : t("health.allClear");
  menu.appendChild(summary);
  for (const group of GROUPS) {
    const checks = CHECKS.filter((check) => check.group === group);
    const heading = document.createElement("div");
    heading.className = "col-menu-sep";
    heading.textContent = t(`health.group.${group}`);
    menu.appendChild(heading);
    for (const check of checks) {
      menu.appendChild(row(checkLabel(check), counts[check.id], check.id));
      if (check.id === "spelling") appendSpellingDetails(menu, details.spelling);
    }
  }
}

// `change` repaints the table and status line after a check is chosen or cleared.
export function initHealth(change) {
  onChange = change;
  syncButton();
  el("health-btn").addEventListener("click", (event) => {
    event.stopPropagation();
    const menu = el("health-menu");
    if (menu.hidden) renderMenu();
    menu.hidden = !menu.hidden;
  });
  document.addEventListener("click", (event) => {
    const menu = el("health-menu");
    if (!menu.hidden && !menu.contains(event.target) && event.target !== el("health-btn")) {
      menu.hidden = true;
    }
  });
}
