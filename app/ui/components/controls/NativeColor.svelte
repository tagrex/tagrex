<script lang="ts">
  import { untrack } from 'svelte';
  import * as Popover from '$lib/components/ui/popover/index.js';
  import { Button } from '$lib/components/ui/button/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import type { HTMLInputAttributes } from 'svelte/elements';
  import { bindNativeControl, dispatchNativeChange } from './native-proxy';

  let { id, value = '#0b6b53', disabled = false, class: className = '', ...rest }: Omit<HTMLInputAttributes, 'type' | 'children'> & { id: string } = $props();
  let open = $state(false);
  let colour = $state(untrack(() => String(value)));
  let hex = $state(untrack(() => String(value)));
  let unavailable = $state(untrack(() => Boolean(disabled)));
  let proxy: HTMLInputElement;
  function attach(node: HTMLSpanElement) {
    proxy = node.querySelector('input')!;
    return bindNativeControl(proxy, ['value', 'disabled'], () => {
      colour = proxy.value;
      hex = proxy.value;
      unavailable = proxy.disabled;
    }, () => node.querySelector<HTMLElement>(`#${id}-control`));
  }
  function change() {
    if (!/^#[\da-f]{6}$/i.test(hex) || unavailable || hex.toLowerCase() === colour) return;
    proxy.value = hex;
    dispatchNativeChange(proxy);
  }
  function choose() {
    if (typeof proxy.showPicker === 'function') proxy.showPicker();
    else proxy.click();
  }
</script>

<span class="compat-colour" {@attach attach}>
  <input {id} type="color" {value} {disabled} class={className} {...rest} hidden aria-hidden="true" tabindex="-1" data-native-proxy />
  <Popover.Root bind:open>
    <Popover.Trigger>
      {#snippet child({ props })}
        <Button {...props} id={`${id}-control`} variant="ghost" size="icon-sm" class="compat-colour-trigger" disabled={unavailable} style={`--native-colour:${colour}`} data-proxy-id={id} />
      {/snippet}
    </Popover.Trigger>
    <Popover.Content class="compat-colour-popover" align="end" sideOffset={8}
      onkeydowncapture={(event) => {
        if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); open = false; }
      }}>
      <label for={`${id}-hex`}>Custom colour</label>
      <div class="compat-colour-fields">
        <Input id={`${id}-hex`} bind:value={hex} oninput={change} onchange={change} spellcheck="false" maxlength={7} aria-label="Hex colour" />
        <Button variant="outline" size="sm" onclick={choose}>Pick</Button>
      </div>
    </Popover.Content>
  </Popover.Root>
</span>

<style>
  .compat-colour { display: contents; }
  :global(.compat-colour-trigger) { width: 22px; height: 22px; min-width: 22px; padding: 0; border: 2px solid var(--bg); border-radius: 50%; background: var(--native-colour); box-shadow: 0 0 0 1px var(--border); }
  :global(.compat-colour-trigger:hover) { background: var(--native-colour); box-shadow: 0 0 0 2px var(--ring-color); }
  :global(.compat-colour-trigger:focus-visible) { outline: 2px solid var(--ring-color); outline-offset: 3px; }
  :global(.compat-colour-popover) { z-index: 120; width: min(300px, calc(100% - 24px)); border: 1px solid var(--border); border-radius: 16px; background: var(--bg); color: var(--text); padding: 12px; box-shadow: var(--shadow); }
  :global(.compat-colour-popover label) { font-size: var(--text-xs); color: var(--muted); }
  .compat-colour-fields { display: flex; gap: 8px; align-items: center; }
  .compat-colour-fields :global(input) { min-width: 0; }
  @media (prefers-reduced-motion: reduce) { :global(.compat-colour-popover) { animation: none; } }
</style>
