// The presets that run as part of each job (#392).
//
// One shared set of TICKED presets, not a chain per job. Each job used to
// carry its own chain (#236), built in the transform dialog next to the preset
// shelf — which made the dialog both the place a chain was edited and the
// place a preset was picked, and made "click a preset" mean replace-or-append
// (#388). Presets are now made in their own window (#391) and simply ticked.
//
// What made one shared chain wrong in #236 was RENAMER wanting a space turned
// into an underscore while FROM NAME wants exactly the opposite. A shared set
// does not bring that back, because every rule names what it acts on (#251)
// and each job takes only the rules aimed at what it produces:
//
//   - importing a release, reading tags from a name: rules on tags;
//   - RENAMER: rules on the file name and extension;
//   - GENERATOR: everything, since its own button is the whole point.
//
// So a file-name preset ticked for RENAMER never touches the tags FROM NAME
// reads, and a tag preset ticked for imports never renames a file. EDITOR has
// none on purpose — a value typed by hand comes out exactly as typed.
//
// The set is ordered by the preset list (your presets, then the built-ins), so
// ticking never has to ask "before or after?".
//
// Since #555 each job keeps a ticked set of its own. Rule targets kept tag
// rules and file-name rules apart, but not two jobs that both write tags:
// ticking a tidy-up for names read out of files (underscores, title case) also
// ran it on every import, where the values were already right. A preset with
// nothing for a job can't be ticked there.
import { invoke } from "./invoke.js";
import { persistActionGroups, renderAllGroupsMenus, ruleForGroup, uniquePresetName } from "./chain.js";
import { actionGroups, builtinGroups, setActionGroups } from "./state.js";

// One list of ticked presets per job (#555), as { context: [keys] }.
const ACTIVE_BY_JOB_KEY = "tagrex.activePresetsByJob";
// The one shared list it replaces, read once to seed every job's (#555).
const ACTIVE_KEY = "tagrex.activePresets";
// The per-job chains this replaces, read once to carry them over (#392).
const LEGACY_CHAINS_KEY = "tagrex.chains";

// The contexts, and the label each uses when it says what it is about to do.
const CHAIN_CONTEXTS = {
  online: "the imported values",
  fromname: "the tags read from the name",
  renamer: "the new names",
  generator: "the selection",
};

const FILE_SCOPES = ["filename", "fileext"];

// A preset's key in the set. A built-in and one of yours may share a name (a
// preset saved before the built-in shipped), so the key says which it is.
function presetKey(group) {
  return `${group.builtin ? "builtin" : "user"}:${group.name}`;
}

function keyList(value) {
  return Array.isArray(value) ? value.filter((k) => typeof k === "string") : [];
}

// The shared list from before #555, or null when there never was one.
function readShared() {
  try {
    const raw = localStorage.getItem(ACTIVE_KEY);
    return raw === null ? null : keyList(JSON.parse(raw));
  } catch (e) {
    return [];
  }
}

function readByJob() {
  try {
    const raw = localStorage.getItem(ACTIVE_BY_JOB_KEY);
    if (raw === null) return null;
    const stored = JSON.parse(raw) || {};
    return Object.fromEntries(Object.keys(CHAIN_CONTEXTS).map((c) => [c, keyList(stored[c])]));
  } catch (e) {
    return Object.fromEntries(Object.keys(CHAIN_CONTEXTS).map((c) => [c, []]));
  }
}

// Null until the one-time carry-over has run (see migrateChains).
let active = readByJob();

function persistActive() {
  try {
    localStorage.setItem(ACTIVE_BY_JOB_KEY, JSON.stringify(active || {}));
  } catch (e) {
    /* localStorage unavailable — the ticks just won't survive a restart */
  }
}

function isPresetActive(group, context) {
  return (active?.[context] || []).includes(presetKey(group));
}

function setPresetActive(group, on, context) {
  if (!active) return;
  const key = presetKey(group);
  const rest = (active[context] || []).filter((k) => k !== key);
  active[context] = on ? [...rest, key] : rest;
  persistActive();
  notifyChainChanged(context);
}

// Keep a renamed preset ticked, and forget a deleted one (`to` null), in every
// job's set.
function renameActivePreset(from, to) {
  if (!active) return;
  const key = `user:${from}`;
  for (const context of Object.keys(active)) {
    if (!active[context].includes(key)) continue;
    active[context] = active[context].filter((k) => k !== key);
    if (to) active[context].push(`user:${to}`);
  }
  persistActive();
}

// Whether `context` takes a rule aimed at `scope`.
function contextTakes(context, scope) {
  const file = FILE_SCOPES.includes(scope);
  if (context === "renamer") return file;
  if (context === "online" || context === "fromname") return !file;
  return true;
}

// Every preset, in list order: yours, then the built-ins.
function allPresets() {
  return [...actionGroups, ...builtinGroups];
}

// The enabled rules of `group` that `context` takes, each with its target
// written out (a rule saved before per-rule targets takes the group's).
function rulesFor(group, context) {
  return (group.rules || [])
    .filter((r) => r.enabled !== false)
    .map((r) => ({ ...ruleForGroup(r), scope: r.scope || group.scope || "tags" }))
    .filter((r) => contextTakes(context, r.scope));
}

