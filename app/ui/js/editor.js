// The EDITOR field grid (#35, #143 split it out of app.js).
//
// The table only edits four columns, but every field the model knows is already
// in `tracks[].tags` — this exposes the rest, including custom ones. Edits are
// staged into the shared buffer and previewed with everything else, so nothing
// here writes.
import { el, escapeHtml, ico, toast } from "./dom.js";
import { edits, selection, selectedPaths, tracks } from "./state.js";
import { EXTENDED_FIELDS, KNOWN_CUSTOM_LABELS } from "./fields.js";
import { hooks } from "./hooks.js";

// Fields the user actually touched in the dialog, staged until they confirm.
let stagedFields = new Map();

// The value a file currently shows for a field: a pending edit if there is one,
// otherwise what's on disk.
function currentFieldValue(path, key) {
  const pending = edits.get(path);
  if (pending && pending.has(key)) return pending.get(key);
  const track = tracks.find((t) => t.path === path);
  return (track && track.tags[key]) || "";
}

// Refresh the field-editor section of the TAGGER panel for the current
// selection (called on entering the mode). Staged-but-unapplied field changes
// are dropped on refresh — they only make sense against the selection they were
// typed for.
function refreshFieldEditor() {
  const paths = selectedPaths();
  stagedFields = new Map();
  el("fields-count").textContent = paths.length ? `— ${paths.length} file(s)` : "";
  closeAddField(); // collapse the add-field row back to its idle affordance
  populateKnownFields();
  renderFieldEditor(paths);
  hooks.refreshCoverWell();
}

// Suggest the custom field names already present on the selected files — the
// "known but not-yet-shown" fields — so common ones don't have to be retyped
// (#114). Feeds the ADD FIELD name input's datalist.
function populateKnownFields() {
  const names = new Set();
  for (const t of tracks) {
    if (!selection.has(t.path) || !t.tags) continue;
    for (const key of Object.keys(t.tags)) {
      if (key.startsWith("custom:")) names.add(key.slice(7));
    }
  }
  el("fields-known").innerHTML = [...names]
    .sort()
    .map((n) => `<option value="${escapeHtml(n)}"></option>`)
    .join("");
}

// The ADD FIELD affordance (#114): idle shows just "+ Add field"; opening it
// reveals the inline name/value row.
function openAddField() {
  el("fields-add-toggle").hidden = true;
  el("fields-add-row").hidden = false;
  el("fields-new-name").focus();
}
function closeAddField() {
  el("fields-add-row").hidden = true;
  el("fields-add-toggle").hidden = false;
  el("fields-new-name").value = "";
  el("fields-new-value").value = "";
}

// The core group is what a DJ touches every session; the rest (+ custom fields)
// lives in a collapsible Extended group (#editor design pass, Q2).
const EDITOR_CORE_KEYS = ["artist", "title", "album", "albumartist", "track", "tracktotal", "disc", "disctotal", "year", "genre"];
// How the Core group lays out: singles, plus the two number/total pairs rendered
// as one row each — "Track [n] / [total]", "Disc [n] / [total]" — mirroring a
// reference tagger's combined NN/TT presentation (#136 pass 2).
const EDITOR_CORE_LAYOUT = [
  {
    duo: [
      { pair: ["track", "tracktotal"], label: "Track" },
      { pair: ["disc", "disctotal"], label: "Disc" },
    ],
  },
  { key: "artist" },
  { key: "title" },
  { key: "album" },
  { key: "albumartist" },
  { key: "year" },
  { key: "genre" },
];
// Numeric/typed fields get a narrow right-aligned input and inline validation
// mirroring the backend's is_writable_value rule, so a bad value shows as an
// error as you type instead of only being rejected at apply.
const EDITOR_NUMERIC_KEYS = new Set(["track", "tracktotal", "disc", "bpm", "year"]);
// Whether each collapsible group is folded; persists across re-renders in a
// session. Standard is open by default; Advanced (raw key/value) starts folded
// so the everyday form isn't buried under technical noise (#136).
let editorStandardCollapsed = false;
let editorAdvancedCollapsed = true;

// Validate a hand-typed value the same way the backend does (empty always OK —
// it clears the field). Returns { ok, hint } — hint is the rule shown on focus.
function validateFieldValue(key, value) {
  if (!EDITOR_NUMERIC_KEYS.has(key)) return { ok: true, hint: "" };
  const v = value.trim();
  if (key === "year") {
    const year = v.split("-")[0];
    return { ok: v === "" || /^\d{4}$/.test(year), hint: "4-digit year" };
  }
  if (key === "bpm") {
    return { ok: v === "" || /^\d+(\.\d+)?$/.test(v), hint: "numbers only" };
  }
  // track / tracktotal / disc — a plain integer.
  return { ok: v === "" || /^\d+$/.test(v), hint: "numbers only" };
}

