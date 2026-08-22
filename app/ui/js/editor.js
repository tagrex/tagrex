// The EDITOR field grid (#35, #143 split it out of app.js).
//
// The table only edits four columns, but every field the model knows is already
// in `tracks[].tags` — this exposes the rest, including custom ones. Edits are
// staged into the shared buffer and previewed with everything else, so nothing
// here writes.
import { confirmDialog, el, escapeHtml, ico, toast } from "./dom.js";
import { list, t as tr, tn } from "./i18n.js";
import { invoke } from "./invoke.js";
import {
  edits,
  previewPlan,
  selection,
  selectedPaths,
  setPreviewPlan,
  setPreviewSource,
  trackAt,
  tracks,
} from "./state.js";
import { EXTENDED_FIELDS, KNOWN_CUSTOM_LABELS, fieldLabel as catalogueLabel } from "./fields.js";
import { isFieldLocked, lockButton } from "./locks.js";
import { hooks } from "./hooks.js";

// Fields the user actually touched in the dialog, staged until they confirm.
let stagedFields = new Map();

// The value a file currently shows for a field: a pending edit if there is one,
// otherwise what's on disk.
function currentFieldValue(path, key) {
  const pending = edits.get(path);
  if (pending && pending.has(key)) return pending.get(key);
  const track = trackAt(path);
  return (track && track.tags[key]) || "";
}

// Refresh the field-editor section of the TAGGER panel for the current
// selection (called on entering the mode). Staged-but-unapplied field changes
// are dropped on refresh — they only make sense against the selection they were
// typed for.
function refreshFieldEditor() {
  const paths = selectedPaths();
  stagedFields = new Map();
  el("fields-count").textContent = paths.length ? `— ${tn("unit.file", paths.length)}` : "";
  renderTagBlocks(paths);
  closeAddField(); // collapse the add-field row back to its idle affordance
  populateKnownFields();
  renderFieldEditor(paths);
  hooks.refreshCoverWell();
}

// Which tag block the values on show came out of, and what else the file
// carries (#47).
//
// Only worth a line when there is something to say, so it stays silent on the
// ordinary file with one block — which is nearly all of them — and speaks up
// exactly where the values can surprise you: a second block holds a second
// answer, and it is the one other software may be reading.
function renderTagBlocks(paths) {
  const line = el("fields-blocks");
  closeConvertPicker();
  if (paths.length === 0) {
    line.hidden = true;
    return;
  }
  // Counted per FILE, not per distinct wording: two files telling the same
  // story are two files, and a set of strings cannot say so.
  let files = 0;
  const wordings = new Map();
  // The blocks that could be stripped, keyed by kind so two files carrying the
  // same spare block offer one button, not two.
  const strippable = new Map();
  // The block the values come from. Conversion reads from it, so it has to be
  // the same kind across the selection or there is no single source to convert.
  const readKinds = new Set();
  for (const path of paths) {
    const blocks = trackAt(path)?.tag_blocks || [];
    if (blocks.length === 0) continue;
    const read = blocks.find((b) => b.read_from) || blocks[0];
    readKinds.add(read.kind);
    if (blocks.length < 2) continue;
    files++;
    const rest = blocks.filter((b) => b !== read);
    // The parts, not a sentence: the sentence is built from the catalogue at
    // render time, and the key is only here to tell one combination from
    // another (#50).
    wordings.set(`${read.label}|${rest.map((b) => b.label).join(",")}`, {
      read: read.label,
      rest: rest.map((b) => b.label),
    });
    for (const block of rest) {
      const entry = strippable.get(block.kind) || { label: block.label, files: 0 };
      entry.files++;
      strippable.set(block.kind, entry);
    }
  }
  if (readKinds.size === 0) {
    line.hidden = true;
    return;
  }
  line.hidden = false;
  if (!files) {
    // Nothing surprising to report, but the line is where converting lives, so
    // it states the plain fact instead of going silent. One block means the
    // values can't have come from anywhere else.
    const only = [...paths]
      .map((path) => (trackAt(path)?.tag_blocks || [])[0])
      .find(Boolean);
    line.textContent =
      readKinds.size === 1
        ? tr("blocks.carrying", { block: only.label })
        : tr("blocks.carryingEach");
  } else if (wordings.size === 1) {
    const only = [...wordings.values()][0];
    line.textContent =
      files === 1
        ? tr("blocks.readingOne", { read: only.read, rest: list(only.rest) })
        : tr("blocks.readingAll", { read: only.read, rest: list(only.rest), files });
  } else {
    line.textContent = tr("blocks.several", { files });
  }
  renderBlockStrippers(line, strippable);
  renderConvertButton(line, readKinds);
}

