// Native pixel scrolling stays native (including trackpad momentum). Smooth
// only discrete horizontal/Shift-wheel steps; never add a second inertia tail.
export function initHorizontalScroll(wrap) {
  const motion = matchMedia('(prefers-reduced-motion: reduce)');
  let frame = 0, target = wrap.scrollLeft, start = target, begun = 0, written = target;
  const duration = 60;
  const bounds = value => {
    const max = Math.max(0, wrap.scrollWidth - wrap.clientWidth);
    return getComputedStyle(wrap).direction === 'rtl'
      ? Math.max(-max, Math.min(0, value)) : Math.max(0, Math.min(max, value));
  };
  function stop() { cancelAnimationFrame(frame); frame = 0; target = wrap.scrollLeft; }
  function write(value) { wrap.scrollLeft = value; written = wrap.scrollLeft; }
  function step(now) {
    const progress = Math.min(1, (now - begun) / duration);
    write(start + (target - start) * (1 - (1 - progress) ** 3));
    if (progress < 1) frame = requestAnimationFrame(step);
    else { frame = 0; write(target); }
  }
  function wheel(event) {
    if (event.ctrlKey || event.metaKey || event.defaultPrevented) return;
    const delta = event.shiftKey && !event.deltaX ? event.deltaY : event.deltaX;
    if (!delta || (!event.shiftKey && Math.abs(event.deltaY) >= Math.abs(event.deltaX))) { stop(); return; }
    // Pixel-mode horizontal wheels/trackpads already have high-resolution,
    // proportional browser scrolling. Let the engine preserve their momentum.
    if (!event.deltaMode && !event.shiftKey) { stop(); return; }
    const unit = event.deltaMode === 1 ? parseFloat(getComputedStyle(wrap).lineHeight) || 16
      : event.deltaMode === 2 ? wrap.clientWidth : 1;
    const next = bounds((frame ? target : wrap.scrollLeft) + delta * unit);
    if (next === wrap.scrollLeft && !frame) return;
    event.preventDefault();
    target = next;
    if (motion.matches) { cancelAnimationFrame(frame); frame = 0; write(target); return; }
    cancelAnimationFrame(frame);
    // Most of the movement happens in the input event, leaving just a short
    // settling tween. Reversals replace the target rather than queue animation.
    write(wrap.scrollLeft + (target - wrap.scrollLeft) * .6);
    start = wrap.scrollLeft;
    begun = performance.now();
    frame = requestAnimationFrame(step);
  }
  const scrolled = () => { if (frame && Math.abs(wrap.scrollLeft - written) > 1) stop(); };
  wrap.addEventListener('wheel', wheel, { passive:false });
  wrap.addEventListener('pointerdown', stop, { passive:true });
  wrap.addEventListener('keydown', stop, true);
  wrap.addEventListener('scroll', scrolled, { passive:true });
  window.addEventListener('resize', stop, { passive:true });
  const dispose = () => {
    stop();
    wrap.removeEventListener('wheel', wheel); wrap.removeEventListener('pointerdown', stop);
    wrap.removeEventListener('keydown', stop, true); wrap.removeEventListener('scroll', scrolled);
    window.removeEventListener('resize', stop);
  };
  window.addEventListener('pagehide', dispose, { once:true });
  return dispose;
}
const wrap = document.getElementById('files-view');
if (wrap) initHorizontalScroll(wrap);
