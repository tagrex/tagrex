// The GENERATOR mode (#143 split it out of app.js).
//
// Everything that mass-produces values for the selection: the transformation
// rule chain (#34), auto-numbering (#39), the vinyl side -> disc mapping (#105),
// and the named action groups (#57) with the shipped preset library (#137).
// They share this panel and nothing else in the UI needs their internals.
import { confirmDialog, el, escapeHtml, ico, toast } from "./dom.js";
import { invoke } from "./invoke.js";
import { enablePointerReorder } from "./reorder.js";
import { hooks } from "./hooks.js";
import { currentFieldValue } from "./editor.js";
import { groupKeyOf } from "./grouping.js";
import {
  actionGroups,
  groupBy,
  setActionGroups,
  builtinGroups,
  setBuiltinGroups,
  edits,
  savedSettings,
  setSavedSettings,
  selectedPaths,
  setPreviewPlan,
  setPreviewSource,
  tracks,
} from "./state.js";

// ---- transformations (#34) ----
// An ordered chain of cleanup rules applied to tags or filenames. The rules
// live here only for the length of the dialog; naming and saving chains is
// tracked separately (#57).
let transformRules = [];
// Stable per-rule id, so pointer-based reorder (#88) can key on identity rather
// than a shifting array index.
let ruleIdCounter = 0;

// Refresh the GENERATOR panel for the current selection (called on entering the
// mode). The rule chain persists across mode switches within a session.
function refreshGenerator() {
  const count = selectedPaths().length;
  el("transform-count").textContent = count ? `— ${count} file(s)` : "";
  el("autonum-count").textContent = count ? `— ${count} selected` : "";
  el("vinyl-count").textContent = count ? `— ${count} selected` : "";
  renderTransformRules();
}

// ---- auto-number selected tracks (#39) ----
// A "meaningful non-numeric position" — a vinyl side like "A1"/"B2" or a bare
// side letter — that we preserve rather than flatten: non-empty and not a plain
// run of digits.
function isVinylSide(value) {
  const v = (value || "").trim();
  return v !== "" && !/^\d+$/.test(v);
}

// Fill TrackNumber across the selection (mapping order) into the pending-edits
// buffer, then preview — so it flows through the usual apply/undo path. Options:
// start value, optional TrackTotal + disc, per-group restart when a grouping is
// active, and preserving existing vinyl-side positions.
async function numberTracks() {
  const paths = selectedPaths(); // mapping order; reads `selection`, survives re-render
  if (paths.length === 0) {
    toast("Select the tracks to number first", true);
    return;
  }
  const start = Math.max(0, Math.floor(Number(el("autonum-start").value) || 1));
  const writeTotal = el("autonum-total").checked;
  const perGroup = el("autonum-per-group").checked && !!groupBy;
  const keepSides = el("autonum-keep-sides").checked;
  const discRaw = el("autonum-disc").value.trim();
  if (discRaw && !/^\d+$/.test(discRaw)) {
    toast("Disc # must be a whole number", true);
    return;
  }
  const disc = discRaw ? String(Number(discRaw)) : "";

  const trackByPath = new Map(tracks.map((t) => [t.path, t]));
  // Assign a number to each writable file. A preserved vinyl side neither gets a
  // number nor consumes one, so the rest stay contiguous.
  const assigned = []; // { path, number, gkey }
  const groupNext = new Map(); // groupKey -> next number
  let flat = start;
  for (const path of paths) {
    const track = trackByPath.get(path);
    if (!track) continue;
    if (keepSides && isVinylSide(track.tags.track)) continue;
    const gkey = perGroup ? groupKeyOf(track) : "";
    let n;
    if (perGroup) {
      n = groupNext.has(gkey) ? groupNext.get(gkey) : start;
      groupNext.set(gkey, n + 1);
    } else {
      n = flat++;
    }
    assigned.push({ path, number: n, gkey });
  }
  if (assigned.length === 0) {
    toast("Nothing to number — every selected position was preserved");
    return;
  }

  // TrackTotal (unpadded): per group when restarting, else the whole run.
  const groupTotals = new Map();
  if (writeTotal && perGroup) {
    for (const a of assigned) groupTotals.set(a.gkey, (groupTotals.get(a.gkey) || 0) + 1);
  }

  // The track-number tag is stored as a plain integer on every format (lofty
  // normalizes it), so zero-padding can't persist here — pad file names instead
  // via the RENAMER (%track:2%). We write the plain number.
  for (const a of assigned) {
    if (!edits.has(a.path)) edits.set(a.path, new Map());
    const fields = edits.get(a.path);
    fields.set("track", String(a.number));
    if (writeTotal) fields.set("tracktotal", String(perGroup ? groupTotals.get(a.gkey) : assigned.length));
    if (disc) fields.set("disc", disc);
  }
  hooks.renderTracks();
  await hooks.previewEdits();
  toast(`Numbered ${assigned.length} track(s)${perGroup ? " (restarted per group)" : ""}`);
}

