<script lang="ts">
  import type { Snippet } from 'svelte';
  import * as Popover from '$lib/components/ui/popover/index.js';
  import { upgradeComponentMarkup } from '$lib/component-elements.js';
  import { connectControllerSurface } from '$lib/controller-surfaces.js';

  let { id, class: className = '', label, labelKey, nativeRoot, nodes = [], bridgeContent = false, preserveFocus = false, children }: {
    id?: string; class?: string; label?: string; labelKey?: string; nativeRoot?: HTMLElement;
    nodes?: Node[]; bridgeContent?: boolean; preserveFocus?: boolean; children?: Snippet;
  } = $props();
  let open = $state(false);
  let root: HTMLElement;
  function connect(node: HTMLElement) {
    root = nativeRoot || node;
    root.classList.add('controller-popup-root');
    // Imperative placement owns the viewport budget. Carry that definite length
    // through the Bits wrapper; max-height: inherit stops at display:contents.
    const syncAvailableHeight = () => {
      const available = root.style.maxHeight || 'calc(100dvh - 16px)';
      if (root.style.getPropertyValue('--controller-available-height') !== available)
        root.style.setProperty('--controller-available-height', available);
    };
    syncAvailableHeight();
    const placement = new MutationObserver(syncAvailableHeight);
    placement.observe(root, { attributes: true, attributeFilter: ['style'] });
    const bridge = connectControllerSurface(root, (value) => {
      open = value;
      const anchor = (root as HTMLElement & { returnFocusTo?: HTMLElement }).returnFocusTo;
      if (!value && anchor?.hasAttribute('aria-expanded')) anchor.setAttribute('aria-expanded', 'false');
    });
    // This chooser also opens without a Popover.Trigger, from header context
    // clicks. Its retained shell needs its own Escape route and focus return.
    const closeColumns = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || event.isComposing || root.hidden ||
          document.querySelector('[role="dialog"][data-state="open"], [role="alertdialog"][data-state="open"]')) return;
      event.preventDefault(); event.stopImmediatePropagation(); root.hidden = true;
      const saved = (root as HTMLElement & { returnFocusTo?: HTMLElement }).returnFocusTo;
      const anchor = saved?.isConnected && saved.getClientRects().length ? saved : document.querySelector<HTMLElement>('#tracks th.sortable[tabindex="0"]');
      anchor?.focus({ preventScroll: true });
    };
    if (root.id === 'columns-menu') window.addEventListener('keydown', closeColumns, true);
    return { destroy() { placement.disconnect(); bridge.destroy(); if (root.id === 'columns-menu') window.removeEventListener('keydown', closeColumns, true); } };
  }
  function content(host: HTMLElement) {
    host.append(...nodes);
    if (!bridgeContent || !nativeRoot) return;
    // Dynamic controllers replace only their own content, never the Bits shell.
    const descriptor = Object.getOwnPropertyDescriptor(Element.prototype, 'innerHTML')!;
    let pendingClear: ReturnType<typeof setTimeout> | undefined;
    let clearRequested = false;
    function clearRetained() {
      clearTimeout(pendingClear);
      if (clearRequested) descriptor.set!.call(host, '');
      clearRequested = false;
    }
    function replace(value: string) {
      clearTimeout(pendingClear);
      clearRequested = false;
      // Completion controllers clear their rows immediately after hiding. Keep
      // those pixels only for the exit, while the logical HTML is already empty.
      if (!value && nativeRoot!.hidden && host.childNodes.length) {
        clearRequested = true;
        pendingClear = setTimeout(clearRetained, 140);
        return;
      }
      descriptor.set!.call(host, value);
      upgradeComponentMarkup(host);
    }
    Object.defineProperty(nativeRoot, 'innerHTML', {
      configurable: true,
      get: () => clearRequested ? '' : descriptor.get!.call(host),
      set: replace,
    });
    Object.defineProperty(nativeRoot, 'appendChild', { configurable: true, value: (node: Node) => { clearRetained(); return host.appendChild(node); } });
    Object.defineProperty(nativeRoot, 'append', { configurable: true, value: (...items: (Node | string)[]) => { clearRetained(); host.append(...items); } });
    Object.defineProperty(nativeRoot, 'prepend', { configurable: true, value: (...items: (Node | string)[]) => { clearRetained(); host.prepend(...items); } });
    return { destroy() {
      clearTimeout(pendingClear);
      delete (nativeRoot as unknown as Record<string, unknown>).innerHTML;
      delete (nativeRoot as unknown as Record<string, unknown>).appendChild;
      delete (nativeRoot as unknown as Record<string, unknown>).append;
      delete (nativeRoot as unknown as Record<string, unknown>).prepend;
    } };
  }
  function changed(value: boolean) { if (!value && open) root.hidden = true; }
  function actionKeys(event: KeyboardEvent) {
    if (event.defaultPrevented || event.isComposing || event.altKey || event.ctrlKey || event.metaKey ||
        !root.matches('.tk-save-menu, #query-preset-menu, #root-recent-menu') ||
        !['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
    if (event.target instanceof Element && event.target.closest('input, textarea, select, [contenteditable="true"]')) return;
    const items = [...root.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')].filter(node => node.getClientRects().length && node.getAttribute('aria-disabled') !== 'true');
    if (!items.length) return;
    event.preventDefault();
    const current = items.indexOf(document.activeElement as HTMLButtonElement);
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1 :
      (current + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
    items[next].focus();
  }
  function escapePopup(event: KeyboardEvent) {
    event.stopPropagation();
    const anchor = (root as HTMLElement & { returnFocusTo?: HTMLElement }).returnFocusTo;
    if (!preserveFocus && anchor?.isConnected) queueMicrotask(() => anchor.focus({ preventScroll: true }));
  }
</script>

<div {id} class={className} hidden={!nativeRoot ? true : undefined} use:connect>
  <Popover.Root {open} onOpenChange={changed}>
    <Popover.Content forceMount portalProps={{ disabled: true }} class="controller-popover-content"
      data-controller-popup data-i18n-aria-label={labelKey} aria-label={label || nativeRoot?.getAttribute('aria-label') || id || 'Options'}
      trapFocus={false} preventScroll={false} onkeydown={actionKeys}
      onInteractOutside={(event) => { if (root?.id === 'columns-menu' && event.target instanceof Element && event.target.closest('#columns-btn')) event.preventDefault(); }}
      onOpenAutoFocus={(event) => { if (preserveFocus) event.preventDefault(); }} onCloseAutoFocus={(event) => event.preventDefault()}
      onEscapeKeydown={escapePopup}>
      <div class="controller-popover-body" use:content>{@render children?.()}</div>
    </Popover.Content>
  </Popover.Root>
</div>
