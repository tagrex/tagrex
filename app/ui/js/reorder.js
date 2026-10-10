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

// Auto-scroll while dragging (#419): a list taller than its box — the column
// picker once enough columns are listed — scrolls when the pointer is held
// within EDGE pixels of the box's leading or trailing edge, up to MAX_SPEED
// pixels a frame at (or past) the edge itself. Without it, moving a row above
// the visible ones meant scrolling first and starting the drag again.
const EDGE = 32;
const MAX_SPEED = 14;

// The nearest box, the container itself included, that scrolls along the drag
// axis and has something to scroll. None when the whole list is on screen.
function scrollerFor(node, horizontal) {
  for (let n = node; n && n !== document.body && n !== document.documentElement; n = n.parentElement) {
    const style = getComputedStyle(n);
    const overflow = horizontal ? style.overflowX : style.overflowY;
    const room = horizontal ? n.scrollWidth > n.clientWidth : n.scrollHeight > n.clientHeight;
    if (room && /auto|scroll/.test(overflow)) return n;
  }
  return null;
}

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
    const scroller = scrollerFor(container, horizontal);
    // The last pointer position, so a scroll that moves rows under a pointer
    // that hasn't moved can re-pick the drop target.
    let last = null;
    let frame = 0;
    const autoScroll = () => {
      frame = 0;
      if (!dragging || !scroller || !last) return;
      const rect = scroller.getBoundingClientRect();
      const pos = horizontal ? last.clientX : last.clientY;
      const lo = (horizontal ? rect.left : rect.top) + EDGE;
      const hi = (horizontal ? rect.right : rect.bottom) - EDGE;
      let delta = 0;
      if (pos < lo) delta = -Math.ceil(MAX_SPEED * Math.min(1, (lo - pos) / EDGE));
      else if (pos > hi) delta = Math.ceil(MAX_SPEED * Math.min(1, (pos - hi) / EDGE));
      if (!delta) return;
      const before = horizontal ? scroller.scrollLeft : scroller.scrollTop;
      if (horizontal) scroller.scrollLeft = before + delta;
      else scroller.scrollTop = before + delta;
      // Already at the end: stop until the pointer moves again.
      if ((horizontal ? scroller.scrollLeft : scroller.scrollTop) === before) return;
      frame = requestAnimationFrame(autoScroll);
    };
    // Rows moved under the pointer — by the auto-scroll above or by the wheel
    // mid-drag — so what it points at changed too.
    const onScroll = () => {
      if (dragging && last) markTarget(last);
    };
    const onMove = (ev) => {
      const now = horizontal ? ev.clientX : ev.clientY;
      if (!dragging) {
        if (Math.abs(now - start) < THRESHOLD) return;
        dragging = true;
        item.classList.add("dragging");
      }
      last = ev;
      markTarget(ev);
      if (!frame) frame = requestAnimationFrame(autoScroll);
    };
    const markTarget = (ev) => {
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
    // Whether the pointer is over the list: inside the container and, when the
    // list scrolls, inside the part of it on screen.
    const insideList = (ev) => {
      const boxes = [container.getBoundingClientRect()];
      if (scroller) boxes.push(scroller.getBoundingClientRect());
      return boxes.every(
        (r) => ev.clientX >= r.left && ev.clientX <= r.right && ev.clientY >= r.top && ev.clientY <= r.bottom
      );
    };
    const onUp = (ev) => {
      // Let go outside the list and the drag is abandoned (#420), whatever
      // marker the last move left — a release can arrive with no move at its
      // own position (a quick flick off the list, or leaving the window).
      if (!insideList(ev)) targetKey = null;
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
      if (scroller) scroller.removeEventListener("scroll", onScroll);
      cancelAnimationFrame(frame);
      frame = 0;
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
    if (scroller) scroller.addEventListener("scroll", onScroll);
  });
}

export { enablePointerReorder };
