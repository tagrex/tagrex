// Named mask presets (#360).
//
// A small pool of saved mask patterns the user can drop into any mask input —
// the rename mask, the reorganize folder pattern, FROM NAME, the export mask.
// One shared pool: a pattern saved from one input is offered in all of them,
// because the mask vocabulary is the same everywhere.
//
// The pool lives in the backend settings, like the transform action groups
// (#57): read once from `load_settings`, rewritten with `save_settings`. The UI
// is a delegated popover shared by every mask input, the same shape as the
// placeholder reference (#148) it sits beside.
import { el, escapeHtml, toast } from "./dom.js";
import { t } from "./i18n.js";
import { invoke } from "./invoke.js";
import { maskPresets, savedSettings, setMaskPresets, setSavedSettings } from "./state.js";

// The input the open popover reads from / writes to, and the button it hangs off.
let target = null;
let anchor = null;

async function persist() {
  // Save the whole settings back, carrying every other slice (action groups,
  // sidecars, …) untouched — the same rewrite-the-file model the settings use.
  setSavedSettings({ ...savedSettings, mask_presets: maskPresets });
  try {
    await invoke("save_settings", { settings: savedSettings });
  } catch (e) {
    toast(String(e), true);
  }
}

function render() {
  const list = el("preset-list");
  if (maskPresets.length === 0) {
    list.innerHTML = `<p class="preset-empty muted">${escapeHtml(t("preset.empty"))}</p>`;
    return;
  }
  list.innerHTML = maskPresets
    .map(
      (preset) => `<div class="preset-row" data-mask="${escapeHtml(preset.mask)}">
        <button type="button" class="preset-apply" title="${escapeHtml(preset.mask)}">
          <span class="preset-name">${escapeHtml(preset.name)}</span>
          <span class="preset-mask">${escapeHtml(preset.mask)}</span>
        </button>
        <button type="button" class="preset-del icon" data-name="${escapeHtml(preset.name)}"
          title="${escapeHtml(t("preset.delete"))}" aria-label="${escapeHtml(t("preset.delete"))}">
          <svg class="ico"><use href="#i-close"/></svg>
        </button>
      </div>`,
    )
    .join("");
}

// Place the popover against its button, mirroring the placeholder reference: a
// tall menu whose panel scrolls, so fixed positioning rather than the absolute
// `.col-menu` shell.
function place(menu, button) {
  const rect = button.getBoundingClientRect();
  const width = Math.min(360, window.innerWidth - 16);
  menu.style.width = `${width}px`;
  const left = Math.min(Math.max(8, rect.right - width), window.innerWidth - width - 8);
  menu.style.left = `${left}px`;
  const below = window.innerHeight - rect.bottom;
  if (below > 260 || below > rect.top) {
    menu.style.top = `${rect.bottom + 4}px`;
    menu.style.bottom = "auto";
    menu.style.maxHeight = `${below - 12}px`;
  } else {
    menu.style.top = "auto";
    menu.style.bottom = `${window.innerHeight - rect.top + 4}px`;
    menu.style.maxHeight = `${rect.top - 12}px`;
  }
}

export function closePresets() {
  el("preset-menu").hidden = true;
  target = null;
  anchor = null;
}

function openPresets(button, input) {
  const menu = el("preset-menu");
  target = input;
  anchor = button;
  el("preset-name").value = "";
  render();
  menu.hidden = false;
  place(menu, button);
  el("preset-name").focus();
}

// Save the mask currently in the target input under the typed name. An existing
// name is overwritten (renaming a preset is just saving over it), and the list
// stays sorted so it reads the same wherever it opens.
async function saveCurrent() {
  if (!target) return;
  const name = el("preset-name").value.trim();
  const mask = target.value.trim();
  if (!name) {
    el("preset-name").focus();
    return;
  }
  if (!mask) {
    toast(t("preset.noMask"), true);
    return;
  }
  const rest = maskPresets.filter((preset) => preset.name !== name);
  rest.push({ name, mask });
  rest.sort((a, b) => a.name.localeCompare(b.name));
  setMaskPresets(rest);
  await persist();
  el("preset-name").value = "";
  render();
  toast(t("preset.saved", { name }));
}

function applyPreset(mask) {
  if (!target) return;
  target.value = mask;
  // The live previews (FROM NAME's read-out, the export name) listen for input.
  target.dispatchEvent(new Event("input", { bubbles: true }));
  target.focus();
  closePresets();
}

async function deletePreset(name) {
  setMaskPresets(maskPresets.filter((preset) => preset.name !== name));
  await persist();
  render();
}

export async function initMaskPresets() {
  try {
    setSavedSettings((await invoke("load_settings", {})) || {});
    setMaskPresets(Array.isArray(savedSettings.mask_presets) ? savedSettings.mask_presets : []);
  } catch (e) {
    setMaskPresets([]);
  }

  // One delegated listener for every trigger, so a new mask input needs only the
  // button markup and no wiring.
  document.addEventListener("click", (e) => {
    const button = e.target.closest?.(".preset-btn");
    if (button) {
      e.preventDefault();
      const input = el(button.dataset.presetTarget);
      if (!input) return;
      if (anchor === button && !el("preset-menu").hidden) closePresets();
      else openPresets(button, input);
      return;
    }
    if (e.target.closest?.("#preset-save")) {
      saveCurrent();
      return;
    }
    const del = e.target.closest?.(".preset-del");
    if (del) {
      deletePreset(del.dataset.name);
      return;
    }
    const apply = e.target.closest?.(".preset-apply");
    if (apply) {
      applyPreset(apply.closest(".preset-row").dataset.mask);
      return;
    }
    if (!e.target.closest?.("#preset-menu")) closePresets();
  });

  // Enter in the name field saves the current mask under it.
  el("preset-name").addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      saveCurrent();
    }
  });

  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !el("preset-menu").hidden) {
      closePresets();
      anchor?.focus();
    }
  });

  // The popover is anchored to a button that moves when the panel or window
  // resizes; re-place rather than leaving it floating somewhere wrong.
  window.addEventListener("resize", () => {
    if (!el("preset-menu").hidden && anchor) place(el("preset-menu"), anchor);
  });
}
