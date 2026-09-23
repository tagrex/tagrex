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
import { confirmDialog, el, fileName, toast } from "./dom.js";
import { t, tn } from "./i18n.js";
import { invoke } from "./invoke.js";
import {
  createRuleChain,
  persistActionGroups,
  renderAllGroupsMenus,
  ruleForGroup,
  uniquePresetName,
} from "./chain.js";
import { actionGroups, builtinGroups, selectedPaths, setActionGroups, tracks } from "./state.js";

const chain = createRuleChain({
  ids: {
    rules: "pe-rules",
    empty: "pe-empty",
    kind: "pe-kind",
    add: "pe-add",
    clear: "pe-clear",
  },
});

// Which preset is open: its name and whether it is a built-in. Null when there
// is none to show (no presets at all).
let current = null;
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

// Rename the open preset from the name field. An empty or taken name is
// refused and the field goes back to the name it had.
function commitName() {
  if (!current || current.builtin) return;
  const input = el("pe-name");
  const name = input.value.trim();
  if (name === current.name) {
    input.value = name;
    return;
  }
  if (!name || [...actionGroups, ...builtinGroups].some((g) => g.name === name)) {
    toast(name ? t("presets.nameTaken", { name }) : t("presets.nameEmpty"), true);
    input.value = current.name;
    return;
  }
  const old = current.name;
  setActionGroups(
    actionGroups
      .map((g) => (g.name === old ? { ...g, name } : g))
      .sort((a, b) => a.name.localeCompare(b.name)),
  );
  current = { name, builtin: false };
  persistActionGroups();
  renderAllGroupsMenus();
  notifyPresetsChanged({ from: old, to: name });
  renderList();
}

// Open a preset in the editor half, first saving the one being left.
function select(name, builtin) {
  commit();
  const group = findPreset(name, builtin);
  current = group ? { name, builtin } : null;
  const input = el("pe-name");
  input.value = group ? group.name : "";
  input.readOnly = !group || builtin;
  el("pe-builtin-note").hidden = !builtin;
  // A fieldset is the native way to make every control in the chain inert at
  // once — inputs, the scope pickers, the ↑/↓/✕ buttons, Add rule.
  el("pe-rules-set").disabled = !group || builtin;
  el("pe-delete").disabled = !group || builtin;
  el("pe-duplicate").disabled = !group;
  el("pe-edit-col").hidden = !group;
  chain.load(group || { rules: [] });
  renderList();
  // A sample the user typed stays; the default follows the preset (#394).
  if (!sampleTyped) el("pe-sample-input").value = defaultSample();
  scheduleSample();
}

// ---- the live example (#394) ----
// Which targets the open preset's rules aim at: the file name, tags, or both.
// A rule on the extension alone gets no line — there is no stem to show it on.
function sampleTargets() {
  const scopes = chain.asGroup().rules.map((r) => r.scope || "tags");
  const targets = [];
  if (scopes.some((sc) => sc !== "filename" && sc !== "fileext")) targets.push("tags");
  if (scopes.includes("filename")) targets.push("filename");
  return targets;
}

// Something worth trying the preset on: the first selected file's title for a
// preset aimed at tags, its name (without the extension) for one aimed at the
// file name, and a made-up but typical value when nothing is selected.
function defaultSample() {
  const path = selectedPaths()[0];
  const track = path && tracks.find((t) => t.path === path);
  const fileOnly = sampleTargets().every((target) => target === "filename");
  if (track) {
    const stem = fileName(track.path).replace(/\.[^.]+$/, "");
    return fileOnly ? stem : track.tags.title || stem;
  }
  return "Björk — Jóga (12\" Mix)";
}

let sampleTyped = false;
let sampleTimer = null;
let sampleSeq = 0;

function scheduleSample() {
  clearTimeout(sampleTimer);
  sampleTimer = setTimeout(renderSample, 150);
}

async function renderSample() {
  const out = el("pe-sample-out");
  const text = el("pe-sample-input").value;
  const targets = sampleTargets();
  const seq = ++sampleSeq;
  if (!targets.length) {
    out.textContent = t("presets.sampleNoRules");
    out.classList.add("muted");
    return;
  }
  const group = { ...chain.asGroup(), name: "" };
  const rows = await Promise.all(
    targets.map(async (target) => {
      try {
        return { target, value: await invoke("transform_sample", { group, text, target }) };
      } catch (e) {
        return { target, error: String(e) };
      }
    }),
  );
  if (seq !== sampleSeq) return; // a newer edit already asked
  out.classList.remove("muted");
  out.innerHTML = "";
  for (const row of rows) {
    const line = document.createElement("div");
    line.className = "pe-sample-line";
    const label = document.createElement("span");
    label.className = "pe-sample-target";
    label.textContent = t(row.target === "filename" ? "presets.target.file" : "presets.target.tags");
    const value = document.createElement("span");
    value.className = row.error ? "pe-sample-value error" : "pe-sample-value";
    value.textContent = row.error ? row.error : row.value;
    if (!row.error && row.value === text) value.classList.add("same");
    line.append(label, value);
    out.appendChild(line);
  }
}

