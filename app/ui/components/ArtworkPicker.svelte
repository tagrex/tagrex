<script lang="ts">
  import { untrack } from 'svelte';
  import * as Dialog from '$lib/components/ui/dialog/index.js';
  import * as ContextMenu from '$lib/components/ui/context-menu/index.js';
  import { Button } from '$lib/components/ui/button/index.js';
  import { prefersReducedMotion } from 'svelte/motion';
  import { fade } from 'svelte/transition';
  import { linear } from 'svelte/easing';
  import ArtworkCanvas from './ArtworkCanvas.svelte';
  import { decodeArtwork, rotateArtwork } from './artwork';
  import { t } from '../js/i18n.js';
  import type { ArtworkPickerOptions, ArtworkChoice, CoverArt } from './artwork';

  let { title, images, links = [], openLink, selected = 0, rotations: initialRotations = {}, fetchImage, onfinish, returnFocus }: ArtworkPickerOptions & {
    onfinish: (result: ArtworkChoice | null) => void;
    returnFocus: HTMLElement | null;
  } = $props();

  let open = $state(true);
  let active = $state(untrack(() => Math.max(0, Math.min(selected, images.length - 1))));
  type DisplayedArtwork = ArtworkChoice & { width: number; height: number; serial: number };
  let displayed = $state.raw<DisplayedArtwork | null>(null);
  let frames = $state.raw<DisplayedArtwork[]>([]);
  let thumbs = $state.raw<Record<number, string>>({});
  let loading = $state(true);
  let failure = $state('');
  let saving = $state(false);
  let rotations = $state<Record<number, number>>(untrack(() => ({ ...initialRotations })));
  let viewer = $state<ArtworkCanvas>();
  let rail: HTMLDivElement;
  let result: ArtworkChoice | null = null;
  let generation = 0;
  let disposed = false;
  // Promise cache is deliberately nonreactive; templates read thumbs/displayed.
  const pending = new Map<number, Promise<CoverArt>>();
  // Decodes that have started but not settled, by image index. Rapid browsing
  // asks for the same image again before its first decode has finished; the
  // second selection waits on the decode already running instead of starting
  // another. Only in-flight work is remembered — once it settles, the browser
  // owns the pixels and the entry goes.
  const decoding: Record<number, Promise<HTMLImageElement>> = {};
  // The pixels of the image currently on show, so committing it does not decode
  // the same artwork a second time.
  let displayedImage: HTMLImageElement | null = null;
  const dataUri = (cover: CoverArt) => `data:${cover.mime};base64,${cover.data_base64}`;
  const ready = $derived(!loading && !failure && displayed?.index === active);
  const dimensions = $derived.by(() => {
    const width = displayed?.index === active ? displayed.width : images[active]?.width;
    const height = displayed?.index === active ? displayed.height : images[active]?.height;
    const swapped = Math.abs(Math.round((rotations[active] || 0) / 90)) % 2;
    return width && height ? (swapped ? `${height} × ${width}` : `${width} × ${height}`) : '';
  });
  const imageLabel = (index: number) => t('artwork.image', { number: index + 1, total: images.length });

  function load(index: number) {
    if (!pending.has(index)) pending.set(index, Promise.resolve().then(() => fetchImage(images[index].url))
      .then((cover) => {
        if (!disposed) thumbs = { ...thumbs, [index]: dataUri(cover) };
        return cover;
      }).catch((error) => { pending.delete(index); throw error; }));
    return pending.get(index)!;
  }

  async function select(index: number) {
    // The image is already on show, decoded and enabled: re-selecting it (the
    // active tile, Home/End on the current index, an arrow wrapping a one-image
    // release) has nothing to fetch and nothing to decode.
    if (active === index && displayed?.index === index && !loading && !failure) return;
    const gen = ++generation;
    active = index;
    loading = true;
    failure = '';
    try {
      const cover = await load(index);
      // A fetch can outlive the selection that asked for it. Decoding is the
      // expensive half, so stop as soon as the answer can no longer be used;
      // the post-decode check still covers a close or supersede during the
      // decode itself.
      if (disposed || gen !== generation || !open) return;
      const image = await decodeOnce(index, cover);
      if (disposed || gen !== generation || !open) return;
      displayed = { index, cover, serial: gen, width: image.naturalWidth, height: image.naturalHeight };
      displayedImage = image;
      frames = [...frames, displayed];
      loading = false;
    } catch (error) {
      if (disposed || gen !== generation || !open) return;
      failure = String(error);
      loading = false;
    }
  }

  function decodeOnce(index: number, cover: CoverArt): Promise<HTMLImageElement> {
    const running = decoding[index];
    if (running) return running;
    const started = decodeArtwork(cover);
    decoding[index] = started;
    const settle = () => { if (decoding[index] === started) delete decoding[index]; };
    started.then(settle, settle);
    return started;
  }

  function close(choice: ArtworkChoice | null = null) {
    result = choice;
    open = false;
    generation++;
  }

  async function useImage(index = active) {
    // A context-menu choice fetches its own image, never the previous preview.
    if (!ready || displayed?.index !== index) await select(index);
    if (open && ready && displayed) await commit(displayed);
  }

  async function commit(choice: ArtworkChoice) {
    if (saving) return;
    saving = true;
    const gen = generation;
    try {
      // The image on show has already been decoded once for the preview; the
      // rotation uses those pixels instead of decoding the base64 a second time.
      const pixels = choice === displayed ? displayedImage : null;
      const cover = await rotateArtwork(choice.cover, rotations[choice.index] || 0, pixels);
      if (open && gen === generation) close({ index: choice.index, cover, rotation: ((rotations[choice.index] || 0) % 360 + 360) % 360 });
    } catch (error) {
      failure = String(error);
    } finally { saving = false; }
  }

  function navigateTo(index: number) {
    select(index);
    const tile = rail?.querySelector<HTMLButtonElement>(`[data-index="${index}"]`);
    tile?.focus({ preventScroll: true });
    tile?.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: prefersReducedMotion.current ? 'instant' : 'smooth' });
  }

  function onKey(event: KeyboardEvent) {
    // Menus own their arrow keys and Escape independently of the viewer.
    if (document.querySelector('[data-context-menu-content][data-state="open"]')) return;
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
      event.preventDefault();
      navigateTo((active + (event.key === 'ArrowRight' ? 1 : -1) + images.length) % images.length);
    } else if (event.key === 'Home' || event.key === 'End') {
      event.preventDefault();
      navigateTo(event.key === 'Home' ? 0 : images.length - 1);
    }
  }

  function focusFirst(event: Event) {
    event.preventDefault();
    requestAnimationFrame(() => {
      if (open) rail?.querySelector<HTMLButtonElement>(`[data-index="${active}"]`)?.focus();
    });
  }

  function attachRail(node: HTMLDivElement) {
    rail = node;
    untrack(() => select(active));
    // Only fetch visible thumbnails, with a small bounded background pool.
    const queue: number[] = [];
    let workers = 0;
    async function drain() {
      if (disposed || workers >= 3 || !queue.length) return;
      workers++;
      try { await load(queue.shift()!); } catch { /* Selecting the tile offers Retry. */ }
      workers--;
      drain();
    }
    const observer = new IntersectionObserver((entries) => {
      for (const entry of entries) if (entry.isIntersecting) {
        observer.unobserve(entry.target);
        queue.push(Number((entry.target as HTMLElement).dataset.index));
      }
      for (let i = 0; i < 3; i++) drain();
    }, { root: rail, rootMargin: '0px 60px' });
    rail.querySelectorAll('[data-index]').forEach((node) => observer.observe(node));
    return () => { disposed = true; generation++; observer.disconnect(); };
  }
