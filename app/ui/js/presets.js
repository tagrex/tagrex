import { describeRule, presetExample, sampleRequest, previewFields } from "./preset-presentation.js";
import { createComponentElement } from "../lib/component-elements.js";
// The preset editor (#391): where presets are made, not where they are used.
//
// A preset is a named rule chain (an "action group", #57). The transform
// dialog used to be both the place to build a chain and the place to pick what
// runs, so every job carried its own half-edited chain and a click on a preset
// had to decide whether it replaced or extended it (#388). This window is the
// one place a preset's rules are edited; everywhere else only picks presets.
//
// Built-in presets (#137) live in the binary and stay as shipped: they open
// read-only here, and Duplicate is how one becomes yours to change. Your own
// presets save as you go — on switching to another preset, on closing, and on
// quit — so there is no Save button to forget.
import { EXTENDED_FIELDS, fieldLabel } from "./fields.js";
import { placeFloating, placeContextMenu, el, fileName, ico, toast } from "./dom.js";
import { t, tn } from "./i18n.js";
import { invoke } from "./invoke.js";
import {
  createRuleChain,
  persistActionGroups,
  presetLabel,
  presetNote,
  renderAllGroupsMenus,
  ruleForGroup,
  uniquePresetName,
} from "./chain.js";
import { actionGroups, builtinGroups, selectedPaths, setActionGroups, tracks } from "./state.js";

const chain = createRuleChain({
  ids: {
    rules: "pe-rules",
    empty: "pe-empty",
    // The fieldset a built-in's rules are shown read-only in (#556).
    set: "pe-rules-set",
  },
});

// Which preset is open: its name and whether it is a built-in. Null when there
// is none to show (no presets at all).
let current = null;
let renaming = null;
let deletedPreset = null;
let deleteTimer;
let menuReturnFocus = null;
const collapsedGroups = new Set();
// Listeners told when a preset's rules or name change, so a panel that runs
// presets can refresh what it shows (#392 hangs the shared set off this).
const changeListeners = [];

function onPresetsChanged(fn) {
  changeListeners.push(fn);
}

function notifyPresetsChanged(renamed) {
  for (const fn of changeListeners) fn(renamed);
}

function findPreset(name, builtin) {
  return (builtin ? builtinGroups : actionGroups).find((g) => g.name === name) || null;
}

// Write what the chain holds back into the open preset, if it is yours and
// something changed. The rules carry their own targets (#251), so the group's
// scope is only the fallback the shape requires.
function commit() {
  if (!current || current.builtin) return;
  const group = findPreset(current.name, false);
  if (!group) return;
  const rules = chain.asGroup().rules;
  const before = JSON.stringify((group.rules || []).map(ruleForGroup));
  if (JSON.stringify(rules) === before) return;
  setActionGroups(actionGroups.map((g) => (g === group ? { ...g, rules } : g)));
  persistActionGroups();
  renderAllGroupsMenus();
  notifyPresetsChanged();
}

// The sidebar owns naming; Escape cancels without touching the saved identity.
function startRename() {
  if (!current || current.builtin) return;
  renaming = current.name;
  renderList();
  const input = el("pe-name"); input.focus(); input.select();
}
function finishRename(cancel = false, fromBlur = false) {
  if (!renaming) return;
  const old = renaming;
  const input = el("pe-name");
  const name = input?.value.trim() || "";
  if (!cancel && (!name || [...actionGroups, ...builtinGroups].some(group => group.name === name && name !== old))) {
    input.setAttribute("aria-invalid", "true");
    toast(name ? t("presets.nameTaken", { name }) : t("presets.nameEmpty"), true);
    input.focus(); return;
  }
  renaming = null;
  if (!cancel && name !== old) {
    setActionGroups(actionGroups.map(group => group.name === old ? { ...group, name } : group).sort((a,b) => a.name.localeCompare(b.name)));
    current = { name, builtin: false };
    persistActionGroups(); renderAllGroupsMenus(); notifyPresetsChanged({ from: old, to: name });
  }
  if (fromBlur) {
    // Do not remove the row the pointer is about to click. Replace only the
    // old name field, leaving other sidebar targets and their focus intact.
    input.closest(".pe-list-item").replaceWith(presetRow(findPreset(current.name, false)));
  } else {
    renderList();
    el("pe-list").querySelector(".pe-list-row.active")?.focus({ preventScroll: true });
  }
}

