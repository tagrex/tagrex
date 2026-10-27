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
// Checks whose details are unfolded (#559). The active check is always open.
const open = new Set();
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

// The active check outside the menu (#559): the button is "set" and counts the
// files it shows, and the filter field carries it as a chip whose × clears it.
function syncButton() {
  const button = el("health-btn");
  button.classList.toggle("on", activeId !== null);
  const label = activeHealthLabel();
  button.title = label ? t("health.active", { check: label }) : t("health.title");
  el("health-n").textContent = label ? String(health().sets[activeId].size) : "";
  el("filter-check").hidden = !label;
  el("filter-check-name").textContent = label || "";
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

// One check: the disclosure slot (a › when it has details, else empty so every
// name starts in one column), then the row that chooses it. The › only unfolds,
// so reading the gaps never costs the filter and choosing never hides them.
function row(label, count, id, detail) {
  const item = document.createElement("div");
  item.className = "health-item";
  if (detail) {
    const more = document.createElement("button");
    more.type = "button";
    more.className = "icon health-more";
    more.innerHTML = '<svg class="ico"><use href="#i-chevron-right"/></svg>';
    const paint = () => {
      const shown = !detail.hidden;
      more.setAttribute("aria-expanded", String(shown));
      more.title = t(shown ? "health.hideDetails" : "health.showDetails");
    };
    detail.hidden = !(open.has(id) || id === activeId);
    paint();
    more.addEventListener("click", (event) => {
      event.stopPropagation();
      if (open.has(id)) open.delete(id);
      else open.add(id);
      detail.hidden = !detail.hidden;
      paint();
    });
    item.appendChild(more);
  } else {
    item.appendChild(document.createElement("span"));
  }
  const button = document.createElement("button");
  button.type = "button";
  button.className = "col-menu-row health-row";
  button.classList.toggle("on", id === activeId);
  button.classList.toggle("zero", count === 0);
  const name = document.createElement("span");
  name.className = "health-name";
  name.textContent = label;
  const number = document.createElement("span");
  number.className = "health-count";
  number.textContent = String(count);
  button.append(name, number);
  button.addEventListener("click", () => choose(id));
  item.appendChild(button);
  return item;
}

// A detail line: the thing itself in full ink, then the gap in tabular figures.
function detailLine(what, gap) {
  const line = document.createElement("div");
  line.className = "health-line";
  const whatSpan = document.createElement("span");
  whatSpan.className = "health-what";
  whatSpan.textContent = what;
  const gapSpan = document.createElement("span");
  gapSpan.className = "health-gap";
  gapSpan.append(...gap);
  line.append(whatSpan, gapSpan);
  return line;
}

function bold(text) {
  const b = document.createElement("b");
  b.textContent = text;
  return b;
}

function moreLine(n) {
  const more = document.createElement("div");
  more.className = "health-line more";
  more.textContent = t("health.spellingMore", { n });
  return more;
}

// What the spelling check found, as the values themselves: no side is picked,
// the person sees which spellings sit side by side and how many files hold each.
const SPELLING_LINES = 4;

function spellingDetails(found) {
  if (!found.length) return null;
  const box = document.createElement("div");
  box.className = "health-detail";
  for (const item of found.slice(0, SPELLING_LINES)) {
    // The most common form's count in bold: the spelling the others stray from.
    const gap = [];
    item.forms.forEach((form, index) => {
      if (index) gap.push(` · ${form.value} ×${form.count}`);
      else gap.push(`${form.value} `, bold(`×${form.count}`));
    });
    box.appendChild(detailLine(fieldLabel(item.field), gap));
  }
  if (found.length > SPELLING_LINES) box.appendChild(moreLine(found.length - SPELLING_LINES));
  return box;
}

// The albums the incomplete check found, each with the track numbers it is
// missing (#534), so a gap is something to go and find rather than a count.
function incompleteDetails(found) {
  if (!found.length) return null;
  const box = document.createElement("div");
  box.className = "health-detail";
  for (const item of found.slice(0, SPELLING_LINES)) {
    const album = item.disc === null ? item.album : t("health.incompleteDisc", { album: item.album, disc: item.disc });
    const share = `${item.have}/${item.total}`;
    const gap = item.missing === null ? [share] : [...withBold(t("health.gapMissing", { tracks: MARK }), item.missing), ` · ${share}`];
    box.appendChild(detailLine(album, gap));
  }
  if (found.length > SPELLING_LINES) box.appendChild(moreLine(found.length - SPELLING_LINES));
  return box;
}

// A translated sentence with one value set in bold: the catalogue string is
// rendered with MARK in the value's place, then split around it.
const MARK = "\u0000";
function withBold(sentence, value) {
  const [before, after = ""] = sentence.split(MARK);
  return [before, bold(String(value)), after];
}

// A head that stays put while the groups scroll (#559), then the groups. A check
// with nothing to show takes no row: a group's zero checks fold into one muted
// line under it. The active check is the exception and stays a row at 0, so a
// finished fix can be seen and cleared.
function renderMenu() {
  const menu = el("health-menu");
  menu.innerHTML = "";
  const head = document.createElement("div");
  head.className = "health-head";
  const summary = document.createElement("span");
  summary.className = "health-sum";
  head.appendChild(summary);
  menu.appendChild(head);
  if (!tracks.length) {
    summary.textContent = t("health.empty");
    return;
  }
  const { counts, flagged, details } = health();
  if (flagged.size) summary.append(...withBold(t("health.summary", { n: MARK, total: tracks.length }), flagged.size));
  else summary.textContent = t("health.allClear");
  if (activeId) {
    const clear = document.createElement("button");
    clear.type = "button";
    clear.className = "health-clear";
    clear.textContent = t("health.clear");
    clear.addEventListener("click", () => choose(activeId));
    head.appendChild(clear);
  }
  const body = document.createElement("div");
  body.className = "health-body";
  menu.appendChild(body);
  for (const group of GROUPS) {
    const checks = CHECKS.filter((check) => check.group === group);
    const heading = document.createElement("div");
    heading.className = "col-menu-sep";
    heading.textContent = t(`health.group.${group}`);
    body.appendChild(heading);
    const zero = [];
    for (const check of checks) {
      const count = counts[check.id];
      if (count === 0 && check.id !== activeId) {
        zero.push(checkLabel(check));
        continue;
      }
      let detail = null;
      if (check.id === "spelling") detail = spellingDetails(details.spelling);
      if (check.id === "incomplete") detail = incompleteDetails(details.incomplete);
      body.appendChild(row(checkLabel(check), count, check.id, detail));
      if (detail) body.appendChild(detail);
    }
    if (zero.length) {
      const line = document.createElement("p");
      line.className = "health-zero";
      line.textContent = t("health.nothingFound", { checks: zero.join(" · ") });
      body.appendChild(line);
    }
  }
}

// `change` repaints the table and status line after a check is chosen or cleared.
export function initHealth(change) {
  onChange = change;
  syncButton();
  el("filter-check-clear").addEventListener("click", () => {
    if (activeId) choose(activeId);
  });
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
