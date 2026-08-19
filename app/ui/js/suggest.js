// Value auto-complete for the table's editable cells (#63).
//
// Typing an album artist that eleven other files in the library already carry
// is how a single letter of drift gets in — "Various" beside "various", "Warp
// Records" beside "Warp". The values are in memory from the scan, so the cell
// can just offer them.
//
// Two decisions shape the rest of this module:
//
// - **It suggests what the library holds, not a dictionary.** The list is built
//   from the open files each time an edit starts, counted per file and ordered
//   by how many carry each value, so the spelling the album already agrees on
//   comes first. A pending edit counts as the value it staged — correcting one
//   row and then reaching for the same wording on the next is the case the
//   issue is about, and an unapplied edit is exactly as real to the user as a
//   written one.
// - **It never types for you.** Nothing is inserted until a row is chosen: no
//   inline completion, no first row selected by default, so a fast typist who
//   ignores the list ends up with precisely what they typed. Enter reaches the
//   list only after an arrow key has stepped into it, which is what keeps the
//   plain "type it and press Enter" gesture untouched.
//
// The list is fixed-positioned like the other floating boxes, and it stages
// through the cell's own `input` event rather than reaching into app.js — one
// staging path, and no import cycle back into the table.
import { escapeHtml, placeFloating } from "./dom.js";
import { edits, tag, tracks } from "./state.js";

// Fields worth completing is everything except the ones where a repeat is
// meaningless. Written as an exclusion so a field added to the catalogue later
// gets suggestions by default — the friendlier way round to be wrong.
//
// A title, an ISRC and a URL identify one recording; a track/disc number and a
// tempo are per-file numbers. Offering a list of them would be noise in front
// of the value being typed.
const NO_SUGGEST = new Set(["title", "isrc", "url", "track", "tracktotal", "disc", "bpm"]);

// How many rows the list shows. Enough to cover an album's worth of spellings,
// short enough that the answer is visible without reading a menu.
const MAX_ROWS = 8;

const menu = document.createElement("div");
menu.className = "sg-menu";
menu.hidden = true;
document.body.appendChild(menu);

// The cell being edited, the values its field has in the library (most-used
// first), what is currently on show, and which row the arrows have stepped
// onto. `active` is -1 until an arrow key is pressed — see the note above.
let cell = null;
let values = [];
let shown = [];
let active = -1;
// True while a chosen value is being written into the cell: the `input` event
// that stages it would otherwise re-open the list on the value it just wrote.
let applying = false;

// Every value the library holds for `field`, with how many files carry it.
//
// Counted over the whole library rather than the rendered rows: the table is
// windowed (#189), so the DOM holds a few dozen rows and the model is the only
// thing that knows what the other four thousand say.
function collectValues(field) {
  const counts = new Map();
  for (const track of tracks) {
    const pending = edits.get(track.path);
    const value = pending && pending.has(field) ? pending.get(field) : tag(track, field);
    const trimmed = value.trim();
    if (!trimmed) continue;
    counts.set(trimmed, (counts.get(trimmed) || 0) + 1);
  }
  return [...counts.entries()]
    .map(([value, files]) => ({ value, files }))
    // How many files agree first — that is the spelling an album has settled
    // on — and alphabetically among equals so the order is stable rather than
    // whatever the scan happened to produce.
    .sort((a, b) => b.files - a.files || a.value.localeCompare(b.value));
}

// The rows for what has been typed: what starts with it first, then what merely
// contains it, each keeping the by-popularity order. An empty cell offers the
// whole list, which is what makes the list worth opening on an arrow key.
function matches(typed) {
  const needle = typed.trim().toLowerCase();
  if (!needle) return values.slice(0, MAX_ROWS);
  const starting = [];
  const containing = [];
  for (const entry of values) {
    const at = entry.value.toLowerCase().indexOf(needle);
    if (at === 0) starting.push({ ...entry, at });
    else if (at > 0) containing.push({ ...entry, at });
  }
  return [...starting, ...containing].slice(0, MAX_ROWS);
}

// The value with the typed part marked, so it is clear why a row is in the list
// — particularly for the ones matching in the middle.
function markMatch(entry, typed) {
  const length = typed.trim().length;
  if (!length || entry.at === undefined || entry.at < 0) return escapeHtml(entry.value);
  const head = entry.value.slice(0, entry.at);
  const hit = entry.value.slice(entry.at, entry.at + length);
  const tail = entry.value.slice(entry.at + length);
  return `${escapeHtml(head)}<b>${escapeHtml(hit)}</b>${escapeHtml(tail)}`;
}

