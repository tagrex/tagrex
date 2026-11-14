<script lang="ts">
  import { untrack, type Snippet } from 'svelte';
  import { Button } from '$lib/components/ui/button/index.js';
  import * as Context from '$lib/components/ui/context-menu/index.js';
  import { useWorkspaceLayout, DOCK_AREAS, type DockArea } from '$lib/workspace-layout';
  let { moduleId, title, defaultArea = 'right', defaultIndex, defaultWeight = 1, chromeTarget, headerless = false, collapsible = false, headerHelp, headerActions, children }: { moduleId: string; title: string; defaultArea?: DockArea; defaultIndex?: number; defaultWeight?: number; chromeTarget?: string; headerless?: boolean; collapsible?: boolean; headerHelp?: Snippet; headerActions?: Snippet; children: Snippet } = $props();
  const layout = useWorkspaceLayout();
  if (!layout) throw new Error('DockModule must be inside DockWorkspace');
  let position = $state(untrack(() => ({ area: defaultArea, index: 0, total: 1, weight: 1, collapsed: false, nextCollapsed: false })));
  let contextOpen = $state(false);
  let root: HTMLElement;
  let chrome: HTMLElement;
  let layoutOpener: HTMLElement | null = null;
  let cancelResize: (() => void) | undefined;
  let collapseTimer: ReturnType<typeof setTimeout> | undefined;
  function attach(node: HTMLElement) {
    root = node;
    chrome = node.querySelector<HTMLElement>('.dock-module-chrome')!;
    // An inspector already has its own tool tabs/headings. Reuse those for
    // module movement without reserving another visible title row.
    const contextSurface = (target: EventTarget | null) => target instanceof Element && !!target.closest('.subtabs, .mode-panel h2');
    const openLayoutMenu = (event: MouseEvent) => {
      if (!headerless || !contextSurface(event.target)) return;
      event.preventDefault(); event.stopPropagation();
      layoutOpener = (event.target as Element).closest<HTMLElement>('button, [tabindex]') || document.activeElement as HTMLElement;
      chrome.dispatchEvent(new MouseEvent('contextmenu', { bubbles:true, cancelable:true, clientX:event.clientX, clientY:event.clientY }));
    };
    const layoutKey = (event: KeyboardEvent) => {
      if (!headerless || !contextSurface(event.target) || !(event.key === 'ContextMenu' || (event.key === 'F10' && event.shiftKey))) return;
      event.preventDefault(); event.stopPropagation();
      layoutOpener = event.target as HTMLElement;
      const rect = (event.target as Element).getBoundingClientRect();
      chrome.dispatchEvent(new MouseEvent('contextmenu', { bubbles:true, cancelable:true, clientX:rect.left + 8, clientY:rect.bottom }));
    };
    node.addEventListener('contextmenu', openLayoutMenu);
    node.addEventListener('keydown', layoutKey);
    if (chromeTarget) {
      const target = node.querySelector(chromeTarget) || document.querySelector(chromeTarget);
      if (target && chrome && chrome.parentElement !== target) {
        if (!collapsible) chrome.dataset.inline = 'true';
        // Reuse an existing heading's actual contents, including controller IDs.
        // Files gets its plain text label in the global toolbar instead.
        if (/^H[1-6]$/.test(target.tagName)) chrome.querySelector('.dock-module-title')?.replaceChildren(...target.childNodes);
        if (collapsible) target.remove();
        else target.append(chrome);
      }
    }
    const unregister = layout.register(moduleId, title, node, defaultArea, defaultIndex, defaultWeight);
    const unsubscribe = layout.subscribe(next => {
      const module = next.modules.find(module => module.id === moduleId);
      if (module) {
        const siblings = next.modules.filter(other => other.area === module.area);
        const index = siblings.findIndex(other => other.id === moduleId);
        position = { ...module, collapsed: module.collapsed === true, total: siblings.length, nextCollapsed: siblings[index + 1]?.collapsed === true };
      }
    });
    return () => { node.removeEventListener('contextmenu', openLayoutMenu); node.removeEventListener('keydown', layoutKey); cancelResize?.(); clearTimeout(collapseTimer); unsubscribe(); unregister(); };
  }
  function toggleCollapsed() {
    if (!collapsible) return;
    clearTimeout(collapseTimer);
    root.dataset.collapseAnimating = 'true';
    chrome.querySelector<HTMLElement>('.dock-module-toggle')?.focus({ preventScroll:true });
    layout.toggleCollapsed(moduleId);
    collapseTimer = setTimeout(() => { delete root.dataset.collapseAnimating; }, 180);
  }
  function headerClick(event: MouseEvent) {
    if (!collapsible || (event.target as Element).closest('button, a, input, select, textarea')) return;
    toggleCollapsed();
  }
  function restoreLayoutFocus(event: Event) {
    if (!headerless && !collapsible) return;
    event.preventDefault();
    const target = layoutOpener || chrome.querySelector<HTMLElement>('.dock-module-toggle');
    target?.focus({ preventScroll:true });
  }
  function keyboardMenu(event: KeyboardEvent) {
    const contextKey = event.key === 'ContextMenu' || (event.key === 'F10' && event.shiftKey);
    if (!contextKey && event.target instanceof Element && event.target !== chrome && event.target.closest('button, input, select, textarea, a')) return;
    if (!['ContextMenu', 'Enter', ' '].includes(event.key) && !(event.key === 'F10' && event.shiftKey)) return;
    event.preventDefault(); event.stopPropagation();
    if (collapsible && event.target instanceof HTMLElement) layoutOpener = event.target;
    const rect = chrome.getBoundingClientRect();
    chrome.dispatchEvent(new MouseEvent('contextmenu', { bubbles:true, cancelable:true, clientX:rect.left + 8, clientY:rect.bottom }));
  }
  function pair() {
    const next = root.nextElementSibling;
    if (!(next instanceof HTMLElement) || !next.hasAttribute('data-dock-module')) return;
    const first = root.getBoundingClientRect().height;
    const second = next.getBoundingClientRect().height;
    return { first, total:first + second };
  }
  function fraction(first: number, total: number) {
    const minimum = Math.min(72, total / 2);
    return Math.max(minimum, Math.min(total - minimum, first)) / total;
  }
  function resize(event: PointerEvent) {
    const sizes = pair();
    if (event.button !== 0 || !sizes) return;
    event.preventDefault(); event.stopPropagation();
    const startY = event.clientY;
    let pending: number | undefined;
    let frame = 0;
    const flush = () => {
      frame = 0;
      if (pending !== undefined) { layout.resizeModule(moduleId, fraction(pending, sizes.total), false); pending = undefined; }
    };
    const move = (next: PointerEvent) => { pending = sizes.first + next.clientY - startY; if (!frame) frame = requestAnimationFrame(flush); };
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
    window.addEventListener('pointerup', stop, { once:true });
    window.addEventListener('pointercancel', stop, { once:true });
  }
  function resizeKey(event: KeyboardEvent) {
    const sizes = pair();
    if (!['ArrowUp', 'ArrowDown'].includes(event.key) || !sizes) return;
    event.preventDefault(); event.stopPropagation();
    layout.resizeModule(moduleId, fraction(sizes.first + (event.key === 'ArrowDown' ? 16 : -16), sizes.total));
  }
