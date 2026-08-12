// Table gestures done by hand (#76, #143 split them out of app.js).
//
// The panel splitter and the column resize both drag with plain mouse events
// rather than a native control, for the same reason the row reorder does:
// WKWebView, the macOS Tauri webview, doesn't drive HTML5 drag reliably. Each
// is a self-contained IIFE that wires itself to the DOM it owns.
import { el } from "./dom.js";
import { columnWidths, setColumnWidths } from "./state.js";
import { COLUMN_MIN_WIDTH, renderTableHead, saveColumnWidths } from "./columns.js";

// ---- resize the table / mode-panel split by dragging the divider ----
// Mouse events (not a native splitter) for the same WKWebView reason as the row
// reorder. The panel has a fixed flex-basis; dragging sets it in pixels.
(function initSplitter() {
  const splitter = el("col-splitter");
  const modeCol = document.querySelector(".mode-col");
  const workarea = document.querySelector(".workarea");
  let dragging = false;

  splitter.addEventListener("mousedown", (e) => {
    e.preventDefault();
    dragging = true;
    document.body.classList.add("resizing");
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  });

  function onMove(e) {
    if (!dragging) return;
    // Panel width = distance from the cursor to the right edge of the work area,
    // clamped so neither column collapses.
    const rect = workarea.getBoundingClientRect();
    const width = Math.min(Math.max(rect.right - e.clientX, 240), rect.width - 360);
    modeCol.style.flexBasis = `${Math.round(width)}px`;
  }

  function onUp() {
    dragging = false;
    document.body.classList.remove("resizing");
    document.removeEventListener("mousemove", onMove);
    document.removeEventListener("mouseup", onUp);
  }

  // Keep the panel within the work area when the window shrinks (#109). The
  // splitter enforces this on drag, but without a resize clamp a panel that was
  // wide (or the default 480 on a narrow window) is pushed off the right edge,
  // clipping its toolbar. Mirror the splitter's clamp; only ever narrow.
  function clampPanel() {
    if (document.body.classList.contains("panel-collapsed")) return;
    const rect = workarea.getBoundingClientRect();
    if (rect.width === 0) return;
    const max = Math.max(240, rect.width - 360);
    if (modeCol.getBoundingClientRect().width > max) {
      modeCol.style.flexBasis = `${Math.round(max)}px`;
    }
  }
  window.addEventListener("resize", clampPanel);
  clampPanel(); // in case the initial window is narrower than the default panel
})();

// ---- resize a table column by dragging its header grip (#76) ----
// Delegated on the header (mousedown), because the sortable ths are rebuilt on
// every column change (#43). Dragging past a threshold suppresses the header's
// sort click. Same manual-mouse approach as the panel splitter (WKWebView).
(function initColumnResize() {
  const thead = el("tracks").querySelector("thead");
  let key = null;
  let startX = 0;
  let startWidth = 0;
  let moved = false;
  let th = null;

  thead.addEventListener("mousedown", (e) => {
    const grip = e.target.closest(".col-resize");
    if (!grip) return;
    e.preventDefault();
    e.stopPropagation(); // don't let the header treat this as a sort click
    key = grip.dataset.key;
    th = grip.closest("th");
    startX = e.clientX;
    startWidth = th.getBoundingClientRect().width;
    moved = false;
    document.body.classList.add("resizing-col");
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  });

  function onMove(e) {
    if (key === null) return;
    const width = Math.max(startWidth + (e.clientX - startX), COLUMN_MIN_WIDTH);
    if (Math.abs(e.clientX - startX) > 2) moved = true;
    columnWidths[key] = Math.round(width);
    if (th) th.style.width = `${columnWidths[key]}px`;
  }

  function onUp() {
    if (key === null) return;
    // A grip drag that never moved is a stray click — don't persist or block sort.
    if (moved) saveColumnWidths();
    key = null;
    th = null;
    document.body.classList.remove("resizing-col");
    document.removeEventListener("mousemove", onMove);
    document.removeEventListener("mouseup", onUp);
  }

  // Double-click a grip to reset that column to its default width.
  thead.addEventListener("dblclick", (e) => {
    const grip = e.target.closest(".col-resize");
    if (!grip) return;
    e.preventDefault();
    e.stopPropagation();
    delete columnWidths[grip.dataset.key];
    saveColumnWidths();
    renderTableHead();
  });
})();
