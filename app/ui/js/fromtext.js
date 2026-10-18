// The FROM TEXT sub-tab of TAGGER (#61): tags read out of a text list.
//
// A tracklist that exists only as text — a forum post, a CD insert, a release
// page no source has — pasted or loaded here, one line per track. Each line is
// read with a mask the way FROM NAME reads a file name; the lines it fits are
// the tracks, and the nth track goes to the nth selected file in table order. Extraction is the mask engine's job
// in Rust, as it is for FROM NAME; this module only stages the plan it returns,
// and the chain FROM NAME runs over its values runs over these too.
import { el, escapeHtml, toast } from "./dom.js";
import { t } from "./i18n.js";
import { invoke } from "./invoke.js";
import { onChainChanged, runChainOverPlan } from "./chains.js";
import { throughChain } from "./fromname.js";
import { hooks } from "./hooks.js";
import { columnLabel } from "./columns.js";
import { previewPlan, previewSource, selectedPaths, setPreviewPlan, setPreviewSource } from "./state.js";

// The pattern is panel state, like FROM NAME's, and kept apart from it: a
// tracklist line ("01. Artist - Title") rarely looks like a file name.
const FROM_TEXT_MASK_STORAGE_KEY = "tagrex.fromTextMask";

function loadFromTextPrefs() {
  try {
    const mask = localStorage.getItem(FROM_TEXT_MASK_STORAGE_KEY);
    if (mask) el("from-text-mask").value = mask;
  } catch (e) {
    /* unreadable or unavailable — fall back to the default in the markup */
  }
}

function saveFromTextPrefs() {
  try {
    localStorage.setItem(FROM_TEXT_MASK_STORAGE_KEY, el("from-text-mask").value);
  } catch (e) {
    /* localStorage unavailable — preference just won't persist */
  }
}

// Whether anything has been pasted at all; which lines are tracks is the
// backend's call, made with the mask.
function hasText() {
  return el("from-text-lines").value.trim() !== "";
}

// How the tracks and the selection pair up, said before anything is staged:
// extra tracks or extra files are left out, and lines the pattern doesn't fit
// are skipped — none of which should be a surprise, so all of it is said.
function renderCount(tracks, files, skipped) {
  let text = "";
  if (tracks && files) {
    if (tracks === files) text = t("fromText.countMatch", { tracks, files });
    else if (tracks > files) text = t("fromText.countMoreLines", { tracks, files });
    else text = t("fromText.countMoreFiles", { tracks, files });
  } else if (tracks || skipped) {
    text = t("fromText.countLines", { tracks });
  }
  if (skipped) text += t("fromText.countSkipped", { skipped });
  el("from-text-count").textContent = text;
}

async function previewTagsFromText() {
  const paths = selectedPaths();
  if (paths.length === 0) {
    toast(t("toast.selectOneTrack"), true);
    return;
  }
  if (!hasText()) {
    toast(t("fromText.noLines"), true);
    return;
  }
  try {
    const raw = await invoke("preview_tags_from_text", {
      mask: el("from-text-mask").value,
      text: el("from-text-lines").value,
      paths,
    });
    // FROM NAME's chain, as part of the same press (#237): both read values
    // out of a string with a mask, and both want them tidied the same way.
    const plan = await runChainOverPlan(raw, "fromname");
    if (plan.changes.length === 0) {
      toast(t("fromText.nothingNew"), true);
      return;
    }
    setPreviewPlan(plan);
    setPreviewSource("fromtext");
    hooks.renderPreview(previewPlan);
  } catch (e) {
    toast(String(e), true);
  }
}

// The live read-out: how many tracks the pattern finds, and what it pulls out
// of the first one — the track the first selected file gets.
async function refreshTextProbe() {
  const box = el("from-text-probe");
  if (!box) return;
  const paths = selectedPaths();
  if (!hasText()) {
    renderCount(0, paths.length, 0);
    box.innerHTML = "";
    return;
  }
  try {
    const result = await invoke("probe_tags_from_text", {
      mask: el("from-text-mask").value,
      text: el("from-text-lines").value,
    });
    renderCount(result.tracks, paths.length, result.skipped);
    const probe = result.first;
    const subject = `<div class="probe-subject">${escapeHtml(probe.subject)}</div>`;
    if (!probe.matched || probe.fields.length === 0) {
      box.innerHTML = `${subject}<div class="probe-fields"><div class="probe-miss">${escapeHtml(t("fromText.probeNoMatch"))}</div></div>`;
      return;
    }
    const fields = paths[0] ? await throughChain(paths[0], probe.fields) : probe.fields;
    const rows = fields
      .map(
        ([field, value]) =>
          `<div class="probe-row"><span class="probe-field">${escapeHtml(columnLabel(field))}</span><span class="probe-value">${escapeHtml(value)}</span></div>`,
      )
      .join("");
    box.innerHTML = `${subject}<div class="probe-fields">${rows}</div>`;
  } catch (e) {
    renderCount(0, paths.length, 0);
    box.innerHTML = `<div class="probe-fields"><div class="probe-miss">${escapeHtml(String(e))}</div></div>`;
  }
}

let textProbeTimer = null;
function scheduleTextProbe() {
  if (el("subtab-fromtext").hidden) return;
  clearTimeout(textProbeTimer);
  textProbeTimer = setTimeout(refreshTextProbe, 180);
}

// A list kept in a file: picked with the system dialog and read by the
// backend, which refuses anything that isn't text rather than pasting it in.
async function loadTextFile() {
  const dialog = window.__TAURI__ && window.__TAURI__.dialog;
  if (!dialog) return;
  try {
    const picked = await dialog.open({
      multiple: false,
      filters: [{ name: t("fromText.fileFilter"), extensions: ["txt", "text", "csv", "tsv", "m3u", "cue"] }],
    });
    if (!picked) return;
    el("from-text-lines").value = await invoke("read_text_list", { path: picked });
    refreshTextProbe();
  } catch (e) {
    toast(String(e), true);
  }
}

// ---- wire up ----
el("from-text-preview").addEventListener("click", previewTagsFromText);
el("from-text-load").addEventListener("click", loadTextFile);
el("from-text-mask").addEventListener("input", () => {
  saveFromTextPrefs();
  scheduleTextProbe();
});
el("from-text-lines").addEventListener("input", scheduleTextProbe);

loadFromTextPrefs();

// The chain is FROM NAME's, so a change to it changes what this panel would
// produce as well (#248).
onChainChanged("fromname", () => {
  refreshTextProbe();
  if (previewSource === "fromtext" && previewPlan?.changes.length) previewTagsFromText();
});

export { refreshTextProbe, scheduleTextProbe };
