// One card for a running operation (#843): apply, undo, redo, BPM detection
// and the identical-audio duplicate scan.
//
// The floating Apply pill grows into the card, so the operation is shown where
// the user started it. The card names the stage, the share done and the file
// at hand; on success it holds the outcome for a moment and gives the pill back
// (or goes, when nothing is staged); an error stays until it is dismissed.
//
// Animation is decoration only. A hidden window runs neither Web Animations
// nor animation frames, so every state change happens on its own and on
// timers, never on an animation finishing.
import { el } from "./dom.js";

const HOLD_MS = 1400;
const LEAVE_MS = 200;
const EASING = "cubic-bezier(.22,1,.36,1)";

const bar = el("diff-actionbar");
const draft = el("ab-draft");
const card = el("ab-activity");

// Whether a staged plan wants the pill shown; the card borrows the bar while
// it runs and gives it back in this state.
let draftWanted = false;
// idle | running | stopping | success | cancelled | error
let state = "idle";
let owner = null;
let foregroundOwner = false;
let onStop = null;
let holdTimer = null;
let leaveTimer = null;
let frame = null;
let latest = null;

const reduced = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;

function animate(node, frames, duration) {
  if (reduced() || typeof node.animate !== "function") return;
  node.animate(frames, { duration, easing: EASING });
}

function cancelAnimations() {
  if (typeof bar.getAnimations !== "function") return;
  for (const animation of bar.getAnimations({ subtree: true })) animation.cancel();
}

function clearTimers() {
  clearTimeout(holdTimer);
  clearTimeout(leaveTimer);
  holdTimer = leaveTimer = null;
  if (frame !== null) cancelAnimationFrame(frame);
  frame = null;
  latest = null;
}

// The bar is centred by translateX(-50%), so every keyframe keeps it.
const CENTRED = "translateX(-50%)";

// From the box the bar had before a content swap to the one it has now. The
// first appearance rises into place instead.
function morph(from) {
  const to = bar.getBoundingClientRect();
  if (from && from.width > 0) {
    animate(
      bar,
      [
        { width: `${from.width}px`, height: `${from.height}px`, overflow: "hidden" },
        { width: `${to.width}px`, height: `${to.height}px`, overflow: "hidden" },
      ],
      380,
    );
  } else {
    animate(
      bar,
      [
        { opacity: 0, transform: `${CENTRED} translateY(10px) scale(.97)` },
        { opacity: 1, transform: `${CENTRED} translateY(0) scale(1)` },
      ],
      320,
    );
  }
}

function boxOf() {
  return bar.hidden ? null : bar.getBoundingClientRect();
}

function showCard(from) {
  draft.hidden = true;
  card.hidden = false;
  bar.hidden = false;
  morph(from);
  animate(card, [{ opacity: 0, transform: "translateY(5px)" }, { opacity: 1, transform: "translateY(0)" }], 320);
}

function paintMeter(percent) {
  const meter = el("act-meter");
  if (percent == null) {
    meter.removeAttribute("aria-valuenow");
    el("act-percent").textContent = "";
    el("act-fill").style.transform = "";
    return;
  }
  const value = Math.max(0, Math.min(100, Math.round(percent)));
  meter.setAttribute("aria-valuenow", String(value));
  el("act-percent").textContent = `${value}%`;
  el("act-fill").style.transform = `scaleX(${value / 100})`;
}

function paintText(id, text) {
  if (text == null) return;
  const node = el(id);
  if (node.textContent !== text) node.textContent = text;
}

function flush() {
  frame = null;
  if (!latest) return;
  const { title, detail, percent, stoppable } = latest;
  latest = null;
  paintText("act-title", title);
  if (title != null) el("act-meter").setAttribute("aria-label", title);
  paintText("act-detail", detail);
  if (percent !== undefined) paintMeter(percent);
  if (stoppable === false) el("act-stop").disabled = true;
}

// Progress events can arrive far faster than the screen redraws: keep the
// newest and paint it once per frame.
function queue(update) {
  latest = { ...(latest || {}), ...update };
  if (frame === null) frame = requestAnimationFrame(flush);
}