// ---- vinyl sides -> disc (#105) ----
// Parse a vinyl-side track value into its side (as a disc ordinal, A=1, B=2, …)
// and its per-side track digits. Handles a bare side ("B"), side-first "A1", and
// the reverse "1A"; any numeric part must be plain digits. Returns
// { disc, track } where `track` is the digit string or null (a bare side has no
// digit — the caller supplies a track number). Returns null for a plain number
// or non-vinyl value. Mirrors `side_disc_from_position` in the backend.
function parseVinylPosition(value) {
  const v = String(value || "").trim();
  if (v.length < 1) return null;
  let side;
  let num;
  if (/[A-Za-z]/.test(v[0]) && /^\d*$/.test(v.slice(1))) {
    side = v[0]; // "B", "A1"
    num = v.slice(1);
  } else if (v.length >= 2 && /[A-Za-z]/.test(v[v.length - 1]) && /^\d+$/.test(v.slice(0, -1))) {
    side = v[v.length - 1]; // reverse "1A", "12B"
    num = v.slice(0, -1);
  } else {
    return null;
  }
  const disc = side.toUpperCase().charCodeAt(0) - 64; // 'A' -> 1
  if (disc < 1 || disc > 26) return null;
  return { disc: String(disc), track: num ? String(parseInt(num, 10)) : null };
}

// Decompose vinyl-side track values in the selection into a plain track number
// plus a disc number, staged into the pending-edits buffer. For files already
// tagged "A1"/"B2" (e.g. by another tool) — the side can't live in the integer
// track tag, so it moves to the disc field. A bare side ("B", the whole side is
// one track) has no digit, so its track becomes 1.
async function splitVinylSides() {
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast("Select the tracks to split first", true);
    return;
  }
  let changed = 0;
  for (const path of paths) {
    const parsed = parseVinylPosition(currentFieldValue(path, "track"));
    if (!parsed) continue;
    if (!edits.has(path)) edits.set(path, new Map());
    const fields = edits.get(path);
    fields.set("track", parsed.track ?? "1");
    fields.set("disc", parsed.disc);
    changed += 1;
  }
  if (changed === 0) {
    toast("No vinyl-side values (A1, B2) in the selection");
    return;
  }
  hooks.renderTracks();
  await hooks.previewEdits();
  toast(`Split ${changed} vinyl position(s) into track + disc`);
}

function addTransformRule() {
  const kind = el("transform-kind").value;
  transformRules.push({
    id: ++ruleIdCounter,
    kind,
    from: "",
    to: "",
    regex: false,
    whole_word: false,
    case_sensitive: false,
    style: kind === "case" ? "title" : kind === "key" ? "camelot" : "",
    enabled: true,
  });
  renderTransformRules();
}

function mkRuleIcon(iconName, title, disabled, onClick) {
  const b = document.createElement("button");
  b.className = "icon";
  b.innerHTML = ico(iconName);
  b.title = title;
  b.setAttribute("aria-label", title);
  b.disabled = disabled;
  b.addEventListener("click", onClick);
  return b;
}

function moveRule(from, to) {
  if (to < 0 || to >= transformRules.length) return;
  [transformRules[from], transformRules[to]] = [transformRules[to], transformRules[from]];
  renderTransformRules();
}