// A "Remove <block>" button per spare block the selection carries (#47).
//
// Only the blocks the app is NOT reading from: stripping the one the values
// come from is emptying the file, which is what CLEAR TAGS is for, and offering
// it here beside "Reading ID3v2" would read as the opposite of what it does.
function renderBlockStrippers(line, strippable) {
  for (const [kind, { label, files }] of strippable) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "text-btn";
    button.textContent = tr("blocks.remove", { block: label });
    button.title =
      files === 1
        ? tr("blocks.removeTitleOne", { block: label })
        : tr("blocks.removeTitleMany", { block: label, files: tn("unit.file", files) });
    button.addEventListener("click", () => previewRemoveTagBlock(kind, label));
    line.append(" ", button);
  }
}

// Preview stripping one tag block from the selection, through the normal
// preview/apply/undo path.
//
// Undo rebuilds the block from what it said rather than from its bytes, so for
// anything but ID3v1 a frame the model cannot express — a cue point, a rating —
// would not come back. That is worth a confirmation, not a footnote: the plan
// says which blocks are in that position and the dialog repeats it.
async function previewRemoveTagBlock(kind, label) {
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast(tr("toast.blocks.selectFirst", { block: label }), true);
    return;
  }
  try {
    const plan = await invoke("preview_remove_tag_block", { paths, kind });
    if (plan.changes.length === 0) {
      toast(tr("toast.blocks.noneCarry", { block: label }));
      return;
    }
    const inexact = plan.changes.some((change) =>
      (change.block_changes || []).some((block) => !block.exact)
    );
    if (inexact) {
      const ok = await confirmDialog(
        tr("blocks.removeConfirm", {
          block: label,
          files: tn("unit.file", plan.changes.length),
        }),
        tr("action.remove")
      );
      if (!ok) return;
    }
    setPreviewPlan(plan);
    setPreviewSource("blocks");
    hooks.renderPreview(previewPlan);
    toast(tr("toast.blocks.previewRemoval", { block: label, files: tn("unit.file", plan.changes.length) }));
  } catch (e) {
    toast(String(e), true);
  }
}

// The "Convert…" affordance beside the block line (#205).
//
// Only when every selected file reads from the same kind of block: the
// conversion takes its values from that block, and a selection reading from two
// different ones has no single source. Offering it anyway would convert half
// the files from somewhere the user didn't look at.
function renderConvertButton(line, readKinds) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "text-btn";
  button.textContent = tr("blocks.convert");
  if (readKinds.size > 1) {
    button.disabled = true;
    button.title = "The selected files read from different tag blocks — convert them separately";
  } else {
    button.title = "Write these tags as a different kind of tag block";
    button.addEventListener("click", () => openConvertPicker([...readKinds][0]));
  }
  line.append(" ", button);
}

function closeConvertPicker() {
  const box = el("fields-blocks-convert");
  box.hidden = true;
  box.replaceChildren();
}

// Fetch the targets for the current selection and offer them. Asked for on
// demand rather than on every selection change: it reads each selected file,
// and making a row click pay for a menu nobody opened is what #184 was about.
async function openConvertPicker(from) {
  const paths = selectedPaths();
  const box = el("fields-blocks-convert");
  let targets;
  try {
    targets = await invoke("tag_block_targets", { paths });
  } catch (e) {
    toast(String(e), true);
    return;
  }
  if (!targets.kinds.length) {
    toast(tr("toast.blocks.noCommonKind"), true);
    return;
  }

  box.replaceChildren();
  box.hidden = false;
  const kindSelect = document.createElement("select");
  for (const { kind, label } of targets.kinds) {
    kindSelect.append(new Option(label, kind));
  }
  kindSelect.value = from;

  // The revision only means anything for ID3v2, so it appears with it and
  // disappears with it rather than sitting there greyed out.
  const revisionSelect = document.createElement("select");
  for (const { kind, label } of targets.revisions) {
    revisionSelect.append(new Option(`ID3v${label}`, kind));
  }
  revisionSelect.value = "id3v24";
  const syncRevision = () => {
    revisionSelect.hidden = kindSelect.value !== "id3v2";
  };
  kindSelect.addEventListener("change", syncRevision);
  syncRevision();

  const go = document.createElement("button");
  go.type = "button";
  go.className = "text-btn";
  go.textContent = tr("action.preview");
  go.addEventListener("click", () =>
    previewConvertTagBlock(from, kindSelect.value, revisionSelect.hidden ? null : revisionSelect.value)
  );
  const cancel = document.createElement("button");
  cancel.type = "button";
  cancel.className = "text-btn";
  cancel.textContent = tr("action.cancel");
  cancel.addEventListener("click", closeConvertPicker);

  const label = document.createElement("span");
  label.className = "muted";
  label.textContent = tr("blocks.writeAs");
  box.append(label, kindSelect, revisionSelect, go, cancel);
}

