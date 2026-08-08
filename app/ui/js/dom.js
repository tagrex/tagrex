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
