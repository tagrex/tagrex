// The placeholder reference (#148).
//
// Every mask input gets a button that opens this: the full list of what the
// parser accepts, grouped, with a line of prose each, and a click to insert one
// at the caret. It exists because the list was previously unknowable from inside
// the app — one stale hint under the RENAMER box named nine of the nineteen
// fields, and %catalognumber% was not among them, so the only way to find it was
// to guess or to be online.
//
// The list comes from the backend (`mask_placeholders`), which builds it off the
// same tables the parser reads. A name shown here parses, by construction.
import { el, escapeHtml } from "./dom.js";
import { t, tOr } from "./i18n.js";
import { invoke } from "./invoke.js";

// Fetched once and kept: the grammar can't change while the app runs, and the
// popover has to open instantly on a click.
let cache = null;
// The input the open popover inserts into, and the button it hangs off.
let target = null;
let anchor = null;

async function catalogue() {
  if (!cache) cache = await invoke("mask_placeholders");
  return cache;
}

// The grammar isn't placeholders, but it's the other half of what someone
// staring at a pattern box needs, and there is nowhere else to put it.
// The grammar rows are the frontend's own — no backend sends them — so they
// live in the catalogue like any other interface text.
const GRAMMAR = ["width", "section", "call", "separator", "quote"];

function directionNote(entry) {
  if (entry.render && entry.extract) return "";
  return t(entry.render ? "ph.nameOnly" : "ph.readOnly");
}

// What a placeholder does, in the chosen language (#268). The backend sends a
// catalogue key beside the English it composed; a key this build has never seen
// — a newer backend against an older frontend — falls back to that English.
function describe(entry) {
  return tOr(entry.code, entry.description);
}

function rows(entries) {
  return entries
    .map(
      (entry) => `<button type="button" class="ph-row" data-token="${escapeHtml(entry.token)}">
        <span class="ph-token">${escapeHtml(entry.token)}</span>
        <span class="ph-desc">${escapeHtml(describe(entry))}</span>
        ${directionNote(entry) ? `<span class="ph-dir">${directionNote(entry)}</span>` : ""}
      </button>`,
    )
    .join("");
}

function render(entries, filter) {
  const needle = filter.trim().toLowerCase();
  const matching = needle
    ? entries.filter(
        (entry) =>
          entry.name.toLowerCase().includes(needle) ||
          describe(entry).toLowerCase().includes(needle),
      )
    : entries;
  const body = el("ph-body");
  if (!matching.length) {
    body.innerHTML = `<p class="ph-empty muted">${escapeHtml(t("ph.noMatch", { filter }))}</p>`;
    return;
  }
  // Group order follows the backend's order rather than a set here, so the
  // sections can be reordered in one place.
  const groups = [];
  for (const entry of matching) {
    const last = groups[groups.length - 1];
    const name = tOr(entry.group_code, entry.group);
    if (last && last.name === name) last.entries.push(entry);
    else groups.push({ name, entries: [entry] });
  }
  const grammar = needle
    ? ""
    : `<div class="ph-group">
        <div class="ph-group-name">${escapeHtml(t("ph.grammar"))}</div>
        ${GRAMMAR.map(
          (rule) => `<div class="ph-row ph-row-static">
            <span class="ph-token">${escapeHtml(t(`ph.grammar.${rule}.token`))}</span>
            <span class="ph-desc">${escapeHtml(t(`ph.grammar.${rule}`))}</span>
          </div>`,
        ).join("")}
      </div>`;
  body.innerHTML =
    groups
      .map(
        (group) => `<div class="ph-group">
          <div class="ph-group-name">${escapeHtml(group.name)}</div>
          ${rows(group.entries)}
        </div>`,
      )
      .join("") + grammar;
}