// Open a preset in the editor half, first saving the one being left.
function select(name, builtin) {
  if (renaming) { finishRename(); if (renaming) return; }
  commit();
  if (current?.name === name && current.builtin === builtin) return;
  const group = findPreset(name, builtin);
  if (current?.name !== name || current?.builtin !== builtin) { previewField = "title"; sampleSource = "example"; sampleTyped = false; }
  current = group ? { name, builtin } : null;
  el("pe-rules-set").dataset.readonly = String(!group || builtin);
  el("pe-copy-to-edit").hidden = !builtin;
  el("pe-edit-col").hidden = !group;
  el("pe-open-add").hidden = !group || builtin || !group.rules.length;
  el("pe-empty-add").disabled = !group || builtin;
  chain.load(group || { rules: [] });
  refreshPreviewField();
  if (sampleSource === "selection" && selectionSample() === undefined) sampleSource = "example";
  renderSampleSource();
  renderList();
  // New presets start with an example; manual input, including empty text,
  // remains exact while editing the same preset.
  el("pe-sample-input").placeholder = defaultSample();
  if (!sampleTyped) el("pe-sample-input").value = defaultSample();
  scheduleSample();
}

// ---- field-specific preview ----
let sampleSource = "example";
let previewField = "title";
function sampleTargets() { return previewFields(chain.asGroup(), EXTENDED_FIELDS.map(([key]) => key)); }
function previewFieldLabel(key) {
  return key === "filename" ? t("presets.target.file") : key === "fileext" ? t("presets.target.extension") : fieldLabel(key);
}
function refreshPreviewField() {
  const fields = sampleTargets();
  const previous = previewField;
  if (!fields.includes(previewField)) previewField = fields.includes("title") ? "title" : fields[0];
  const select = el("pe-preview-field");
  if (JSON.stringify(fields) !== select.dataset.fields) {
    select.replaceChildren(...fields.map(field => { const option = document.createElement("option"); option.value = field; option.textContent = previewFieldLabel(field); return option; }));
    select.dataset.fields = JSON.stringify(fields);
  }
  select.value = previewField;
  select.hidden = false;
  return previous !== previewField;
}
function selectionSample() {
  const path = selectedPaths()[0];
  const track = path && tracks.find(track => track.path === path);
  if (!track) return undefined;
  const name = fileName(track.path);
  if (previewField === "fileext") return name.match(/\.([^.]+)$/)?.[1];
  if (previewField === "filename") return name.replace(/\.[^.]+$/, "");
  return track.tags[previewField] ?? "";
}
function defaultSample() {
  if (sampleSource === "selection") return selectionSample() ?? "";
  return presetExample(current && findPreset(current.name, current.builtin));
}
function renderSampleSource() {
  for (const source of ["example", "selection"]) {
    const button = el("pe-" + source);
    button.setAttribute("aria-pressed", String(sampleSource === source));
    button.classList.toggle("active", sampleSource === source);
  }
  const value = selectionSample();
  el("pe-selection").hidden = value === undefined || sampleSource === "selection";
  el("pe-example").hidden = sampleSource === "example" && !sampleTyped;
  const path = selectedPaths()[0];
  el("pe-selection").title = t("presets.useSelection", { field: previewFieldLabel(previewField), file: path ? fileName(path) : "" });

}
let sampleTyped = false;
let sampleTimer = null;
let sampleSeq = 0;

