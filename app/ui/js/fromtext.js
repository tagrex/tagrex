// The FROM TEXT sub-tab of TAGGER (#61): tags read out of a text list.
//
// A tracklist that exists only as text — a forum post, a CD insert, a release
// page no source has — pasted or loaded here, one line per track. Each line is
// read with a mask the way FROM NAME reads a file name; the lines it fits are
// the tracks, and the nth track goes to the nth selected file in table order. Extraction is the mask engine's job
// in Rust, as it is for FROM NAME; this module only stages the plan it returns,
// and the chain FROM NAME runs over its values runs over these too.
import { el, escapeHtml, toast } from "./dom.js";
import { t, tn } from "./i18n.js";
import { invoke } from "./invoke.js";
import { onChainChanged, runChainOverPlan } from "./chains.js";
import { throughChain } from "./fromname.js";
import { hooks } from "./hooks.js";
import { probeMessage, renderProbeTracks } from "./probe-view.js";
import { captureSelection, isSelectionCurrent, previewPlan, previewSource, selectedPaths, setPreviewPlan, setPreviewSource } from "./state.js";

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
  const parts = [];
  if (tracks) parts.push(tn("inspector.matchCount", Math.min(tracks, files) || tracks));
  if (tracks && !files) parts.push(t("inspector.noFiles"));
  if (files && tracks && files > tracks) parts.push(tn("inspector.unmatchedFiles", files - tracks));
  if (files && tracks > files) parts.push(tn("inspector.extraLines", tracks - files));
  if (skipped) parts.push(tn("inspector.skippedLines", skipped));
  el("from-text-count").textContent = parts.join(" · ");
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

// The characters that decide whether a mask fits and can't be told apart on
// screen (#521): a tab looks like spaces, a page's en dash like the keyboard's
// hyphen, a non-breaking space like a space. Each maps to the name its marker's
// tooltip gives; a tab is drawn as an arrow, the rest as themselves, marked.
const MARKED_CHARS = new Map([
  ["\t", "fromText.char.tab"],
  ["\u2010", "fromText.char.hyphen"],
  ["\u2011", "fromText.char.hyphen"],
  ["\u2012", "fromText.char.figureDash"],
  ["\u2013", "fromText.char.enDash"],
  ["\u2014", "fromText.char.emDash"],
  ["\u2015", "fromText.char.bar"],
  ["\u2212", "fromText.char.minus"],
  ["\u00a0", "fromText.char.nbsp"],
  ["\u2007", "fromText.char.nbsp"],
  ["\u202f", "fromText.char.nbsp"],
  ["\u2009", "fromText.char.thinSpace"],
  ["\u200a", "fromText.char.thinSpace"],
  ["\u200b", "fromText.char.zeroWidth"],
  ["\u2060", "fromText.char.zeroWidth"],
  ["\ufeff", "fromText.char.zeroWidth"],
]);

// The probe's subject line with those characters made visible. Display only:
// the list itself, and what the mask is matched against, stay as they are.
function visibleSubject(text) {
  let html = "";
  for (const ch of text) {
    const name = MARKED_CHARS.get(ch);
    if (!name) {
      html += escapeHtml(ch);
      continue;
    }
    const code = `U+${ch.codePointAt(0).toString(16).toUpperCase().padStart(4, "0")}`;
    const title = escapeHtml(`${t(name)} · ${code}`);
    if (ch === "\t") html += `<span class="char-mark tab" title="${title}">⇥</span>`;
    else if (name === "fromText.char.zeroWidth") html += `<span class="char-mark empty" title="${title}">·</span>`;
    else html += `<span class="char-mark" title="${title}">${escapeHtml(ch)}</span>`;
  }
  return html;
}