// The card goes; the pill comes back if a plan is still staged.
function close() {
  if (state === "idle") return;
  const from = boxOf();
  clearTimers();
  cancelAnimations();
  state = "idle";
  owner = null;
  foregroundOwner = false;
  onStop = null;
  delete bar.dataset.activity;
  if (draftWanted) {
    card.hidden = true;
    draft.hidden = false;
    bar.hidden = false;
    morph(from);
    animate(draft, [{ opacity: 0 }, { opacity: 1 }], 260);
    return;
  }
  animate(
    bar,
    [
      { opacity: 1, transform: `${CENTRED} translateY(0)` },
      { opacity: 0, transform: `${CENTRED} translateY(8px)` },
    ],
    LEAVE_MS,
  );
  const leave = () => {
    leaveTimer = null;
    if (state !== "idle") return;
    bar.hidden = !draftWanted;
    card.hidden = true;
    draft.hidden = false;
  };
  if (reduced()) leave();
  else leaveTimer = setTimeout(leave, LEAVE_MS);
}

const INERT = Object.freeze({ progress() {}, stopping() {}, finish() {} });

/// Starts an operation on the card and returns its handle. `foreground` is an
/// apply, undo or redo: it takes the card over from anything else, while a
/// background run (BPM, duplicate scan) started during one stays off the card.
/// A handle whose operation was taken over does nothing, so a late event can
/// never touch a newer operation.
export function beginActivity({ title, detail = "", stop = null, foreground = false }) {
  const busy = state === "running" || state === "stopping";
  if (!foreground && busy && foregroundOwner) return INERT;
  const from = boxOf();
  clearTimers();
  cancelAnimations();
  const me = Symbol("activity");
  owner = me;
  foregroundOwner = foreground;
  onStop = stop;
  state = "running";
  bar.dataset.activity = state;
  el("act-title").textContent = title;
  el("act-meter").setAttribute("aria-label", title);
  el("act-detail").textContent = detail;
  paintMeter(null);
  el("act-stop").hidden = !stop;
  el("act-stop").disabled = false;
  el("act-dismiss").hidden = true;
  showCard(from);
  return {
    /// { title?, detail?, percent? (null = not known yet), stoppable? }
    progress(update) {
      if (owner === me && state === "running") queue(update);
    },
    stopping(stopTitle, stopDetail = "") {
      if (owner !== me) return;
      state = "stopping";
      bar.dataset.activity = state;
      latest = null;
      el("act-stop").disabled = true;
      el("act-title").textContent = stopTitle;
      el("act-detail").textContent = stopDetail;
    },
    /// outcome: "success" | "cancelled" | "error" | null (just close).
    finish(outcome = null, outcomeTitle = "", outcomeDetail = "") {
      if (owner !== me) return;
      if (frame !== null) cancelAnimationFrame(frame);
      frame = null;
      latest = null;
      if (!outcome) {
        close();
        return;
      }
      const from = boxOf();
      cancelAnimations();
      state = outcome;
      bar.dataset.activity = state;
      el("act-title").textContent = outcomeTitle;
      el("act-meter").setAttribute("aria-label", outcomeTitle);
      el("act-detail").textContent = outcomeDetail;
      el("act-stop").hidden = true;
      el("act-dismiss").hidden = outcome !== "error";
      paintMeter(outcome === "success" ? 100 : null);
      morph(from);
      const mark = card.querySelector(outcome === "success" ? ".act-check" : ".act-alert");
      animate(mark, [{ opacity: 0, transform: "scale(.6)" }, { opacity: 1, transform: "scale(1)" }], 300);
      animate(el("act-title"), [{ opacity: 0.35, transform: "translateY(3px)" }, { opacity: 1, transform: "translateY(0)" }], 280);
      // An error waits for the user; anything else holds, then goes.
      if (outcome !== "error") holdTimer = setTimeout(close, HOLD_MS);
    },
  };
}

/// What the staged plan wants: the pill shown or not. While the card is up it
/// only records the wish, honoured when the card goes.
export function setDraftVisible(on) {
  draftWanted = on;
  if (state === "idle") {
    clearTimeout(leaveTimer);
    leaveTimer = null;
    cancelAnimations();
    bar.hidden = !on;
    card.hidden = true;
    draft.hidden = false;
  }
}

el("act-stop").addEventListener("click", () => {
  if (state === "running" && onStop) onStop();
});
el("act-dismiss").addEventListener("click", close);
