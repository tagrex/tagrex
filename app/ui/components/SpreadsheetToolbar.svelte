<script lang="ts">
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu/index.js';
  import { Copy, ClipboardPaste, ArrowDownToLine, X, Pencil } from '@lucide/svelte';

  type Snapshot = {
    label: string; detail: string; active: boolean; editable: boolean;
    canFill: boolean; busy: boolean; reviewing: boolean;
  };
  let { onaction }: { onaction: (action: string) => void } = $props();
  let info = $state<Snapshot>({ label: 'Table', detail: '', active: false, editable: false, canFill: false, busy: false, reviewing: false });
  let contextOpen = $state(false);
  let contextPoint = $state({ x: 0, y: 0 });

  export function update(snapshot: Snapshot) { info = snapshot; }
  export function openContext(x: number, y: number) {
    contextPoint = { x, y };
    contextOpen = true;
  }
  function focusCell(event: Event) {
    event.preventDefault();
    onaction('focus');
  }
</script>

<!-- Cell state is announced without taking a row away from the file list. -->
<div class="sr-only" role="status" aria-live="polite" aria-atomic="true">{info.label}. {info.detail}</div>

<DropdownMenu.Root bind:open={contextOpen}>
  <DropdownMenu.Trigger class="spreadsheet-context-anchor" style={`left:${contextPoint.x}px;top:${contextPoint.y}px`} tabindex={-1} aria-hidden="true" />
  <DropdownMenu.Content class="spreadsheet-menu" align="start" sideOffset={2} onCloseAutoFocus={focusCell}>
    {@render actions()}
  </DropdownMenu.Content>
</DropdownMenu.Root>

{#snippet actions()}
  <DropdownMenu.Label class="sheet-menu-context">{info.label}</DropdownMenu.Label>
  {#if !info.editable}<p class="sheet-menu-reason">{info.detail}</p>{/if}
  <DropdownMenu.Item onclick={() => onaction('copy')} disabled={!info.active}><Copy />Copy<DropdownMenu.Shortcut>⌘/Ctrl C</DropdownMenu.Shortcut></DropdownMenu.Item>
  <DropdownMenu.Item onclick={() => onaction('paste')} disabled={!info.editable || info.busy}><ClipboardPaste />Paste<DropdownMenu.Shortcut>⌘/Ctrl V</DropdownMenu.Shortcut></DropdownMenu.Item>
  <DropdownMenu.Item onclick={() => onaction('fill')} disabled={!info.canFill || info.busy}><ArrowDownToLine />Fill down<DropdownMenu.Shortcut>⌘/Ctrl D</DropdownMenu.Shortcut></DropdownMenu.Item>
  <DropdownMenu.Separator />
  <DropdownMenu.Item onclick={() => onaction('edit')} disabled={!info.editable || info.busy}><Pencil />Edit<DropdownMenu.Shortcut>F2</DropdownMenu.Shortcut></DropdownMenu.Item>
  <DropdownMenu.Item onclick={() => onaction('clear')}><X />Clear<DropdownMenu.Shortcut>Esc</DropdownMenu.Shortcut></DropdownMenu.Item>
{/snippet}