function scheduleSample() {
  if (!current || !editorOpen()) return;
  clearTimeout(sampleTimer);
  sampleSeq++; // Invalidate older results immediately, including the debounce window.
  const changed = refreshPreviewField();
  if (sampleSource === "selection" && selectionSample() === undefined) sampleSource = "example";
  if (changed && (sampleSource === "selection" || !sampleTyped)) el("pe-sample-input").value = defaultSample();
  renderSampleSource();
  chain.refreshPresentation();
  el("pe-open-add").hidden = !chain.length || !!current?.builtin;
  const note = el("pe-list").querySelector(".pe-list-row.active .pe-list-note");
  if (note && !current?.builtin) note.textContent = shortPresetNote(chain.asGroup());
  for (const error of el("pe-rules").querySelectorAll(".pe-rule-error")) error.hidden = true;
  for (const input of el("pe-rules").querySelectorAll("[aria-invalid]")) input.removeAttribute("aria-invalid");
  el("pe-sample-out").setAttribute("aria-busy", "true");
  el("pe-preview-status").textContent = t("presets.previewUpdating");
  sampleTimer = setTimeout(renderSample, 150);
}

async function renderSample() {
  const out = el("pe-sample-out");
  const input = el("pe-sample-input");
  const text = input.value;
  const seq = ++sampleSeq;
  if (!chain.length) {
    out.textContent = t("presets.sampleNoRules");
    out.classList.remove("error");
    out.removeAttribute("aria-busy");
    el("pe-preview-status").textContent = t("presets.sampleNoRules");
    return;
  }
  const group = { ...chain.asGroup(), name: "" };
  try {
    const value = await invoke("transform_sample", { ...sampleRequest(group, previewField), text });
    if (seq !== sampleSeq) return;
    out.textContent = value;
    out.classList.remove("error");
    out.classList.toggle("same", value === text);
    el("pe-preview-status").textContent = value === text ? t("presets.unchanged") : "";
  } catch (error) {
    if (seq !== sampleSeq) return;
    out.textContent = t("presets.checkRule");
    out.classList.add("error");
    // Use the same native engine for diagnosis, rather than a second regex
    // dialect in the browser. Never label an unrelated field as invalid.
    for (const rule of chain.rules().filter(rule => rule.enabled !== false && rule.kind === "replace" && rule.regex)) {
      if (seq !== sampleSeq) return;
      const single = sampleRequest({ ...group, rules: [rule] }, previewField);
      if (!single.group.rules.length) continue;
      try { await invoke("transform_sample", { ...single, text }); }
      catch (cause) {
        if (seq !== sampleSeq) return;
        const card = el("pe-rules").querySelector(`[data-key="${rule.id}"]`);
        const message = card?.querySelector(".pe-rule-error"); const input = card?.querySelector(".pe-rule-field input");
        if (message) { message.textContent = t("presets.patternError") + " " + String(cause); message.hidden = false; }
        if (input) { input.setAttribute("aria-invalid", "true"); input.setAttribute("aria-describedby", message.id); }
      }
    }
    if (seq !== sampleSeq) return;
    el("pe-preview-status").textContent = t("presets.checkRule");
  }
  out.removeAttribute("aria-busy");
}

function shortPresetNote(group) {
  if (group.builtin) {
    const key = "presets.short." + group.name.toLowerCase().replace(/[^a-z0-9]+/g, "-");
    return t(key) === key ? presetNote(group) || tn("unit.rule", group.rules.length) : t(key);
  }
  if (!group.rules.length) return t("presets.emptyTitle");
  return describeRule(group.rules[0], t).title + (group.rules.length > 1 ? " · " + tn("unit.rule", group.rules.length) : "");
}

let pressedPresetMenu = null;