function renderTransformRules() {
  const body = el("transform-rules");
  body.innerHTML = "";
  el("transform-empty").hidden = transformRules.length > 0;

  transformRules.forEach((rule, index) => {
    const card = document.createElement("div");
    card.className = "rule-card";
    // A disabled step (#57) stays in the chain but is skipped and shown dimmed.
    card.classList.toggle("rule-disabled", rule.enabled === false);
    card.dataset.index = index;
    card.dataset.key = rule.id; // identity key for pointer reorder (#88)

    // ---- header: grip · n · kind · ↑ ↓ ✕ ----
    const head = document.createElement("div");
    head.className = "rule-head";

    const grip = document.createElement("span");
    grip.className = "rule-grip";
    grip.innerHTML = ico("grip");
    grip.title = "Drag to reorder";
    // Order is semantic (case before/after an acronym fix differs). Pointer-based
    // reorder — WKWebView's HTML5 DnD is unreliable (#88); ↑/↓ stay as fallback.
    enablePointerReorder(grip, card, el("transform-rules"), ".rule-card", (draggedKey, targetKey, below) => {
      const dragged = transformRules.find((r) => String(r.id) === draggedKey);
      if (!dragged) return;
      const order = transformRules.filter((r) => r !== dragged);
      let to = order.findIndex((r) => String(r.id) === targetKey);
      if (to < 0) return;
      if (below) to += 1;
      order.splice(to, 0, dragged);
      transformRules = order;
      renderTransformRules();
    });

    const n = document.createElement("span");
    n.className = "rule-n";
    n.textContent = index + 1;

    const kind = document.createElement("span");
    kind.className = "rule-kind";
    kind.textContent =
      rule.kind === "replace"
        ? "Find and replace"
        : rule.kind === "case"
          ? "Change case"
          : rule.kind === "key"
            ? "Key notation"
            : rule.kind === "transliterate"
              ? "Transliterate to Latin"
              : rule.kind === "untransliterate"
                ? "Transliterate to Cyrillic"
                : "Remove diacritics";

    const spacer = document.createElement("span");
    spacer.className = "spacer";

    const acts = document.createElement("span");
    acts.className = "rule-acts";
    // Enable/disable this step (#57): kept in the chain either way, skipped when off.
    const toggle = mkRuleIcon(
      "check",
      rule.enabled === false ? "Step off — click to enable" : "Step on — click to disable",
      false,
      () => {
        rule.enabled = rule.enabled === false;
        renderTransformRules();
      }
    );
    toggle.classList.add("rule-toggle");
    if (rule.enabled === false) toggle.classList.add("off");
    acts.append(toggle);
    // ↑/↓ stay as the keyboard / no-pointer fallback for reordering.
    acts.append(
      mkRuleIcon("caret-up", "Move up", index === 0, () => moveRule(index, index - 1)),
      mkRuleIcon("caret-down", "Move down", index === transformRules.length - 1, () =>
        moveRule(index, index + 1)
      )
    );
    const remove = mkRuleIcon("close", "Remove rule", false, () => {
      transformRules.splice(index, 1);
      renderTransformRules();
    });
    remove.classList.add("rm");
    acts.append(remove);

    head.append(grip, n, kind, spacer, acts);
    card.append(head);

    // ---- body (per-kind); diacritics is header-only ----
    if (rule.kind === "replace") {
      const b = document.createElement("div");
      b.className = "rule-body";
      const fields = document.createElement("div");
      fields.className = "rule-fields";
      const from = document.createElement("input");
      from.type = "text";
      from.placeholder = "find";
      from.value = rule.from;
      from.spellcheck = false;
      from.addEventListener("input", () => (rule.from = from.value));
      const to = document.createElement("input");
      to.type = "text";
      to.placeholder = "replace with";
      to.value = rule.to;
      to.spellcheck = false;
      to.addEventListener("input", () => (rule.to = to.value));
      fields.append(from, to);

      const flags = document.createElement("div");
      flags.className = "rule-flags";
      for (const [key, text, hint] of [
        ["regex", "regex", "Treat the pattern as a regular expression"],
        ["whole_word", "whole word", "Only match complete words"],
        ["case_sensitive", "match case", "Distinguish upper and lower case"],
      ]) {
        const label = document.createElement("label");
        label.className = "rule-flag";
        label.title = hint;
        const box = document.createElement("input");
        box.type = "checkbox";
        box.checked = rule[key];
        box.addEventListener("change", () => (rule[key] = box.checked));
        label.append(box, document.createTextNode(text));
        flags.appendChild(label);
      }
      b.append(fields, flags);
      card.append(b);
    } else if (rule.kind === "case") {
      const b = document.createElement("div");
      b.className = "rule-body";
      const seg = document.createElement("div");
      seg.className = "seg";
      for (const [value, text] of [
        ["title", "Title"],
        ["lower", "lower"],
        ["upper", "UPPER"],
        ["sentence", "Sentence"],
      ]) {
        const btn = document.createElement("button");
        btn.type = "button";
        btn.className = "seg-btn" + (rule.style === value ? " active" : "");
        btn.textContent = text;
        btn.addEventListener("click", () => {
          rule.style = value;
          seg.querySelectorAll(".seg-btn").forEach((s) => s.classList.toggle("active", s === btn));
        });
        seg.appendChild(btn);
      }
      const note = document.createElement("span");
      note.className = "rule-note";
      note.textContent = "Known acronyms & roman numerals keep their casing.";
      b.append(seg, note);
      card.append(b);
    } else if (rule.kind === "key") {
      const b = document.createElement("div");
      b.className = "rule-body";
      const seg = document.createElement("div");
      seg.className = "seg";
      for (const [value, text] of [
        ["camelot", "Camelot"],
        ["openkey", "Open Key"],
        ["musical", "Musical"],
      ]) {
        const btn = document.createElement("button");
        btn.type = "button";
        btn.className = "seg-btn" + (rule.style === value ? " active" : "");
        btn.textContent = text;
        btn.addEventListener("click", () => {
          rule.style = value;
          seg.querySelectorAll(".seg-btn").forEach((s) => s.classList.toggle("active", s === btn));
        });
        seg.appendChild(btn);
      }
      const note = document.createElement("span");
      note.className = "rule-note";
      note.textContent = "Converts the musical key (best scoped to the Key field). Unrecognized values are left as-is.";
      b.append(seg, note);
      card.append(b);
    } else if (rule.kind === "untransliterate") {
      // The one step whose losses are worth stating on the card: reversing a
      // romanization can't recover what the forward direction dropped, and the
      // per-word guard is the reason English text survives it.
      const b = document.createElement("div");
      b.className = "rule-body";
      const note = document.createElement("span");
      note.className = "rule-note";
      note.textContent =
        "Latin → Russian Cyrillic, for tags that arrived romanized. A word with no Cyrillic reading (Jazz, The) is left alone; ъ/ь can't be recovered and й/ы both come back as й.";
      b.append(note);
      card.append(b);
    }

    body.append(card);
  });
}

