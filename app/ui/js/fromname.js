// The FROM NAME sub-tab of TAGGER (#139, #143 split it out of app.js).
//
// The file's own name read back into tags: the pattern, the cleanup chain that
// tidies what it extracts (#144), and the live read-out of what the pattern
// sees. Extraction itself is the mask engine's job in Rust — one grammar, one
// implementation — so this only stages the plan it returns.
import { el, escapeHtml, toast } from "./dom.js";
import { invoke } from "./invoke.js";
import { hooks } from "./hooks.js";
import { createGroupsMenu, createRuleChain } from "./chain.js";
import { columnLabel } from "./columns.js";
import { previewPlan, selectedPaths, setPreviewPlan, setPreviewSource } from "./state.js";

// FROM NAME's pattern and its cleanup chain. Both are the user's working state
// for the panel, not backend settings, so they live in localStorage beside the
// other persisted panel prefs — a chain retyped every session would defeat the
// point of having one. (The action groups it can load from are settings.json's,
// shared with GENERATOR; only this panel's live chain is stored here.)
const FROM_NAME_MASK_STORAGE_KEY = "tagrex.fromNameMask";
const FROM_NAME_CHAIN_STORAGE_KEY = "tagrex.fromNameChain";
// Superseded by the chain above (#144). Read once so a table typed under #141
// carries over as ordinary replace rules instead of silently disappearing.
const FROM_NAME_REPL_STORAGE_KEY = "tagrex.fromNameReplacements";

// Separators are what names carry instead of spaces, so the chain starts with
// the one everybody needs; it's an ordinary rule and can be deleted.
const DEFAULT_CHAIN = {
  scope: "tags",
  rules: [{ kind: "replace", from: "_", to: " " }],
};

// The chain persists on every edit and the read-out follows it, so both hang off
// the component's one change hook.
const cleanupChain = createRuleChain({
  ids: { rules: "fn-rules", empty: "fn-empty", kind: "fn-kind", add: "fn-add", scope: "fn-scope" },
  onChange: () => {
    saveFromNamePrefs();
    scheduleNameProbe();
  },
});

// The Groups popover over the cleanup chain (#144), assigned below once the
// panel's own chain exists.
let cleanupMenu = null;

// The cleanup as the backend takes it: a list of action groups, so this panel's
// chain and a saved group are the same thing to it (#144). The live chain runs
// first — it is the generic pass over every value — and the ticked groups after
// it, in list order, each on the value its scope names. That is how per-field
// cleanup is expressed: tick one group scoped to Artist and another to Title.
function cleanupGroups() {
  return [cleanupChain.asGroup(), ...(cleanupMenu?.tickedInOrder() || [])];
}

function loadFromNamePrefs() {
  let chain = null;
  try {
    const mask = localStorage.getItem(FROM_NAME_MASK_STORAGE_KEY);
    if (mask) el("from-name-mask").value = mask;
    const stored = localStorage.getItem(FROM_NAME_CHAIN_STORAGE_KEY);
    if (stored) {
      const saved = JSON.parse(stored);
      // An empty chain is a real choice (the user deleted every rule), so only
      // a malformed value falls back to the default.
      if (saved && Array.isArray(saved.rules)) chain = saved;
    } else {
      const table = JSON.parse(localStorage.getItem(FROM_NAME_REPL_STORAGE_KEY) || "null");
      if (Array.isArray(table)) {
        chain = { scope: "tags", rules: table.map((r) => ({ kind: "replace", ...r })) };
      }
    }
  } catch (e) {
    /* unreadable or unavailable — fall back to the defaults */
  }
  cleanupChain.load(chain || DEFAULT_CHAIN);
}

function saveFromNamePrefs() {
  try {
    localStorage.setItem(FROM_NAME_MASK_STORAGE_KEY, el("from-name-mask").value);
    localStorage.setItem(FROM_NAME_CHAIN_STORAGE_KEY, JSON.stringify(cleanupChain.asGroup()));
  } catch (e) {
    /* localStorage unavailable — preference just won't persist */
  }
}

