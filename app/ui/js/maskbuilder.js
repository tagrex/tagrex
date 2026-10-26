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
//
// The same window serves FROM NAME (#548), the grammar run the other way: the
// file name in, tags out. There the list is a table, the file name and then a
// column for every tag the mask names, each value marked as the preview would
// treat it. A mode says which box the mask comes from and goes back to, how the
// list is computed and what OK stages.
import { el, escapeHtml } from "./dom.js";
import { t, tn } from "./i18n.js";
import { invoke } from "./invoke.js";
import { activeRuleCount, chainHasRules, onChainChanged, runChainOverPlan } from "./chains.js";
import { columnLabel } from "./columns.js";
import { previewTagsFromName } from "./fromname.js";
import { openTransformModal } from "./generator.js";
import { catalogue, insertAt, renderReference } from "./placeholders.js";
import { maskPart, planForMask, preview } from "./renamer.js";
import { selectedPaths, tracks } from "./state.js";

// Rows the list shows. A list is for reading, and a few hundred names is past
// what anyone reads; the count says how many more there are.
const LIMIT = 300;
const DEBOUNCE_MS = 200;
// The size the window was last left at (#541), as "width,height" in pixels.
const SIZE_STORAGE_KEY = "tagrex.maskBuilderSize";

const modal = el("mask-builder");
const dialog = modal.querySelector(".mask-builder");
const expr = el("mb-expr");

let entries = [];
let timer = null;
// Which refresh is the latest, so a slow answer to an older expression never
// paints over the answer to a newer one.
let generation = 0;

function baseName(path) {
  return String(path).split(/[\\/]/).pop();
}

// The ticked presets' share in what the list shows (#543), said as the
// one-file example under the RENAMER mask box says it.
function rulesNote() {
  const rules = activeRuleCount(mode.context);
  return rules ? escapeHtml(t("renamer.exampleChain", { rules: tn("unit.rule", rules) })) : "";
}