// The ticked presets as the groups a run takes, cut down to what `context`
// acts on; a preset with nothing left for it is left out.
function presetsFor(context) {
  return allPresets()
    .filter((g) => isPresetActive(g, context))
    .map((g) => ({ name: g.name, scope: "tags", rules: rulesFor(g, context) }))
    .filter((g) => g.rules.length);
}

// How many rules would run in `context` — what the wand button's dot and the
// RENAMER example report.
function activeRuleCount(context) {
  return presetsFor(context).reduce((n, g) => n + g.rules.length, 0);
}

// How many of a preset's rules `context` would run, ticked or not: the count
// the checklist shows beside it (#556).
function presetRuleCount(group, context) {
  return rulesFor(group, context).length;
}

// Whether a preset has anything for `context`, ticked or not — the checklist
// dims the ones that don't.
function presetAppliesTo(group, context) {
  return rulesFor(group, context).length > 0;
}

// What to redo when the set changes (#248). The panels register their own
// "show me again" — the read-out under a pattern, the staged plan that came
// from this very context — because changing a rule and then having to press the
// button again to see it is the two-step this whole design got rid of. A tick
// changes one job's set (#555) and tells that job; an edited preset can change
// any job, so it tells them all.
const listeners = new Map();
function onChainChanged(context, fn) {
  if (!listeners.has(context)) listeners.set(context, []);
  listeners.get(context).push(fn);
}
function notifyChainChanged(context) {
  for (const [job, fns] of listeners) {
    if (context && job !== context) continue;
    for (const fn of fns) fn();
  }
}

function chainHasRules(context) {
  return activeRuleCount(context) > 0;
}

// Run the ticked presets over a plan just built, and give back the revised plan
// — or the plan untouched when there is nothing to run (#237).
//
// This is what makes the presets part of the operation instead of a second
// thing to remember: one press produces the values and cleans them up, one
// Apply and one undo entry, in the order a person would do it by hand. A
// failure returns the original plan rather than nothing: the values are still
// worth showing, and a broken rule is not a reason to lose the work of the step
// before it.
async function runChainOverPlan(plan, context) {
  if (!plan || !plan.changes?.length) return plan;
  const groups = presetsFor(context);
  if (!groups.length) return plan;
  try {
    return await invoke("preview_transform_over_plan", { plan, groups });
  } catch (e) {
    return plan;
  }
}

// Carry the per-job chains over, once (#392): each job's chain becomes one of
// your presets, and the ones that ran on their own (every job but GENERATOR,
// whose chain only ran on its button) are ticked when their job takes any of
// their rules — so the first run after the change does what the last one
// before it did. Runs after the saved presets are loaded, and does nothing once
// the set exists.
function migrateChains() {
  if (active !== null) return;
  // The shared list from before #555 seeds every job's set, each keeping only
  // the presets that have something for it, so the first run after the change
  // does what the last one before it did.
  const shared = readShared();
  if (shared !== null) {
    seedJobs(shared);
    return;
  }
  let legacy = {};
  try {
    legacy = JSON.parse(localStorage.getItem(LEGACY_CHAINS_KEY) || "{}") || {};
  } catch (e) {
    legacy = {};
  }
  const labels = [
    ["online", "Import rules"],
    ["fromname", "From name rules"],
    ["renamer", "Renamer rules"],
    ["generator", "Generator rules"],
  ];
  const added = [];
  const ticked = [];
  for (const [context, label] of labels) {
    const chain = legacy[context];
    if (!chain || !Array.isArray(chain.rules) || !chain.rules.length) continue;
    const name = uniquePresetName(label, added);
    const rules = chain.rules.map((r) => ({ ...ruleForGroup(r), scope: r.scope || chain.scope || "tags" }));
    added.push({ name, scope: chain.scope || "tags", rules });
    // Ticked only if its own job takes something from it. A chain whose rules
    // all aim elsewhere — file-name rules on the import chain — did nothing
    // useful there, and ticked in the shared set it would start running in the
    // job those rules DO aim at, ahead of that job's own preset.
    if (context !== "generator" && rules.some((r) => contextTakes(context, r.scope))) {
      ticked.push(`user:${name}`);
    }
  }
  if (added.length) {
    setActionGroups([...actionGroups, ...added].sort((a, b) => a.name.localeCompare(b.name)));
    persistActionGroups();
    renderAllGroupsMenus();
  }
  seedJobs(ticked);
}

// Every job's set from one list of keys (#555): each keeps the presets that
// have something for it.
function seedJobs(keys) {
  const byKey = new Map(allPresets().map((g) => [presetKey(g), g]));
  active = Object.fromEntries(
    Object.keys(CHAIN_CONTEXTS).map((context) => [
      context,
      keys.filter((key) => byKey.has(key) && presetAppliesTo(byKey.get(key), context)),
    ])
  );
  persistActive();
  renderAllGroupsMenus();
  notifyChainChanged();
}

export {
  CHAIN_CONTEXTS,
  activeRuleCount,
  chainHasRules,
  isPresetActive,
  migrateChains,
  notifyChainChanged,
  onChainChanged,
  presetAppliesTo,
  presetRuleCount,
  presetsFor,
  renameActivePreset,
  runChainOverPlan,
  setPresetActive,
};
