// What Settings offers beyond its own controls (#438, #439): a Reset to defaults
// per group, and a search across all of them.
//
// The groups know their own defaults and how to go back to them (settings.js
// hands them in); this only draws the button and decides when it shows. The
// search works on the DOM as it stands, so it finds a setting in whatever
// language the interface is in.
import { el } from "./dom.js";
import { t } from "./i18n.js";

let groups = [];

function groupElement(id) {
  return document.querySelector(`.settings-group[data-reset-group="${id}"]`);
}

// Show a group's Reset only while something in the group is not at its default.
export function refreshGroupResets() {
  for (const group of groups) {
    const button = groupElement(group.id)?.querySelector(".group-reset");
    if (button) button.hidden = group.isDefault();
  }
}

// `defs`: [{ id, isDefault(): boolean, reset(): void }], one per group that has a
// `data-reset-group` in the markup.
export function initGroupResets(defs) {
  groups = defs;
  for (const group of groups) {
    const host = groupElement(group.id);
    const button = document.createElement("button");
    button.type = "button";
    button.className = "group-reset text-btn";
    button.dataset.i18n = "settings.resetGroup";
    button.dataset.i18nTitle = "settings.resetGroupTitle";
    button.textContent = t("settings.resetGroup");
    button.title = t("settings.resetGroupTitle");
    button.hidden = true;
    button.addEventListener("click", () => {
      group.reset();
      refreshGroupResets();
    });
    host.appendChild(button);
  }
  // Any control in the sheet may have moved a group off its default; look again
  // once the control's own handler has run.
  const body = document.querySelector(".settings-body");
  for (const type of ["input", "change", "click"]) {
    body.addEventListener(type, () => setTimeout(refreshGroupResets, 0));
  }
}

// ---- the groups list on the left ----
//
// One group fills the pane at a time; the list is built from the groups in the
// markup, so a new group appears in it without further wiring. A search lifts
// the limit and shows every group that has a match, and the list then shows
// only those.
const GROUP_KEY = "tagrex.settings.group";
// Remembered by name, not by position, so a reordered list still opens on the
// group the person left (the first group, Display, until they choose another).
let activeGroup = "";
try {
  activeGroup = localStorage.getItem(GROUP_KEY) || "";
} catch (e) {
  /* the first group is the default */
}

function allGroups() {
  return [...document.querySelectorAll(".settings-body .settings-group")];
}

// Position of the remembered group in the list as it stands now; the first one
// when it is not there (never chosen, or no longer a group).
function activeIndex() {
  const i = allGroups().findIndex((group) => group.dataset.icon === activeGroup);
  return i < 0 ? 0 : i;
}

function groupLabel(group) {
  const title = group.querySelector("h3").cloneNode(true);
  title.querySelectorAll(".lab-tag").forEach((tag) => tag.remove());
  return title.textContent.trim();
}

function applyPane() {
  const searching = el("settings-search").value.trim() !== "";
  const groups = allGroups();
  const buttons = [...el("settings-nav-list").children];
  const current = activeIndex();
  groups.forEach((group, i) => {
    group.classList.toggle("pane-hidden", !searching && i !== current);
    const button = buttons[i];
    if (!button) return;
    button.setAttribute("aria-selected", String(!searching && i === current));
    button.tabIndex = i === current ? 0 : -1;
    button.hidden = searching && group.classList.contains(HIDDEN);
  });
}

export function selectSettingsGroup(index) {
  const groups = allGroups();
  const chosen = groups[Math.min(Math.max(index, 0), groups.length - 1)];
  activeGroup = chosen.dataset.icon || "";
  try {
    localStorage.setItem(GROUP_KEY, activeGroup);
  } catch (e) {
    /* only the remembered group is lost */
  }
  if (el("settings-search").value) clearSettingsSearch();
  else applyPane();
  document.querySelector(".settings-body").scrollTop = 0;
}

// Drawn when Settings opens and again when the interface language changes, so
// the names follow the headings.
export function renderSettingsNav() {
  const list = el("settings-nav-list");
  list.replaceChildren(
    ...allGroups().map((group, i) => {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "settings-nav-item";
      button.setAttribute("role", "tab");
      const label = document.createElement("span");
      label.textContent = groupLabel(group);
      if (group.dataset.icon) {
        const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
        svg.setAttribute("class", "ico");
        const use = document.createElementNS("http://www.w3.org/2000/svg", "use");
        use.setAttribute("href", `#${group.dataset.icon}`);
        svg.append(use);
        button.append(svg);
      }
      button.append(label);
      button.addEventListener("click", () => selectSettingsGroup(i));
      return button;
    })
  );
  list.onkeydown = (event) => {
    const step = { ArrowDown: 1, ArrowUp: -1 }[event.key];
    if (!step) return;
    event.preventDefault();
    selectSettingsGroup(activeIndex() + step);
    list.children[activeIndex()]?.focus();
  };
  applyPane();
}

// ---- search ----

const HIDDEN = "search-hidden";

// A group's children in blocks: whatever comes before its first row, then each
// row with the hints and notes that follow it. A block is shown or hidden whole.
function blocksOf(group) {
  const blocks = [[]];
  for (const child of group.children) {
    if (child.tagName === "H3" || child.classList.contains("group-reset")) continue;
    // A sub-heading starts a block of its own; a row starts one too, unless it
    // is the first thing under a sub-heading, which it stays with.
    const last = blocks[blocks.length - 1];
    const underHeading = last.length === 1 && last[0].tagName === "H4";
    if (child.tagName === "H4" || (child.classList.contains("settings-row") && !underHeading)) {
      blocks.push([]);
    }
    blocks[blocks.length - 1].push(child);
  }
  return blocks.filter((block) => block.length);
}

function textOf(nodes) {
  return nodes.map((node) => node.textContent).join(" ").toLowerCase();
}

export function filterSettings(query) {
  const terms = query.toLowerCase().split(/\s+/).filter(Boolean);
  const everything = (haystack) => terms.every((term) => haystack.includes(term));
  let anyShown = false;
  for (const group of document.querySelectorAll(".settings-body .settings-group")) {
    const title = group.querySelector("h3")?.textContent.toLowerCase() || "";
    const titleMatches = terms.length > 0 && everything(title);
    let groupShown = !terms.length;
    for (const block of blocksOf(group)) {
      const shown = !terms.length || titleMatches || everything(`${title} ${textOf(block)}`);
      block.forEach((node) => node.classList.toggle(HIDDEN, !shown));
      groupShown = groupShown || shown;
    }
    group.classList.toggle(HIDDEN, !groupShown);
    anyShown = anyShown || groupShown;
  }
  el("settings-search-empty").hidden = anyShown || !terms.length;
  applyPane();
}

export function clearSettingsSearch() {
  el("settings-search").value = "";
  filterSettings("");
}

export function initSettingsSearch() {
  const input = el("settings-search");
  input.addEventListener("input", () => filterSettings(input.value));
  // Esc empties the box first; only an empty one lets Esc go on to close the sheet.
  input.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && input.value) {
      event.stopPropagation();
      clearSettingsSearch();
    }
  });
}
