<script lang="ts">
  import { untrack } from 'svelte';
  import * as Popover from '$lib/components/ui/popover/index.js';
  import { t } from '../js/i18n.js';
  import { nextControlId } from '$lib/control-id';
  let { native, getValues, getSeparator }: { native: HTMLInputElement; getValues: () => string[]; getSeparator: () => string } = $props();
  const listId = nextControlId('genre-options');
  const common = ['Acid House', 'Acid Jazz', 'Acoustic', 'Alternative', 'Ambient', 'Blues', 'Breakbeat', 'Breaks', 'Classical', 'Country', 'Dance', 'Dancehall', 'Deep House', 'Disco', 'Downtempo', 'Drum & Bass', 'Dub', 'Dubstep', 'Electronic', 'Electronica', 'Electro', 'Experimental', 'Folk', 'Funk', 'Garage', 'Gospel', 'Grime', 'Happy Hardcore', 'Hard House', 'Hard Trance', 'Hardcore', 'Hardstyle', 'Hip-Hop', 'House', 'IDM', 'Indie', 'Industrial', 'Instrumental', 'Jazz', 'Jungle', 'Latin', 'Metal', 'Minimal', 'New Age', 'New Wave', 'Opera', 'Pop', 'Pop/Rock', 'Progressive House', 'Progressive Trance', 'Psytrance', 'Punk', 'R&B', 'Reggae', 'Rock', 'Rock & Roll', 'Ska', 'Soul', 'Soundtrack', 'Spoken Word', 'Swing', 'Synthpop', 'Tech House', 'Techno', 'Trance', 'Trip-Hop', 'UK Garage', 'World'];
  const collator = new Intl.Collator(undefined, { sensitivity: 'base' });
  let open = $state(false);
  let disabled = $state(false);
  let options = $state.raw<string[]>([]);
  let shown = $state.raw<string[]>([]);
  let active = $state(-1);
  let content = $state<HTMLDivElement | null>(null);
  let disposed = false;
  let anchorTop = 0;
  let anchorLeft = 0;

  function token() {
    const value = native.value;
    const cursor = native.selectionStart ?? value.length;
    const endCursor = native.selectionEnd ?? cursor;
    const configured = getSeparator().trim();
    // A slash can be part of one genre (Pop/Rock), never an implicit delimiter.
    const delimiters = [...new Set([';', ...(configured && configured !== '/' ? [configured] : [])])];
    let start = 0, end = value.length;
    for (const delimiter of delimiters) {
      const before = value.slice(0, cursor).lastIndexOf(delimiter);
      if (before >= 0) start = Math.max(start, before + delimiter.length);
      const after = value.indexOf(delimiter, endCursor);
      if (after >= 0) end = Math.min(end, after);
    }
    return { value, start, end, needle: value.slice(start, end).trim().toLocaleLowerCase() };
  }
  function filter() {
    const needle = token().needle;
    const matches = options.filter(value => value.toLocaleLowerCase().includes(needle));
    shown = [...matches.filter(value => value.toLocaleLowerCase().startsWith(needle)), ...matches.filter(value => !value.toLocaleLowerCase().startsWith(needle))].slice(0, 100);
    active = -1;
    reflect();
  }
  function prepare() {
    const separator = getSeparator().trim();
    const split = separator && separator !== '/' && separator !== ';' ? new RegExp(`;|${separator.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}`) : /;/;
    const values = [...getValues().flatMap(value => value.split(split)), ...common];
    const unique = new Map<string, string>();
    for (const value of values) {
      const trimmed = value.trim();
      if (trimmed && !unique.has(trimmed.toLocaleLowerCase())) unique.set(trimmed.toLocaleLowerCase(), trimmed);
      if (unique.size >= 2048) break;
    }
    options = [...unique.values()].sort(collator.compare);
    filter();
  }
  function reflect() {
    native.setAttribute('aria-expanded', String(open));
    if (open && active >= 0) native.setAttribute('aria-activedescendant', `${listId}-${active}`);
    else native.removeAttribute('aria-activedescendant');
  }
  function setOpen(next: boolean) {
    if (disposed || native.disabled || !native.isConnected) next = false;
    if (next && !open) {
      const bounds = native.getBoundingClientRect();
      anchorTop = bounds.top;
      anchorLeft = bounds.left;
      prepare();
    }
    open = next;
    if (!next) active = -1;
    reflect();
  }
  function choose(value: string) {
    if (!open || native.disabled || !native.isConnected) return;
    const current = token();
    const prefix = current.value.slice(0, current.start);
    const suffix = current.value.slice(current.end);
    const leading = current.value.slice(current.start, current.end).match(/^\s*/)?.[0] || (prefix ? ' ' : '');
    const next = prefix + leading + value + suffix;
    native.value = next;
    native.dispatchEvent(new Event('input', { bubbles: true }));
    setOpen(false);
    native.focus({ preventScroll: true });
    const caret = prefix.length + leading.length + value.length;
    native.setSelectionRange(caret, caret);
  }
  function inputChanged() { if (open) filter(); else setOpen(true); }
  function key(event: KeyboardEvent) {
    if (event.isComposing || native.disabled) return;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault(); event.stopPropagation();
      if (!open) setOpen(true);
      const delta = event.key === 'ArrowDown' ? 1 : -1;
      active = shown.length ? (active < 0 ? (delta > 0 ? 0 : shown.length - 1) : (active + delta + shown.length) % shown.length) : -1;
      reflect();
      queueMicrotask(() => document.getElementById(`${listId}-${active}`)?.scrollIntoView({ block: 'nearest' }));
    } else if (event.key === 'Escape' && open) {
      event.preventDefault(); event.stopPropagation(); setOpen(false);
    } else if (event.key === 'Enter' && open && active >= 0) {
      event.preventDefault(); event.stopPropagation(); choose(shown[active]);
    } else if (event.key === 'Tab') setOpen(false);
  }
  function attachNative(host: HTMLSpanElement) {
    return untrack(() => {
    host.append(native);
    const attributes = ['role', 'aria-controls', 'aria-expanded', 'aria-autocomplete', 'aria-activedescendant'];
    const saved = new Map(attributes.map(name => [name, native.getAttribute(name)]));
    native.setAttribute('role', 'combobox');
    native.setAttribute('aria-controls', listId);
    native.setAttribute('aria-autocomplete', 'list');
    reflect();
    const state = () => { disabled = native.disabled; if (disabled || native.hidden) setOpen(false); };
    const observer = new MutationObserver(state);
    observer.observe(native, { attributes: true, attributeFilter: ['disabled', 'hidden'] });
    native.addEventListener('input', inputChanged);
    native.addEventListener('keydown', key, true);
    const click = () => { if (open) filter(); };
    native.addEventListener('click', click);
    state();
    return () => {
      disposed = true; observer.disconnect();
      native.removeEventListener('input', inputChanged); native.removeEventListener('keydown', key, true); native.removeEventListener('click', click);
      for (const [name, value] of saved) { if (value === null) native.removeAttribute(name); else native.setAttribute(name, value); }
    };
    });
  }
  function scroll(event: Event) {
    if (!open || content?.contains(event.target as Node)) return;
    // Focusing an off-screen field scrolls it before input/click, but its
    // queued scroll event can arrive after the popup opens. Only dismiss when
    // the anchor has actually moved since opening, not on that delayed event.
    const bounds = native.getBoundingClientRect();
    if (Math.abs(bounds.top - anchorTop) > .5 || Math.abs(bounds.left - anchorLeft) > .5) setOpen(false);
  }
