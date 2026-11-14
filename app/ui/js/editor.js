import { importedValue, recordLiveField, liveInputChanged } from "./live-preview.js";
import { attachGenrePicker } from "../lib/genre-picker";
import { createComponentElement } from "../lib/component-elements.js";
// The EDITOR field grid (#35, #143 split it out of app.js).
//
// The table only edits four columns, but every field the model knows is already
// in `tracks[].tags` — this exposes the rest, including custom ones. Edits are
// staged into the shared buffer and previewed with everything else, so nothing
// here writes.
import { confirmDialog, el, escapeHtml, ico, toast } from "./dom.js";
import { list, planText, t as tr, tn } from "./i18n.js";
import { invoke } from "./invoke.js";
import {
  edits,
  contentRevision,
  captureSelection,
  isSelectionCurrent,
  previewPlan,
  savedSettings,
  selectedPaths,
  setPreviewPlan,
  setPreviewSource,
  trackAt,
  tracks,
} from "./state.js";
import { EXTENDED_FIELDS, customLabel, fieldLabel as catalogueLabel } from "./fields.js";
import { isFieldLocked, lockButton, lockedFieldKeys, reflectLockButton } from "./locks.js";
import { hooks } from "./hooks.js";

// Fields the user actually touched in the dialog, staged until they confirm.
let stagedFields = new Map();

// The value a file currently shows for a field: a pending edit if there is one,
// otherwise what's on disk.
function currentFieldValue(path, key) {
  const pending = edits.get(path);
  if (pending && pending.has(key)) return pending.get(key);
  const imported = importedValue(path, key);
  if (imported !== undefined) return imported;
  const track = trackAt(path);
  return (track && track.tags[key]) || "";
}

// Preserve drafts and native input identity across table-only repaints. Hidden
// editors derive nothing; opening the tab catches up to the latest subject once.
let editorSubject = null;
let editorContent = null;
let editorPaths = new Set();
let editorLocks = "";
function hasEditorDraft() {
  const paths = selectedPaths();
  return stagedFields.size > 0 && paths.length === editorPaths.size && paths.every(path => editorPaths.has(path));
}
function resetFieldEditor() {
  stagedFields.clear();
  editorSubject = null;
  editorContent = null;
  editorPaths.clear();
  for (const input of el("fields-body").querySelectorAll('.field-input.dirty')) input.classList.remove('dirty');
}
function refreshFieldEditor({ force = false, resetDraft = false } = {}) {
  const paths = selectedPaths();
  const sameSubject = paths.length === editorPaths.size && paths.every(path => editorPaths.has(path));
  if (!sameSubject || resetDraft) {
    stagedFields.clear();
    for (const input of el("fields-body").querySelectorAll('.field-input.dirty')) input.classList.remove('dirty');
  }
  if (el("panel-tagger").hidden || el("subtab-editor").hidden) return;
  const locks = JSON.stringify(lockedFieldKeys().sort());
  const content = new Map(paths.map(path => [path, [trackAt(path)?.tags, trackAt(path)?.tag_blocks,
    JSON.stringify([...(edits.get(path) || [])])]]));
  const sameContent = sameSubject && editorContent?.size === content.size && [...content].every(([path, entry]) =>
    entry.every((value, at) => value === editorContent.get(path)?.[at]));
  if (!force && !resetDraft && sameSubject && isSelectionCurrent(editorSubject) && (sameContent || stagedFields.size)) {
    if (locks !== editorLocks) {
      reflectFieldLocks();
      editorLocks = locks;
    }
    return;
  }
  if (!sameSubject || resetDraft) stagedFields = new Map();
  editorPaths = new Set(paths);
  editorContent = content;
  editorLocks = locks;
  editorSubject = captureSelection();
  el("fields-count").textContent = paths.length ? `· ${tn("unit.file", paths.length)}` : "";
  renderTagBlocks(paths);
  renderFieldEditor(paths);
  hooks.refreshCoverWell();
}

