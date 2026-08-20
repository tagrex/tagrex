// The GENERATOR mode (#143 split it out of app.js).
//
// Everything that mass-produces values for the selection: the transformation
// rule chain (#34), auto-numbering (#39) and the vinyl side -> disc mapping
// (#105). The chain itself and the named action groups behind it (#57) are a
// component shared with TAGGER › FROM NAME (#144) and live in chain.js;
// what stays here is this panel's instance of it and how the panel runs it.
//
// "How the panel runs it" has two cases since #142. With nothing staged the
// chain reads the files. With a plan staged it reads THE PLAN — the values it
// proposes — and gives back a revised plan, so producing values and cleaning
// them up stay one Apply and one undo entry instead of two.
import { el, plural, toast } from "./dom.js";
import { invoke } from "./invoke.js";
import { hooks } from "./hooks.js";
import { createGroupsMenu, createRuleChain, ruleForGroup } from "./chain.js";
import { currentFieldValue } from "./editor.js";
import { groupKeyOf } from "./grouping.js";
import { parseVinylPosition } from "./vinyl.js";
import {
  currentMode,
  groupBy,
  edits,
  previewPlan,
  selectedPaths,
  setPreviewPlan,
  setPreviewSource,
  tracks,
} from "./state.js";
import { saveTransformDocked, transformDocked } from "./prefs.js";
import { setChainFor, setLiveChainSource, storedChainFor } from "./chains.js";

// ---- transformations (#34) ----
// This panel's chain, over the tags and names already on disk. It lives for the
// session and is not persisted; naming and saving chains is what the groups are
// for (#57).
const transformChain = createRuleChain({
  ids: {
    rules: "transform-rules",
    empty: "transform-empty",
    kind: "transform-kind",
    add: "transform-add",
    scope: "transform-scope",
  },
});

// Whether the chain is about to act on a staged plan rather than on the files
// (#142). One question, asked everywhere, so the block's wording and what the
// run actually does can never disagree.
function overStagedPlan() {
  return !!previewPlan && previewPlan.changes.length > 0;
}

// Refresh the GENERATOR panel for the current selection (called on entering the
// mode). The rule chain persists across mode switches within a session.
function refreshGenerator() {
  const count = selectedPaths().length;
  const staged = overStagedPlan();
  // Docked, the chain has no button, so the note about acting on a staged plan
  // would be describing a press that cannot happen here (#237).
  const docked = !el("transform-dock").hidden;
  el("transform-count").textContent = staged
    ? `— ${plural(previewPlan.changes.length, "staged file", "staged files")}`
    : count
      ? `— ${plural(count, "file", "files")}`
      : "";
  el("transform-over-plan").hidden = !staged || docked;
  el("transform-preview").textContent = staged ? "Clean up staged" : "Preview changes";
  el("autonum-count").textContent = count ? `— ${count} selected` : "";
  el("vinyl-count").textContent = count ? `— ${count} selected` : "";
  transformChain.render();
}

// Stage the plan a run produced and report it. Shared by the single-chain and
// the checklist paths, and by both the on-disk and the over-a-plan cases (#142).
//
// `scopes` are the scopes that ran: a file name or extension makes this a
// rename, which has to apply through the rename path. Over a staged plan the
// source is the one already there — the plan is still the import / FROM NAME /
// rename it started as, and apply() branches on that to decide what to do with
// the pending-edits buffer — unless a file scope has now put a rename in it.
function stageRun(plan, scopes, wasStaged) {
  const renames = scopes.some((scope) => ["filename", "fileext"].includes(scope));
  setPreviewPlan(plan);
  if (!wasStaged) {
    setPreviewSource(renames ? "rename" : "transform");
  } else if (renames) {
    setPreviewSource("rename");
  }
  // Nothing to dismiss any more: the chain is pinned beside the table rather
  // than floating over it (#233), so the result lands in full view with the
  // rules that produced it still on screen to adjust.
  hooks.renderPreview(plan);
}