</script>

<svelte:window onscrollcapture={scroll} onresize={() => setOpen(false)} />
<Popover.Root {open} onOpenChange={setOpen}>
  <span class="genre-combobox">
    <span class="genre-native" {@attach attachNative}></span>
    <Popover.Trigger class="genre-toggle" aria-label={t('field.genre')} {disabled} onpointerdown={(event) => event.preventDefault()} onclick={() => { if (!open) native.focus({ preventScroll: true }); }}>
      <svg aria-hidden="true" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><path d="m4 6 4 4 4-4"/></svg>
    </Popover.Trigger>
  </span>
  <Popover.Content bind:ref={content} customAnchor={native} class="genre-options" align="start" sideOffset={4} trapFocus={false} preventScroll={false} data-controller-popup
    onOpenAutoFocus={(event) => event.preventDefault()} onCloseAutoFocus={(event) => event.preventDefault()} onEscapeKeydown={(event) => { event.preventDefault(); event.stopPropagation(); setOpen(false); }}
    onInteractOutside={(event) => { if (event.target === native) event.preventDefault(); }}>
    <div id={listId} role="listbox" aria-label={t('field.genre')}>
      {#each shown as value, index (value)}
        <button type="button" role="option" id={`${listId}-${index}`} class={['genre-option', { active: active === index }]} aria-selected={active === index} tabindex={-1}
          onpointerdown={(event) => { event.preventDefault(); choose(value); }}>{value}</button>
      {/each}
      {#if !shown.length}<div class="genre-no-results" role="status">{t('field.genre')}: {token().value.slice(token().start, token().end).trim()}</div>{/if}
    </div>
  </Popover.Content>
</Popover.Root>
