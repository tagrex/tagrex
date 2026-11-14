import { createComponentElement } from "../lib/component-elements.js";
import { positionControllerToast } from "../lib/controller-surfaces.js";
// Small DOM helpers shared by every part of the frontend (#143 split them out
// of app.js). Nothing here knows about the library, the selection or any
// command — they take arguments and touch the document, so any module can use
// them without pulling app state along.

export const el = (id) => document.getElementById(id);

/// A transient status message at the bottom of the window; `isError` tints it.
export function toast(message, isError) {
  showToast(String(message), [], !!isError);
}

/// The outcome of a run with several results (#558): `title` is what was
/// staged or done, and each part ({ text, problem }) gets its own line, muted
/// when it is fine and red when it needs a look. A report with a problem in it
/// stays until it is dismissed; one that is all fine times out like a toast.
export function report(title, parts) {
  showToast(String(title), parts.filter(Boolean), false);
  // A report shown with a plan staged is about that plan (#562): it closes
  // when the plan is applied or discarded.
  reportBound = barShown();
}

// Whether the toast on screen is a report tied to the staged plan.
let reportBound = false;

function barShown() {
  const bar = el("diff-actionbar");
  return !!bar && !bar.hidden;
}

// With a plan staged, the toast sits on the change bar's axis just above it,
// and a report takes the bar's width so it reads as part of it (#562). The
// bar is centred on the table, not the window, so CSS alone can't line them up.
function placeToast() {
  const box = el("toast");
  positionControllerToast(!box.hidden && barShown() ? el("diff-actionbar").getBoundingClientRect() : null);
}
window.addEventListener("resize", placeToast);

/// Closes a report tied to the staged plan; for the caller letting the plan go.
export function closePlanReport() {
  if (!reportBound) return;
  reportBound = false;
  clearTimeout(toast._timer);
  el("toast").hidden = true;
}

/// Closes a toast that waits to be dismissed (a report with a problem, an
/// error). True when there was one: Esc closes it before doing anything else.
export function dismissStickyToast() {
  if (el("toast").hidden || el("toast-close").hidden) return false;
  clearTimeout(toast._timer);
  el("toast").hidden = true;
  reportBound = false;
  return true;
}

function showToast(title, parts, isError) {
  reportBound = false;
  const box = el("toast");
  el("toast-title").textContent = title;
  const list = el("toast-parts");
  list.innerHTML = "";
  for (const part of parts) {
    const item = document.createElement("li");
    item.className = part.problem ? "toast-part problem" : "toast-part";
    item.textContent = part.text;
    list.appendChild(item);
  }
  list.hidden = parts.length === 0;
  // A report with a problem waits to be read. An error is often only "select
  // something first", so it times out too, given longer and a × to close it.
  // A `keep` part waits as well without reading as a problem: an outcome the
  // user chose that is still worth reading in full (#683).
  const stays = parts.some((part) => part.problem || part.keep);
  el("toast-close").hidden = !stays && !isError;
  box.classList.toggle("error", isError);
  box.hidden = false;
  placeToast();
  clearTimeout(toast._timer);
  if (!stays) toast._timer = setTimeout(() => (box.hidden = true), isError ? 6000 : 3200);
}

el("toast-close").addEventListener("click", () => {
  clearTimeout(toast._timer);
  el("toast").hidden = true;
});

// "1 track" / "2 tracks" (#167). A count and its noun, agreeing — writing the
// noun inline is what produced "1 tracks" on every single-track release.
//
// Nothing a user reads goes through this any more: it offers two forms and no
// way to ask for a third, which is why a Russian heading read "3 file" (#50).
// Interface text uses `tn` from i18n.js. What is left here is the browser mock,
// which stands in for the *backend* — the plan descriptions it fakes are
// composed in Rust and are English there too, until #268 changes that.
export function plural(count, one, many) {
  return `${count} ${count === 1 ? one : many}`;
}

export function fileName(path) {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1];
}

// The last segment of a folder path, whether or not it ends in a separator —
// `fileName` alone answers "" for `/music/album/` (#267). A path with no
// segment to take, the filesystem root, keeps what it had: a message naming
// nothing at all is worse than one naming a slash.
export function folderName(path) {
  const trimmed = String(path).replace(/[\\/]+$/, "");
  return fileName(trimmed) || String(path);
}

export function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    '"': "&quot;",
    "'": "&#39;",
  })[c]);
}