// Totals use the full backend probe. Only a small window starting at its
// first matching line is returned as per-line data, avoiding a library-sized
// IPC payload on every keystroke. Sparse lists may show fewer sample tracks.
function sampleTextWindow(text, firstSubject) {
  let start = 0;
  while (start < text.length) {
    const newline = text.indexOf("\n", start);
    const end = newline < 0 ? text.length : newline;
    if (text.slice(start, end).trim() === firstSubject) {
      let limit = start;
      for (let line = 0; line < 64; line++) {
        const next = text.indexOf("\n", limit);
        if (next < 0) return text.slice(start);
        limit = next + 1;
      }
      return text.slice(start, limit);
    }
    if (newline < 0) break;
    start = newline + 1;
  }
  return null;
}

let textProbeGeneration = 0;
async function refreshTextProbe() {
  const box = el("from-text-probe");
  if (!box) return;
  const generation = ++textProbeGeneration;
  const subject = captureSelection();
  const paths = subject.paths;
  const mask = el("from-text-mask").value;
  const text = el("from-text-lines").value;
  const current = () => generation === textProbeGeneration && isSelectionCurrent(subject) && mask === el("from-text-mask").value && text === el("from-text-lines").value;
  if (!text.trim()) {
    renderCount(0, paths.length, 0);
    box.innerHTML = "";
    return;
  }
  try {
    const result = await invoke("probe_tags_from_text", { mask, text });
    if (!current()) return;
    const first = result.first;
    if (!first.matched || !first.fields.length) {
      renderCount(result.tracks, paths.length, result.skipped);
      const tabs = !result.tracks && first.subject.includes("\t") && !mask.includes("\\t");
      box.innerHTML = `<div class="probe-source"><div class="probe-subject">${visibleSubject(first.subject)}</div></div><p class="probe-miss">${escapeHtml(t("fromText.probeNoMatch"))}</p>${tabs ? `<p class="probe-miss">${escapeHtml(t("fromText.tabHint"))}</p>` : ""}`;
      return;
    }
    let probes = [{ probe: first, path: paths[0] }];
    // Match whole trimmed lines, not substrings of skipped lines. The backend
    // remains the authority on matches and the nth-match → nth-file mapping.
    if (result.tracks > 1 && paths.length > 1) {
      const window = sampleTextWindow(text, first.subject);
      if (window !== null) {
        const sample = await invoke("probe_text_lines", { mask, text: window, paths: paths.slice(0, 3) });
        if (!current()) return;
        const matches = sample.lines.filter(line => line.matched && line.path).slice(0, 3);
        if (matches[0]?.line === first.subject) probes = matches.map(line => ({ probe: { ...line, subject: line.line }, path: line.path }));
      }
    }
    const samples = [];
    for (const { probe, path } of probes) {
      const fields = path ? await throughChain(path, probe.fields) : probe.fields;
      if (!current()) return;
      samples.push({ probe, fields, path });
    }
    renderCount(result.tracks, paths.length, result.skipped);
    renderProbeTracks(box, samples, { subjectHtml: visibleSubject });
  } catch (e) {
    if (!current()) return;
    renderCount(0, paths.length, 0);
    probeMessage(box, String(e));
  }
}

let textProbeTimer = null;
function scheduleTextProbe() {
  textProbeGeneration++;
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

// A mask read off the list's own layout (#522): filled in only when asked,
// since it replaces what is in the box — the probe then shows what it reads.
async function suggestMask() {
  if (!hasText()) {
    toast(t("fromText.noLines"), true);
    return;
  }
  try {
    const mask = await invoke("suggest_text_list_mask", { text: el("from-text-lines").value });
    if (!mask) {
      toast(t("fromText.noSuggestion"), true);
      return;
    }
    el("from-text-mask").value = mask;
    saveFromTextPrefs();
    refreshTextProbe();
  } catch (e) {
    toast(String(e), true);
  }
}

// ---- wire up ----
el("from-text-preview").addEventListener("click", previewTagsFromText);
el("from-text-suggest").addEventListener("click", suggestMask);
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

export { previewTagsFromText, refreshTextProbe, scheduleTextProbe, visibleSubject };
