// The FROM NAME sub-tab of TAGGER (#139, #141, #143 split it out of app.js).
//
// The file's own name read back into tags: the pattern, the replacement table
// that cleans up what it extracts, and the live read-out of what the pattern
// sees. Extraction itself is the mask engine's job in Rust — one grammar, one
// implementation — so this only stages the plan it returns.
import { el, escapeHtml, toast } from "./dom.js";
import { invoke } from "./invoke.js";
import { hooks } from "./hooks.js";
import { columnLabel } from "./columns.js";
import { previewPlan, selectedPaths, setPreviewPlan, setPreviewSource } from "./state.js";

// FROM NAME's pattern and its replacement table (#141). Both are the user's
// working state for the panel, not backend settings, so they live in
// localStorage beside the other persisted panel prefs — a replacement table
// retyped every session would defeat the point of having one.
const FROM_NAME_MASK_STORAGE_KEY = "tagrex.fromNameMask";
const FROM_NAME_REPL_STORAGE_KEY = "tagrex.fromNameReplacements";
// Separators are what names carry instead of spaces, so the table starts with
// the one everybody needs; it's an ordinary row and can be deleted.
const DEFAULT_REPLACEMENTS = [{ from: "_", to: " " }];
let nameReplacements = DEFAULT_REPLACEMENTS.slice();

function loadFromNamePrefs() {
  try {
    const mask = localStorage.getItem(FROM_NAME_MASK_STORAGE_KEY);
    if (mask) el("from-name-mask").value = mask;
    const stored = localStorage.getItem(FROM_NAME_REPL_STORAGE_KEY);
    if (stored) {
      const rows = JSON.parse(stored);
      // An empty table is a real choice (the user deleted every row), so only
      // a malformed value falls back to the default.
      if (Array.isArray(rows)) {
        nameReplacements = rows.map((r) => ({ from: String(r.from || ""), to: String(r.to || "") }));
      }
    }
  } catch (e) {
    /* unreadable or unavailable — fall back to the defaults */
  }
  renderReplacements();
}

function saveFromNamePrefs() {
  try {
    localStorage.setItem(FROM_NAME_MASK_STORAGE_KEY, el("from-name-mask").value);
    localStorage.setItem(FROM_NAME_REPL_STORAGE_KEY, JSON.stringify(nameReplacements));
  } catch (e) {
    /* localStorage unavailable — preference just won't persist */
  }
}

// One row per replacement, rebuilt whenever the list changes. Typing in a row
// updates the list in place (no re-render, or the caret would jump).
function renderReplacements() {
  const box = el("repl-rows");
  box.innerHTML = nameReplacements
    .map(
      (row, i) => `
      <div class="repl-row" data-index="${i}">
        <input class="repl-from" type="text" placeholder="find" spellcheck="false" value="${escapeHtml(row.from)}" />
        <span class="repl-arrow" aria-hidden="true">→</span>
        <input class="repl-to" type="text" placeholder="replace with" spellcheck="false" value="${escapeHtml(row.to)}" />
        <button class="icon repl-del" type="button" title="Remove" aria-label="Remove"><svg class="ico"><use href="#i-close"/></svg></button>
      </div>`,
    )
    .join("");
  if (!nameReplacements.length) {
    box.innerHTML = `<div class="repl-empty muted">Values go into the tags exactly as the name spells them.</div>`;
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
      replacements: nameReplacements,
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
      replacements: nameReplacements,
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

// The replacement table (#141). Typing edits the row in place — re-rendering on
// every keystroke would move the caret — and the list is what both the probe
// and the preview send.
el("repl-rows").addEventListener("input", (e) => {
  const row = e.target.closest(".repl-row");
  if (!row) return;
  const entry = nameReplacements[Number(row.dataset.index)];
  if (!entry) return;
  if (e.target.classList.contains("repl-from")) entry.from = e.target.value;
  if (e.target.classList.contains("repl-to")) entry.to = e.target.value;
  saveFromNamePrefs();
  scheduleNameProbe();
});
el("repl-rows").addEventListener("click", (e) => {
  const del = e.target.closest(".repl-del");
  if (!del) return;
  nameReplacements.splice(Number(del.closest(".repl-row").dataset.index), 1);
  renderReplacements();
  saveFromNamePrefs();
  scheduleNameProbe();
});
el("repl-add").addEventListener("click", () => {
  nameReplacements.push({ from: "", to: "" });
  renderReplacements();
  el("repl-rows").querySelector(".repl-row:last-child .repl-from")?.focus();
});

// The pattern and the table come back from the last session (#141).
loadFromNamePrefs();

export { previewTagsFromName, refreshNameProbe, scheduleNameProbe };
