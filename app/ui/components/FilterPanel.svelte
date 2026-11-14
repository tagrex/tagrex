<script lang="ts">
  import type { Snippet } from 'svelte';
  import * as Collapsible from '$lib/components/ui/collapsible/index.js';
  import { Button } from '$lib/components/ui/button/index.js';
  import '../filter-panel.css';
  import FilterConditions from './FilterConditions.svelte';
  import { conditionLabel } from '../js/filter-query.js';

  let { children }: { children: Snippet } = $props();
  const storageKey = 'tagrex.filterPanelOpen';
  function savedOpen() {
    try { return localStorage.getItem(storageKey) === '1'; } catch { return false; }
  }
  let open = $state(savedOpen());
  let summary = $state('');
  let active = $derived(Boolean(summary));
  let conditionSummary = '';
  let advancedSummary = '';

  function changeOpen(value: boolean) {
    open = value;
    if (!value) {
      const presets = document.getElementById('presets-menu');
      if (presets) presets.hidden = true;
    }
    try { localStorage.setItem(storageKey, value ? '1' : '0'); } catch { /* Optional preference. */ }
  }
  function clear() {
    document.dispatchEvent(new CustomEvent('tagrex:filter-conditions', { detail: { conditions: [], mode: 'all' } }));
    const input = document.getElementById('filter') as HTMLInputElement | null;
    if (input?.value) {
      input.value = '';
      input.dispatchEvent(new Event('input', { bubbles: true }));
    }
    if (!document.getElementById('filter-check')?.hidden) document.getElementById('filter-check-clear')?.click();
    requestAnimationFrame(() => {
      if (open) input?.focus();
      else document.getElementById('filter-panel-toggle')?.focus();
    });
  }
  function attachPanel(root: HTMLElement) {
    const paint = () => {
      const query = (root.querySelector<HTMLInputElement>('#filter')?.value || '').trim();
      const check = root.querySelector<HTMLElement>('#filter-check');
      const health = check && !check.hidden ? root.querySelector('#filter-check-name')?.textContent?.trim() : '';
      summary = [query, conditionSummary, health, advancedSummary].filter(Boolean).join(' · ');
      placePresets();
    };
    const focus = () => {
      // A shortcut can be invoked while the compact Inspector pane is showing.
      const library = document.getElementById('pane-library');
      if (library?.getAttribute('aria-selected') === 'false') library.click();
      changeOpen(true);
      requestAnimationFrame(() => requestAnimationFrame(() => {
        const input = root.querySelector<HTMLInputElement>('#filter');
        input?.focus();
        input?.select();
      }));
    };
    function placePresets() {
      const menu = root.querySelector<HTMLElement>('#presets-menu');
      const button = root.querySelector<HTMLElement>('#presets-btn');
      if (!menu || menu.hidden || !button) return;
      const anchor = button.getBoundingClientRect();
      if (!menu.classList.contains('floating')) menu.classList.add('floating');
      menu.style.top = 'auto';
      menu.style.bottom = `${window.innerHeight - anchor.top + 4}px`;
      menu.style.maxHeight = `${Math.max(80, anchor.top - 12)}px`;
      menu.style.width = '';
      const width = menu.offsetWidth;
      let left = Math.max(8, Math.min(anchor.left, innerWidth - width - 8));
      // On a narrow dock the preset row wraps below the disclosure. Keep that
      // disclosure clickable while the menu opens upward beside it.
      const toggle = root.querySelector('#filter-panel-toggle')?.getBoundingClientRect();
      const bottom = anchor.top - 4;
      if (toggle && bottom - menu.offsetHeight < toggle.bottom && bottom > toggle.top &&
          left < toggle.right && left + width > toggle.left && innerWidth - toggle.right - 14 >= 190) {
        menu.style.width = `${Math.min(width, innerWidth - toggle.right - 14)}px`;
        left = toggle.right + 6;
      }
      menu.style.left = `${left}px`;
    }
    const clicked = (event: MouseEvent) => {
      if ((event.target as Element).closest('#presets-btn')) requestAnimationFrame(placePresets);
    };
    const stateChanged = (event: Event) => {
      const state = (event as CustomEvent).detail;
      conditionSummary = state.conditions.map(conditionLabel).join(state.mode === 'any' ? ' OR ' : ' · ');
      advancedSummary = [state.error ? 'Invalid filter; last valid results' : '', state.regex ? 'Regex' : '', state.caseSensitive ? 'Case-sensitive' : ''].filter(Boolean).join(' · ');
      paint();
    };
    document.addEventListener('tagrex:filter-state', stateChanged);
    const column = root.closest<HTMLElement>('.files-col');
    const size = new ResizeObserver(() => {
      column?.style.setProperty('--filter-dock-height', `${root.getBoundingClientRect().height}px`);
    });
    size.observe(root);
    const observer = new MutationObserver(paint);
    observer.observe(root, { attributes: true, attributeFilter: ['class', 'hidden', 'aria-pressed'], childList: true, characterData: true, subtree: true });
    root.addEventListener('input', paint);
    root.addEventListener('click', clicked, true);
    window.addEventListener('resize', placePresets);
    document.addEventListener('tagrex:focus-filter', focus);
    paint();
    return () => {
      observer.disconnect();
      size.disconnect();
      column?.style.removeProperty('--filter-dock-height');
      document.removeEventListener('tagrex:filter-state', stateChanged);
      root.removeEventListener('input', paint);
      root.removeEventListener('click', clicked, true);
      window.removeEventListener('resize', placePresets);
      document.removeEventListener('tagrex:focus-filter', focus);
    };
  }
</script>

<div id="filter-panel" class={active ? 'filter-panel has-filter' : 'filter-panel'} {@attach attachPanel}>
  <Collapsible.Root {open} onOpenChange={changeOpen}>
    <div class="filter-panel-head">
      <Collapsible.Trigger>
        {#snippet child({ props })}
          <Button {...props} id="filter-panel-toggle" variant="ghost" size="sm" class="filter-panel-toggle">
            <svg class="ico filter-panel-icon" viewBox="0 0 16 16" aria-hidden="true"><path d="M2 3h12L9 8.5V13l-2-1V8.5Z"/></svg>
            <span>Filter</span>
            <svg class="ico filter-panel-caret" aria-hidden="true"><use href="#i-caret-down"/></svg>
            {#if active}<span class="filter-panel-active" aria-hidden="true"></span>{/if}
          </Button>
        {/snippet}
      </Collapsible.Trigger>
      <span id="filter-panel-summary" class="filter-panel-summary" title={summary}>{summary}</span>
      {#if active}<Button id="filter-panel-clear" variant="ghost" size="sm" class="filter-panel-clear" onclick={clear} aria-label="Clear all filters">Clear</Button>{/if}
    </div>
    <Collapsible.Content id="filter-panel-content" forceMount class="filter-panel-content" aria-hidden={!open} inert={!open}>
      <div class="filter-panel-inner">{@render children()}<FilterConditions /></div>
    </Collapsible.Content>
  </Collapsible.Root>
</div>
