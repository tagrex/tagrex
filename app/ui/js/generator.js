// The GENERATOR mode (#143 split it out of app.js).
//
// Everything that mass-produces values for the selection: the transformation
// rule chain (#34), auto-numbering (#39) and the vinyl side -> disc mapping
// (#105). The chain itself and the named action groups behind it (#57) are a
// component shared with TAGGER › FROM NAME (#144) and live in chain.js;
// what stays here is this panel's instance of it and how the panel runs it.
import { el, toast } from "./dom.js";
import { invoke } from "./invoke.js";
import { hooks } from "./hooks.js";
import { createGroupsMenu, createRuleChain, ruleForGroup } from "./chain.js";
import { currentFieldValue } from "./editor.js";
import { groupKeyOf } from "./grouping.js";
import { parseVinylPosition } from "./vinyl.js";
import {
  groupBy,
  edits,
  selectedPaths,
  setPreviewPlan,
  setPreviewSource,
  tracks,
} from "./state.js";

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

// Refresh the GENERATOR panel for the current selection (called on entering the
// mode). The rule chain persists across mode switches within a session.
function refreshGenerator() {
  const count = selectedPaths().length;
  el("transform-count").textContent = count ? `— ${count} file(s)` : "";
  el("autonum-count").textContent = count ? `— ${count} selected` : "";
  el("vinyl-count").textContent = count ? `— ${count} selected` : "";
  transformChain.render();
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

async function previewTransform() {
  const paths = selectedPaths();
  if (transformChain.length === 0) {
    toast("Add at least one rule", true);
    return;
  }
  try {
    // Reported from the plan just built, not from the staged one (#145): an
    // empty plan makes renderPreview leave the diff state, which clears the
    // staged plan out from under the message below.
    const plan = await invoke("preview_transform", {
      paths,
      rules: transformChain.rules(),
      scope: transformChain.getScope(),
    });
    setPreviewPlan(plan);
    // A filename or extension transform is a rename; a tag transform is an edit.
    // Either way it applies through the normal preview/apply/undo path.
    setPreviewSource(["filename", "fileext"].includes(transformChain.getScope())
      ? "rename"
      : "transform");
    // The result lands on the table, which the popover sits over (#149). An
    // error leaves it open instead, so the chain can be fixed where it is.
    closeTransformPopover();
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

// Run the ticked groups as one plan. Order matters and is the list's order, so
// a group that rewrites the file name and one that rewrites its extension
// compose into a single rename instead of the second undoing the first.
async function runTickedGroups(groups) {
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
    closeTransformPopover();
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

// ---- the chain, reachable from any mode (#149) ----
//
// The cleanup a chain does is almost always cleanup after something done in
// another mode — tags just read out of file names, a just-finished import — so
// requiring a trip into GENERATOR to fix a case is backwards.
//
// The popover is filled by MOVING `#transform-block` out of the GENERATOR panel
// and putting it back on close. That is the whole trick: there is one chain,
// one set of elements and one set of listeners, so the two entry points cannot
// drift, and the chain's renderer neither knows nor cares where the block
// currently lives. (FROM NAME's chain is a different chain over different
// values — #144 — not a third entry point to this one.)

function transformPopoverOpen() {
  return !el("transform-pop").hidden;
}

// Place the popover under its toolbar button, clamped to the window. Fixed
// rather than absolute for the same reason as the placeholder reference: the
// area below the toolbar scrolls and would clip it.
function placeTransformPopover() {
  const pop = el("transform-pop");
  const rect = el("transform-btn").getBoundingClientRect();
  const width = Math.min(420, window.innerWidth - 16);
  pop.style.width = `${width}px`;
  pop.style.left = `${Math.min(Math.max(8, rect.left), window.innerWidth - width - 8)}px`;
  pop.style.top = `${rect.bottom + 4}px`;
  pop.style.maxHeight = `${window.innerHeight - rect.bottom - 16}px`;
}

function openTransformPopover() {
  const pop = el("transform-pop");
  pop.appendChild(el("transform-block"));
  pop.hidden = false;
  // The block's header counts the selection, and refreshGenerator only runs on
  // entering the mode — so bring them up to date for the mode we're actually in.
  refreshGenerator();
  placeTransformPopover();
}

function closeTransformPopover() {
  if (!transformPopoverOpen()) return;
  // Back into the GENERATOR panel, at its marked position, so that panel is
  // whole again whether or not the user ever opens the mode.
  el("transform-home").after(el("transform-block"));
  el("transform-pop").hidden = true;
}

function toggleTransformPopover() {
  if (transformPopoverOpen()) closeTransformPopover();
  else openTransformPopover();
}

// ---- wire up ----
// The Groups popover over this panel's chain: ticks and Run ticked, because
// several groups composing into one plan is what this mode is for (#137).
createGroupsMenu({
  btn: "groups-btn",
  menu: "groups-menu",
  chain: transformChain,
  ticks: true,
  onRun: runTickedGroups,
});
el("transform-preview").addEventListener("click", previewTransform);
el("transform-btn").addEventListener("click", (e) => {
  e.stopPropagation();
  toggleTransformPopover();
});
// Dismissal: anywhere outside, or Escape. The Groups popover nested inside opens
// over the block, so a click landing in it must not count as "outside".
document.addEventListener("click", (e) => {
  if (!transformPopoverOpen()) return;
  if (e.target.closest?.("#transform-pop, #transform-btn")) return;
  closeTransformPopover();
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && transformPopoverOpen()) {
    closeTransformPopover();
    el("transform-btn").focus();
  }
});
window.addEventListener("resize", () => {
  if (transformPopoverOpen()) placeTransformPopover();
});
el("autonum-run").addEventListener("click", numberTracks);
el("vinyl-split").addEventListener("click", splitVinylSides);
// Rule reorder is wired per-card by the chain component via enablePointerReorder
// (grip drag), with ↑/↓ as the fallback — no container-level HTML5 DnD (#88).

export {
  closeTransformPopover,
  numberTracks,
  previewTransform,
  refreshGenerator,
  splitVinylSides,
};