// What a run reports when it changed nothing — the staged case has its own
// wording, since "the selection" is not what it looked at.
function nothingChanged(wasStaged, subject) {
  return wasStaged
    ? `${subject} change nothing in the staged plan`
    : `${subject} change nothing on the selection`;
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
  toast(`Numbered ${plural(assigned.length, "track", "tracks")}${perGroup ? " (restarted per group)" : ""}`);
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
  toast(`Split ${plural(changed, "vinyl position", "vinyl positions")} into track + disc`);
}

async function previewTransform() {
  if (transformChain.length === 0) {
    toast("Add at least one rule", true);
    return;
  }
  const wasStaged = overStagedPlan();
  try {
    // Reported from the plan just built, not from the staged one (#145): an
    // empty plan makes renderPreview leave the diff state, which clears the
    // staged plan out from under the message below.
    const plan = wasStaged
      ? await invoke("preview_transform_over_plan", {
          plan: previewPlan,
          groups: [transformChain.asGroup()],
        })
      : await invoke("preview_transform", {
          paths: selectedPaths(),
          rules: transformChain.rules(),
          scope: transformChain.getScope(),
        });
    stageRun(plan, [transformChain.getScope()], wasStaged);
    toast(
      plan.changes.length
        ? `Previewing ${plural(plan.changes.length, "file", "files")} — click Apply`
        : nothingChanged(wasStaged, "These rules"),
      plan.changes.length === 0
    );
  } catch (e) {
    toast(String(e), true);
  }
}

// Run the ticked groups as one plan. Order matters and is the list's order, so
// a group that rewrites the file name and one that rewrites its extension
// compose into a single rename instead of the second undoing the first.
async function runTickedGroups(groups) {
  if (!groups.length) return;
  const wasStaged = overStagedPlan();
  const paths = selectedPaths();
  if (!wasStaged && !paths.length) {
    toast("Select at least one file", true);
    return;
  }
  const payload = groups.map((g) => ({
    name: g.name,
    scope: g.scope,
    rules: (g.rules || []).map(ruleForGroup),
  }));
  try {
    // Same as the single-chain preview (#145): report from this plan, not the
    // staged one, which an empty result clears.
    const plan = wasStaged
      ? await invoke("preview_transform_over_plan", { plan: previewPlan, groups: payload })
      : await invoke("preview_transform_groups", { paths, groups: payload });
    stageRun(plan, groups.map((g) => g.scope), wasStaged);
    toast(
      plan.changes.length
        ? `Previewing ${plural(plan.changes.length, "file", "files")} — click Apply`
        : nothingChanged(wasStaged, "These groups"),
      plan.changes.length === 0
    );
  } catch (e) {
    toast(String(e), true);
  }
}

// ---- the chain, reachable from any mode (#149, #233) ----
//
// The cleanup a chain does is almost always cleanup after something done in
// another mode — tags just read out of file names, a just-finished import — so
// requiring a trip into GENERATOR to fix a case is backwards.
//
// It used to travel into a popover over the table, which fought the work it was
// for: it covered the rows being cleaned up, closed on any click outside, and
// opened a second popover on top of itself for the groups. Now it is PINNED —
// moved into `#transform-dock` at the foot of the side column, where it stays
// until it is closed, through mode switches and through staging.
//
// The mechanism is unchanged and is the whole trick: `#transform-block` is
// MOVED, not copied, so there is one chain, one set of elements and one set of
// listeners, and no two entry points can drift. (FROM NAME's chain is a
// different chain over different values — #144 — not a third entry point to
// this one.)

// Pinned by the user, which is not the same as "on screen right now": in
// GENERATOR the block goes home to the panel that owns it, because that mode is
// the transform panel and two visible copies is what the single instance exists
// to prevent.
let dockPinned = transformDocked();

// Which job's chain the block is holding (#236). Null where a chain has no
// business existing: EDITOR, where a value typed by hand must come out exactly
// as typed, and the modes that produce no values at all.
function chainContext() {
  if (currentMode === "generator") return "generator";
  if (currentMode === "renamer") return "renamer";
  if (currentMode !== "tagger") return null;
  const sub = document.querySelector(".subtab.active")?.dataset.subtab;
  if (sub === "filename") return "fromname";
  if (sub === "online") return "online";
  return null;
}