function paint(typed) {
  menu.innerHTML = shown
    .map(
      (entry, index) => `<button type="button" class="sg-row${index === active ? " active" : ""}"
        data-index="${index}">
        <span class="sg-value">${markMatch(entry, typed)}</span>
        <span class="sg-count">${entry.files}</span>
      </button>`,
    )
    .join("");
}

function place() {
  // At least as wide as the cell, so the list reads as belonging to it, and
  // never so wide that a long album title pushes it off screen.
  menu.style.width = "";
  menu.style.minWidth = `${cell.getBoundingClientRect().width}px`;
  placeFloating(menu, cell, { gap: 2 });
}

export function closeCellSuggest() {
  menu.hidden = true;
  menu.innerHTML = "";
  shown = [];
  active = -1;
}

// Start an edit session: remember the cell and read the library once (#189 —
// off the model, not the rows). Nothing is shown until something is typed, or
// until the down arrow asks for the list.
export function openCellSuggest(target) {
  closeCellSuggest();
  cell = target;
  const field = target.dataset.field;
  values = NO_SUGGEST.has(field) ? [] : collectValues(field);
}

export function endCellSuggest() {
  closeCellSuggest();
  cell = null;
  values = [];
}

// Re-filter after a keystroke. Called from the table's own `input` listener, so
// it sees exactly what was staged.
export function refreshCellSuggest(target) {
  if (applying || target !== cell || values.length === 0) return;
  const typed = target.textContent;
  shown = matches(typed);
  // Nothing to offer, or the only offer is what is already typed: the list has
  // no answer left to give, so it gets out of the way.
  const settled = shown.length === 1 && shown[0].value.toLowerCase() === typed.trim().toLowerCase();
  if (shown.length === 0 || settled) {
    closeCellSuggest();
    return;
  }
  // A list that re-filters under a moving highlight would accept the wrong row
  // on Enter, so stepping in starts again with each keystroke.
  active = -1;
  menu.hidden = false;
  paint(typed);
  place();
}

function step(delta) {
  const opening = menu.hidden;
  if (opening) {
    shown = matches(cell.textContent);
    if (shown.length === 0) return;
    menu.hidden = false;
    active = -1;
  }
  active = (active + delta + shown.length + 1) % (shown.length + 1);
  // The extra position is "none of them": arrowing past the end lets go of the
  // list again and hands the typed value back.
  if (active === shown.length) active = -1;
  paint(cell.textContent);
  // Painted first: placing measures the box, and an empty one would be placed
  // as if it were the width of nothing.
  if (opening) place();
}

function accept(index) {
  const entry = shown[index];
  if (!entry) return;
  const target = cell;
  applying = true;
  target.textContent = entry.value;
  // The caret goes to the end, where typing would carry on from.
  const range = document.createRange();
  range.selectNodeContents(target);
  range.collapse(false);
  const selection = window.getSelection();
  selection.removeAllRanges();
  selection.addRange(range);
  // Stage it the way a keystroke would: the table's own `input` listener is the
  // single path a cell edit takes, and going around it would be a second one.
  target.dispatchEvent(new InputEvent("input", { bubbles: true }));
  applying = false;
  closeCellSuggest();
}

// The keys the list claims, in the cell's keydown handler. Returns true when it
// took the key, so the table's own Enter/Escape handling stays untouched
// whenever the list has nothing to say.
export function cellSuggestKey(e) {
  if (!cell || e.target !== cell || values.length === 0) return false;
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    step(e.key === "ArrowDown" ? 1 : -1);
    return true;
  }
  if (menu.hidden) return false;
  if (e.key === "Escape") {
    e.preventDefault();
    closeCellSuggest();
    return true;
  }
  // Enter and Tab reach the list only once an arrow key has stepped into it —
  // which is what keeps "type it and press Enter" the gesture it has always
  // been for someone who never looks at the list.
  if ((e.key === "Enter" || e.key === "Tab") && active >= 0) {
    e.preventDefault();
    accept(active);
    return true;
  }
  return false;
}

// Choosing with the mouse. `mousedown` rather than `click`, defaulted away, so
// the cell never loses focus — a blur ends the edit and would take the list
// with it before the click landed.
menu.addEventListener("mousedown", (e) => {
  const row = e.target.closest(".sg-row");
  if (!row) return;
  e.preventDefault();
  accept(Number(row.dataset.index));
});

// A fixed-positioned box has to go when what it is anchored to moves. The table
// scrolls its own container, and the window's own scroll is caught in the
// capture phase like the cell tooltip's.
window.addEventListener("scroll", () => closeCellSuggest(), true);
window.addEventListener("resize", () => closeCellSuggest());