async function previewTransform() {
  const paths = selectedPaths();
  if (transformRules.length === 0) {
    toast("Add at least one rule", true);
    return;
  }
  try {
    // Reported from the plan just built, not from the staged one (#145): an
    // empty plan makes renderPreview leave the diff state, which clears the
    // staged plan out from under the message below.
    const plan = await invoke("preview_transform", {
      paths,
      rules: transformRules,
      scope: el("transform-scope").value,
    });
    setPreviewPlan(plan);
    // A filename or extension transform is a rename; a tag transform is an edit.
    // Either way it applies through the normal preview/apply/undo path.
    setPreviewSource(["filename", "fileext"].includes(el("transform-scope").value)
      ? "rename"
      : "transform");
    hooks.renderPreview(plan);
    toast(
      plan.changes.length
        ? `Previewing ${plan.changes.length} file(s) — click Apply`
        : "These rules change nothing on the selection",
      plan.changes.length === 0
    );
  } catch (e) {
    toast(String(e), true);
  }
}

// ---- named action groups (#57): saved transform chains ----
// The last full SettingsDto we loaded/saved, so persisting groups (or the
// Settings slide-over) never drops the other's fields (both write settings.json).

async function initActionGroups() {
  try {
    setSavedSettings((await invoke("load_settings", {})) || {});
    setActionGroups(Array.isArray(savedSettings.action_groups) ? savedSettings.action_groups : []);
  } catch (e) {
    setActionGroups([]);
  }
  renderGroupsMenu();
}

async function persistActionGroups() {
  setSavedSettings({ ...savedSettings, action_groups: actionGroups });
  try {
    await invoke("save_settings", { settings: savedSettings });
  } catch (e) {
    toast(String(e), true);
  }
}

// ---- the shipped preset library (#137) ----
// Action groups that come with the app rather than being saved by the user.
// They are ordinary groups in every way that matters — same rule shape, same
// scope, run and loaded through the same code — but they live in the binary,
// not in settings.json, so they can't be deleted and can't drift. Loading one
// copies its steps into the live chain, where they can be edited and saved
// under a new name; the preset itself stays as shipped.
//
// The list is the backend's (`builtin_action_groups`) rather than a copy here,
// so a preset's patterns are covered by the tests that build them into chains.

