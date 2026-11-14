<script lang="ts">
  import { untrack } from 'svelte';
  import { Checkbox } from '$lib/components/ui/checkbox/index.js';
  import { Switch } from '$lib/components/ui/switch/index.js';
  import type { HTMLInputAttributes } from 'svelte/elements';
  import { bindNativeControl } from './native-proxy';

  let { id, checked = false, disabled = false, class: className = '', input, toggle = false, ...rest }: Omit<HTMLInputAttributes, 'type' | 'children'> & {
    id: string; input?: HTMLInputElement; toggle?: boolean;
  } = $props();
  let selected = $state(untrack(() => Boolean(checked)));
  let unavailable = $state(untrack(() => Boolean(disabled)));
  let mixed = $state(false);
  let proxy: HTMLInputElement;
  function attach(node: HTMLSpanElement) {
    proxy = input || node.querySelector('input')!;
    const control = () => node.querySelector<HTMLElement>('[role="checkbox"], [role="switch"]');
    return bindNativeControl(proxy, ['checked', 'disabled', 'indeterminate'], () => {
      selected = proxy.checked;
      unavailable = proxy.disabled;
      mixed = proxy.indeterminate;
    }, control);
  }
  function stopControlClick(event: MouseEvent) {
    // The proxy's native change event drives legacy row selection. The visible
    // button must not also invoke its containing row's click action.
    event.stopPropagation();
  }
  function change(next: boolean) {
    if (!proxy || proxy.disabled) return;
    if (next === proxy.checked && !proxy.indeterminate) return;
    // Native activation preserves existing click listeners and browser event
    // ordering (click, input, change), including indeterminate clearing.
    proxy.checked = !next;
    proxy.click();
  }
</script>

<span class="compat-native" {@attach attach}>
  {#if !input}<input {id} type="checkbox" {checked} {disabled} class={className} {...rest} hidden aria-hidden="true" tabindex="-1" data-native-proxy />{/if}
  {#if toggle}
    <Switch id={`${id}-control`} checked={selected} disabled={unavailable} onCheckedChange={change} onclick={stopControlClick} class="compat-switch" data-proxy-id={id} />
  {:else}
    <Checkbox id={`${id}-control`} checked={selected} indeterminate={mixed} disabled={unavailable} onCheckedChange={change} onclick={stopControlClick} class="compat-checkbox" data-proxy-id={id} />
  {/if}
</span>

<style>
  .compat-native { display: contents; }
  :global(.compat-checkbox) { width: 16px; height: 16px; min-width: 16px; padding: 0; border-radius: 5px; flex: none; background: var(--bg); color: var(--text); transition: background-color 120ms, border-color 120ms; }
  :global(.compat-checkbox[data-checked]) { background: var(--accent); color: var(--accent-text); border-color: var(--accent); }
  :global(.compat-checkbox[data-indeterminate]) { background: var(--accent); color: var(--accent-text); border-color: var(--accent); }
  :global(.compat-checkbox:focus-visible), :global(.compat-switch:focus-visible) { outline: 2px solid var(--ring-color); outline-offset: 3px; }
  :global(.compat-switch) { width: 34px; height: 20px; min-width: 34px; padding: 1px; border: 0; border-radius: 999px; flex: none; background: var(--border); transition: background-color 140ms; }
  :global(.compat-switch[data-checked]) { background: var(--accent); }
  :global(.compat-switch [data-slot='switch-thumb']) { width: 18px; height: 18px; background: var(--bg); transition: transform 140ms; }
  :global(.compat-switch[data-checked] [data-slot='switch-thumb']) { transform: translateX(14px); }
  :global(.compat-switch[data-unchecked] [data-slot='switch-thumb']) { transform: translateX(0); }
  @media (prefers-reduced-motion: reduce) { :global(.compat-checkbox), :global(.compat-switch), :global(.compat-switch [data-slot='switch-thumb']) { transition: none; } }
</style>
