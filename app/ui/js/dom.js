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
}

function showToast(title, parts, isError) {
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
  const stays = parts.some((part) => part.problem);
  el("toast-close").hidden = !stays && !isError;
  box.classList.toggle("error", isError);
  box.hidden = false;
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
