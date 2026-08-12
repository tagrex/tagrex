// Small DOM helpers shared by every part of the frontend (#143 split them out
// of app.js). Nothing here knows about the library, the selection or any
// command — they take arguments and touch the document, so any module can use
// them without pulling app state along.

export const el = (id) => document.getElementById(id);

/// A transient status message at the bottom of the window; `isError` tints it.
export function toast(message, isError) {
  const t = el("toast");
  t.textContent = message;
  t.classList.toggle("error", !!isError);
  t.hidden = false;
  clearTimeout(toast._timer);
  toast._timer = setTimeout(() => (t.hidden = true), 3200);
}

export function fileName(path) {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1];
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

// Place a floating box against the button that opened it (#160).
//
// A CSS-positioned popover is always below its anchor and always inside the
// scrolling panel that holds it — which is why the placeholder reference and
// the transform popover were already placed from JS. This is the third one, so
// it is a helper: `position: fixed` (out of any clipping ancestor), clamped
// horizontally to the window, and flipped ABOVE the anchor when there is not
// enough room below. Without the flip a button near the bottom of the window —
// a pinned panel footer, a popover anchored on the floating diff bar — opens its
// menu off the bottom edge, where most of it cannot be reached.
//
// `width` is for boxes that set their own; leave it out to keep the box's
// natural width, in which case it must already be visible so it can be measured.
export function placeFloating(box, anchor, { align = "left", gap = 4, width = null } = {}) {
  const rect = anchor.getBoundingClientRect();
  if (width) box.style.width = `${width}px`;
  const boxWidth = width || box.offsetWidth;
  const left = align === "right" ? rect.right - boxWidth : rect.left;
  box.style.left = `${Math.min(Math.max(8, left), Math.max(8, window.innerWidth - boxWidth - 8))}px`;

  const below = window.innerHeight - rect.bottom - gap - 8;
  const above = rect.top - gap - 8;
  // Below by preference — that is where a menu is expected — unless there is
  // real room above and little below.
  if (below >= 240 || below >= above) {
    box.style.top = `${rect.bottom + gap}px`;
    box.style.bottom = "auto";
    box.style.maxHeight = `${Math.max(0, below)}px`;
  } else {
    box.style.top = "auto";
    box.style.bottom = `${window.innerHeight - rect.top + gap}px`;
    box.style.maxHeight = `${Math.max(0, above)}px`;
  }
}

// A minimal in-app confirm dialog returning a Promise<boolean> (#102). Used for
// the overwrite prompt; WKWebView's window.confirm is unreliable, so this is a
// self-managed modal. Backdrop click or Cancel resolves false.
export function confirmDialog(message, okLabel = "OK") {
  return new Promise((resolve) => {
    const modal = el("confirm-modal");
    el("confirm-message").textContent = message;
    const okBtn = el("confirm-ok");
    const cancelBtn = el("confirm-cancel");
    okBtn.textContent = okLabel;
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
    okBtn.focus();
  });
}
