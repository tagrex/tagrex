// Field/action popups follow their control; context menus follow a point.
// Coordinates are viewport based, including inside translated/scaled dialogs.
const EDGE = 8;
const placements = new Map();
let frame = 0;

function layoutSize(box, axis) {
  if (typeof getComputedStyle === 'undefined') return axis === 'width' ? box.offsetWidth : box.offsetHeight;
  const style = getComputedStyle(box), value = parseFloat(style[axis]);
  if (!Number.isFinite(value)) return axis === 'width' ? box.offsetWidth : box.offsetHeight;
  if (style.boxSizing === 'border-box') return value;
  const sides = axis === 'width' ? ['Left', 'Right'] : ['Top', 'Bottom'];
  return value + sides.reduce((sum, side) => sum + (parseFloat(style['padding' + side]) || 0) + (parseFloat(style['border' + side + 'Width']) || 0), 0);
}
function measure(box, width) {
  box.style.left = '0px'; box.style.top = '0px'; box.style.bottom = 'auto';
  box.style.maxHeight = `${Math.max(0, innerHeight - EDGE * 2)}px`;
  box.style.setProperty?.('--controller-available-height', box.style.maxHeight);
  if (width != null) box.style.width = `${Math.min(width, Math.max(0, innerWidth - EDGE * 2))}px`;
  const origin = box.getBoundingClientRect();
  // offsetWidth/Height round to integers: treating that rounding as a scale
  // creates several pixels of drift for an auto-width menu far from the origin.
  return { origin, sx: origin.width / layoutSize(box, 'width') || 1, sy: origin.height / layoutSize(box, 'height') || 1 };
}
function remember(box, anchor, options) {
  for (const [node] of placements) if (!node.isConnected || node.hidden) placements.delete(node);
  if (anchor?.nodeType === 1) box.returnFocusTo = anchor;
  placements.set(box, { anchor, options });
}
function clamp(value, max) { return Math.max(EDGE, Math.min(value, Math.max(EDGE, max))); }

export function placeFloating(box, anchor, { align = 'left', gap = 4, width = null } = {}) {
  const rect = anchor.getBoundingClientRect();
  const { origin, sx, sy } = measure(box, width);
  const below = Math.max(0, innerHeight - rect.bottom - gap - EDGE);
  const above = Math.max(0, rect.top - gap - EDGE);
  const down = below >= origin.height || below >= above;
  const budget = Math.min(innerHeight - EDGE * 2, down ? below : above);
  box.style.maxHeight = `${Math.max(0, budget / sy)}px`;
  box.style.setProperty?.('--controller-available-height', box.style.maxHeight);
  const size = box.getBoundingClientRect();
  const left = clamp(align === 'right' ? rect.right - size.width : rect.left, innerWidth - size.width - EDGE);
  const top = clamp(down ? rect.bottom + gap : rect.top - size.height - gap, innerHeight - size.height - EDGE);
  box.style.left = `${(left - origin.left) / sx}px`;
  box.style.top = `${(top - origin.top) / sy}px`;
  remember(box, anchor, { align, gap, width });
}

export function placeContextMenu(box, x, y) {
  const { origin, sx, sy } = measure(box, null);
  box.style.left = `${(clamp(x, innerWidth - origin.width - EDGE) - origin.left) / sx}px`;
  box.style.top = `${(clamp(y, innerHeight - origin.height - EDGE) - origin.top) / sy}px`;
  remember(box, null, null);
}

function anchorVisible(anchor) {
  if (anchor.isConnected === false || (anchor.getClientRects && !anchor.getClientRects().length)) return false;
  const rect = anchor.getBoundingClientRect();
  if (rect.bottom <= 0 || rect.top >= innerHeight || rect.right <= 0 || rect.left >= innerWidth) return false;
  for (let parent = anchor.parentElement; parent && parent !== document.body; parent = parent.parentElement) {
    const style = getComputedStyle(parent), clip = parent.getBoundingClientRect();
    if (style.display === 'contents') continue; // no box exists to clip the owner
    if (/(auto|scroll|hidden|clip)/.test(style.overflowY) && (rect.bottom <= clip.top || rect.top >= clip.bottom)) return false;
    if (/(auto|scroll|hidden|clip)/.test(style.overflowX) && (rect.right <= clip.left || rect.left >= clip.right)) return false;
  }
  return true;
}
function dismiss(box) {
  box.hidden = true;
  if (box.returnFocusTo?.hasAttribute('aria-expanded')) box.returnFocusTo.setAttribute('aria-expanded', 'false');
  placements.delete(box);
}

// Scrolling a list inside its popup must not dismiss it. Scrolling the owner
// keeps anchored tools attached; point menus close because their target moved.
if (typeof document !== 'undefined') {
  const pending = new Set();
  const update = event => {
    for (const [box, placement] of placements) {
      if (!box.isConnected || box.hidden) { placements.delete(box); continue; }
      if (event.target instanceof Node && box.contains(event.target)) continue;
      if (!placement.anchor) {
        if (event.type === 'wheel' || event.type === 'resize') { dismiss(box); }
      } else if (event.type !== 'wheel') pending.add(box);
    }
    if (frame || !pending.size) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      const work = [...pending]; pending.clear();
      for (const box of work) {
        const placement = placements.get(box);
        if (!placement) continue;
        const { anchor, options } = placement;
        if (!box.isConnected || box.hidden) { placements.delete(box); continue; }
        if (!anchor) continue;
        if (!anchorVisible(anchor)) {
          dismiss(box); continue;
        }
        placeFloating(box, anchor, options);
      }
    });
  };
  document.addEventListener('scroll', update, { capture: true, passive: true });
  document.addEventListener('wheel', update, { capture: true, passive: true });
  window.addEventListener('resize', update, { passive: true });
}