// A storage key as the field editor writes it. A custom field is named the way
// the file spells it, and a key with no row of its own is shown as it comes —
// better a raw key in the warning than a field silently left out of it.
function fieldLabel(key) {
  if (key.startsWith("custom:")) {
    const name = key.slice(7);
    return KNOWN_CUSTOM_LABELS[name] || name;
  }
  return catalogueLabel(key);
}

// Preview a conversion, through the normal preview/apply/undo path.
//
// The confirmation is the point of this whole flow: the target block may have
// no room for a field the source held, and that is a loss the user has to see
// BEFORE it is staged, not discover afterwards in the diff. The backend works
// out what would go, per file, by putting the values through the conversion in
// memory — so the list is what will actually happen, not a guess.
async function previewConvertTagBlock(from, to, revision) {
  const paths = selectedPaths();
  try {
    const plan = await invoke("preview_convert_tag_block", { paths, from, to, revision });
    if (plan.changes.length === 0) {
      toast(tr("toast.blocks.noneCarryThat"));
      return;
    }
    // Switching an ID3v2 block between 2.3 and 2.4 restamps the header and
    // keeps every frame, so there is nothing to warn about. Every other
    // conversion REBUILDS the target block out of the values the app can see,
    // and what it cannot see — cue points, ratings, player-specific frames —
    // does not survive that. The computed list below names what is certainly
    // lost; the sentence covers what cannot be listed.
    const revisionOnly = from === to && to === "id3v2";
    if (!revisionOnly) {
      const lostFields = new Set();
      let lostPictures = 0;
      for (const change of plan.changes) {
        for (const block of change.block_changes || []) {
          for (const field of block.lost_fields || []) lostFields.add(field);
          lostPictures += block.lost_pictures || 0;
        }
      }
      const parts = [];
      if (lostFields.size) parts.push([...lostFields].map(fieldLabel).sort().join(", "));
      if (lostPictures) parts.push(tn("unit.embeddedImage", lostPictures));
      const drops = parts.length ? " " + tr("blocks.noRoomFor", { what: list(parts) }) : "";
      // Whether every block this touches can be put back exactly. An ID3v2
      // block is journaled as bytes (#206), so undo restores it frame for
      // frame; anything else is rebuilt from what the app can read, and what it
      // cannot read is gone for good. The two are different promises and the
      // dialog should not make the wrong one.
      const undoable = plan.changes.every((change) =>
        (change.block_changes || []).every((block) => block.exact)
      );
      const ok = await confirmDialog(
        tr("blocks.convertConfirm", {
          drops,
          undo: tr(undoable ? "blocks.convertUndoable" : "blocks.convertNotUndoable"),
          files: tn("unit.file", plan.changes.length),
        }),
        tr("action.convert")
      );
      if (!ok) return;
    }
    closeConvertPicker();
    setPreviewPlan(plan);
    setPreviewSource("blocks");
    hooks.renderPreview(previewPlan);
    toast(tr("toast.previewingWhat", { what: plan.description.toLowerCase(), files: tn("unit.file", plan.changes.length) }));
  } catch (e) {
    toast(String(e), true);
  }
}