// Keep raw drafts, focus and the opacity transition alive when only locks
// change. A new selection/content revision still gets a full, fresh render.
function reflectFieldLocks() {
  for (const row of el("fields-body").querySelectorAll(".fe-row")) {
    if (row.classList.contains('fe-compilation')) {
      const input = row.querySelector('input[type="checkbox"]');
      const keys = JSON.parse(row.dataset.compilationKeys);
      input.disabled = row.dataset.supported !== 'true' || keys.some(isFieldLocked);
      row.classList.toggle('locked', keys.length > 0 && keys.some(isFieldLocked));
      const button = row.querySelector('.fe-lock');
      if (button) reflectLockButton(button);
      continue;
    }
    const inputs = [...row.querySelectorAll(".field-input")];
    for (const input of inputs) input.disabled = !editorPaths.size || isFieldLocked(input.dataset.key);
    row.classList.toggle("locked", inputs.length > 0 && inputs.every(input => input.disabled));
    const button = row.querySelector(".fe-lock");
    if (button) reflectLockButton(button);
  }
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
  renderFixEncodingButton(line);
}

// The format dialog is reachable from every tool, independently of the Editor.
function refreshTagFormats() { renderTagBlocks(selectedPaths()); }

// "Fix encoding" (#698), only with a code page chosen: without one nothing here
// reads as anything but what the standard says, so there is nothing to fix.
function renderFixEncodingButton(line) {
  if (!savedSettings.legacy_code_page) return;
  const button = createComponentElement("button");
  button.type = "button";
  button.className = "text-btn";
  button.textContent = tr("blocks.fixEncoding");
  button.title = tr("blocks.fixEncodingTitle");
  button.addEventListener("click", previewFixEncoding);
  line.append(" ", button);
}

// Preview rewriting the selection's text in Unicode. The table already shows it
// as it will read, so the preview only says which files it touches. Undo puts
// the bytes back, so unlike a removal this needs no confirmation.
async function previewFixEncoding() {
  try {
    const plan = await invoke("preview_fix_encoding", { paths: selectedPaths() });
    if (plan.changes.length === 0) {
      toast(tr("toast.blocks.nothingToFix"));
      return;
    }
    setPreviewPlan(plan);
    setPreviewSource("blocks");
    hooks.renderPreview(previewPlan);
    toast(
      tr("toast.previewingWhat", {
        what: planText(plan).toLowerCase(),
        files: tn("unit.file", plan.changes.length),
      })
    );
  } catch (e) {
    toast(String(e), true);
  }
}

