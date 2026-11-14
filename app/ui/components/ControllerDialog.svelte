<script lang="ts">
  import type { Snippet } from 'svelte';
  import * as Dialog from '$lib/components/ui/dialog/index.js';
  import { connectControllerSurface, retainControllerNodes } from '$lib/controller-surfaces.js';

  let { id, role = 'dialog', label, labelledby, describedby, closeId, settings = false, nativeRoot,
    nodes = [], children }: {
    id?: string; role?: 'dialog' | 'alertdialog'; label?: string; labelledby?: string; describedby?: string;
    closeId?: string; settings?: boolean; nativeRoot?: HTMLElement;
    nodes?: Node[]; children?: Snippet;
  } = $props();
  let open = $state(false);
  let previousFocus: HTMLElement | null = null;
  let root: HTMLElement;

  function connect(node: HTMLElement) {
    root = nativeRoot || node;
    const bridge = connectControllerSurface(root, (value) => {
      if (value) previousFocus = document.activeElement as HTMLElement;
      open = value;
    });
    return { destroy: bridge.destroy };
  }
  function close() {
    if (closeId) document.getElementById(closeId)?.click();
    if (root && !root.hidden) root.hidden = true;
  }
  function changed(value: boolean) { if (!value && open) close(); }
  function restoreFocus(event: Event) {
    event.preventDefault();
    if (previousFocus?.isConnected) previousFocus.focus({ preventScroll: true });
  }
</script>

<div {id} class={!nativeRoot ? 'modal-backdrop controller-dialog-root' : undefined} hidden={!nativeRoot ? true : undefined} use:connect>
  <Dialog.Root {open} onOpenChange={changed}>
    <Dialog.Content {role} forceMount portalProps={{ disabled: true }} showCloseButton={false}
      class={settings ? 'controller-dialog-content controller-settings-content' : 'controller-dialog-content'}
      overlayProps={{ class: 'controller-dialog-overlay', forceMount: true, onclick: close }}
      data-i18n-aria-label={settings ? "topbar.settings" : undefined} aria-label={label} aria-labelledby={labelledby} aria-describedby={describedby}
      onCloseAutoFocus={restoreFocus}
      onEscapeKeydown={(event) => event.stopPropagation()} preventBodyPointerEvents={false}>
      <div class="controller-dialog-body" use:retainControllerNodes={nodes}>
        {@render children?.()}
      </div>
    </Dialog.Content>
  </Dialog.Root>
</div>