function renderFieldEditor(paths) {
  const body = el("fields-body");
  body.innerHTML = "";

  // Well-known fields plus any custom keys present anywhere in the selection —
  // both on disk and among pending edits, so a custom field staged a moment ago
  // is still listed when the dialog is reopened.
  const customs = new Set();
  for (const path of paths) {
    const track = tracks.find((t) => t.path === path);
    if (track) {
      for (const key of Object.keys(track.tags)) {
        if (key.startsWith("custom:")) customs.add(key);
      }
    }
    const pending = edits.get(path);
    if (pending) {
      for (const key of pending.keys()) {
        if (key.startsWith("custom:")) customs.add(key);
      }
    }
  }

  const labelOf = new Map(EXTENDED_FIELDS);
  const coreRows = EDITOR_CORE_LAYOUT.map((r) =>
    r.duo ? { duo: r.duo } : { key: r.key, label: labelOf.get(r.key) }
  );

  // Standard = the known extended fields, plus any custom keys we can name
  // (promoted, appended after the curated ones and sorted by label). The raw
  // rest — technical frames with no friendly name — drop into Advanced (#136).
  const standardRows = EXTENDED_FIELDS.filter(([key]) => !EDITOR_CORE_KEYS.includes(key)).map(
    ([key, label]) => ({ key, label })
  );
  const promoted = [];
  const advancedRows = [];
  for (const key of [...customs].sort()) {
    const raw = key.slice("custom:".length);
    const friendly = KNOWN_CUSTOM_LABELS[raw.toUpperCase()];
    if (friendly) promoted.push({ key, label: friendly });
    else advancedRows.push({ key, label: raw });
  }
  promoted.sort((a, b) => a.label.localeCompare(b.label));
  standardRows.push(...promoted);

  body.appendChild(fieldGroup("Core", coreRows, paths, "core"));
  body.appendChild(fieldGroup("Standard", standardRows, paths, "standard"));
  // Only surface Advanced when there are raw keys to show.
  if (advancedRows.length) body.appendChild(fieldGroup("Advanced", advancedRows, paths, "advanced"));
}

// Build one field group (a header + its rows). `kind` is "core" (always open),
// "standard", or "advanced" (both fold, tracking their own collapse flag). The
// Advanced group renders raw key/value rows (#136).
function fieldGroup(title, rows, paths, kind) {
  const collapsible = kind !== "core";
  const advanced = kind === "advanced";
  const collapsed = advanced ? editorAdvancedCollapsed : kind === "standard" ? editorStandardCollapsed : false;

  const group = document.createElement("div");
  group.className = "fe-group" + (collapsible ? " collapsible" : "");
  if (collapsed) group.classList.add("collapsed");

  const head = document.createElement("div");
  head.className = "fe-group-head";
  head.textContent = title;
  if (collapsible) {
    const count = document.createElement("span");
    count.className = "fe-group-count";
    count.textContent = String(rows.length);
    const caret = document.createElement("span");
    caret.className = "fe-group-caret";
    caret.innerHTML = ico("caret-down");
    head.append(" ", count, caret);
    head.addEventListener("click", () => {
      const next = !group.classList.contains("collapsed");
      group.classList.toggle("collapsed", next);
      if (advanced) editorAdvancedCollapsed = next;
      else editorStandardCollapsed = next;
    });
  }
  group.appendChild(head);

  const grid = document.createElement("div");
  grid.className = "fe-grid";
  for (const r of rows) {
    grid.appendChild(r.duo ? fieldDuoRow(r.duo, paths) : fieldRow(r.key, r.label, paths));
  }
  group.appendChild(grid);
  return group;
}

function fieldRow(key, label, paths) {
  const values = new Set(paths.map((path) => currentFieldValue(path, key)));
  const shared = values.size === 1 ? [...values][0] : null;
  const numeric = EDITOR_NUMERIC_KEYS.has(key);

  const row = document.createElement("div");
  row.className = "fe-row";

  const marker = document.createElement("span");
  marker.className = "fe-marker";

  const labelCell = document.createElement("span");
  labelCell.className = "fe-label";
  labelCell.textContent = label;
  labelCell.title = label; // full name on hover — labels wrap, never truncate (#136)

  const cell = document.createElement("span");
  cell.className = "fe-cell" + (numeric ? " num" : "");

  const input = document.createElement("input");
  input.type = "text";
  input.className = "field-input" + (numeric ? " num" : "");
  input.dataset.key = key;
  input.spellcheck = false;
  if (numeric) input.inputMode = "numeric";
  if (shared === null) {
    row.classList.add("multiple");
    input.classList.add("multiple");
    input.placeholder = "<multiple values>";
  } else {
    input.value = shared;
  }

  const hint = document.createElement("span");
  hint.className = "fe-hint";
  const { hint: ruleText } = validateFieldValue(key, "");
  if (ruleText) hint.innerHTML = `<span class="rule">${escapeHtml(ruleText)}</span>`;

  // Reflect the field's state (dirty / error) on the row and input.
  const reflect = () => {
    const { ok } = validateFieldValue(key, input.value);
    row.classList.toggle("error", !ok);
    input.classList.toggle("error", !ok);
    if (!ok) input.setAttribute("aria-invalid", "true");
    else input.removeAttribute("aria-invalid");
  };

  input.addEventListener("input", () => {
    stagedFields.set(key, input.value);
    row.classList.remove("multiple");
    input.classList.remove("multiple");
    row.classList.add("dirty");
    input.classList.add("dirty");
    reflect();
  });

  // A pre-filled value that's already invalid (rare — the backend guards it)
  // should show its error state on first render too.
  if (shared) reflect();

  cell.append(input, hint);
  row.append(marker, labelCell, cell);
  return row;
}

