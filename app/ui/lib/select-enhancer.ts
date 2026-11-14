import { mount, unmount } from 'svelte';
import AnimatedSelect from '../components/AnimatedSelect.svelte';

type SelectControl = { native: HTMLSelectElement; component: ReturnType<typeof mount>; wrapper: HTMLSpanElement };
const mounted = new WeakMap<HTMLSelectElement, SelectControl>();
const wrappers = new WeakMap<Element, SelectControl>();

export function enhanceSelect(native: HTMLSelectElement) {
  if (mounted.has(native) || native.multiple || native.size > 1 || native.closest('[data-native-select-owner]')) return;
  const parent = native.parentElement;
  if (!parent) return;
  const wrapper = document.createElement('span');
  wrapper.className = 'app-select';
  wrapper.dataset.nativeSelectOwner = 'true';
  // Stylesheets size the wrapper in its layout context. Preserve deliberate
  // inline dimensions without freezing a computed pixel width at mount time.
  wrapper.style.width = native.style.width;
  wrapper.style.flex = native.style.flex;
  wrapper.style.alignSelf = native.style.alignSelf;
  parent.insertBefore(wrapper, native);
  wrapper.append(native);
  const component = mount(AnimatedSelect, { target: wrapper, props: { native } });
  const entry = { native, component, wrapper };
  mounted.set(native, entry);
  wrappers.set(wrapper, entry);
}

export function releaseDetachedSelects(removed: Iterable<Element>) {
  const candidates = new Set<SelectControl>();
  function collect(node: Element) {
    const entry = node instanceof HTMLSelectElement ? mounted.get(node) : wrappers.get(node);
    if (entry) candidates.add(entry);
  }
  for (const root of removed) {
    collect(root);
    root.querySelectorAll('select, [data-native-select-owner]').forEach(collect);
  }
  for (const entry of candidates) {
    if (entry.native.isConnected && entry.wrapper.isConnected) continue;
    mounted.delete(entry.native);
    wrappers.delete(entry.wrapper);
    void unmount(entry.component);
    entry.wrapper.remove();
  }
}
