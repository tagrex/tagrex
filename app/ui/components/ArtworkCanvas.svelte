<script lang="ts">
  import { flushSync, untrack } from 'svelte';
  import { Tween, prefersReducedMotion } from 'svelte/motion';
  import { cubicOut } from 'svelte/easing';
  import { t } from '../js/i18n.js';
  import { Button } from '$lib/components/ui/button/index.js';
  import { subscribeArtworkWheel } from '$lib/artwork-wheel';

  let { src, width, height, label, rotation = 0, onrotate }: {
    src: string; width: number; height: number; label: string; rotation?: number;
    onrotate: (rotation: number) => void;
  } = $props();
  let area = $state({ width: 0, height: 0 });
  let controls = $state(false);
  let dragging = $state(false);
  let atFit = true;
  let moved = false;
  let drag: { id: number; x: number; y: number; deltaX: number; deltaY: number } | null = null;
  let dragLayer: HTMLDivElement;
  const view = new Tween({ scale: 1, x: 0, y: 0 }, { duration: 240, easing: cubicOut });
  const angle = new Tween(untrack(() => rotation), { duration: 240, easing: cubicOut });
  const motionDuration = () => prefersReducedMotion.current ? 0 : 240;
  const fit = (rotationAngle = angle.target) => {
    const swapped = Math.abs(Math.round(rotationAngle / 90)) % 2;
    return Math.min(Math.max(1, area.width - 36) / (swapped ? height : width), Math.max(1, area.height - 36) / (swapped ? width : height), 1);
  };
  const zoomPercent = $derived(`${Math.round(view.current.scale * 100)}%`);

  export function reset() {
    atFit = true;
    view.set({ scale: fit(), x: 0, y: 0 }, { duration: motionDuration() });
  }
  export function rotate(direction: number) {
    const next = angle.target + direction * 90;
    const scale = atFit ? fit(next) : view.target.scale;
    angle.set(next, { duration: motionDuration() });
    view.set({ ...view.target, scale }, { duration: motionDuration() });
    onrotate(next);
    controls = true;
  }
  export function actualSize() { zoomTo(1); controls = true; }

  function zoomTo(next: number, point = { x: 0, y: 0 }) {
    const scale = Math.max(Math.min(fit() * .25, .1), Math.min(16, next));
    const old = view.target;
    const factor = scale / old.scale;
    atFit = Math.abs(scale - fit()) < .001;
    const dx = drag?.deltaX || 0, dy = drag?.deltaY || 0;
    view.set({ ...old, scale, x: point.x - (point.x - old.x - dx) * factor - dx, y: point.y - (point.y - old.y - dy) * factor - dy }, { duration: motionDuration() });
  }
  export function zoomStep(direction: number) {
    const steps = [...new Set([fit() * .25, fit() * .5, fit(), 1, 1.5, 2, 3, 4, 6, 8, 12, 16])].sort((a, b) => a - b);
    const scale = view.target.scale;
    const next = direction > 0 ? steps.find((step) => step > scale + .01) : [...steps].reverse().find((step) => step < scale - .01);
    if (next !== undefined) zoomTo(next);
    else if (direction > 0) reset();
    controls = true;
  }

  function attachViewer(node: HTMLDivElement) {
    let initialized = false;
    const observer = new ResizeObserver(([entry]) => {
      area = { width: entry.contentRect.width, height: entry.contentRect.height };
      if (atFit) view.set({ ...view.target, scale: fit(), x: 0, y: 0 }, { duration: initialized ? motionDuration() : 0 });
      initialized = true;
    });
    observer.observe(node);
    const wheel = (event: WheelEvent) => {
      if (!(event.target instanceof Node) || !node.contains(event.target) || node.closest('[inert]')) return;
      event.preventDefault();
      const bounds = node.getBoundingClientRect();
      const unit = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? area.height : 1;
      const amount = Math.max(-180, Math.min(180, event.deltaY * unit));
      zoomTo(view.target.scale * Math.exp(-amount * .0025), { x: event.clientX - bounds.left - bounds.width / 2, y: event.clientY - bounds.top - bounds.height / 2 });
      controls = true;
    };
    // Capture at the document so WebKit routes wheel input through JavaScript
    // even when the preview is a compositor-only, non-scrolling layer. Scope
    // handling to this live canvas; the gallery and surrounding UI still scroll.
    const unsubscribe = subscribeArtworkWheel(node.ownerDocument, wheel);
    return () => { observer.disconnect(); unsubscribe(); };
  }

  function pointerDown(event: PointerEvent) {
    if (drag || (event.button !== 0 && event.button !== 1 && event.pointerType !== 'touch')) return;
    event.preventDefault();
    const current = view.current;
    view.set(current, { duration: 0 });
    drag = { id: event.pointerId, x: event.clientX, y: event.clientY, deltaX: 0, deltaY: 0 };
    moved = false;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  }
  function pointerMove(event: PointerEvent) {
    if (!drag || drag.id !== event.pointerId) return;
    const x = event.clientX - drag.x, y = event.clientY - drag.y;
    if (Math.hypot(x, y) > 3) moved = true;
    if (moved) {
      if (!dragging) { dragging = true; controls = true; }
      atFit = false;
      drag.deltaX = x;
      drag.deltaY = y;
      // Pointer motion stays off the reactive/tween path: one compositor-only
      // write, with no layout reads or component rendering for each event.
      dragLayer.style.transform = `translate3d(${x}px, ${y}px, 0)`;
    }
  }
  function pointerUp(event: PointerEvent) {
    if (drag?.id !== event.pointerId) return;
    if (moved) {
      const current = view.current, target = view.target;
      const dx = drag.deltaX, dy = drag.deltaY;
      // Fold the transient drag into the stored view once, without a jump.
      flushSync(() => { view.set({ ...current, x: current.x + dx, y: current.y + dy }, { duration: 0 }); });
      dragLayer.style.transform = 'translate3d(0, 0, 0)';
      if (current.scale !== target.scale) {
        view.set({ ...target, x: target.x + dx, y: target.y + dy }, { duration: motionDuration() });
      }
    }
    drag = null;
    dragging = false;
    const node = event.currentTarget as HTMLElement;
    if (node.hasPointerCapture(event.pointerId)) node.releasePointerCapture(event.pointerId);
  }
  function onKey(event: KeyboardEvent) {
    if (['+', '=', '-', '0', '1', 'r', 'R'].includes(event.key)) {
      event.preventDefault();
      if (event.key === '0') reset();
      else if (event.key === '1') actualSize();
      else if (event.key.toLowerCase() === 'r') rotate(event.shiftKey ? -1 : 1);
      else zoomStep(event.key === '-' ? -1 : 1);
      controls = true;
    }
  }