function presetRow(group) {
  const wrap = document.createElement("div"); wrap.className = "pe-list-item";
  const row = createComponentElement("button"); row.type = "button"; row.className = "pe-list-row";
  const active = current?.name === group.name && current.builtin === !!group.builtin;
  row.classList.toggle("active", active); row.setAttribute("aria-pressed", String(active));
  const name = document.createElement("span"); name.className = "pe-list-name"; name.textContent = presetLabel(group);
  const note = document.createElement("span"); note.className = "pe-list-note";
  note.textContent = shortPresetNote(group);
  row.append(name, note); row.addEventListener("click", () => select(group.name, !!group.builtin));
  const more = createComponentElement("button"); more.type = "button"; more.className = "icon pe-preset-more"; more.innerHTML = ico("more");
  more.setAttribute("aria-label", t("presets.options", { name: presetLabel(group) })); more.setAttribute("aria-haspopup", "menu"); more.setAttribute("aria-controls", "pe-preset-menu");
  const openMenu = event => {
    event.preventDefault(); event.stopPropagation(); select(group.name, !!group.builtin);
    if (current?.name !== group.name || current.builtin !== !!group.builtin) return;
    const anchor = el("pe-list").querySelector(".pe-list-row.active");
    const menu = el("pe-preset-menu");
    for (const cmd of ["rename", "delete"]) menu.querySelector(`[data-cmd="${cmd}"]`).closest('[role="menuitem"]').hidden = !!group.builtin;
    menu.querySelector(".ctx-sep").hidden = !!group.builtin;
    const context = event.type === "contextmenu";
    const owner = context || event.type === "keydown" ? anchor : anchor.closest('.pe-list-item').querySelector('.pe-preset-more');
    showMenu(menu, owner, context ? { x: event.clientX, y: event.clientY } : null, "right");
  };
  more.addEventListener("click", openMenu); row.addEventListener("contextmenu", openMenu);
  // Preserve the pressed row if a sticky group heading retargets the native
  // contextmenu event after the sidebar scrolls it into view.
  row.addEventListener("pointerdown", event => {
    if (event.button !== 2) return;
    event.preventDefault();
    pressedPresetMenu = { open: openMenu, x: event.clientX, y: event.clientY };
  });
  row.addEventListener("keydown", event => {
    if (event.key === "F2" && !group.builtin) { event.preventDefault(); select(group.name, false); startRename(); }
    if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) openMenu(event);
  });
  if (renaming === group.name && !group.builtin) {
    const input = createComponentElement("input"); input.type = "text"; input.id = "pe-name"; input.value = group.name; input.setAttribute("aria-label", t("presets.name"));
    input.addEventListener("keydown", event => { if (event.isComposing) return; if (["Enter", "Escape"].includes(event.key)) { event.preventDefault(); event.stopImmediatePropagation(); finishRename(event.key === "Escape"); } });
    input.addEventListener("blur", () => { if (renaming) finishRename(false, true); }); wrap.append(input);
  } else wrap.append(row, more);
  return wrap;
}

function renderList() {
  const box = el("pe-list"); box.replaceChildren();
  function section(id, labelKey, groups) {
    const section = document.createElement("section"); section.className = "pe-sidebar-group";
    const heading = document.createElement("h3");
    const button = createComponentElement("button"); button.type = "button"; button.className = "text-btn pe-group-toggle";
    button.setAttribute("aria-controls", "pe-group-" + id);
    const label = document.createElement("span"); label.textContent = t(labelKey);
    const caret = document.createElement("span"); caret.innerHTML = ico("caret-down"); caret.className = "pe-group-caret"; caret.setAttribute("aria-hidden", "true"); button.append(label, caret); heading.append(button);
    const panel = document.createElement("div"); panel.id = "pe-group-" + id; panel.className = "pe-group-panel";
    const content = document.createElement("div"); content.className = "pe-group-content";
    if (!groups.length) { const empty = document.createElement("p"); empty.className = "pe-list-empty"; empty.textContent = t("presets.noneYet"); content.append(empty); }
    for (const group of groups) content.append(presetRow(group));
    panel.append(content);
    const setState = () => {
      const expanded = !collapsedGroups.has(id);
      section.classList.toggle("is-collapsed", !expanded); panel.inert = !expanded; panel.setAttribute("aria-hidden", String(!expanded)); button.setAttribute("aria-expanded", String(expanded));
    };
    button.addEventListener("click", () => { if (collapsedGroups.has(id)) collapsedGroups.delete(id); else collapsedGroups.add(id); setState(); });
    setState(); section.append(heading, panel); box.append(section);
  }
  section("personal", "presets.yours", actionGroups);
  if (builtinGroups.length) section("builtin", "chain.builtIn", builtinGroups);
}