// A "Remove <block>" button per spare block the selection carries (#47).
//
// Only the blocks the app is NOT reading from: stripping the one the values
// come from is emptying the file, which is what CLEAR TAGS is for, and offering
// it here beside "Reading ID3v2" would read as the opposite of what it does.
function renderBlockStrippers(line, strippable) {
  for (const [kind, { label, files }] of strippable) {
    const button = createComponentElement("button");
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
        tr("action.remove"),
        { danger: true }
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
  const button = createComponentElement("button");
  button.type = "button";
  button.className = "text-btn";
  button.textContent = tr("blocks.convert");
  if (readKinds.size > 1) {
    button.disabled = true;
    button.title = tr("blocks.differentKinds");
  } else {
    button.title = tr("blocks.convertTitle");
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

  const go = createComponentElement("button");
  go.type = "button";
  go.className = "text-btn";
  go.textContent = tr("action.preview");
  go.addEventListener("click", () =>
    previewConvertTagBlock(from, kindSelect.value, revisionSelect.hidden ? null : revisionSelect.value)
  );
  const cancel = createComponentElement("button");
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
    return customLabel(name) || name;
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
    toast(
      tr("toast.previewingWhat", {
        what: planText(plan).toLowerCase(),
        files: tn("unit.file", plan.changes.length),
      })
    );
  } catch (e) {
    toast(String(e), true);
  }
}

// Everyday fields live in Core; the remaining fields and existing custom
// metadata live in Standard or Advanced.
const EDITOR_CORE_KEYS = ["artist", "title", "album", "albumartist", "track", "tracktotal", "disc", "disctotal", "year", "genre", "comment", "bpm", "key"];
// Core follows the tagging workflow: compact number pairs, identity fields,
// paired short fields, then multiline comment. Long fields keep a fixed rail.
const EDITOR_CORE_LAYOUT = [
  { duo: [
    { pair: ["track", "tracktotal"], key: "track" },
    { pair: ["disc", "disctotal"], key: "disc" },
  ] },
  { key: "title" },
  { key: "artist" },
  { key: "album" },
  { key: "albumartist" },
  { yearOptions: true },
  { key: "genre" },
  { key: "comment" },
  { inline: ["bpm", "key"] },
];
// Numeric/typed fields get a narrow right-aligned input and inline validation
// mirroring the backend's is_writable_value rule, so a bad value shows as an
// error as you type instead of only being rejected at apply.
const EDITOR_NUMERIC_KEYS = new Set(["track", "tracktotal", "disc", "disctotal", "bpm", "year"]);
// Whether each collapsible group is folded; persists across re-renders in a
// session. Core is open by default; More tags and Advanced start folded
// so the everyday form isn't buried under technical noise (#136).
const editorGroupCollapsed = { standard: true, advanced: true };

// A vinyl position as a track or disc number (#641), the backend's
// is_vinyl_position: one or two letters with up to three digits ("A", "B2",
// "Aa1"), or up to three digits with one letter ("12a").
const VINYL_POSITION = /^(?:[A-Za-z]{1,2}\d{0,3}|\d{1,3}[A-Za-z])$/;
// The files that store track and disc numbers as integers, so a vinyl
// position has nowhere to go there: MP4, by the backend's extensions for it.
const INTEGER_NUMBER_FILE = /\.(?:mp4|m4[abprv]|3gp)$/i;

// Validate a hand-typed value the same way the backend does (empty always OK —
// it clears the field). Returns { ok, hint } — hint is the rule shown on focus.
// `paths` are the files the value goes to, for the one rule that depends on
// the format.
function validateFieldValue(key, value, paths = []) {
  if (!EDITOR_NUMERIC_KEYS.has(key)) return { ok: true, hint: "" };
  const v = value.trim();
  if (key === "year") {
    const year = v.split("-")[0];
    return { ok: v === "" || /^\d{4}$/.test(year), hint: tr("editor.hintYear") };
  }
  if (key === "bpm") {
    return { ok: v === "" || /^\d+(\.\d+)?$/.test(v), hint: tr("editor.hintNumbers") };
  }
  // track / disc — a plain integer, or a vinyl position where the file keeps
  // the number as text.
  if (
    (key === "track" || key === "disc") &&
    VINYL_POSITION.test(v) &&
    !paths.some((path) => INTEGER_NUMBER_FILE.test(path))
  ) {
    return { ok: true, hint: tr("editor.hintNumbers") };
  }
  // tracktotal, and the rest — a plain integer.
  return { ok: v === "" || /^\d+$/.test(v), hint: tr("editor.hintNumbers") };
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
      : r.yearOptions ? r
      : r.inline ? { inline: r.inline.map(key => ({ key, label: catalogueLabel(key) })) }
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
  const compilationKeys = new Set(paths.map(path => trackAt(path)?.compilation_field).filter(Boolean));
  for (const key of [...customs].sort()) {
    if (compilationKeys.has(key)) continue; // the dedicated flag already edits this native field
    const raw = key.slice("custom:".length);
    const friendly = customLabel(raw.toUpperCase());
    if (friendly) promoted.push({ key, label: friendly });
    else advancedRows.push({ key, label: raw });
  }
  promoted.sort((a, b) => a.label.localeCompare(b.label));
  // Pair the two compact identifiers; every other extended/custom field keeps
  // its full row and original relative order.
  const catalogue = standardRows.find(row => row.key === "catalognumber");
  const isrc = standardRows.find(row => row.key === "isrc");
  if (catalogue && isrc) {
    standardRows.splice(standardRows.indexOf(catalogue), 1, { inline: [catalogue, isrc] });
    standardRows.splice(standardRows.indexOf(isrc), 1);
  }
  standardRows.push(...promoted);

  body.appendChild(fieldGroup(tr("editor.group.core"), coreRows, paths, "core"));
  body.appendChild(fieldGroup(tr("editor.group.standard"), standardRows, paths, "standard"));
  // Only surface Advanced when there are raw keys to show.
  if (advancedRows.length)
    body.appendChild(fieldGroup(tr("editor.group.advanced"), advancedRows, paths, "advanced"));
}

// All groups fold in place. Their session preferences are independent of the
// selected files; drafts remain tied to editorPaths in refreshFieldEditor.
function fieldGroup(title, rows, paths, kind) {
  const collapsible = kind !== 'core';
  const collapsed = collapsible && editorGroupCollapsed[kind];

  const group = document.createElement("div");
  group.className = "fe-group" + (collapsible ? " collapsible" : "");
  group.dataset.group = kind;
  if (collapsed) group.classList.add("collapsed");

  const head = collapsible ? createComponentElement("button") : document.createElement('div');
  head.className = "fe-group-head";
  head.textContent = title;
  head.type = "button";
  head.setAttribute("aria-expanded", String(!collapsed));
  head.setAttribute("aria-controls", `editor-group-${kind}`);
  const count = document.createElement("span");
  count.className = "fe-group-count";
  count.textContent = String(rows.reduce((total, row) =>
    total + (row.duo ? row.duo.reduce((fields, entry) => fields + entry.pair.length, 0) : row.inline?.length || 1), 0));
  const caret = document.createElement("span");
  caret.className = "fe-group-caret";
  caret.setAttribute("aria-hidden", "true");
  caret.innerHTML = ico("caret-down");
  head.append(" ", count, caret);
  if (collapsible) group.appendChild(head);

  const grid = document.createElement("div");
  grid.className = "fe-grid";
  grid.id = `editor-group-${kind}`;
  grid.inert = collapsed;
  for (const r of rows) {
    if (r.yearOptions) {
      const shared = document.createElement('div');
      shared.className = 'fe-year-options';
      shared.append(fieldRow('year', catalogueLabel('year'), paths), compilationRow(paths));
      grid.append(shared);
    } else if (r.inline) {
      const shared = document.createElement("div");
      shared.className = "fe-inline-fields";
      for (const field of r.inline) shared.append(fieldRow(field.key, field.label, paths));
      grid.append(shared);
    } else grid.appendChild(r.duo ? fieldDuoRow(r.duo, paths) : fieldRow(r.key, r.label, paths));
  }
  group.appendChild(grid);
  head.addEventListener("click", () => {
    const next = !editorGroupCollapsed[kind];
    editorGroupCollapsed[kind] = next;
    group.classList.toggle("collapsed", next);
    head.setAttribute("aria-expanded", String(!next));
    grid.inert = next;
  });
  return group;
}

function compilationRow(paths) {
  const row = document.createElement('div');
  row.className = 'fe-row fe-compilation';
  const byField = new Map();
  let supported = paths.length > 0;
  const values = [];
  for (const path of paths) {
    const field = trackAt(path)?.compilation_field;
    if (!field) { supported = false; continue; }
    if (!byField.has(field)) byField.set(field, []);
    byField.get(field).push(path);
    values.push(/^(1|true|yes)$/i.test(currentFieldValue(path, field).trim()));
  }
  const keys = [...byField.keys()];
  row.dataset.compilationKeys = JSON.stringify(keys);
  row.dataset.supported = String(supported);
  const marker = document.createElement('span'); marker.className = 'fe-marker';
  const label = document.createElement('label'); label.className = 'fe-compilation-label';
  const input = createComponentElement('input', { type: 'checkbox' });
  input.id = 'editor-compilation';
  input.checked = values.length > 0 && values.every(Boolean);
  input.indeterminate = values.some(Boolean) && !values.every(Boolean);
  input.disabled = !supported || keys.some(isFieldLocked);
  label.htmlFor = input.id;
  label.title = !supported ? tr('editor.compilationUnsupported') : input.indeterminate ? tr('editor.compilationMixed') : '';
  label.append(input, document.createTextNode(tr('editor.compilation')));
  input.addEventListener('change', () => {
    if (input.disabled) return;
    input.indeterminate = false;
    const value = input.checked ? '1' : '0';
    row.classList.add('dirty');
    label.title = '';
    for (const [field, targets] of byField) {
      stagedFields.set(field, value);
      recordLiveField(targets, field, value);
    }
  });
  row.append(marker, label);
  if (keys.length) row.append(lockButton(keys, tr('editor.compilation'), refreshFieldEditor));
  return row;
}

function sharedFieldValue(paths, key) {
  if (!paths.length) return '';
  const first = currentFieldValue(paths[0], key);
  for (let i = 1; i < paths.length; i++) if (currentFieldValue(paths[i], key) !== first) return null;
  return first;
}

function restoreDraft(input, row, key) {
  if (!stagedFields.has(key)) return;
  input.value = stagedFields.get(key);
  input.classList.remove("multiple");
  row.classList.remove("multiple");
  input.classList.add("dirty");
  row.classList.add("dirty");
}

function fieldRow(key, label, paths) {
  const shared = sharedFieldValue(paths, key);
  const numeric = EDITOR_NUMERIC_KEYS.has(key);

  const row = document.createElement("div");
  row.className = "fe-row";

  const marker = document.createElement("span");
  marker.className = "fe-marker";

  const labelCell = document.createElement("label");
  labelCell.className = "fe-label";
  labelCell.textContent = label;
  labelCell.title = label; // full name on hover — labels wrap, never truncate (#136)

  const cell = document.createElement("span");
  cell.className = "fe-cell" + (numeric ? " num" : "");

  const input = createComponentElement(key === "comment" ? "textarea" : "input");
  if (key !== "comment") input.type = "text";
  else { input.rows = 3; row.classList.add("fe-comment"); }
  input.className = "field-input" + (numeric ? " num" : "");
  input.dataset.key = key;
  input.id = `editor-field-${encodeURIComponent(key)}`;
  labelCell.htmlFor = input.id;
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
    input.placeholder = numeric ? '—' : tr("editor.mixedValue");
    if (numeric) input.title = tr("editor.multipleValues");
  } else {
    input.value = shared;
    input.placeholder = '';
  }

  const hint = document.createElement("span");
  hint.className = "fe-hint";

  // Reflect the field's state (dirty / error) on the row and input.
  const reflect = () => {
    const { ok } = validateFieldValue(key, input.value, paths);
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
    recordLiveField(paths, key, input.value, validateFieldValue(key, input.value, paths).ok);
  });

  // A pre-filled value that's already invalid (rare — the backend guards it)
  // should show its error state on first render too.
  restoreDraft(input, row, key);
  if (shared || stagedFields.has(key)) reflect();
  if (!paths.length) input.disabled = true;
  if (numeric) numericEntry(input, key);

  cell.append(input, hint);
  if (key === "genre") attachGenrePicker(input,
    // Suggestions come from saved library values. Including this pending draft
    // would offer an unfinished token ahead of its completion (Ho before House).
    () => tracks.map(track => track.tags?.genre ?? ""),
    () => savedSettings?.multi_value_separator ?? "; ");
  row.append(marker, labelCell, cell, lockButton([key], label, refreshFieldEditor));
  return row;
}