// FROM NAME (#139): read each selected file's own name back into tags — the
// extract direction of the mask RENAMER renders with. The result is an ordinary
// tag plan, so it lands in the same in-table diff and applies/undoes like any
// other change.
async function previewTagsFromName() {
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast("Select at least one track", true);
    return;
  }
  try {
    const plan = await invoke("preview_tags_from_name", {
      mask: el("from-name-mask").value,
      paths,
      cleanup: cleanupGroups(),
    });
    // Distinguish "nothing to do" from a silent no-op: with this feature an
    // empty plan usually means the pattern doesn't fit the names.
    if (plan.changes.length === 0) {
      toast("No selected name gives this pattern anything new", true);
      return;
    }
    setPreviewPlan(plan);
    setPreviewSource("fromname");
    hooks.renderPreview(previewPlan);
  } catch (e) {
    toast(String(e), true);
  }
}

// The live read-out under the pattern box: what the mask pulls out of the first
// selected file, tags-on-disk irrelevant. Extraction stays in Rust — one
// grammar, one implementation (mask.rs) — so this is a per-keystroke round trip,
// debounced by its caller.
async function refreshNameProbe() {
  const box = el("from-name-probe");
  if (!box) return;
  const path = selectedPaths()[0];
  if (!path) {
    box.innerHTML = `<div class="probe-miss">Select a track to see what the pattern reads.</div>`;
    return;
  }
  try {
    const probe = await invoke("probe_tags_from_name", {
      mask: el("from-name-mask").value,
      path,
      cleanup: cleanupGroups(),
    });
    const subject = `<div class="probe-subject">${escapeHtml(probe.subject)}</div>`;
    if (!probe.matched || probe.fields.length === 0) {
      box.innerHTML = `${subject}<div class="probe-miss">This name doesn't fit the pattern.</div>`;
      return;
    }
    const rows = probe.fields
      .map(
        ([field, value]) =>
          `<div class="probe-row"><span class="probe-field">${escapeHtml(columnLabel(field))}</span><span class="probe-value">${escapeHtml(value)}</span></div>`,
      )
      .join("");
    box.innerHTML = subject + rows;
  } catch (e) {
    // A pattern that can't parse at all — say so where the result would be.
    box.innerHTML = `<div class="probe-miss">${escapeHtml(String(e))}</div>`;
  }
}

let nameProbeTimer = null;
function scheduleNameProbe() {
  if (el("subtab-filename").hidden) return;
  clearTimeout(nameProbeTimer);
  nameProbeTimer = setTimeout(refreshNameProbe, 180);
}

// ---- wire up ----
// FROM NAME (#139): the probe follows the pattern as it's typed.
el("from-name-preview").addEventListener("click", previewTagsFromName);
el("from-name-mask").addEventListener("input", () => {
  saveFromNamePrefs();
  scheduleNameProbe();
});
// The Groups popover over the cleanup chain (#144). A tick here means "take
// part in the cleanup" rather than "run now" — the values being cleaned don't
// exist until Preview tags builds them — so there is no Run button and the
// button below counts what is ticked instead. Groups scoped to the file name or
// its extension are hidden: there is neither among the values a mask extracts.
cleanupMenu = createGroupsMenu({
  btn: "fn-groups-btn",
  menu: "fn-groups-menu",
  chain: cleanupChain,
  hideFileScopes: true,
  tickTitle: "Include in the cleanup, after the chain above",
  onTicksChanged: (ticked) => {
    el("fn-groups-count").textContent = ticked.length ? String(ticked.length) : "";
    scheduleNameProbe();
  },
});

// The pattern and the cleanup chain come back from the last session (#144).
loadFromNamePrefs();

export { previewTagsFromName, refreshNameProbe, scheduleNameProbe };