function addPreset(group) {
  collapsedGroups.delete("personal");
  setActionGroups([...actionGroups, group].sort((a, b) => a.name.localeCompare(b.name)));
  persistActionGroups();
  renderAllGroupsMenus();
  notifyPresetsChanged();
}

function newPreset() {
  if (renaming) { finishRename(); if (renaming) return; }
  commit();
  const name = uniquePresetName(t("presets.newName"));
  addPreset({ name, scope: "tags", rules: [] });
  select(name, false);
  startRename();
}

// A copy of the open preset under a new name — for a built-in, the only way to
// change it.
function duplicatePreset() {
  if (!current) return;
  if (renaming) { finishRename(); if (renaming) return; }
  commit();
  const source = findPreset(current.name, current.builtin);
  if (!source) return;
  const name = uniquePresetName(t("presets.copyName", { name: presetLabel(source) }));
  addPreset({
    name,
    scope: source.scope || "tags",
    // Materialize each rule's target, the way loading into a chain does.
    rules: (source.rules || []).map((r) => ({
      ...ruleForGroup(r),
      scope: r.scope || source.scope || "tags",
    })),
  });
  select(name, false);
  startRename();
}

function deletePreset() {
  if (!current || current.builtin) return;
  commit();
  const name = current.name;
  deletedPreset = findPreset(name, false);
  setActionGroups(actionGroups.filter(group => group.name !== name));
  persistActionGroups(); renderAllGroupsMenus(); notifyPresetsChanged({ from: name, to: null });
  current = null;
  const next = actionGroups[0] || builtinGroups[0]; select(next?.name || "", !!next?.builtin);
  el("pe-deleted-label").textContent = t("presets.deleted", { name }); el("pe-deleted").hidden = false;
  clearTimeout(deleteTimer); deleteTimer = setTimeout(() => { deletedPreset = null; el("pe-deleted").hidden = true; }, 10000);
  el("pe-undo-delete").focus();
}
function undoDelete() {
  if (!deletedPreset) return;
  const restored = { ...deletedPreset, name: uniquePresetName(deletedPreset.name) };
  deletedPreset = null; clearTimeout(deleteTimer); el("pe-deleted").hidden = true;
  addPreset(restored); select(restored.name, false);
  el("pe-list").querySelector(".pe-list-row.active")?.focus();
}
function showMenu(menu, anchor, point = null, align = "left") {
  closeMenus(false); menuReturnFocus = anchor;
  menu.hidden = false;
  if (point) placeContextMenu(menu, point.x, point.y);
  else placeFloating(menu, anchor, { align });
  queueMicrotask(() => { if (!menu.hidden) menu.querySelector('[role="menuitem"]:not([hidden]) button, button[role="menuitem"]:not([hidden])')?.focus({ preventScroll: true }); });
}

function closeMenus(restore = true) {
  for (const id of ["pe-preset-menu", "pe-rule-picker"]) el(id).hidden = true;
  if (restore && menuReturnFocus?.isConnected) menuReturnFocus.focus({ preventScroll: true });
}
function openRulePicker(event) {
  showMenu(el("pe-rule-picker"), event.currentTarget);
}

function editorOpen() {
  return !el("pe-editor").hidden;
}

// Open the editor, on `name` when given (the preset a row was showing), else on
// your first preset, else on the first built-in.
function openPresetEditor(name) {
  current = null;
  const target =
    (name && (findPreset(name, false) || findPreset(name, true))) || actionGroups[0] || builtinGroups[0];
  el("pe-editor").hidden = false;
  select(target ? target.name : "", target ? !!target.builtin : false);
}

function closePresetEditor() {
  if (!editorOpen()) return;
  if (renaming) { finishRename(); if (renaming) finishRename(true); }
  closeMenus(false);
  commit();
  clearTimeout(sampleTimer);
  sampleSeq++;
  el("pe-editor").hidden = true;
}