// A row of one or more "number / total" pairs on a single line, e.g.
// "Track [n] / [total]   Disc [n] / [total]" (#136). Each pair has a label
// above its inputs. Each sub-input
// stages and validates its own key; the row's dirty/error marker reflects any
// half — recomputed across every input so fixing one can't clear another's error.
function fieldDuoRow(pairs, paths) {
  const row = document.createElement("div");
  row.className = "fe-row duo";

  const marker = document.createElement("span");
  marker.className = "fe-marker";

  const value = document.createElement("span");
  value.className = "fe-duo";

  const inputs = [];
  const reflectRow = () => {
    let anyError = false;
    for (const inp of inputs) {
      const { ok } = validateFieldValue(inp.dataset.key, inp.value, paths);
      inp.classList.toggle("error", !ok);
      if (!ok) inp.setAttribute("aria-invalid", "true");
      else inp.removeAttribute("aria-invalid");
      anyError = anyError || !ok;
    }
    row.classList.toggle("error", anyError);
  };

  const makeSub = (key, aria) => {
    const shared = sharedFieldValue(paths, key);
    const input = createComponentElement("input");
    input.type = "text";
    input.className = "field-input num";
    input.dataset.key = key;
    input.id = `editor-field-${encodeURIComponent(key)}`;
    input.spellcheck = false;
    input.inputMode = "numeric";
    input.setAttribute("aria-label", aria);
    if (shared === null) {
      input.classList.add("multiple");
      input.placeholder = '—';
      input.title = tr("editor.multipleValues");
    } else {
      input.value = shared;
      input.placeholder = '';
    }
    if (!paths.length || isFieldLocked(key)) input.disabled = true;
    input.addEventListener("input", () => {
      stagedFields.set(key, input.value);
      input.classList.remove("multiple");
      input.classList.add("dirty");
      row.classList.add("dirty");
      reflectRow();
      recordLiveField(paths, key, input.value, validateFieldValue(key, input.value, paths).ok);
    });
    restoreDraft(input, row, key);
    numericEntry(input, key);
    inputs.push(input);
    return input;
  };

  pairs.forEach((p) => {
    const unit = document.createElement("span");
    unit.className = "fe-duo-unit";
    const inlineLabel = document.createElement("label");
    inlineLabel.className = "fe-duo-label";
    inlineLabel.htmlFor = `editor-field-${encodeURIComponent(p.pair[0])}`;
    inlineLabel.textContent = p.label;
    unit.append(inlineLabel);
    const group = document.createElement("span");
    group.className = "fe-pair";
    const sep = document.createElement("span");
    sep.className = "fe-pair-sep";
    sep.textContent = "/";
    group.append(
      makeSub(p.pair[0], tr("editor.pairNumber", { field: p.label })),
      sep,
      makeSub(p.pair[1], tr("editor.pairTotal", { field: p.label })),
    );
    unit.append(group);
    value.append(unit);
  });

  reflectRow(); // surface a pre-filled invalid value on first render (rare)
  // One lock for the number/total pair, aligned with the other field locks.
  // Track and Disc share one row and the existing grouped lock semantics.
  const keys = pairs.flatMap((p) => p.pair);
  const label = pairs.map((p) => p.label).join(" / ");
  if (keys.every((key) => isFieldLocked(key))) row.classList.add("locked");
  row.append(marker, value, lockButton(keys, label, refreshFieldEditor));
  return row;
}

