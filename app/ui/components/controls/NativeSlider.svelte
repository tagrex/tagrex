<script lang="ts">
  import { untrack } from 'svelte';
  import { Slider } from '$lib/components/ui/slider/index.js';
  import type { HTMLInputAttributes } from 'svelte/elements';
  import { bindNativeControl, dispatchNativeChange } from './native-proxy';

  let { id, value = 0, min = 0, max = 100, step = 1, disabled = false, input, class: className = '', ...rest }: Omit<HTMLInputAttributes, 'type' | 'children'> & {
    id: string; input?: HTMLInputElement;
  } = $props();
  let current = $state(untrack(() => Number(value)));
  let minimum = $state(untrack(() => Number(min)));
  let maximum = $state(untrack(() => Number(max)));
  let increment = $state(untrack(() => Number(step)));
  let unavailable = $state(untrack(() => Boolean(disabled)));
  let proxy: HTMLInputElement;
  let sliding = false;
  function attach(node: HTMLSpanElement) {
    proxy = input || node.querySelector('input')!;
    const thumb = () => node.querySelector<HTMLElement>('[role="slider"]');
    return bindNativeControl(proxy, ['value', 'min', 'max', 'step', 'disabled'], () => {
      current = Number(proxy.value);
      minimum = Number(proxy.min || 0);
      maximum = Number(proxy.max || 100);
      increment = Number(proxy.step || 1);
      unavailable = proxy.disabled;
    }, thumb);
  }
  function change(next: number) {
    if (!proxy) return;
    if (next === Number(proxy.value)) return;
    proxy.value = String(next);
    sliding = true;
    dispatchNativeChange(proxy, false);
  }
  function complete(next: number) {
    if (!proxy) return;
    if (!sliding) return;
    proxy.value = String(next);
    sliding = false;
    proxy.dispatchEvent(new Event('change', { bubbles: true }));
  }
</script>

<span class="compat-slider-wrap" {@attach attach}>
  {#if !input}<input {id} type="range" {value} {min} {max} {step} {disabled} class={className} {...rest} hidden aria-hidden="true" tabindex="-1" data-native-proxy />{/if}
  <Slider id={`${id}-control`} type="single" value={current} min={minimum} max={maximum} step={increment} disabled={unavailable}
    onValueChange={change} onValueCommit={complete} class="compat-slider" data-proxy-id={id} />
</span>

<style>
  .compat-slider-wrap { display: inline-flex; flex: 1; min-width: 70px; align-items: center; }
  .compat-slider-wrap :global(.compat-slider) { min-width: 70px; min-height: 24px; padding: 0 6px; width: 100%; }
  .compat-slider-wrap :global([data-slot='slider-track']) { height: 4px; border-radius: 999px; background: var(--border); }
  .compat-slider-wrap :global([data-slot='slider-range']) { background: var(--accent); }
  .compat-slider-wrap :global([data-slot='slider-thumb']) { width: 13px; height: 13px; border: 2px solid var(--accent); border-radius: 50%; background: var(--bg); transition: box-shadow 120ms; }
  .compat-slider-wrap :global([data-slot='slider-thumb']:focus-visible) { outline: 2px solid var(--ring-color); outline-offset: 3px; }
  @media (prefers-reduced-motion: reduce) { .compat-slider-wrap :global([data-slot='slider-thumb']) { transition: none; } }
</style>