// ---- wire up ----

el("pe-preview-field").addEventListener("change", () => {
  previewField = el("pe-preview-field").value;
  if (sampleSource === "selection" || !sampleTyped) el("pe-sample-input").value = defaultSample();
  scheduleSample();
});
for (const source of ["example", "selection"]) el("pe-" + source).addEventListener("click", () => {
  sampleSource = source;
  sampleTyped = false;
  renderSampleSource();
  el("pe-sample-input").value = defaultSample();
  el("pe-sample-input").placeholder = defaultSample();
  scheduleSample();
});
el("pe-new").addEventListener("click", newPreset);
el("pe-copy-to-edit").addEventListener("click", duplicatePreset);
el("pe-undo-delete").addEventListener("click", undoDelete);
for (const id of ["pe-open-add", "pe-empty-add"]) el(id).addEventListener("click", openRulePicker);
el("pe-rule-picker").addEventListener("click", event => {
  const item = event.target.closest("[data-cmd]"); if (!item) return;
  closeMenus(false); chain.addRule(item.dataset.cmd);
});
el("pe-preset-menu").addEventListener("click", event => {
  const item = event.target.closest("[data-cmd]"); if (!item) return;
  closeMenus(false);
  if (item.dataset.cmd === "copy") duplicatePreset();
  else if (item.dataset.cmd === "rename") startRename();
  else if (item.dataset.cmd === "delete") deletePreset();
});
el("pe-editor-close").addEventListener("click", closePresetEditor);

el("pe-sample-input").addEventListener("input", () => {
  // Manual input, including an empty string, is previewed exactly as entered.
  sampleTyped = true;
  if (sampleSource === "selection") sampleSource = "example";
  scheduleSample();
});
// Every edit to the rules — typing a pattern, a flag, a case style, a target,
// a step switched off — bubbles here after the chain component has applied it.
for (const type of ["input", "change"]) {
  el("pe-rules-set").addEventListener(type, scheduleSample);
}
el("pe-rules-set").addEventListener("click", event => {
  if (event.target.closest(".seg-btn")) scheduleSample();
});
el("pe-editor").addEventListener("click", (e) => {
  if (e.target === el("pe-editor")) closePresetEditor();
});
// Capture phase, so Escape closes only this window and not the transform
// dialog it was opened from, which listens on the bubbling phase.
document.addEventListener(
  "keydown",
  (e) => {
    if (e.key === "Escape" && editorOpen() && el("confirm-modal").hidden) {
      if (renaming && e.target.id === "pe-name") return;
      if (document.querySelector('.app-select-trigger[aria-expanded="true"]')) return;
      const menus = ["pe-preset-menu", "pe-rule-picker"].map(el);
      e.preventDefault(); e.stopImmediatePropagation();
      if (menus.some(menu => !menu.hidden)) closeMenus(); else closePresetEditor();
    }
  },
  true,
);
document.addEventListener("pointerdown", event => {
  if (!event.target.closest("#pe-preset-menu, #pe-rule-picker, .pe-preset-more, #pe-open-add, #pe-empty-add")) closeMenus(false);
});
el("pe-list").addEventListener("contextmenu", event => {
  const pressed = pressedPresetMenu; pressedPresetMenu = null;
  if (pressed && Math.abs(event.clientX - pressed.x) < 4 && Math.abs(event.clientY - pressed.y) < 4) pressed.open(event);
}, true);
document.addEventListener("pointerdown", () => { pressedPresetMenu = null; }, true);
window.addEventListener("pointercancel", () => { pressedPresetMenu = null; });
window.addEventListener("resize", () => closeMenus(false));
window.addEventListener("beforeunload", commit);
// Structural edits update the count and preview without rebuilding inputs.
new MutationObserver(() => {
  // Structural edits update the sample; typing never rebuilds an input node.
  // Cards added, removed or reordered change the result too (#394).
  scheduleSample();
}).observe(el("pe-rules"), { childList: true });

export { onPresetsChanged, openPresetEditor };