</script>

{#snippet toolIcon(kind: string)}
  <svg aria-hidden="true" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
    {#if kind === 'plus' || kind === 'minus'}
      <circle cx="8.5" cy="8.5" r="5.5"/><path d="m13 13 4 4M6 8.5h5"/>{#if kind === 'plus'}<path d="M8.5 6v5"/>{/if}
    {:else if kind === 'left'}<path d="M3 8a7 7 0 1 1 1 7M3 3v5h5"/>
    {:else if kind === 'right'}<path d="M17 8a7 7 0 1 0-1 7M17 3v5h-5"/>
    {:else}<path d="M7 3H3v4m10-4h4v4M3 13v4h4m10-4v4h-4"/><path d="M7 7h6v6H7z"/>{/if}
  </svg>
{/snippet}

<div class="aw-viewer" {@attach attachViewer} data-scale={view.current.scale} data-pan-x={view.current.x} data-pan-y={view.current.y} data-angle={angle.current}>
  <button type="button" class={['aw-surface', { dragging }]}
    aria-label={t('artwork.canvas')} aria-expanded={controls} onkeydown={onKey}
    onpointerdown={pointerDown} onpointermove={pointerMove} onpointerup={pointerUp} onpointercancel={pointerUp}
    onauxclick={(event) => { if (event.button === 1) event.preventDefault(); }}
    onclick={(event) => { if (event.detail < 2 && (event.detail === 0 || !moved)) controls = !controls; }}
    ondblclick={() => { if (!moved) { reset(); controls = true; } }}>
    <div class="aw-drag-layer" {@attach (node) => { dragLayer = node; }}>
    <img class="aw-hero" {src} alt={label} draggable="false" style:width={`${width}px`} style:height={`${height}px`}
      style:transform={`translate(-50%, -50%) translate(${view.current.x}px, ${view.current.y}px) rotate(${angle.current}deg) scale(${view.current.scale})`} />
    </div>
  </button>
  <div class={['aw-tools', { visible: controls }]} role="toolbar" aria-label={t('artwork.controls')} aria-hidden={!controls} inert={!controls}>
    <Button variant="ghost" size="icon-sm" aria-label={t('artwork.zoomOut')} title={t('artwork.zoomOut')} onclick={() => zoomStep(-1)}>{@render toolIcon('minus')}</Button>
    <span class="aw-scale" aria-live="off">{zoomPercent}</span>
    <Button variant="ghost" size="icon-sm" aria-label={t('artwork.zoomIn')} title={t('artwork.zoomIn')} onclick={() => zoomStep(1)}>{@render toolIcon('plus')}</Button>
    <span class="aw-tool-divider"></span>
    <Button variant="ghost" size="icon-sm" aria-label={t('artwork.rotateLeft')} title={t('artwork.rotateLeft')} onclick={() => rotate(-1)}>{@render toolIcon('left')}</Button>
    <Button variant="ghost" size="icon-sm" aria-label={t('artwork.rotateRight')} title={t('artwork.rotateRight')} onclick={() => rotate(1)}>{@render toolIcon('right')}</Button>
    <Button variant="ghost" size="icon-sm" aria-label={t('artwork.reset')} title={t('artwork.reset')} onclick={reset}>{@render toolIcon('reset')}</Button>
  </div>
</div>

<style>
  .aw-viewer { position: absolute; inset: 0; }
  .aw-surface { position: absolute; inset: 0; width: 100%; height: 100%; border: 0; padding: 0; border-radius: 0; background: transparent; box-shadow: none; overflow: hidden; cursor: grab; touch-action: none; }
  .aw-surface:hover:not(:disabled) { background: transparent; }
  .aw-surface.dragging { cursor: grabbing; }
  .aw-drag-layer { position: absolute; inset: 0; transform: translate3d(0, 0, 0); will-change: transform; pointer-events: none; }
  .aw-hero { position: absolute; left: 50%; top: 50%; max-width: none; max-height: none; object-fit: contain; pointer-events: none; user-select: none; will-change: transform; border-radius: 2px; box-shadow: 0 8px 30px rgb(0 0 0 / .2); }
  .aw-tools { position: absolute; bottom: 12px; right: 12px; display: flex; align-items: center; gap: 2px; padding: 5px; border: 1px solid var(--border); border-radius: 12px; background: color-mix(in srgb, var(--bg) 94%, transparent); box-shadow: 0 4px 16px rgb(0 0 0 / .12); opacity: 0; transform: translateY(6px); transition: opacity 180ms, transform 220ms cubic-bezier(.2,.8,.2,1); pointer-events: none; }
  .aw-tools.visible { opacity: 1; transform: translateY(0); pointer-events: auto; }
  .aw-tools :global(button) { display: grid; place-items: center; width: 32px; height: 34px; padding: 0; border: 0; border-radius: 999px; background: transparent; box-shadow: none; color: var(--text); }
  .aw-tools :global(button:hover) { background: var(--sel-tint); color: var(--accent-ink); }
  .aw-tools :global(button:active) { transform: scale(.94); }
  svg { width: 18px; height: 18px; }
  .aw-scale { min-width: 42px; text-align: center; font-size: 11px; font-variant-numeric: tabular-nums; color: var(--muted); }
  .aw-tool-divider { height: 18px; width: 1px; margin: 0 3px; background: var(--border); }
  @media (max-width: 600px) { .aw-tools { bottom: 8px; right: 8px; } .aw-tools :global(button) { width: 40px; height: 40px; } }
  @media (prefers-reduced-motion: reduce) { .aw-tools { transition: none; } }
</style>
