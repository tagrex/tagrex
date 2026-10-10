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

// ---- search ----

const HIDDEN = "search-hidden";

// A group's children in blocks: whatever comes before its first row, then each
// row with the hints and notes that follow it. A block is shown or hidden whole.
function blocksOf(group) {
  const blocks = [[]];
  for (const child of group.children) {
    if (child.tagName === "H3" || child.classList.contains("group-reset")) continue;
    if (child.classList.contains("settings-row")) blocks.push([]);
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