</script>

<section id={`dock-module-${moduleId}`} class="dock-module" data-dock-module={moduleId} data-collapsible={collapsible} aria-label={title} {@attach attach}>
  <Context.Root bind:open={contextOpen}>
    <Context.Trigger class="dock-module-chrome" hidden={headerless} role={collapsible ? 'presentation' : 'button'} tabindex={collapsible ? undefined : 0} aria-label={collapsible ? undefined : `${title} module layout`} title={collapsible ? undefined : 'Right-click to move this module'} onkeydown={keyboardMenu} onclick={headerClick}>
      {#if collapsible}
        <Button variant="ghost" size="sm" class="dock-module-toggle" aria-labelledby={`dock-title-${moduleId}`} aria-expanded={!position.collapsed} aria-controls={`dock-body-${moduleId}`} onclick={toggleCollapsed}>
          <svg class="ico dock-module-caret" aria-hidden="true"><use href="#i-caret-down"/></svg>
          <span id={`dock-title-${moduleId}`} class="dock-module-title">{title}</span>
        </Button>
        {@render headerHelp?.()}
        <span class="dock-header-space" aria-hidden="true"></span>
        {@render headerActions?.()}
      {:else}
        <span class="dock-module-title">{title}</span>
      {/if}
    </Context.Trigger>
    <Context.Content class="dock-layout-menu" aria-hidden={!contextOpen} inert={!contextOpen} data-controller-popup onCloseAutoFocus={restoreLayoutFocus}>
      <Context.Label>Move {title}</Context.Label>
      {#each DOCK_AREAS as area (area)}<Context.Item disabled={position.area === area} onSelect={() => layout.move(moduleId, area)}>Dock {area}</Context.Item>{/each}
      <Context.Separator />
      <Context.Item disabled={position.index === 0} onSelect={() => layout.reorder(moduleId, -1)}>Move earlier</Context.Item>
      <Context.Item disabled={position.index >= position.total - 1} onSelect={() => layout.reorder(moduleId, 1)}>Move later</Context.Item>
      <Context.Separator />
      <Context.Item onSelect={() => layout.reset()}>Reset workspace layout</Context.Item>
    </Context.Content>
  </Context.Root>
  <div id={`dock-body-${moduleId}`} class="dock-module-body" inert={collapsible && position.collapsed}>{@render children()}</div>
  <Button variant="ghost" class="dock-module-splitter" role="separator" aria-label={`Resize ${title} module`} aria-orientation="horizontal" aria-valuemin={10} aria-valuemax={2000} aria-valuenow={Math.round(position.weight * 100)} hidden={position.collapsed || position.nextCollapsed || position.total < 2 || position.index === position.total - 1} onpointerdown={resize} onkeydown={resizeKey} />
</section>
