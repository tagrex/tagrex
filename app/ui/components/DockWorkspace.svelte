<script lang="ts">
  import { onMount, untrack, type Snippet } from 'svelte';
  import { Button } from '$lib/components/ui/button/index.js';
  import { createWorkspaceLayout, provideWorkspaceLayout, type DockArea, type WorkspaceLayout } from '$lib/workspace-layout';
  import '../docking.css';
  let { storageKey = 'tagrex.workspace.v1', controller = $bindable<WorkspaceLayout>(), children }: { storageKey?: string; controller?: WorkspaceLayout; children: Snippet } = $props();
  const layout = untrack(() => controller || createWorkspaceLayout(storageKey));
  provideWorkspaceLayout(layout);
  let view = $state.raw(layout.snapshot());
  let canvas: HTMLElement;
  let cancelResize: (() => void) | undefined;
  const topHost = (node: HTMLElement) => layout.host('top', node);
  const leftHost = (node: HTMLElement) => layout.host('left', node);
  const rightHost = (node: HTMLElement) => layout.host('right', node);
  const bottomHost = (node: HTMLElement) => layout.host('bottom', node);
  const has = (area: DockArea) => view.modules.some(module => module.area === area);
  const areaResizable = (area: 'top' | 'bottom') => view.modules.some(module => module.area === area && !module.collapsed);
  const areaSize = (area: 'top' | 'bottom') => {
    const modules = view.modules.filter(module => module.area === area);
    if (!modules.length) return '0px';
    return modules.every(module => module.collapsed)
      ? `calc(${modules.length} * var(--dock-collapsed-height, 32px))`
      : `min(${view[area]}px, 36%)`;
  };
  onMount(() => {
    controller = layout;
    const unsubscribe = layout.subscribe(next => { view = next; });
    const media = window.matchMedia('(max-width: 760px), (max-width: 900px) and (max-height: 520px)');
    const adapt = () => layout.setCompact(media.matches);
    media.addEventListener('change', adapt);
    adapt();
    queueMicrotask(() => layout.ready());
    return () => { unsubscribe(); media.removeEventListener('change', adapt); cancelResize?.(); };
  });
  function tabKey(event: KeyboardEvent, index: number) {
    const count = view.modules.length;
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? count - 1 : event.key === 'ArrowRight' ? (index + 1) % count : event.key === 'ArrowLeft' ? (index + count - 1) % count : -1;
    if (next < 0) return;
    event.preventDefault(); event.stopPropagation();
    const id = view.modules[next].id;
    layout.reveal(id);
    const tabs = canvas.closest('.dock-frame')?.querySelectorAll<HTMLElement>('.dock-module-tab');
    tabs?.[next]?.focus();
  }
  function resize(event: PointerEvent, area: 'left' | 'top' | 'bottom') {
    if (event.button !== 0 || view.compact) return;
    event.preventDefault();
    const rect = canvas.getBoundingClientRect();
    let pending: number | undefined;
    let frame = 0;
    const flush = () => {
      frame = 0;
      if (pending !== undefined) { layout.resize(area, pending, false); pending = undefined; }
    };
    const move = (next: PointerEvent) => {
      pending = area === 'left' ? (next.clientX - rect.left) / rect.width * 100 : area === 'top' ? next.clientY - rect.top : rect.bottom - next.clientY;
      if (!frame) frame = requestAnimationFrame(flush);
    };
    const stop = () => {
      if (frame) cancelAnimationFrame(frame);
      flush();
      window.removeEventListener('pointermove', move); window.removeEventListener('pointerup', stop); window.removeEventListener('pointercancel', stop);
      cancelResize = undefined;
      layout.finishResize();
    };
    cancelResize?.(); cancelResize = stop;
    layout.beginResize();
    (event.currentTarget as HTMLElement).setPointerCapture?.(event.pointerId);
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', stop, { once: true });
    window.addEventListener('pointercancel', stop, { once: true });
  }
  function resizeKey(event: KeyboardEvent, area: 'left' | 'top' | 'bottom') {
    const delta = area === 'left' ? 2 : 16;
    if (event.key === 'Home') layout.resize(area, area === 'left' ? 72 : area === 'top' ? 138 : 190);
    else if (['ArrowLeft', 'ArrowUp', 'ArrowRight', 'ArrowDown'].includes(event.key)) {
      const direction = ['ArrowRight', 'ArrowDown'].includes(event.key) ? 1 : -1;
      layout.resize(area, view[area] + direction * delta * (area === 'bottom' ? -1 : 1));
    }
    else return;
    event.preventDefault(); event.stopPropagation();
  }
</script>

<div class="dock-frame" data-dock-compact={view.compact}>
  <div class="dock-switcher" role="tablist" aria-label="Workspace module">
    {#each view.modules as module, index (module.id)}
      <Button variant="ghost" size="sm" class="dock-module-tab" role="tab" aria-selected={view.active === module.id} tabindex={view.active === module.id ? 0 : -1} onkeydown={event => tabKey(event, index)} aria-controls={`dock-module-${module.id}`} onclick={() => layout.reveal(module.id)}>{module.title}</Button>
    {/each}
  </div>
  <div class="dock-workspace" {@attach node => { canvas = node; return layout.surface(node); }}
    data-has-left={has('left')} data-has-right={has('right')}
    style={`--dock-left:${view.left}%;--dock-top-size:${areaSize('top')};--dock-bottom-size:${areaSize('bottom')};--dock-top-handle:${areaResizable('top') ? '4px' : '0px'};--dock-bottom-handle:${areaResizable('bottom') ? '4px' : '0px'}`}>
    <div class="dock-area dock-top" role="region" aria-label="Top dock area" data-dock-host="top" {@attach topHost}></div>
    <Button variant="ghost" class="dock-splitter dock-top-splitter" role="separator" aria-label="Resize top modules" aria-orientation="horizontal" aria-valuemin={70} aria-valuemax={600} aria-valuenow={view.top} hidden={!areaResizable('top')} onpointerdown={event => resize(event, 'top')} onkeydown={event => resizeKey(event, 'top')} />
    <div class="dock-middle">
      <div class="dock-area dock-left" role="region" aria-label="Left dock area" data-dock-host="left" {@attach leftHost}></div>
      <Button variant="ghost" class="dock-splitter dock-column-splitter" role="separator" aria-label="Resize workspace columns" aria-orientation="vertical" aria-valuemin={20} aria-valuemax={82} aria-valuenow={view.left} hidden={!has('left') || !has('right')} onpointerdown={event => resize(event, 'left')} onkeydown={event => resizeKey(event, 'left')} />
      <div class="dock-area dock-right" role="region" aria-label="Right dock area" data-dock-host="right" {@attach rightHost}></div>
    </div>
    <Button variant="ghost" class="dock-splitter dock-bottom-splitter" role="separator" aria-label="Resize bottom modules" aria-orientation="horizontal" aria-valuemin={70} aria-valuemax={600} aria-valuenow={view.bottom} hidden={!areaResizable('bottom')} onpointerdown={event => resize(event, 'bottom')} onkeydown={event => resizeKey(event, 'bottom')} />
    <div class="dock-area dock-bottom" role="region" aria-label="Bottom dock area" data-dock-host="bottom" {@attach bottomHost}></div>
  </div>
  <div class="dock-parking">{@render children()}</div>
  <p class="dock-announcement" aria-live="polite">{view.modules.find(module => module.id === view.active)?.title || ''}</p>
</div>