async function initBuiltinGroups() {
  try {
    setBuiltinGroups((await invoke("builtin_action_groups")).map((g) => ({ ...g, builtin: true })));
  } catch (e) {
    setBuiltinGroups([]); // no shelf is better than a broken one
  }
  renderGroupsMenu();
}

// A plain, serializable copy of one transform rule (no DOM id, `enabled` normalized).
function ruleForGroup(r) {
  return {
    kind: r.kind,
    from: r.from || "",
    to: r.to || "",
    regex: !!r.regex,
    whole_word: !!r.whole_word,
    case_sensitive: !!r.case_sensitive,
    style: r.style || "",
    enabled: r.enabled !== false,
  };
}

// Save the current chain (+ scope) under `name`, replacing a same-named group.
function saveCurrentGroup(name) {
  name = name.trim();
  if (!name) return;
  if (transformRules.length === 0) {
    toast("Add at least one rule before saving a group", true);
    return;
  }
  setActionGroups(actionGroups.filter((g) => g.name !== name));
  actionGroups.push({ name, scope: el("transform-scope").value, rules: transformRules.map(ruleForGroup) });
  actionGroups.sort((a, b) => a.name.localeCompare(b.name));
  persistActionGroups();
  renderGroupsMenu();
  toast(`Saved action group “${name}”`);
}

// Load a group's steps + scope into the live chain (fresh ids for reorder).
function loadGroup(group) {
  transformRules = (group.rules || []).map((r) => ({ id: ++ruleIdCounter, ...ruleForGroup(r) }));
  if (group.scope) el("transform-scope").value = group.scope;
  renderTransformRules();
}

function deleteGroup(name) {
  setActionGroups(actionGroups.filter((g) => g.name !== name));
  tickedGroups.delete(name); // a tick on a group that no longer exists would run nothing
  persistActionGroups();
  renderGroupsMenu();
}

// Readable names for the scopes whose stored key isn't already a field label.
const SCOPE_LABELS = {
  tags: "all tags",
  filename: "file name",
  fileext: "file extension",
};

// Which groups are ticked for the next run (#137), by name. Session-only: a
// tick says "these, now", not a preference worth persisting.
const tickedGroups = new Set();

function toggleTicked(name, on) {
  if (on) tickedGroups.add(name);
  else tickedGroups.delete(name);
  updateRunTicked();
}

// Every group the checklist can offer, saved ones first — the order they run in.
function allGroups() {
  return [...actionGroups, ...builtinGroups];
}

function tickedInOrder() {
  return allGroups().filter((g) => tickedGroups.has(g.name));
}

function updateRunTicked() {
  const btn = el("run-ticked");
  if (!btn) return;
  const n = tickedInOrder().length;
  btn.disabled = n === 0;
  btn.textContent = n ? `Run ${n} ticked` : "Run ticked";
}

// Run the ticked groups as one plan. Order matters and is the list's order, so
// a group that rewrites the file name and one that rewrites its extension
// compose into a single rename instead of the second undoing the first.
async function runTickedGroups() {
  const groups = tickedInOrder();
  if (!groups.length) return;
  const paths = selectedPaths();
  if (!paths.length) {
    toast("Select at least one file", true);
    return;
  }
  try {
    // Same as the single-chain preview (#145): report from this plan, not the
    // staged one, which an empty result clears.
    const plan = await invoke("preview_transform_groups", {
      paths,
      groups: groups.map((g) => ({ name: g.name, scope: g.scope, rules: (g.rules || []).map(ruleForGroup) })),
    });
    setPreviewPlan(plan);
    // A run that renames has to apply through the rename path; one that only
    // edits tags through the transform path. Mixed, rename wins — it is the
    // stricter of the two.
    setPreviewSource(groups.some((g) => ["filename", "fileext"].includes(g.scope))
      ? "rename"
      : "transform");
    hooks.renderPreview(plan);
    toast(
      plan.changes.length
        ? `Previewing ${plan.changes.length} file(s) — click Apply`
        : "These groups change nothing on the selection",
      plan.changes.length === 0
    );
  } catch (e) {
    toast(String(e), true);
  }
}

// One-line summary of a group for its tooltip.
function groupSummary(group) {
  const on = (group.rules || []).filter((r) => r.enabled !== false).length;
  const total = (group.rules || []).length;
  const scope = SCOPE_LABELS[group.scope] || group.scope || "all tags";
  return `${on}/${total} step(s) · ${scope}`;
}