// Place the popover against its button. Fixed positioning rather than the
// absolute `.col-menu` shell every other popover uses: this one is tall, and the
// mode panel it opens from scrolls (`overflow: auto`), which would clip it.
function place(menu, button) {
  const rect = button.getBoundingClientRect();
  const width = Math.min(360, window.innerWidth - 16);
  menu.style.width = `${width}px`;
  const left = Math.min(Math.max(8, rect.right - width), window.innerWidth - width - 8);
  menu.style.left = `${left}px`;
  // Below the button when there's room, above it when there isn't.
  const below = window.innerHeight - rect.bottom;
  if (below > 260 || below > rect.top) {
    menu.style.top = `${rect.bottom + 4}px`;
    menu.style.bottom = "auto";
    menu.style.maxHeight = `${below - 12}px`;
  } else {
    menu.style.top = "auto";
    menu.style.bottom = `${window.innerHeight - rect.top + 4}px`;
    menu.style.maxHeight = `${rect.top - 12}px`;
  }
}

export function closeReference() {
  el("ph-menu").hidden = true;
  target = null;
  anchor = null;
}

async function openReference(button, input) {
  const entries = await catalogue();
  const menu = el("ph-menu");
  target = input;
  anchor = button;
  el("ph-filter").value = "";
  render(entries, "");
  menu.hidden = false;
  place(menu, button);
  el("ph-filter").focus();
}

// Insert at the caret of the mask input rather than appending: a pattern is
// usually edited in the middle. The `input` event is dispatched by hand because
// a programmatic value change doesn't fire one, and the live previews (FROM
// NAME's read-out, the export name) listen for it.
function insert(token) {
  if (!target) return;
  const start = target.selectionStart ?? target.value.length;
  const end = target.selectionEnd ?? start;
  target.value = target.value.slice(0, start) + token + target.value.slice(end);
  // A function arrives as an empty call — `$upper()`, `$substr(,,)` — and the
  // next thing to type is its first argument, so the caret lands inside the
  // parentheses rather than after them (#73). A placeholder has nothing to fill
  // in and keeps the caret at the end.
  const opening = token.indexOf("(");
  const caret = start + (opening >= 0 && token.endsWith(")") ? opening + 1 : token.length);
  target.focus();
  target.setSelectionRange(caret, caret);
  target.dispatchEvent(new Event("input", { bubbles: true }));
}

export async function initPlaceholderReference() {
  // One delegated listener for every trigger, so a new mask input needs only the
  // button markup and no wiring.
  document.addEventListener("click", (e) => {
    const button = e.target.closest?.(".ph-btn");
    if (button) {
      e.preventDefault();
      const input = el(button.dataset.phTarget);
      if (!input) return;
      if (anchor === button && !el("ph-menu").hidden) closeReference();
      else openReference(button, input);
      return;
    }
    const row = e.target.closest?.(".ph-row[data-token]");
    if (row) {
      insert(row.dataset.token);
      // Stays open: inserting several placeholders in a row is the common case.
      return;
    }
    if (!e.target.closest?.("#ph-menu")) closeReference();
  });

  el("ph-filter").addEventListener("input", (e) => {
    if (cache) render(cache, e.target.value);
  });

  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !el("ph-menu").hidden) {
      closeReference();
      anchor?.focus();
    }
  });

  // The popover is anchored to a button that moves when the panel or window
  // resizes; re-place rather than leaving it floating somewhere wrong.
  window.addEventListener("resize", () => {
    if (!el("ph-menu").hidden && anchor) place(el("ph-menu"), anchor);
  });

  // Warmed at startup so the column-header lookup below can be synchronous —
  // it runs once per header on every table paint.
  await catalogue();
}

// The placeholder that addresses a column, for its header tooltip (#148) — the
// other half of the fix: the table itself answers "what do I write for this?".
// Read straight from the warmed catalogue, so a column that no placeholder
// addresses (File, Position) correctly gets nothing.
export function placeholderToken(key) {
  // Placeholders only: the catalogue also carries the functions (#73), and a
  // column header wants `%artist%`, never `$replace(,,)`.
  return cache?.find((entry) => entry.name === key && entry.token.startsWith("%"))?.token ?? null;
}