// Suggest the custom field names already present on the selected files — the
// "known but not-yet-shown" fields — so common ones don't have to be retyped
// (#114). Feeds the ADD FIELD name input's datalist.
function populateKnownFields() {
  // Over the selection, not over the library (#184): this runs on every
  // selection change, and walking a few thousand files to read a handful of
  // them is most of what made a row click expensive.
  const names = new Set();
  for (const path of selection) {
    const track = trackAt(path);
    if (!track || !track.tags) continue;
    for (const key of Object.keys(track.tags)) {
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
      { pair: ["track", "tracktotal"], key: "track" },
      { pair: ["disc", "disctotal"], key: "disc" },
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
    const track = trackAt(path);
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

  const coreRows = EDITOR_CORE_LAYOUT.map((r) =>
    r.duo
      ? { duo: r.duo.map((d) => ({ ...d, label: catalogueLabel(d.key) })) }
      : { key: r.key, label: catalogueLabel(r.key) }
  );

  // Standard = the known extended fields, plus any custom keys we can name
  // (promoted, appended after the curated ones and sorted by label). The raw
  // rest — technical frames with no friendly name — drop into Advanced (#136).
  const standardRows = EXTENDED_FIELDS.filter(([key]) => !EDITOR_CORE_KEYS.includes(key)).map(
    ([key]) => ({ key, label: catalogueLabel(key) })
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

  body.appendChild(fieldGroup(tr("editor.group.core"), coreRows, paths, "core"));
  body.appendChild(fieldGroup(tr("editor.group.standard"), standardRows, paths, "standard"));
  // Only surface Advanced when there are raw keys to show.
  if (advancedRows.length)
    body.appendChild(fieldGroup(tr("editor.group.advanced"), advancedRows, paths, "advanced"));
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
  // A locked field cannot be typed into here either (#48). The backend is what
  // refuses the change; this is so the refusal is visible before the typing
  // rather than after it, as an edit that quietly failed to stage.
  if (isFieldLocked(key)) {
    row.classList.add("locked");
    input.disabled = true;
  }
  if (shared === null) {
    row.classList.add("multiple");
    input.classList.add("multiple");
    input.placeholder = tr("editor.multipleValues");
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
  row.append(marker, labelCell, cell, lockButton([key], label, refreshFieldEditor));
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
    if (isFieldLocked(key)) input.disabled = true;
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
  // One padlock for the whole row, in the same column every other row keeps it
  // in (#48). Not one per pair: four numbers on one line leave no room for two
  // more buttons — the second pair wraps onto a line of its own — and the
  // numbers are one idea anyway. Where a file sits in its release is what a
  // renumbering rewrites, and it rewrites the disc alongside the track.
  const keys = pairs.flatMap((p) => p.pair);
  const label = pairs.map((p) => p.label).join(" / ");
  if (keys.every((key) => isFieldLocked(key))) row.classList.add("locked");
  row.append(marker, labelCell, value, lockButton(keys, label, refreshFieldEditor));
  return row;
}

// Add a custom field row; it stages immediately so an empty-valued custom field
// can still be created.
function addCustomField() {
  const name = el("fields-new-name").value.trim();
  if (!name) {
    toast(tr("toast.editor.nameField"), true);
    return;
  }
  const key = name.startsWith("custom:") ? name : `custom:${name}`;
  stagedFields.set(key, el("fields-new-value").value);
  toast(tr("toast.editor.stagedCustom", { name }));
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
    toast(tr("toast.editor.selectFirst"), true);
    return;
  }
  if (stagedFields.size === 0) {
    toast(tr("toast.editor.nothingToStage"));
    return;
  }
  let changed = 0;
  for (const path of paths) {
    if (!edits.has(path)) edits.set(path, new Map());
    const fields = edits.get(path);
    for (const [key, value] of stagedFields) {
      // Skip no-ops so the preview stays honest.
      const track = trackAt(path);
      const onDisk = (track && track.tags[key]) || "";
      if (value === onDisk && !fields.has(key)) continue;
      fields.set(key, value);
      changed += 1;
    }
    if (fields.size === 0) edits.delete(path);
  }
  // Same as the import path (#186): the diff render that `previewEdits` triggers
  // replaces this one outright.
  refreshFieldEditor();
  await hooks.previewEdits();
  toast(
    changed
      ? tr("toast.editor.staged", {
          fields: tn("unit.field", stagedFields.size),
          files: tn("unit.file", paths.length),
        })
      : tr("toast.nothingChanged")
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