// As markup: the counts of the two problems are marked in red (#546), the way
// their rows are.
function summary(rows, total) {
  const count = (kind) => rows.filter((row) => row.kind === kind).length;
  const parts = [];
  const plain = (text) => escapeHtml(text);
  const marked = (text, kind) => `<span class="mb-sum-${kind}">${escapeHtml(text)}</span>`;
  const changed = rows.filter((row) => row.kind === "changed" || row.kind === "clash").length;
  if (changed) parts.push(plain(tn("maskBuilder.changed", changed)));
  if (count("unchanged")) parts.push(plain(tn("maskBuilder.unchangedCount", count("unchanged"))));
  if (count("unrendered")) parts.push(marked(tn("maskBuilder.missingCount", count("unrendered")), "missing"));
  if (count("clash")) parts.push(marked(tn("maskBuilder.clashCount", count("clash")), "clash"));
  if (total > rows.length) parts.push(plain(t("maskBuilder.limited", { shown: rows.length, total })));
  if (rulesNote()) parts.push(rulesNote());
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

function rowsHtml(rows) {
  return rows
    .map((row) => {
      // The empty placeholder keeps its code face inside the sentence (#546).
      const token = `%${row.field}%`;
      const after =
        row.kind === "unrendered"
          ? escapeHtml(t("maskBuilder.needs", { field: token })).replace(
              escapeHtml(token),
              `<code>${escapeHtml(token)}</code>`
            )
          : escapeHtml(row.kind === "unchanged" ? t("maskBuilder.unchanged") : row.target);
      const note = row.kind === "clash" ? `<span class="mb-note">${escapeHtml(t("maskBuilder.clash"))}</span>` : "";
      return `<div class="mb-row mb-${row.kind}">
        <span class="mb-old" title="${escapeHtml(row.path)}">${escapeHtml(baseName(row.path))}</span>
        <span class="mb-arrow" aria-hidden="true">→</span>
        <span class="mb-new">${after}</span>${note}
      </div>`;
    })
    .join("");
}

// Render mode (RENAMER): the plan Preview would stage, one row per file.
async function renderList(mask, shown, total) {
  const rows = rowsFor(await planForMask(mask, shown), shown);
  return { html: rowsHtml(rows), summary: summary(rows, total) };
}

// Extract mode (FROM NAME, #548): what the mask reads out of each name, through
// the FROM NAME presets as Preview tags runs them (#237), against the tags the
// file carries now. A value is marked when it is the same as the tag already
// there, when a rule changed what was read (the reading in its tooltip), and
// when the field cannot hold it; a name the mask does not fit is one row.
async function extractList(mask, shown, total) {
  const probes = await invoke("probe_tags_from_names", { mask, paths: shown });
  const readings = probes.files.map((probe) => new Map(probe.fields));
  let tidied = readings;
  if (chainHasRules("fromname")) {
    const plan = {
      description: "",
      changes: shown.map((path, index) => ({
        path,
        rename_to: null,
        tag_changes: [...readings[index]].map(([field, value]) => ({ field, old: null, new: value })),
      })),
    };
    const out = await runChainOverPlan(plan, "fromname");
    const byPath = new Map((out?.changes || []).map((change) => [change.path, change]));
    tidied = shown.map((path, index) => {
      const change = byPath.get(path);
      return change ? new Map(change.tag_changes.map((tc) => [tc.field, tc.new ?? ""])) : readings[index];
    });
  }
  const current = new Map(tracks.map((track) => [track.path, track.tags || {}]));
  const counts = { read: 0, noFit: 0, tidied: 0, bad: 0 };
  const head = `<tr><th class="mb-old">${escapeHtml(t("maskBuilder.fileName"))}</th>${probes.fields
    .map((field) => `<th>${escapeHtml(columnLabel(field))}</th>`)
    .join("")}</tr>`;
  const body = probes.files
    .map((probe, index) => {
      const path = shown[index];
      const name = `<td class="mb-old" title="${escapeHtml(path)}"><span>${escapeHtml(baseName(path))}</span><span class="mb-arrow" aria-hidden="true">→</span></td>`;
      if (!probe.matched) {
        counts.noFit += 1;
        return `<tr class="mb-nomatch">${name}<td class="mb-miss" colspan="${Math.max(1, probes.fields.length)}">${escapeHtml(t("maskBuilder.noFit"))}</td></tr>`;
      }
      counts.read += 1;
      const cells = probes.fields.map((field) => {
        const raw = readings[index].get(field);
        if (raw === undefined) return `<td class="mb-none">—</td>`;
        const value = tidied[index].get(field) ?? raw;
        if (probe.invalid.includes(field)) {
          counts.bad += 1;
          return `<td class="mb-bad" title="${escapeHtml(t("maskBuilder.cantWrite"))}">${escapeHtml(value)}</td>`;
        }
        if (value !== raw) {
          counts.tidied += 1;
          return `<td class="mb-tidied" title="${escapeHtml(t("maskBuilder.readAs", { raw }))}">${escapeHtml(value)}</td>`;
        }
        const same = (current.get(path)?.[field] ?? "") === value;
        return `<td${same ? ' class="mb-same"' : ""}>${escapeHtml(value)}</td>`;
      });
      return `<tr>${name}${cells.join("")}</tr>`;
    })
    .join("");
  const parts = [escapeHtml(tn("unit.file", total))];
  if (counts.read) parts.push(escapeHtml(tn("maskBuilder.readCount", counts.read)));
  if (counts.noFit) parts.push(`<span class="mb-sum-missing">${escapeHtml(tn("maskBuilder.noFitCount", counts.noFit))}</span>`);
  if (counts.tidied) parts.push(`<span class="mb-sum-tidied">${escapeHtml(tn("maskBuilder.tidiedCount", counts.tidied))}</span>`);
  if (counts.bad) parts.push(`<span class="mb-sum-clash">${escapeHtml(tn("maskBuilder.badCount", counts.bad))}</span>`);
  if (total > shown.length) parts.push(escapeHtml(t("maskBuilder.limited", { shown: shown.length, total })));
  if (rulesNote()) parts.push(rulesNote());
  return {
    html: `<table class="mb-table"><thead>${head}</thead><tbody>${body}</tbody></table>`,
    summary: parts.join(" · "),
  };
}

const MODES = {
  render: {
    context: "renamer",
    source: "mask",
    opener: "mask-builder-open",
    title: "maskBuilder.title",
    list: renderList,
    accept: () => preview(),
  },
  extract: {
    context: "fromname",
    source: "from-name-mask",
    opener: "from-name-builder-open",
    title: "maskBuilder.titleExtract",
    list: extractList,
    accept: () => previewTagsFromName(),
  },
};
let mode = MODES.render;

async function refresh() {
  const mine = ++generation;
  const paths = selectedPaths();
  if (!paths.length) {
    el("mb-error").hidden = true;
    expr.parentElement.classList.remove("has-error");
    el("mb-summary").textContent = "";
    el("mb-list").innerHTML = `<p class="mb-empty muted">${escapeHtml(t("maskBuilder.noSelection"))}</p>`;
    return;
  }
  const shown = paths.slice(0, LIMIT);
  try {
    const result = await mode.list(expr.value, shown, paths.length);
    if (mine !== generation) return;
    el("mb-error").hidden = true;
    expr.parentElement.classList.remove("has-error");
    el("mb-list").innerHTML = result.html;
    el("mb-summary").innerHTML = result.summary;
  } catch (e) {
    if (mine !== generation) return;
    // The list stays as the last expression that parsed left it: half-typed
    // `$if(` is a moment, not a reason to blank the screen.
    el("mb-error").textContent = String(e);
    el("mb-error").hidden = false;
    expr.parentElement.classList.add("has-error");
  }
}

function schedule() {
  clearTimeout(timer);
  timer = setTimeout(refresh, DEBOUNCE_MS);
}

// The window opens at the size it was left at, if that still fits; the
// stylesheet's maximum keeps a size from a larger screen inside this one.
function restoreSize() {
  let stored = null;
  try {
    stored = localStorage.getItem(SIZE_STORAGE_KEY);
  } catch (e) {
    /* storage unavailable: the default size */
  }
  const [width, height] = String(stored || "").split(",").map(Number);
  dialog.style.width = width > 0 ? `${width}px` : "";
  dialog.style.height = height > 0 ? `${height}px` : "";
}

// Kept only when the corner was dragged: a default size written back on every
// close would stop the window following the app window's own size.
let openedSize = null;

function rememberSize() {
  if (modal.hidden || !openedSize) return;
  const { width, height } = dialog.getBoundingClientRect();
  if (Math.abs(width - openedSize.width) < 2 && Math.abs(height - openedSize.height) < 2) return;
  try {
    localStorage.setItem(SIZE_STORAGE_KEY, `${Math.round(width)},${Math.round(height)}`);
  } catch (e) {
    /* storage unavailable: the size just isn't kept */
  }
}

// The Transform button says how many rules run (#545): the count beside the
// wand (#546) and, the way the toolbar's own wand does, its tooltip.
function paintTransformButton() {
  const button = el("mb-transform");
  const count = activeRuleCount(mode.context);
  el("mb-rules").textContent = count ? String(count) : "";
  button.classList.toggle("is-on", count > 0);
  button.title = count
    ? t("generator.chainBtn.some", { rules: tn("unit.rule", count) })
    : t("generator.chainBtn.none");
  button.setAttribute("aria-label", button.title);
  if (button.dataset.tipText) button.dataset.tipText = button.title;
}

// One line, then as many as the mask wraps to, up to MAX_LINES (#545). Also
// tells the field whether it is empty, which hides Clear (#546).
const MAX_LINES = 5;
function fitField() {
  expr.parentElement.classList.toggle("is-empty", !expr.value);
  expr.style.height = "auto";
  const line = parseFloat(getComputedStyle(expr).lineHeight) || 20;
  const chrome = expr.offsetHeight - expr.clientHeight;
  const padding = parseFloat(getComputedStyle(expr).paddingTop) + parseFloat(getComputedStyle(expr).paddingBottom);
  const most = line * MAX_LINES + padding + chrome;
  const wanted = expr.scrollHeight + chrome;
  expr.style.height = `${Math.min(wanted, most)}px`;
  expr.style.overflowY = wanted > most ? "auto" : "hidden";
}

// The reference offers what this direction can use: reading a name back cannot
// run a function or fill a computed value (#548).
function referenceEntries() {
  return mode === MODES.extract ? entries.filter((entry) => entry.extract) : entries;
}

async function open(which) {
  mode = which;
  dialog.classList.toggle("is-extract", mode === MODES.extract);
  el("mb-title").textContent = t(mode.title);
  restoreSize();
  paintTransformButton();
  expr.value = el(mode.source).value;
  el("mb-ref-filter").value = "";
  el("mb-ref-filter").parentElement.classList.add("is-empty");
  el("mb-error").hidden = true;
  modal.hidden = false;
  fitField();
  const { width, height } = dialog.getBoundingClientRect();
  openedSize = { width, height };
  expr.focus();
  expr.setSelectionRange(expr.value.length, expr.value.length);
  refresh();
  try {
    entries = await catalogue();
    renderReference(referenceEntries(), "", el("mb-ref-body"));
  } catch (e) {
    el("mb-ref-body").textContent = String(e);
  }
}

function close() {
  rememberSize();
  clearTimeout(timer);
  generation++;
  modal.hidden = true;
  el(mode.opener).focus();
}

// The mask goes into the box the way typing it there would (it is stored and
// the one-file example follows), and the preview is staged as Preview would.
function accept() {
  const mask = el(mode.source);
  mask.value = expr.value;
  mask.dispatchEvent(new Event("input", { bubbles: true }));
  close();
  mode.accept();
}

el("mask-builder-open").addEventListener("click", () => open(MODES.render));
el("from-name-builder-open").addEventListener("click", () => open(MODES.extract));
el("mb-close").addEventListener("click", close);
el("mb-cancel").addEventListener("click", close);
el("mb-ok").addEventListener("click", accept);
// Cleared as an edit of the field (#542), so Cmd+Z brings the mask back; the
// direct assignment is only for an engine without the editing command.
el("mb-clear").addEventListener("click", () => {
  expr.focus();
  expr.select();
  if (!expr.value || document.execCommand("delete")) return;
  expr.value = "";
  expr.dispatchEvent(new Event("input", { bubbles: true }));
});
expr.addEventListener("input", () => {
  // A mask is one line: a pasted line break would only ever be an error.
  if (/[\r\n]/.test(expr.value)) {
    const caret = expr.selectionStart;
    const before = expr.value.slice(0, caret).replace(/[\r\n]+/g, "");
    expr.value = before + expr.value.slice(caret).replace(/[\r\n]+/g, "");
    expr.setSelectionRange(before.length, before.length);
  }
  fitField();
  schedule();
});
expr.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && !e.isComposing) {
    e.preventDefault();
    accept();
  }
});
// The filter's × (#549): shown only while there is text to clear.
el("mb-ref-clear").addEventListener("click", () => {
  const box = el("mb-ref-filter");
  box.value = "";
  box.dispatchEvent(new Event("input", { bubbles: true }));
  box.focus();
});
el("mb-ref-filter").addEventListener("input", (e) => {
  e.target.parentElement.classList.toggle("is-empty", !e.target.value);
  renderReference(referenceEntries(), e.target.value, el("mb-ref-body"));
});
el("mb-ref-body").addEventListener("click", (e) => {
  const row = e.target.closest(".ph-row[data-token]");
  if (row) insertAt(expr, row.dataset.token);
});
el("mb-transform").addEventListener("click", openTransformModal);
// A narrower or wider window wraps the mask differently.
new ResizeObserver(() => {
  if (!modal.hidden) fitField();
}).observe(dialog);
// A preset ticked or edited in the Transform window changes every value here.
for (const context of ["renamer", "fromname"]) {
  onChainChanged(context, () => {
    if (modal.hidden || mode.context !== context) return;
    paintTransformButton();
    schedule();
  });
}
modal.addEventListener("keydown", (e) => {
  // Whatever is open on top of the builder (the presets menu, the Transform
  // window, the preset editor) closes on Escape first.
  const above =
    !el("preset-menu").hidden ||
    !!document.querySelector(".modal-backdrop:not([hidden]):not(#mask-builder)");
  if (e.key === "Escape" && !above) {
    e.preventDefault();
    close();
  }
});
modal.addEventListener("mousedown", (e) => {
  if (e.target === modal) close();
});
