<script lang="ts">
  import { untrack } from 'svelte';
  import { nextControlId } from '$lib/control-id';
  import * as Select from '$lib/components/ui/select/index.js';

  let { native }: { native: HTMLSelectElement } = $props();
  type Choice = { token: string; label: string; description: string; disabled: boolean; hidden: boolean; option: HTMLOptionElement };
  let choices = $state.raw<Choice[]>([]);
  let token = $state('');
  let disabled = $state(false);
  let name = $state('');
  let title = $state('');
  let trigger = $state<HTMLButtonElement | null>(null);
  let open = $state(false);
  let optionNodes: HTMLOptionElement[] = [];
  let optionGeneration = 0;
  let labels: HTMLLabelElement[] = [];
  const selected = $derived(choices.find(choice => choice.token === token)?.label || '');
  const triggerId = untrack(() => `${native.id || nextControlId('select')}-control`);

  function optionsChanged(options: HTMLOptionElement[]) {
    return options.length !== optionNodes.length || options.some((option, index) => option !== optionNodes[index]);
  }
  function labelText(label: HTMLLabelElement) {
    const copy = label.cloneNode(true) as HTMLLabelElement;
    copy.querySelectorAll('.app-select, select, input, button, textarea').forEach(node => node.remove());
    return copy.textContent?.trim() || '';
  }
  function synchronize() {
    // Native setters may run inside another component's binding effect. Keep
    // this imperative bridge from subscribing that effect to the state it sets.
    untrack(() => {
      const options = Array.from(native.options);
      if (optionsChanged(options)) {
        // Tokens are valid only for this option order. A controller replacing the
        // list must close the old menu before any stale item can be activated.
        open = false;
        optionNodes = options;
        optionGeneration++;
      }
      const nextChoices = options.map((option, index) => ({ option, token: `option-${optionGeneration}-${index}`, label: option.textContent || option.value, description: option.dataset.description || '', disabled: option.disabled || (option.parentElement instanceof HTMLOptGroupElement && option.parentElement.disabled), hidden: option.hidden }));
      choices = nextChoices;
      token = nextChoices[native.selectedIndex]?.token || '';
      disabled = native.disabled;
      name = native.getAttribute('aria-label') || labels.map(labelText).join(' ').trim();
      title = native.title;
      const wrapper = native.closest<HTMLElement>('.app-select');
      if (wrapper) wrapper.hidden = native.hidden;
      if (native.hidden || native.disabled) open = false;
    });
  }
  function choose(next: string) {
    untrack(() => {
      const options = Array.from(native.options);
      if (optionsChanged(options)) { synchronize(); return; }
      const choice = choices.find(choice => choice.token === next);
      const index = choice ? options.indexOf(choice.option) : -1;
      if (index < 0 || native.hidden || native.disabled || choice?.disabled || choice?.hidden || native.selectedIndex === index) return;
      native.selectedIndex = index;
      native.dispatchEvent(new Event('input', { bubbles: true }));
      native.dispatchEvent(new Event('change', { bubbles: true }));
      synchronize();
    });
  }
  function dismiss(event: KeyboardEvent) {
    if (event.key !== 'Escape' || !open) return;
    event.preventDefault(); event.stopPropagation(); open = false;
  }
  function attachProxy(host: HTMLSpanElement) {
    host.append(native);
    const originalTabIndex = native.getAttribute('tabindex');
    const originalAria = native.getAttribute('aria-hidden');
    native.tabIndex = -1;
    native.setAttribute('aria-hidden', 'true');
    native.dataset.selectProxy = 'true';
    const saved = new Map<string, PropertyDescriptor | undefined>();
    for (const key of ['value', 'selectedIndex', 'disabled', 'hidden']) {
      const descriptor = Object.getOwnPropertyDescriptor(key === 'hidden' ? HTMLElement.prototype : HTMLSelectElement.prototype, key);
      if (!descriptor?.get || !descriptor.set) continue;
      saved.set(key, Object.getOwnPropertyDescriptor(native, key));
      Object.defineProperty(native, key, { configurable: true, get: () => descriptor.get!.call(native), set: value => { descriptor.set!.call(native, value); synchronize(); } });
    }
    const focus = native.focus;
    native.focus = options => trigger?.focus(options);
    labels = [...native.labels || []];
    const labelTargets = labels.map(label => ({ label, previous: label.htmlFor }));
    // Explicitly target the visible button for both for= labels and wrapping
    // labels, so clicking their text opens the same accessible select.
    const activateLabel = (event: MouseEvent) => {
      const target = event.target;
      if (!(target instanceof Element) || target.closest('button, input, select, textarea, a') || native.disabled || native.hidden) return;
      // Bits opens on pointer/keyboard down; a label's browser activation only
      // sends click. Open it directly for label text and avoid a second click.
      event.preventDefault();
      trigger?.focus();
      open = true;
    };
    labelTargets.forEach(({ label }) => {
      label.htmlFor = triggerId;
      label.addEventListener('click', activateLabel);
    });
    const observer = new MutationObserver(synchronize);
    observer.observe(native, { attributes: true, childList: true, subtree: true, characterData: true });
    native.addEventListener('change', synchronize);
    const reset = () => queueMicrotask(synchronize);
    native.form?.addEventListener('reset', reset);
    synchronize();
    return () => {
      observer.disconnect(); native.removeEventListener('change', synchronize); native.form?.removeEventListener('reset', reset);
      native.focus = focus;
      for (const [key, descriptor] of saved) { if (descriptor) Object.defineProperty(native, key, descriptor); else Reflect.deleteProperty(native, key); }
      labelTargets.forEach(({ label, previous }) => {
        label.htmlFor = previous;
        label.removeEventListener('click', activateLabel);
      });
      if (originalTabIndex === null) native.removeAttribute('tabindex'); else native.setAttribute('tabindex', originalTabIndex);
      if (originalAria === null) native.removeAttribute('aria-hidden'); else native.setAttribute('aria-hidden', originalAria);
      delete native.dataset.selectProxy;
    };
  }
</script>

<span class="app-select-proxy" {@attach attachProxy}></span>
<Select.Root type="single" bind:open value={token} onValueChange={choose} {disabled}>
  <Select.Trigger bind:ref={trigger} id={triggerId} aria-label={name || undefined} {title} class="app-select-trigger" data-proxy-id={native.id} onkeydowncapture={dismiss}>
    <span class="app-select-label">{selected}</span>
  </Select.Trigger>
  <Select.Content class="app-select-menu" data-controller-popup align="start" collisionPadding={8} onkeydowncapture={dismiss}>
    {#each choices.filter(choice => !choice.hidden) as choice (choice.token)}
      <Select.Item value={choice.token} label={choice.label} disabled={choice.disabled}>
          <span class="app-select-choice"><span>{choice.label}</span>{#if choice.description}<span class="app-select-description">{choice.description}</span>{/if}</span>
      </Select.Item>
    {/each}
  </Select.Content>
</Select.Root>