// Inline-SVG icon markup from the shared sprite (#115): the JS counterpart of
// `<svg class="ico"><use href="#i-name"/></svg>` in index.html, for glyphs built
// dynamically (carets, grips, sort indicators, player transport). Kept in sync
// with the sprite symbol ids.
export function ico(name) {
  return `<svg class="ico"><use href="#i-${name}"/></svg>`;
}

// Shared field/action and context positioning contracts.
export { placeFloating, placeContextMenu } from "./popup-position.js";

// A minimal in-app confirm dialog returning a Promise<boolean> (#102). Used for
// the overwrite prompt; WKWebView's window.confirm is unreliable, so this is a
// self-managed modal. Backdrop click or Cancel resolves false.
// `detail` is an optional muted line saying what the act touches. A `danger`
// act (one that destroys something) gets the danger button and leaves focus on
// Cancel, so Return never confirms a deletion by default (#560).
export function confirmDialog(message, okLabel = "OK", { detail = "", danger = false } = {}) {
  return new Promise((resolve) => {
    const modal = el("confirm-modal");
    el("confirm-message").textContent = message;
    el("confirm-detail").textContent = detail;
    el("confirm-detail").hidden = !detail;
    const okBtn = el("confirm-ok");
    const cancelBtn = el("confirm-cancel");
    okBtn.textContent = okLabel;
    okBtn.classList.toggle("danger", danger);
    modal.hidden = false;
    const done = (result) => {
      modal.hidden = true;
      okBtn.removeEventListener("click", onOk);
      cancelBtn.removeEventListener("click", onCancel);
      modal.removeEventListener("click", onBackdrop);
      resolve(result);
    };
    const onOk = () => done(true);
    const onCancel = () => done(false);
    const onBackdrop = (e) => {
      if (e.target === modal) done(false);
    };
    okBtn.addEventListener("click", onOk);
    cancelBtn.addEventListener("click", onCancel);
    modal.addEventListener("click", onBackdrop);
    (danger ? cancelBtn : okBtn).focus();
  });
}

// The confirm modal with two actions instead of one, plus the cancel button:
// resolves to "primary", "secondary" or null (cancelled). The primary action is
// the safe default and holds the focus, so Return takes it (#680); the
// secondary one gets the danger style, since it is the one that overwrites
// something, and its button is made here and taken away after.
//
// The answers stack full width (#683), top to bottom in the order they are
// meant to be read: primary, secondary, cancel. The buttons are moved into
// that order rather than reordered by CSS, so Tab walks them as they are seen,
// and put back afterwards. `subject` names what the question is about (a file)
// on a line of its own, in the value font.
export function choiceDialog(message, { subject = "", detail = "", primary, secondary, cancel }) {
  return new Promise((resolve) => {
    const modal = el("confirm-modal");
    const box = modal.querySelector(".modal");
    el("confirm-message").textContent = message;
    el("confirm-detail").textContent = detail;
    el("confirm-detail").hidden = !detail;
    const subjectLine = document.createElement("p");
    subjectLine.className = "modal-subject mono";
    subjectLine.textContent = subject;
    if (subject) el("confirm-message").after(subjectLine);
    const okBtn = el("confirm-ok");
    const cancelBtn = el("confirm-cancel");
    const actions = okBtn.parentElement;
    const okText = okBtn.textContent;
    const cancelText = cancelBtn.textContent;
    const altBtn = createComponentElement("button", { variant: "destructive" });
    altBtn.className = "danger";
    altBtn.textContent = secondary;
    actions.prepend(okBtn);
    okBtn.after(altBtn);
    okBtn.textContent = primary;
    okBtn.classList.remove("danger");
    if (cancel) cancelBtn.textContent = cancel;
    box.classList.add("choice");
    modal.hidden = false;
    const done = (result) => {
      modal.hidden = true;
      box.classList.remove("choice");
      subjectLine.remove();
      altBtn.remove();
      actions.append(okBtn);
      okBtn.textContent = okText;
      cancelBtn.textContent = cancelText;
      okBtn.removeEventListener("click", onOk);
      cancelBtn.removeEventListener("click", onCancel);
      modal.removeEventListener("click", onBackdrop);
      resolve(result);
    };
    const onOk = () => done("primary");
    const onAlt = () => done("secondary");
    const onCancel = () => done(null);
    const onBackdrop = (e) => {
      if (e.target === modal) done(null);
    };
    okBtn.addEventListener("click", onOk);
    altBtn.addEventListener("click", onAlt);
    cancelBtn.addEventListener("click", onCancel);
    modal.addEventListener("click", onBackdrop);
    okBtn.focus();
  });
}