// One checklist row (#137): a tick, the name with the scope it acts on, and
// Load. Built-ins get no Delete — they aren't the user's to remove — and carry
// their note in the tooltip instead of the bare summary.
//
// Ticking rather than running on click is what lets several groups go out as
// one plan: a cleanup is usually two or three of these in a row, and running
// them one at a time means previewing and applying each in turn.
function groupMenuRow(group, menu) {
  const row = document.createElement("div");
  row.className = "col-menu-row preset-row";

  const tick = document.createElement("input");
  tick.type = "checkbox";
  tick.className = "group-tick";
  tick.checked = tickedGroups.has(group.name);
  tick.title = "Include in the next run";
  tick.addEventListener("change", (e) => {
    e.stopPropagation();
    toggleTicked(group.name, tick.checked);
  });

  const run = document.createElement("button");
  run.type = "button";
  run.className = "text-btn preset-apply";
  const scope = document.createElement("span");
  scope.className = "group-scope";
  scope.textContent = SCOPE_LABELS[group.scope] || group.scope || "all tags";
  run.append(document.createTextNode(group.name), scope);
  run.title = group.note ? `${group.note}\n${groupSummary(group)}` : groupSummary(group);
  // The name is the checkbox's label, just a bigger target for it.
  run.addEventListener("click", () => {
    tick.checked = !tick.checked;
    toggleTicked(group.name, tick.checked);
  });

  const load = document.createElement("button");
  load.type = "button";
  load.className = "text-btn group-load";
  load.textContent = "Load";
  load.title = group.builtin
    ? "Load into the chain to edit — the built-in stays as shipped"
    : "Load into the chain without running";
  load.addEventListener("click", (e) => {
    e.stopPropagation();
    loadGroup(group);
    menu.hidden = true;
  });

  row.append(tick, run, load);

  if (!group.builtin) {
    const del = document.createElement("button");
    del.type = "button";
    del.className = "preset-del";
    del.innerHTML = ico("close");
    del.title = `Delete “${group.name}”`;
    del.addEventListener("click", (e) => {
      e.stopPropagation();
      deleteGroup(group.name);
    });
    row.append(del);
  }

  return row;
}

// Build the Groups popover — mirrors the presets menu (#44): a Run/Load/Delete
// row per group, plus a footer to name and save the current chain. The user's
// own groups come first: they're the ones being iterated on, and the shipped
// library below them (#137) is a stable shelf to reach for.
function renderGroupsMenu() {
  const menu = el("groups-menu");
  menu.innerHTML = "";
  if (!actionGroups.length) {
    const empty = document.createElement("div");
    empty.className = "col-menu-sep";
    empty.textContent = "No saved groups";
    menu.appendChild(empty);
  }
  for (const group of actionGroups) menu.appendChild(groupMenuRow(group, menu));

  if (builtinGroups.length) {
    const shipped = document.createElement("div");
    shipped.className = "col-menu-sep";
    shipped.textContent = "Built-in";
    menu.appendChild(shipped);
    for (const group of builtinGroups) menu.appendChild(groupMenuRow(group, menu));
  }

  // The checklist's one action, above the save row: what the ticks are for.
  const runFoot = document.createElement("div");
  runFoot.className = "col-menu-foot preset-run";
  const runBtn = document.createElement("button");
  runBtn.type = "button";
  runBtn.id = "run-ticked";
  runBtn.className = "text-btn";
  runBtn.title = "Preview the ticked groups, run in list order, as one plan";
  runBtn.addEventListener("click", () => {
    menu.hidden = true;
    runTickedGroups();
  });
  runFoot.appendChild(runBtn);
  menu.appendChild(runFoot);

  const foot = document.createElement("div");
  foot.className = "col-menu-foot preset-save";
  const input = document.createElement("input");
  input.type = "text";
  input.placeholder = "Save current chain as…";
  input.spellcheck = false;
  input.className = "preset-name";
  const save = document.createElement("button");
  save.type = "button";
  save.className = "text-btn";
  save.textContent = "Save";
  const commit = () => {
    if (input.value.trim()) {
      saveCurrentGroup(input.value);
      input.value = "";
    }
  };
  save.addEventListener("click", commit);
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      commit();
    }
  });
  foot.append(input, save);
  menu.appendChild(foot);
  updateRunTicked();
}

export {
  addTransformRule,
  initActionGroups,
  initBuiltinGroups,
  numberTracks,
  previewTransform,
  refreshGenerator,
  renderGroupsMenu,
  splitVinylSides,
};