</script>

{#snippet icon(name: string)}
  <svg aria-hidden="true" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><use href={`#i-${name}`} /></svg>
{/snippet}

{#snippet menuItems(index: number, preview = false)}
  <ContextMenu.Item class="aw-menu-item" onSelect={() => useImage(index)}>{t('artwork.use')}</ContextMenu.Item>
  {#if !preview}
    <ContextMenu.Item class="aw-menu-item" onSelect={() => select(index)}>{t('artwork.preview')}</ContextMenu.Item>
  {:else}
    <ContextMenu.Item class="aw-menu-item" disabled={!ready} onSelect={() => viewer?.actualSize()}>{t('artwork.actualSize')}</ContextMenu.Item>
    <ContextMenu.Item class="aw-menu-item" disabled={!ready} onSelect={() => viewer?.reset()}>{t('artwork.reset')}</ContextMenu.Item>
    <ContextMenu.Item class="aw-menu-item" disabled={!ready} onSelect={() => viewer?.rotate(-1)}>{t('artwork.rotateLeft')}</ContextMenu.Item>
    <ContextMenu.Item class="aw-menu-item" disabled={!ready} onSelect={() => viewer?.rotate(1)}>{t('artwork.rotateRight')}</ContextMenu.Item>
  {/if}
{/snippet}

<Dialog.Root bind:open onOpenChangeComplete={(isOpen) => { if (!isOpen) onfinish(result); }}>
    <Dialog.Content class="aw-window translate-x-0 translate-y-0" overlayProps={{ class: 'modal-backdrop artwork-backdrop' }} showCloseButton={false} data-artwork-ui onkeydown={onKey}
      onOpenAutoFocus={focusFirst}
      onCloseAutoFocus={(event) => { event.preventDefault(); returnFocus?.isConnected && returnFocus.focus(); }}>
      <header class="aw-header">
        <div class="aw-heading">
          <Dialog.Title class="aw-title">{t('artwork.title')}</Dialog.Title>
          <Dialog.Description class="aw-release">
            {#if links.length}
              <nav aria-label={t('artwork.releaseDetails')}>
                <ol class="aw-breadcrumbs">
                  {#each links as link, index (link)}
                    <li>
                      {#if index}<span class="aw-crumb-separator" aria-hidden="true">›</span>{/if}
                      <a href={link.url} title={link.name} onclick={(event) => { event.preventDefault(); openLink?.(link); }}>{link.name}</a>
                    </li>
                  {/each}
                </ol>
              </nav>
            {:else}{title}{/if}
          </Dialog.Description>
        </div>
        <Dialog.Close>
          {#snippet child({ props })}
            <Button {...props} variant="ghost" size="icon-sm" class="aw-close" aria-label={t('action.close')}>{@render icon('close')}</Button>
          {/snippet}
        </Dialog.Close>
      </header>

      <ContextMenu.Root>
        <ContextMenu.Trigger class="aw-stage" aria-label={t('artwork.preview')}>
          <div class="aw-layers">
          {#each frames as artwork (artwork.serial)}
              <div class="aw-frame" style:z-index={artwork.serial}
                onintroend={() => { frames = frames.filter((frame) => frame.serial >= artwork.serial); }}
                inert={loading || artwork.serial !== displayed?.serial} aria-hidden={loading || artwork.serial !== displayed?.serial} in:fade={{ duration: prefersReducedMotion.current ? 0 : 140, easing: linear }}>
                <ArtworkCanvas bind:this={viewer} src={dataUri(artwork.cover)} width={artwork.width} height={artwork.height}
                  label={imageLabel(artwork.index)} rotation={rotations[artwork.index] || 0}
                  onrotate={(angle) => { rotations[artwork.index] = angle; }} />
              </div>
          {/each}
          </div>
          {#if loading}
            <div class="aw-loading" role="status" aria-label={t('artwork.loading')}><span class="aw-spinner"></span></div>
          {:else if failure}
            <div class="aw-error" role="alert"><p>{t('artwork.failed')}</p><Button variant="outline" onclick={() => select(active)}>{t('artwork.retry')}</Button></div>
          {/if}
        </ContextMenu.Trigger>
        <ContextMenu.Content class="aw-menu" data-artwork-ui>{@render menuItems(active, true)}</ContextMenu.Content>
      </ContextMenu.Root>

      <div class="aw-rail" {@attach attachRail} role="group" aria-label={t('artwork.title')}>
        {#each images as image, index (image)}
          <ContextMenu.Root>
            <ContextMenu.Trigger>
              {#snippet child({ props })}
                <button {...props} type="button" class="aw-thumb" data-index={index} tabindex={active === index ? 0 : -1} aria-label={imageLabel(index)}
                  aria-pressed={active === index} onclick={() => select(index)}>
                  {#if thumbs[index]}<img src={thumbs[index]} alt="" draggable="false" />{:else}<span class="aw-thumb-placeholder">{@render icon('image')}</span>{/if}
                  <span class="aw-thumb-number">{index + 1}</span>
                  {#if active === index}<span class="aw-thumb-check">{@render icon('check')}</span>{/if}
                </button>
              {/snippet}
            </ContextMenu.Trigger>
            <ContextMenu.Content class="aw-menu" data-artwork-ui>{@render menuItems(index)}</ContextMenu.Content>
          </ContextMenu.Root>
        {/each}
      </div>

      <footer class="aw-footer">
        <div class="aw-readout" aria-live="polite" aria-atomic="true"><span>{imageLabel(active)}</span>{#if dimensions}<span class="aw-dimensions">{dimensions}</span>{/if}</div>
        <span class="aw-hint">{t('artwork.browseHint')}</span>
        <Button class="aw-use" disabled={!ready || saving} onclick={() => { if (ready && displayed) commit(displayed); }}>{t('artwork.use')}{@render icon('check')}</Button>
      </footer>
    </Dialog.Content>
</Dialog.Root>

<style>
  :global(.artwork-backdrop) { z-index: 90; background: rgba(0, 0, 0, .42); backdrop-filter: blur(6px); animation: aw-scrim-in 240ms ease both; }
  :global(.artwork-backdrop[data-state='closed']) { animation: aw-scrim-out 180ms ease both; }
  :global(.aw-window) { position: fixed; z-index: 91; top: 50%; left: 50%; translate: none; transform: translate(-50%, -50%); display: grid; gap: 0; grid-template-rows: auto minmax(0, 1fr) auto auto; width: min(960px, calc(100vw - 48px)); max-width: none; height: min(820px, calc(100dvh - 48px)); border: 1px solid var(--border); border-radius: var(--r-dialog); background: var(--bg); color: var(--text); box-shadow: 0 24px 100px rgba(0, 0, 0, .3); padding: 0; outline: none; overflow: hidden; animation: aw-open 280ms cubic-bezier(.2,.8,.2,1) both; }
  :global(.aw-window[data-state='closed']) { animation: aw-close 180ms cubic-bezier(.4,0,1,1) both; pointer-events: none; }
  .aw-header { display: flex; align-items: center; gap: 16px; padding: var(--sp-4) var(--sp-5); }
  .aw-heading { flex: 1; min-width: 0; }
  :global(.aw-title) { margin: 0 0 4px; color: var(--text); font-size: var(--type-dialog-size); font-weight: var(--fw-strong); letter-spacing: 0; }
  :global(.aw-release) { margin: 0; color: var(--muted); font-size: var(--type-help-size); overflow: hidden; overflow-wrap: anywhere; line-height: 1.6; }
  :global(.aw-close) { display: grid; place-items: center; flex: none; width: var(--ctl-h); height: var(--ctl-h); padding: 0; border: 0; border-radius: var(--r-compact); background: transparent; color: var(--muted); box-shadow: none; transition: background 120ms, color 120ms; }
  :global(.aw-close:hover) { background: var(--row-hover); color: var(--text); }
  svg { width: 16px; height: 16px; flex: none; }
  :global(.aw-stage) { position: relative; min-height: 0; margin: 0 16px; overflow: hidden; isolation: isolate; border-radius: var(--r-inset); background: color-mix(in srgb, var(--text) 3%, var(--bg)); }
  .aw-layers { position: absolute; inset: 0; isolation: isolate; }
  /* Fade the incoming opaque canvas over the preserved previous composition.
     No additive blending or simultaneous fade-out: neither dark dip nor lift.
     Completed intros release the older frames, including during rapid browsing. */
  .aw-frame { position: absolute; inset: 0; background: color-mix(in srgb, var(--text) 3%, var(--bg)); }
  :global(.aw-release) a { color: inherit; text-decoration: none; border-bottom: 1px solid transparent; }
  :global(.aw-release) a:hover { color: var(--accent-ink); border-bottom-color: currentColor; }
  .aw-breadcrumbs { display: flex; align-items: center; gap: 0; list-style: none; margin: 0; padding: 0; min-width: 0; }
  .aw-breadcrumbs li { display: flex; align-items: center; min-width: 0; flex: 0 1 auto; }
  .aw-breadcrumbs li:not(:last-child) { max-width: 28%; }
  .aw-breadcrumbs li:last-child { flex: 1; }
  .aw-breadcrumbs a { display: block; min-width: 0; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .aw-crumb-separator { flex: none; margin: 0 7px; opacity: .6; }
  .aw-loading, .aw-error { position: absolute; inset: 0; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 12px; pointer-events: none; }
  .aw-loading { animation: aw-scrim-in 100ms 140ms both; }
  .aw-error { padding: 16px; background: color-mix(in srgb, var(--bg) 90%, transparent); text-align: center; }
  .aw-error p { margin: 0; }
  .aw-error :global(button) { pointer-events: auto; border-radius: var(--r-pill); padding: var(--sp-2) var(--sp-4); }
  .aw-spinner { width: 22px; height: 22px; border: 2px solid var(--border); border-top-color: var(--accent); border-radius: 50%; animation: aw-spin 650ms linear infinite; }
  .aw-rail { display: flex; gap: var(--sp-2); padding: var(--sp-4) var(--sp-5) var(--sp-3); overflow-x: auto; overflow-y: hidden; overscroll-behavior-x: contain; scrollbar-width: thin; }
  .aw-thumb { position: relative; width: 68px; height: 68px; flex: 0 0 68px; display: grid; place-items: center; overflow: hidden; padding: 3px; border: 1px solid var(--border); border-radius: var(--r-control); background: color-mix(in srgb, var(--text) 3%, var(--bg)); opacity: .68; transition: opacity 160ms, border-color 160ms, box-shadow 180ms, transform 180ms cubic-bezier(.2,.8,.2,1); }
  .aw-thumb:hover { opacity: 1; transform: translateY(-2px); }
  .aw-thumb[aria-pressed='true'] { opacity: 1; border-color: var(--accent); box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 32%, transparent); }
  .aw-thumb img { width: 100%; height: 100%; min-height: 0; object-fit: contain; border-radius: calc(var(--r-control) - 4px); animation: aw-scrim-in 180ms ease; }
  .aw-thumb-number { position: absolute; left: 4px; bottom: 3px; min-width: 15px; padding: 0 3px; border-radius: 4px; font-size: 10px; line-height: 16px; background: var(--bg); color: var(--text); }
  .aw-thumb-check { position: absolute; right: 3px; top: 3px; display: grid; place-items: center; width: 16px; height: 16px; border-radius: 5px; background: var(--accent); color: var(--accent-text); }
  .aw-thumb-check svg { width: 12px; height: 12px; }
  .aw-thumb-placeholder { color: var(--muted); }
  .aw-footer { display: flex; align-items: center; gap: 16px; min-width: 0; padding: var(--sp-1) var(--sp-5) var(--sp-4); }
  .aw-readout { overflow-wrap: anywhere; display: flex; flex-direction: column; gap: 2px; font-size: var(--type-help-size); flex: 1; min-width: 0; }
  .aw-dimensions { color: var(--muted); font-size: var(--type-meta-size); }
  .aw-hint { color: var(--muted); font-size: var(--type-help-size); }
  :global(.aw-use) { display: inline-flex; align-items: center; justify-content: center; gap: 16px; flex: none; min-height: 44px; padding: 0 18px; border: 1px solid var(--accent); border-radius: var(--r-pill); background: var(--accent); color: var(--accent-text); font-weight: var(--fw-strong); box-shadow: none; transition: filter 140ms, transform 140ms, opacity 140ms; }
  :global(.aw-use:hover:not(:disabled)) { background: var(--accent); filter: brightness(1.08); }
  :global(.aw-use:active:not(:disabled)) { transform: scale(.97); }
  :global(.aw-use:disabled) { opacity: .4; }
  :global(.aw-menu) { z-index: 100; min-width: 180px; padding: 7px; border: 1px solid var(--border); border-radius: var(--r-menu); background: var(--bg); color: var(--text); box-shadow: 0 10px 35px rgba(0, 0, 0, .22); outline: none; animation: aw-menu-in 140ms ease both; }
  :global(.aw-menu-item) { min-height: var(--ctl-h); line-height: 1.5; padding: var(--sp-1) var(--sp-3); border-radius: var(--r-menu-item); font-size: var(--type-body-size); outline: none; cursor: default; }
  :global(.aw-menu-item[data-highlighted]) { background: var(--sel-tint); color: var(--accent-ink); }
  :global(.aw-menu-item[data-disabled]) { opacity: .4; }
  @keyframes aw-open { from { opacity: 0; transform: translate(-50%, calc(-50% + 14px)) scale(.97); } to { opacity: 1; transform: translate(-50%, -50%) scale(1); } }
  @keyframes aw-close { to { opacity: 0; transform: translate(-50%, calc(-50% + 8px)) scale(.985); } }
  @keyframes aw-scrim-in { from { opacity: 0; } to { opacity: 1; } }
  @keyframes aw-scrim-out { to { opacity: 0; } }
  @keyframes aw-menu-in { from { opacity: 0; transform: translateY(-3px) scale(.98); } }
  @keyframes aw-spin { to { transform: rotate(360deg); } }
  @media (max-width: 600px) {
    :global(.aw-window) { width: calc(100vw - 24px); height: calc(100dvh - 24px); border-radius: var(--r-dialog-compact); }
    .aw-header { padding: var(--sp-3) var(--sp-4); }
    :global(.aw-stage) { margin: 0 var(--sp-3); }
    .aw-rail { padding: 12px 16px; }
    .aw-footer { padding: 0 var(--sp-4) var(--sp-3); gap: var(--sp-2); }
    .aw-hint { display: none; }
  }
  @media (max-height: 520px) {
    :global(.aw-window) { height: calc(100dvh - 20px); }
    .aw-header { padding: var(--sp-2) var(--sp-4); }
    .aw-rail { padding-top: 8px; padding-bottom: 8px; gap: 8px; }
    .aw-thumb { width: 48px; height: 48px; flex-basis: 48px; }
    .aw-footer { padding-bottom: var(--sp-2); }
    .aw-hint { display: none; }
  }
  @media (prefers-reduced-motion: reduce) {
    :global(.aw-window), :global(.artwork-backdrop), :global(.aw-menu), .aw-thumb img { animation-duration: 1ms !important; }
    .aw-thumb, :global(.aw-use) { transition: none; }
    .aw-spinner, .aw-loading { animation: none; }
  }
</style>
