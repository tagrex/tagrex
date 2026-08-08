// Pointer-based drag reorder for a vertical list (#88, #143 split it out of
// app.js). Shared by the column picker, the transform rule chain and the
// tag-read priority list — three lists that all needed the same gesture.
// Pointer-based drag reorder for a vertical list, keyed by each item's
// `data-key`. WKWebView's HTML5 drag-and-drop is unreliable (dynamically set
// `draggable` often never starts a drag), which is why the file-table reorder
// and this helper both use mouse events. `onReorder(dragged, target, below)`
// receives the dragged key, the key it was dropped onto, and whether it landed
// in that row's lower half.
function enablePointerReorder(grip, item, container, itemSelector, onReorder) {
  grip.addEventListener("mousedown", (e) => {
    e.preventDefault(); // don't start a text selection
    const draggedKey = item.dataset.key;
    item.classList.add("dragging");
    let targetKey = null;
    let below = false;
    const clearMarks = () =>
      container
        .querySelectorAll(itemSelector)
        .forEach((it) => it.classList.remove("drop-above", "drop-below"));
    const onMove = (ev) => {
      clearMarks();
      targetKey = null;
      const under = document.elementFromPoint(ev.clientX, ev.clientY);
      const row = under && under.closest(itemSelector);
      if (!row || row === item || !container.contains(row)) return;
      const rect = row.getBoundingClientRect();
      below = ev.clientY > rect.top + rect.height / 2;
      row.classList.add(below ? "drop-below" : "drop-above");
      targetKey = row.dataset.key;
    };
    const onUp = () => {
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
      clearMarks();
      item.classList.remove("dragging");
      if (targetKey !== null && targetKey !== draggedKey) {
        onReorder(draggedKey, targetKey, below);
      }
    };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  });
}

export { enablePointerReorder };
