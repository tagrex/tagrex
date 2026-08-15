// Pointer-based drag reorder for a vertical list (#88, #143 split it out of
// app.js). Shared by the column picker, the transform rule chain and the
// tag-read priority list — three lists that all needed the same gesture.
// Pointer-based drag reorder for a vertical list, keyed by each item's
// `data-key`. WKWebView's HTML5 drag-and-drop is unreliable (dynamically set
// `draggable` often never starts a drag), which is why the file-table reorder
// and this helper both use mouse events. `onReorder(dragged, target, below)`
// receives the dragged key, the key it was dropped onto, and whether it landed
// in that row's lower half.
// `axis: "x"` reorders along a row instead of down a list (#89) — same gesture,
// same order model, marks on the left/right edge rather than the top/bottom one.
// The last argument of `onReorder` then means "dropped on the right half".
//
// A drag has to travel `THRESHOLD` pixels before it counts as one, so a plain
// click on something that is also a button — a column header sorts on click —
// still reads as a click. And once it does count, the click the browser fires
// after the mouse comes up is swallowed, so a reorder never sorts as well.
const THRESHOLD = 4;

function enablePointerReorder(grip, item, container, itemSelector, onReorder, { axis = "y" } = {}) {
  const horizontal = axis === "x";
  grip.addEventListener("mousedown", (e) => {
    // On a column header the right-edge grip is the resize handle (#76), a
    // different gesture that owns its own drag.
    if (e.target.closest(".col-resize")) return;
    e.preventDefault(); // don't start a text selection
    const draggedKey = item.dataset.key;
    const start = horizontal ? e.clientX : e.clientY;
    let dragging = false;
    let targetKey = null;
    let past = false;
    const clearMarks = () =>
      container
        .querySelectorAll(itemSelector)
        .forEach((it) =>
          it.classList.remove("drop-above", "drop-below", "drop-before", "drop-after")
        );
    const onMove = (ev) => {
      const now = horizontal ? ev.clientX : ev.clientY;
      if (!dragging) {
        if (Math.abs(now - start) < THRESHOLD) return;
        dragging = true;
        item.classList.add("dragging");
      }
      clearMarks();
      targetKey = null;
      const under = document.elementFromPoint(ev.clientX, ev.clientY);
      const row = under && under.closest(itemSelector);
      if (!row || row === item || !container.contains(row)) return;
      const rect = row.getBoundingClientRect();
      past = horizontal
        ? ev.clientX > rect.left + rect.width / 2
        : ev.clientY > rect.top + rect.height / 2;
      if (horizontal) row.classList.add(past ? "drop-after" : "drop-before");
      else row.classList.add(past ? "drop-below" : "drop-above");
      targetKey = row.dataset.key;
    };
    const onUp = () => {
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
      clearMarks();
      item.classList.remove("dragging");
      if (dragging) {
        // Kill the click this mouseup is about to produce, so a header that was
        // dragged doesn't also sort. Disarmed on the next tick rather than by
        // `once`: a drag that ends outside the window produces no click at all,
        // and a listener left armed would eat an unrelated one later.
        const swallow = (ev) => {
          ev.stopPropagation();
          ev.preventDefault();
        };
        document.addEventListener("click", swallow, true);
        setTimeout(() => document.removeEventListener("click", swallow, true), 0);
      }
      if (targetKey !== null && targetKey !== draggedKey) {
        onReorder(draggedKey, targetKey, past);
      }
    };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  });
}

export { enablePointerReorder };