// The context whose chain is in the block right now, so a switch knows what it
// is putting away.
let shownContext = null;

// Put the current chain away and take the incoming one out. The block's DOM is
// the one live chain there is, so this is what makes four chains out of it.
function swapChainTo(context) {
  if (context === shownContext) return;
  if (shownContext) setChainFor(shownContext, transformChain.asGroup());
  shownContext = context;
  if (context) transformChain.load(storedChainFor(context));
}

// So a plan built anywhere can run the chain of its own context without asking
// this module whether that context happens to be on screen.
setLiveChainSource(() =>
  shownContext ? { context: shownContext, chain: transformChain.asGroup() } : null
);
// A chain typed and never switched away from would otherwise be lost on quit.
window.addEventListener("beforeunload", () => {
  if (shownContext) setChainFor(shownContext, transformChain.asGroup());
});

// Put the block where the current mode says it belongs, and show the dock only
// when it is holding something. One function, called from everywhere, so the
// button's state and the block's whereabouts cannot disagree.
function syncTransformPlacement() {
  const context = chainContext();
  swapChainTo(context);
  const dock = el("transform-dock");
  const inGenerator = context === "generator";
  const docked = dockPinned && !!context && !inGenerator;
  if (docked) dock.append(el("transform-block"));
  else el("transform-home").after(el("transform-block"));
  dock.hidden = !docked;
  // Offered only where a chain applies (#236): not in EDITOR, not in the modes
  // that produce no values, and not in GENERATOR, where the block is already
  // on screen and the button would pull it out of the panel it sits in.
  el("transform-btn").hidden = !context || inGenerator;
  el("transform-btn").setAttribute("aria-pressed", String(dockPinned));
  // Docked, the chain has no run button of its own: the context's own action
  // runs it (#237). In GENERATOR the block keeps its button, which is also the
  // way to clean up a plan staged from somewhere with no chain of its own.
  el("transform-preview").hidden = docked;
  el("transform-dock-note").hidden = !docked;
  if (docked) {
    el("transform-dock-note").textContent = DOCK_NOTES[context];
    // The side column scrolls, and the dock is at the foot of it: bring it into
    // view rather than leaving the user to find what they just pinned.
    dock.scrollIntoView({ block: "nearest" });
    // The header counts the selection (or the staged plan); refreshGenerator
    // only runs on entering the mode, so bring it up to date for where the
    // block has just landed.
    refreshGenerator();
  }
}

// What the docked chain says about itself, since it has no button to say it.
const DOCK_NOTES = {
  online: "Runs on the imported values when you import a release.",
  fromname: "Runs on the tags read out of the name when you press Preview tags.",
  renamer: "Runs on the new names when you preview a rename or a move.",
};

function setTransformDocked(on) {
  dockPinned = on;
  saveTransformDocked(on);
  syncTransformPlacement();
}

function toggleTransformDock() {
  setTransformDocked(!dockPinned);
}

// ---- wire up ----
// The group list inside this panel's chain block (#234): ticks and Run ticked,
// because several groups composing into one plan is what this mode is for
// (#137), and a click on a name to load one.
createGroupsMenu({
  menu: "groups-menu",
  chain: transformChain,
  onRun: runTickedGroups,
  inline: true,
});
el("transform-preview").addEventListener("click", previewTransform);
el("transform-btn").addEventListener("click", toggleTransformDock);
// The plan bar's entry point to the same chain (#142): Clean up pins it rather
// than opening anything, and pins it whether or not it was pinned already —
// pressing "clean this up" and getting nothing would be the wrong answer.
el("transform-undock").addEventListener("click", () => setTransformDocked(false));
el("autonum-run").addEventListener("click", numberTracks);
el("vinyl-split").addEventListener("click", splitVinylSides);
// Rule reorder is wired per-card by the chain component via enablePointerReorder
// (grip drag), with ↑/↓ as the fallback — no container-level HTML5 DnD (#88).

export {
  syncTransformPlacement,
  numberTracks,
  previewTransform,
  refreshGenerator,
  splitVinylSides,
};
