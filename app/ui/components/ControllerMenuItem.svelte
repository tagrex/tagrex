<script lang="ts">
  import type { Snippet } from 'svelte';
  import * as ContextMenu from '$lib/components/ui/context-menu/index.js';
  import { Button } from '$lib/components/ui/button/index.js';
  let { cmd, danger = false, children }: { cmd: string; danger?: boolean; children: Snippet } = $props();
  let disabled = $state(false);
  let button = $state<HTMLButtonElement | null>(null);
  $effect(() => {
    if (!button) return;
    const node = button;
    const observer = new MutationObserver(() => { disabled = node.disabled; });
    observer.observe(node, { attributes: true, attributeFilter: ['disabled'] });
    return () => observer.disconnect();
  });
  function select() {
    // Keyboard activation must use the same caret-preserving path as a mouse.
    if (button?.closest('#edit-ctx')) {
      button.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
    }
  }
</script>

<ContextMenu.Item {disabled} closeOnSelect={false} onSelect={select}>
  {#snippet child({ props })}
    <Button {...props} variant={danger ? 'destructive' : 'ghost'} size="sm" type="button"
      class={danger ? 'ctx-item danger' : 'ctx-item'} data-cmd={cmd} bind:ref={button}>
      {@render children()}
    </Button>
  {/snippet}
</ContextMenu.Item>
