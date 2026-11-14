import '../row-selection.css';

// The rectangle is only a view. Model-row intersections and the shared Set
// remain owned by app.js, including paths outside the virtual DOM window.
export function initRowSelection(options) {
  const { wrap } = options;
  const currentSelection = () => options.getSelection?.() || options.selection;
  let gesture = null;
  let swallowClick = false;
  let swallowDoubleClick = false;
  let autoFrame = 0;
  const rectangle = document.createElement('div');
  rectangle.className = 'row-selection-rectangle';
  rectangle.hidden = true;
  rectangle.setAttribute('aria-hidden', 'true');
  document.body.append(rectangle);

  function viewport() {
    const rect = wrap.getBoundingClientRect();
    const header = wrap.querySelector('thead')?.getBoundingClientRect();
    return {
      left: rect.left + wrap.clientLeft,
      top: Math.max(rect.top + wrap.clientTop, header?.bottom || rect.top),
      right: rect.left + wrap.clientLeft + wrap.clientWidth,
      bottom: rect.top + wrap.clientTop + wrap.clientHeight,
      origin: rect,
    };
  }
  const clamp = (value, min, max) => Math.min(max, Math.max(min, value));

  function paint() {
    if (!gesture?.active) return;
    const box = viewport();
    const x = clamp(gesture.x, box.left, box.right);
    const y = clamp(gesture.y, box.top, box.bottom);
    const firstX = gesture.startX - wrap.scrollLeft + box.origin.left;
    const firstY = gesture.startY - wrap.scrollTop + box.origin.top;
    const left = clamp(Math.min(firstX, x), box.left, box.right);
    const top = clamp(Math.min(firstY, y), box.top, box.bottom);
    const right = clamp(Math.max(firstX, x), box.left, box.right);
    const bottom = clamp(Math.max(firstY, y), box.top, box.bottom);
    Object.assign(rectangle.style, {
      left: `${left}px`, top: `${top}px`, width: `${right - left}px`, height: `${bottom - top}px`,
    });
    const endY = y - box.origin.top + wrap.scrollTop;
    const paths = options.pathsInRange(Math.min(gesture.startY, endY), Math.max(gesture.startY, endY));
    const selection = currentSelection();
    const range = new Set(paths);
    gesture.paths = paths;
    gesture.downward = endY >= gesture.startY;
    // Reconcile membership rather than clearing/re-adding the entire captured
    // base on every pointer move. Unchanged members retain their revision.
    let changed = false;
    for (const path of selection) {
      if (!range.has(path) && !(gesture.additive && gesture.base.has(path))) { selection.delete(path); changed = true; }
    }
    if (gesture.additive) for (const path of gesture.base) {
      if (!selection.has(path)) { selection.add(path); changed = true; }
    }
    for (const path of range) if (!selection.has(path)) { selection.add(path); changed = true; }
    if (changed) options.onChange();
  }

  function autoScroll() {
    autoFrame = 0;
    if (!gesture?.active) return;
    const box = viewport();
    const edge = 28;
    const speed = gesture.y < box.top + edge ? -Math.min(16, (box.top + edge - gesture.y) / 2)
      : gesture.y > box.bottom - edge ? Math.min(16, (gesture.y - box.bottom + edge) / 2) : 0;
    if (!speed) return;
    const before = wrap.scrollTop;
    wrap.scrollTop += speed;
    if (wrap.scrollTop === before) return;
    options.onScroll?.();
    paint();
    autoFrame = requestAnimationFrame(autoScroll);
  }

  function moved(event) {
    if (!gesture || !gesture.dragAllowed) return;
    if (!options.enabled()) { finish(true); return; }
    gesture.x = event.clientX;
    gesture.y = event.clientY;
    if (!gesture.active) {
      if (Math.hypot(event.clientX - gesture.clientX, event.clientY - gesture.clientY) < 4) return;
      gesture.active = true;
      options.onStart?.();
      rectangle.hidden = false;
      document.body.classList.add('selecting-rows');
      window.getSelection()?.removeAllRanges();
    }
    event.preventDefault();
    paint();
    if (!autoFrame) autoFrame = requestAnimationFrame(autoScroll);
  }

  function finish(cancelled = false) {
    const ended = gesture;
    if (!ended) return;
    gesture = null;
    cancelAnimationFrame(autoFrame);
    autoFrame = 0;
    rectangle.hidden = true;
    document.body.classList.remove('selecting-rows');
    document.removeEventListener('mousemove', moved);
    document.removeEventListener('mouseup', released);
    document.removeEventListener('keydown', keydown, true);
    window.removeEventListener('blur', blurred);
    if (!ended.active) {
      options.onCommit({ cancelled, click: true, paths: [], startPath: ended.startPath });
      return;
    }
    swallowClick = true;
    swallowDoubleClick = true;
    if (cancelled) {
      const selection = currentSelection();
      selection.clear();
      for (const path of ended.base) selection.add(path);
    }
    options.onCommit({
      cancelled, beforePress: ended.beforePress, paths: ended.paths, startPath: ended.startPath,
      focusPath: ended.downward ? ended.paths.at(-1) : ended.paths[0],
    });
  }
  function released() { finish(); }
  function blurred() { finish(true); }
  function keydown(event) {
    if (event.key !== 'Escape' || !gesture?.active) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    finish(true);
  }
  function pressed(event) {
    if (event.button !== 0) return;
    swallowClick = false;
    swallowDoubleClick = false;
    if (!options.enabled()) return;
    if (event.target.closest('thead, td.sel, button, input, textarea, select, a, [contenteditable="true"], .col-resize, tr.group-head, tr.unreadable')) return;
    const box = viewport();
    // A scrollbar drag remains a scrollbar drag, including overlay scrollbars.
    if (event.clientX < box.left || event.clientX >= box.right || event.clientY < box.top || event.clientY >= box.bottom) return;
    // Rows are selection surfaces; text/native HTML dragging belongs only to
    // an explicitly editing cell. Cancel it before the browser starts a drag.
    event.preventDefault();
    const base = new Set(currentSelection());
    // Selection feedback belongs to the press, not a later click/release.
    // Capture the original set first so Escape and additive drags remain exact.
    const beforePress = event.detail < 2 ? options.onPress?.(event) : null;
    gesture = {
      clientX: event.clientX, clientY: event.clientY, x: event.clientX, y: event.clientY,
      startX: event.clientX - box.origin.left + wrap.scrollLeft,
      startY: event.clientY - box.origin.top + wrap.scrollTop,
      startPath: event.target.closest('tr[data-path]')?.dataset.path || null,
      base, beforePress, additive: event.ctrlKey || event.metaKey,
      active: false, paths: [], downward: true, dragAllowed: !event.shiftKey,
    };
    document.addEventListener('mousemove', moved, { passive: false });
    document.addEventListener('mouseup', released);
    document.addEventListener('keydown', keydown, true);
    window.addEventListener('blur', blurred);
  }
  function clicked(event) {
    if (!swallowClick) return;
    swallowClick = false;
    event.preventDefault();
    event.stopImmediatePropagation();
  }
  function doubleClicked(event) {
    if (!swallowDoubleClick) return;
    swallowDoubleClick = false;
    event.preventDefault();
    event.stopImmediatePropagation();
  }
  function scrolled() { paint(); }
  wrap.addEventListener('mousedown', pressed);
  wrap.addEventListener('click', clicked, true);
  wrap.addEventListener('dblclick', doubleClicked, true);
  wrap.addEventListener('scroll', scrolled, { passive: true });
  const destroy = () => {
    finish(true);
    wrap.removeEventListener('mousedown', pressed);
    wrap.removeEventListener('click', clicked, true);
    wrap.removeEventListener('dblclick', doubleClicked, true);
    wrap.removeEventListener('scroll', scrolled);
    rectangle.remove();
  };
  window.addEventListener('pagehide', destroy, { once: true });
  return { destroy };
}
