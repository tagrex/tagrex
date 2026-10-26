// The mask builder (#540): a window in which the RENAMER mask and what it makes
// of every selected file are on screen together.
//
// The box under the mask shows one file (#382) and the placeholder reference
// closes once something is inserted (#148), which leaves a longer expression to
// be written blind. Here the reference stays open beside the expression and the
// list below it is the whole selection, current name and new name, recomputed
// as you type. It is computed by the same call Preview makes, through the same
// rules, so the list and the plan OK stages cannot disagree. Nothing is written
// from the window: OK puts the mask in the box and stages the preview.
import { el, escapeHtml } from "./dom.js";
import { t, tn } from "./i18n.js";
import { catalogue, insertAt, renderReference } from "./placeholders.js";
import { maskPart, planForMask, preview } from "./renamer.js";
import { selectedPaths } from "./state.js";

// Rows the list shows. A list is for reading, and a few hundred names is past
// what anyone reads; the count says how many more there are.
const LIMIT = 300;
const DEBOUNCE_MS = 200;

const modal = el("mask-builder");
const expr = el("mb-expr");

let entries = [];
let timer = null;
// Which refresh is the latest, so a slow answer to an older expression never
// paints over the answer to a newer one.
let generation = 0;

function baseName(path) {
  return String(path).split(/[\\/]/).pop();
}

function summary(rows, total) {
  const count = (kind) => rows.filter((row) => row.kind === kind).length;
  const parts = [];
  const changed = rows.filter((row) => row.kind === "changed" || row.kind === "clash").length;
  if (changed) parts.push(tn("maskBuilder.changed", changed));
  if (count("unchanged")) parts.push(tn("maskBuilder.unchangedCount", count("unchanged")));
  if (count("unrendered")) parts.push(tn("maskBuilder.missingCount", count("unrendered")));
  if (count("clash")) parts.push(tn("maskBuilder.clashCount", count("clash")));
  if (total > rows.length) parts.push(t("maskBuilder.limited", { shown: rows.length, total }));
  return parts.join(" · ");
}

// One row per file: what it is called now and what the mask calls it, marked
// when the name stays, when a required placeholder is empty for it, and when
// another file would end up with the same name.
function rowsFor(plan, paths) {
  const changes = new Map(plan.changes.map((change) => [change.path, change]));
  const missing = new Map();
  for (const entry of plan.unrendered || []) {
    for (const path of entry.paths || []) missing.set(path, entry.field);
  }
  // Where each file ends up, the ones that stay included, compared without case:
  // two names that differ only in case are one file on a case-insensitive disk.
  const landing = new Map();
  for (const path of paths) {
    const key = (changes.get(path)?.rename_to || path).toLowerCase();
    landing.set(key, (landing.get(key) || 0) + 1);
  }
  return paths.map((path) => {
    const change = changes.get(path);
    if (missing.has(path)) return { path, kind: "unrendered", field: missing.get(path) };
    if (!change?.rename_to) return { path, kind: "unchanged" };
    const target = maskPart(change.rename_to, change.rename_root);
    const clash = landing.get(change.rename_to.toLowerCase()) > 1;
    return { path, kind: clash ? "clash" : "changed", target };
  });
}

function paintRows(rows) {
  el("mb-list").innerHTML = rows
    .map((row) => {
      const after =
        row.kind === "unrendered"
          ? t("maskBuilder.needs", { field: `%${row.field}%` })
          : row.kind === "unchanged"
            ? t("maskBuilder.unchanged")
            : row.target;
      const note = row.kind === "clash" ? `<span class="mb-note">${escapeHtml(t("maskBuilder.clash"))}</span>` : "";
      return `<div class="mb-row mb-${row.kind}">
        <span class="mb-old" title="${escapeHtml(row.path)}">${escapeHtml(baseName(row.path))}</span>
        <span class="mb-arrow" aria-hidden="true">→</span>
        <span class="mb-new">${escapeHtml(after)}</span>${note}
      </div>`;
    })
    .join("");
}

async function refresh() {
  const mine = ++generation;
  const paths = selectedPaths();
  if (!paths.length) {
    el("mb-error").hidden = true;
    el("mb-summary").textContent = "";
    el("mb-list").innerHTML = `<p class="mb-empty muted">${escapeHtml(t("maskBuilder.noSelection"))}</p>`;
    return;
  }
  const shown = paths.slice(0, LIMIT);
  try {
    const plan = await planForMask(expr.value, shown);
    if (mine !== generation) return;
    el("mb-error").hidden = true;
    const rows = rowsFor(plan, shown);
    paintRows(rows);
    el("mb-summary").textContent = summary(rows, paths.length);
  } catch (e) {
    if (mine !== generation) return;
    // The list stays as the last expression that parsed left it: half-typed
    // `$if(` is a moment, not a reason to blank the screen.
    el("mb-error").textContent = String(e);
    el("mb-error").hidden = false;
  }
}

function schedule() {
  clearTimeout(timer);
  timer = setTimeout(refresh, DEBOUNCE_MS);
}

async function open() {
  expr.value = el("mask").value;
  el("mb-ref-filter").value = "";
  el("mb-error").hidden = true;
  modal.hidden = false;
  expr.focus();
  expr.setSelectionRange(expr.value.length, expr.value.length);
  refresh();
  try {
    entries = await catalogue();
    renderReference(entries, "", el("mb-ref-body"));
  } catch (e) {
    el("mb-ref-body").textContent = String(e);
  }
}

function close() {
  clearTimeout(timer);
  generation++;
  modal.hidden = true;
  el("mask-builder-open").focus();
}

// The mask goes into the box the way typing it there would (it is stored and
// the one-file example follows), and the preview is staged as Preview would.
function accept() {
  const mask = el("mask");
  mask.value = expr.value;
  mask.dispatchEvent(new Event("input", { bubbles: true }));
  close();
  preview();
}

el("mask-builder-open").addEventListener("click", open);
el("mb-close").addEventListener("click", close);
el("mb-cancel").addEventListener("click", close);
el("mb-ok").addEventListener("click", accept);
expr.addEventListener("input", () => {
  // A mask is one line: a pasted line break would only ever be an error.
  if (/[\r\n]/.test(expr.value)) {
    const caret = expr.selectionStart;
    const before = expr.value.slice(0, caret).replace(/[\r\n]+/g, "");
    expr.value = before + expr.value.slice(caret).replace(/[\r\n]+/g, "");
    expr.setSelectionRange(before.length, before.length);
  }
  schedule();
});
expr.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && !e.isComposing) {
    e.preventDefault();
    accept();
  }
});
el("mb-ref-filter").addEventListener("input", (e) => {
  renderReference(entries, e.target.value, el("mb-ref-body"));
});
el("mb-ref-body").addEventListener("click", (e) => {
  const row = e.target.closest(".ph-row[data-token]");
  if (row) insertAt(expr, row.dataset.token);
});
modal.addEventListener("keydown", (e) => {
  // The presets menu opened from here closes itself on Escape first.
  if (e.key === "Escape" && el("preset-menu").hidden) {
    e.preventDefault();
    close();
  }
});
modal.addEventListener("mousedown", (e) => {
  if (e.target === modal) close();
});