// A row of one or more "number / total" pairs on a single line, e.g.
// "Track [n] / [total]   Disc [n] / [total]" (#136). The first pair's label sits
// in the aligned label column; later pairs get an inline label. Each sub-input
// stages and validates its own key; the row's dirty/error marker reflects any
// half — recomputed across every input so fixing one can't clear another's error.
function fieldDuoRow(pairs, paths) {
  const row = document.createElement("div");
  row.className = "fe-row duo";

  const marker = document.createElement("span");
  marker.className = "fe-marker";

  const labelCell = document.createElement("span");
  labelCell.className = "fe-label";
  labelCell.textContent = pairs[0].label;
  labelCell.title = pairs[0].label;

  const value = document.createElement("span");
  value.className = "fe-duo";

  const inputs = [];
  const reflectRow = () => {
    let anyError = false;
    for (const inp of inputs) {
      const { ok } = validateFieldValue(inp.dataset.key, inp.value);
      inp.classList.toggle("error", !ok);
      if (!ok) inp.setAttribute("aria-invalid", "true");
      else inp.removeAttribute("aria-invalid");
      anyError = anyError || !ok;
    }
    row.classList.toggle("error", anyError);
  };

  const makeSub = (key, label, aria) => {
    const values = new Set(paths.map((path) => currentFieldValue(path, key)));
    const shared = values.size === 1 ? [...values][0] : null;
    const input = document.createElement("input");
    input.type = "text";
    input.className = "field-input num";
    input.dataset.key = key;
    input.spellcheck = false;
    input.inputMode = "numeric";
    input.setAttribute("aria-label", `${label} ${aria}`);
    if (shared === null) {
      input.classList.add("multiple");
      input.placeholder = "—"; // narrow numeric box: a dash reads better than "<multiple values>"
    } else {
      input.value = shared;
    }
    input.addEventListener("input", () => {
      stagedFields.set(key, input.value);
      input.classList.remove("multiple");
      input.classList.add("dirty");
      row.classList.add("dirty");
      reflectRow();
    });
    inputs.push(input);
    return input;
  };

  pairs.forEach((p, i) => {
    const unit = document.createElement("span");
    unit.className = "fe-duo-unit";
    // The first pair's label is the row's aligned column label; the rest inline.
    if (i > 0) {
      const inlineLabel = document.createElement("span");
      inlineLabel.className = "fe-duo-label";
      inlineLabel.textContent = p.label;
      unit.append(inlineLabel);
    }
    const group = document.createElement("span");
    group.className = "fe-pair";
    const sep = document.createElement("span");
    sep.className = "fe-pair-sep";
    sep.textContent = "/";
    group.append(makeSub(p.pair[0], p.label, "number"), sep, makeSub(p.pair[1], p.label, "total"));
    unit.append(group);
    value.append(unit);
  });

  reflectRow(); // surface a pre-filled invalid value on first render (rare)
  row.append(marker, labelCell, value);
  return row;
}

// Add a custom field row; it stages immediately so an empty-valued custom field
// can still be created.
function addCustomField() {
  const name = el("fields-new-name").value.trim();
  if (!name) {
    toast("Name the custom field first", true);
    return;
  }
  const key = name.startsWith("custom:") ? name : `custom:${name}`;
  stagedFields.set(key, el("fields-new-value").value);
  toast(`Staged custom field "${name}" — press Stage changes to apply`);
  // Keep the row open and refocus the name so several fields add in a row (#114).
  el("fields-new-name").value = "";
  el("fields-new-value").value = "";
  el("fields-new-name").focus();
}

// Push the staged fields into the shared pending-edits buffer for every
// selected file, then preview them alongside any other pending edits.
async function applyFieldEditor() {
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast("Select the tracks to edit first", true);
    return;
  }
  if (stagedFields.size === 0) {
    toast("No field changes to stage");
    return;
  }
  let changed = 0;
  for (const path of paths) {
    if (!edits.has(path)) edits.set(path, new Map());
    const fields = edits.get(path);
    for (const [key, value] of stagedFields) {
      // Skip no-ops so the preview stays honest.
      const track = tracks.find((t) => t.path === path);
      const onDisk = (track && track.tags[key]) || "";
      if (value === onDisk && !fields.has(key)) continue;
      fields.set(key, value);
      changed += 1;
    }
    if (fields.size === 0) edits.delete(path);
  }
  hooks.renderTracks();
  refreshFieldEditor();
  await hooks.previewEdits();
  toast(
    changed
      ? `Staged ${stagedFields.size} field(s) across ${paths.length} file(s)`
      : "Nothing changed"
  );
}

export {
  currentFieldValue,
  refreshFieldEditor,
  openAddField,
  closeAddField,
  validateFieldValue,
  addCustomField,
  applyFieldEditor,
};
