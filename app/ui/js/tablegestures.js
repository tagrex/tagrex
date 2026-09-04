// Table gestures done by hand (#76, #143 split them out of app.js).
//
// The panel splitter and the column resize both drag with plain mouse events
// rather than a native control, for the same reason the row reorder does:
// WKWebView, the macOS Tauri webview, doesn't drive HTML5 drag reliably. Each
// is a self-contained IIFE that wires itself to the DOM it owns.
import { el } from "./dom.js";
import { columnWidths, setColumnWidths } from "./state.js";
import { COLUMN_MIN_WIDTH, fitColumn, saveColumnWidths } from "./columns.js";

// ---- resize the table / mode-panel split by dragging the divider ----
// Mouse events (not a native splitter) for the same WKWebView reason as the row
// reorder. The panel has a fixed flex-basis; dragging sets it in pixels.
(function initSplitter() {
  const splitter = el("col-splitter");
  const modeCol = document.querySelector(".mode-col");
  const workarea = document.querySelector(".workarea");
  let dragging = false;
  // The width the last drag settled on. A pure display choice → localStorage, on
  // the same footing as the table-font control (#282), so a chosen split survives
  // a reload instead of snapping back to the 480px default.
  const PANEL_WIDTH_KEY = "tagrex.panelWidthPx";
  let lastWidth = 0;

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
    lastWidth = Math.round(width);
    modeCol.style.flexBasis = `${lastWidth}px`;
  }

  function onUp() {
    dragging = false;
    document.body.classList.remove("resizing");
    document.removeEventListener("mousemove", onMove);
    document.removeEventListener("mouseup", onUp);
    // Persist what the drag settled on (only if it actually moved).
    if (lastWidth) {
      try {
        localStorage.setItem(PANEL_WIDTH_KEY, String(lastWidth));
      } catch (e) {
        /* localStorage unavailable — the width just won't persist */
      }
    }
  }

  // Restore a previously dragged width. The clamp below still narrows it if the
  // current window is too small for it; the stored value is left intact so a
  // wider window later gets the full width back.
  function restorePanelWidth() {
    let v;
    try {
      v = parseInt(localStorage.getItem(PANEL_WIDTH_KEY), 10);
    } catch (e) {
      return;
    }
    if (Number.isFinite(v) && v >= 240) modeCol.style.flexBasis = `${v}px`;
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
  restorePanelWidth(); // a remembered width first (#282), then clamp it to fit
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

  // Double-click a grip to fit that column to its content (#151) — the gesture
  // every table with draggable columns uses for it. It replaced a reset to the
  // default width: that is the rarer want by a distance, and the columns menu
  // still offers "Reset to default" for the whole set.
  thead.addEventListener("dblclick", (e) => {
    const grip = e.target.closest(".col-resize");
    if (!grip) return;
    e.preventDefault();
    e.stopPropagation();
    fitColumn(grip.dataset.key);
  });
})();
