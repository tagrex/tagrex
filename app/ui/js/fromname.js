// The FROM NAME sub-tab of TAGGER (#139, #143 split it out of app.js).
//
// The file's own name read back into tags: the pattern, and the live read-out
// of what the pattern sees. Extraction itself is the mask engine's job in Rust
// — one grammar, one implementation — so this only stages the plan it returns.
//
// It carried a cleanup of its own for a while (a replacement table in #141, a
// rule chain in #144). Both are gone (#159): a chain now runs over the staged
// plan (#142), which is the one mechanism every producer of values shares, so
// the values here come out as the name spells them and are tidied up on the
// preview bar like everything else.
import { el, toast } from "./dom.js";
import { t } from "./i18n.js";
import { invoke } from "./invoke.js";
import { chainHasRules, onChainChanged, runChainOverPlan } from "./chains.js";
import { hooks } from "./hooks.js";
import { probeMessage, renderProbeTracks } from "./probe-view.js";
import {
  captureSelection,
  isSelectionCurrent,
  previewPlan,
  previewSource,
  selectedPaths,
  setPreviewPlan,
  setPreviewSource,
} from "./state.js";

// The pattern is the user's working state for the panel, not a backend setting,
// so it lives in localStorage beside the other persisted panel prefs.
const FROM_NAME_MASK_STORAGE_KEY = "tagrex.fromNameMask";

function loadFromNamePrefs() {
  try {
    const mask = localStorage.getItem(FROM_NAME_MASK_STORAGE_KEY);
    if (mask) el("from-name-mask").value = mask;
  } catch (e) {
    /* unreadable or unavailable — fall back to the default in the markup */
  }
}

function saveFromNamePrefs() {
  try {
    localStorage.setItem(FROM_NAME_MASK_STORAGE_KEY, el("from-name-mask").value);
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
    toast(t("toast.selectOneTrack"), true);
    return;
  }
  try {
    const raw = await invoke("preview_tags_from_name", {
      mask: el("from-name-mask").value,
      paths,
    });
    // The chain for THIS job runs as part of the press (#237). Reading tags out
    // of a name and tidying what comes out is one intention — an underscore
    // becomes a space, a lower-cased name becomes a title — and making it two
    // presses only meant forgetting the second one.
    const plan = await runChainOverPlan(raw, "fromname");
    // Distinguish "nothing to do" from a silent no-op: with this feature an
    // empty plan usually means the pattern doesn't fit the names.
    if (plan.changes.length === 0) {
      toast(t("fromname.nothingNew"), true);
      return;
    }
    setPreviewPlan(plan);
    setPreviewSource("fromname");
    hooks.renderPreview(previewPlan);
  } catch (e) {
    toast(String(e), true);
  }
}

// The read-out has to show what the BUTTON would produce, not what the mask
// alone reads (#247): since the chain runs as part of Preview tags (#237),
// showing the raw extraction would be previewing values that will never be
// written. The extracted fields go through this context's chain the same way
// the real plan does — as a one-file plan, so there is one implementation of
// "what the chain does to these values", not a second one here.
async function throughChain(path, fields) {
  if (!chainHasRules("fromname") || !fields.length) return fields;
  const plan = {
    description: "",
    changes: [
      {
        path,
        rename_to: null,
        tag_changes: fields.map(([field, value]) => ({ field, old: null, new: value })),
      },
    ],
  };
  const out = await runChainOverPlan(plan, "fromname");
  const changed = out?.changes?.[0]?.tag_changes;
  return changed ? changed.map((t) => [t.field, t.new ?? ""]) : fields;
}

// The sample is bounded; the full Preview action still covers the selection.
// Bind every await to both the pattern and selection revision (including A→B→A).
let nameProbeGeneration = 0;
async function refreshNameProbe() {
  const box = el("from-name-probe");
  if (!box) return;
  const generation = ++nameProbeGeneration;
  const subject = captureSelection();
  const mask = el("from-name-mask").value;
  const current = () => generation === nameProbeGeneration && isSelectionCurrent(subject) && mask === el("from-name-mask").value;
  const paths = subject.paths.slice(0, 3);
  if (!paths.length) {
    probeMessage(box, t("fromname.probeSelect"));
    return;
  }
  try {
    const result = await invoke("probe_tags_from_names", { mask, paths });
    if (!current()) return;
    const samples = [];
    for (const [index, probe] of result.files.entries()) {
      const fields = probe.matched ? await throughChain(paths[index], probe.fields) : [];
      if (!current()) return;
      samples.push({ probe, fields, path: paths[index] });
    }
    renderProbeTracks(box, samples, { name: true });
  } catch (e) {
    if (current()) probeMessage(box, String(e));
  }
}

let nameProbeTimer = null;
function scheduleNameProbe() {
  nameProbeGeneration++;
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

// The pattern comes back from the last session (#141).
loadFromNamePrefs();

export { previewTagsFromName, refreshNameProbe, scheduleNameProbe, throughChain };

// Changing the chain changes what this panel would produce, so it says so at
// once (#248): the read-out always, and the staged plan too when the one on the
// table came from here — re-running is exactly what pressing the button again
// would do, which is what the user would otherwise have to remember.
onChainChanged("fromname", () => {
  refreshNameProbe();
  if (previewSource === "fromname" && previewPlan?.changes.length) previewTagsFromName();
});
