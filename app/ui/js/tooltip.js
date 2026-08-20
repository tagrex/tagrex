// The app's own tooltip (#230).
//
// Written for the controls that carry no label at all. The platform bubble a
// `title` attribute produces takes about a second to appear, is drawn by the
// OS, and looks like a visitor from another program — tolerable behind a
// control that already says what it does in words, useless behind a bare glyph.
// This shows the same text, in the app's chrome, after a short delay.
//
// It reads the `title` and takes it OFF the element while the pointer is there,
// putting it back on the way out. That is the only way to keep the platform
// bubble from turning up underneath ours; the attribute is present at every
// other moment, so the accessible name and every existing `btn.title = …` in
// the code carry on working untouched.
//
// Deliberately scoped to the chrome — the bars, the mode tabs and the panels —
// and never the table: a cell's title is data-ish, follows the pointer across
// hundreds of rows, and the native bubble is the right weight for it.
import { el } from "./dom.js";

// Long enough not to fire while the pointer crosses a row of buttons on its way
// somewhere, short enough to feel like an answer rather than a wait.
const DELAY = 400;

// Where a title belongs to a control rather than to content.
const SCOPE = "header.topbar, .modebar, .view-tabs, footer.statusbar, .mode-panel";

let timer = 0;
let anchor = null;

function box() {
  return el("tooltip");
}

// Centred under the anchor, flipped above when there is no room below, and
// clamped to the window either way — the same treatment the menus get, minus
// the scrolling they need and this does not.
function place(tip, target) {
  const rect = target.getBoundingClientRect();
  const gap = 6;
  tip.hidden = false;
  const { offsetWidth: w, offsetHeight: h } = tip;
  const left = rect.left + rect.width / 2 - w / 2;
  tip.style.left = `${Math.min(Math.max(6, left), Math.max(6, window.innerWidth - w - 6))}px`;
  const below = rect.bottom + gap;
  tip.style.top = below + h + 6 <= window.innerHeight ? `${below}px` : `${rect.top - gap - h}px`;
}

function show(target) {
  const text = target.dataset.tipText;
  if (!text) return;
  const tip = box();
  tip.textContent = text;
  place(tip, target);
}

export function hideTooltip() {
  clearTimeout(timer);
  timer = 0;
  if (anchor) {
    // Give the attribute back, unless something changed it while it was away —
    // `updateLibAction` rewrites the title on the button under the pointer.
    if (!anchor.hasAttribute("title") && anchor.dataset.tipText) {
      anchor.setAttribute("title", anchor.dataset.tipText);
    }
    delete anchor.dataset.tipText;
    anchor = null;
  }
  const tip = box();
  if (tip) tip.hidden = true;
}

function arm(target) {
  hideTooltip();
  const text = target.getAttribute("title");
  if (!text) return;
  anchor = target;
  target.dataset.tipText = text;
  target.removeAttribute("title");
  timer = setTimeout(() => show(target), DELAY);
}

// One tooltip element, one set of listeners, however many controls there are.
export function initTooltips() {
  if (!box()) return;
  document.addEventListener("pointerover", (e) => {
    if (e.pointerType && e.pointerType !== "mouse") return;
    const target = e.target.closest?.("[title]");
    if (!target || !target.closest(SCOPE)) {
      if (anchor && !anchor.contains(e.target)) hideTooltip();
      return;
    }
    if (target !== anchor) arm(target);
  });
  document.addEventListener("pointerout", (e) => {
    if (anchor && !anchor.contains(e.relatedTarget)) hideTooltip();
  });
  // A tooltip that outlives what it points at is worse than none: a click opens
  // a menu over it, a scroll moves the anchor out from under it.
  document.addEventListener("pointerdown", hideTooltip, true);
  document.addEventListener("scroll", hideTooltip, true);
  window.addEventListener("blur", hideTooltip);
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") hideTooltip();
  });
}