function presetRow(group) {
  const row = document.createElement("button");
  row.type = "button";
  row.className = "pe-list-row";
  const open = current && current.name === group.name && current.builtin === !!group.builtin;
  row.classList.toggle("active", !!open);
  const name = document.createElement("span");
  name.className = "pe-list-name";
  name.textContent = group.name;
  const meta = document.createElement("span");
  meta.className = "pe-list-meta muted";
  meta.textContent = tn("unit.rule", (group.rules || []).length);
  row.append(name, meta);
  row.addEventListener("click", () => select(group.name, !!group.builtin));
  return row;
}

function renderList() {
  const box = el("pe-list");
  box.innerHTML = "";
  const head = (key) => {
    const h = document.createElement("div");
    h.className = "col-menu-sep";
    h.textContent = t(key);
    box.appendChild(h);
  };
  head("presets.yours");
  if (!actionGroups.length) {
    const empty = document.createElement("p");
    empty.className = "pe-list-empty muted";
    empty.textContent = t("presets.noneYet");
    box.appendChild(empty);
  }
  for (const g of actionGroups) box.appendChild(presetRow(g));
  if (builtinGroups.length) {
    head("chain.builtIn");
    for (const g of builtinGroups) box.appendChild(presetRow(g));
  }
}

function addPreset(group) {
  setActionGroups([...actionGroups, group].sort((a, b) => a.name.localeCompare(b.name)));
  persistActionGroups();
  renderAllGroupsMenus();
  notifyPresetsChanged();
}

function newPreset() {
  commit();
  const name = uniquePresetName(t("presets.newName"));
  addPreset({ name, scope: "tags", rules: [] });
  select(name, false);
  el("pe-name").focus();
  el("pe-name").select();
}

// A copy of the open preset under a new name — for a built-in, the only way to
// change it.
function duplicatePreset() {
  if (!current) return;
  commit();
  const source = findPreset(current.name, current.builtin);
  if (!source) return;
  const name = uniquePresetName(t("presets.copyName", { name: source.name }));
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
}

async function deletePreset() {
  if (!current || current.builtin) return;
  const name = current.name;
  const ok = await confirmDialog(t("presets.deleteConfirm", { name }), t("action.delete"));
  if (!ok) return;
  setActionGroups(actionGroups.filter((g) => g.name !== name));
  persistActionGroups();
  renderAllGroupsMenus();
  notifyPresetsChanged({ from: name, to: null });
  current = null;
  const next = actionGroups[0] || builtinGroups[0];
  select(next ? next.name : "", next ? !!next.builtin : false);
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
  commitName();
  commit();
  el("pe-editor").hidden = true;
}

// ---- wire up ----
el("pe-new").addEventListener("click", newPreset);
el("pe-duplicate").addEventListener("click", duplicatePreset);
el("pe-delete").addEventListener("click", deletePreset);
el("pe-editor-close").addEventListener("click", closePresetEditor);
el("pe-name").addEventListener("change", commitName);
el("pe-sample-input").addEventListener("input", () => {
  sampleTyped = true;
  scheduleSample();
});
// Every edit to the rules — typing a pattern, a flag, a case style, a target,
// a step switched off — bubbles here after the chain component has applied it.
for (const type of ["input", "change", "click"]) {
  el("pe-rules-set").addEventListener(type, scheduleSample);
}
el("pe-name").addEventListener("keydown", (e) => {
  if (e.key === "Enter") {
    e.preventDefault();
    commitName();
  }
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
      e.stopImmediatePropagation();
      closePresetEditor();
    }
  },
  true,
);
window.addEventListener("beforeunload", commit);
// The open preset's rule count in the list follows the cards as they are added
// and removed, not only once the preset is saved.
new MutationObserver(() => {
  const meta = el("pe-list").querySelector(".pe-list-row.active .pe-list-meta");
  if (meta) meta.textContent = tn("unit.rule", el("pe-rules").querySelectorAll(".rule-card").length);
  // Cards added, removed or reordered change the result too (#394).
  scheduleSample();
}).observe(el("pe-rules"), { childList: true });

export { onPresetsChanged, openPresetEditor };