// Push the staged fields into the shared pending-edits buffer for every
// selected file, then preview them alongside any other pending edits.
// Reject non-numeric entry before it reaches staging. Existing on-disk values
// remain readable until the user explicitly replaces them.
function numericEntry(input, key) {
  const valid = value => (key === 'bpm' ? /^(?:\d+(?:\.\d*)?)?$/ : /^\d*$/).test(value);
  let accepted = input.value;
  input.addEventListener('beforeinput', event => {
    if (event.isComposing || event.data == null || !event.inputType.startsWith('insert')) return;
    const next = input.value.slice(0, input.selectionStart) + event.data + input.value.slice(input.selectionEnd);
    if (!valid(next)) event.preventDefault();
  });
  input.addEventListener('input', event => {
    if (event.isComposing) return;
    if (!valid(input.value)) { input.value = accepted; event.stopImmediatePropagation(); }
    else accepted = input.value;
  }, true);
}

async function applyFieldEditor() {
  // Compatibility entry point for tools that formerly staged the Editor.
  liveInputChanged();
  await hooks.previewEdits();
}

export {
  currentFieldValue,
  hasEditorDraft,
  resetFieldEditor,
  refreshFieldEditor,
  refreshTagFormats,
  validateFieldValue,
  applyFieldEditor,
};
