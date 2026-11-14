import { mount, unmount } from 'svelte';
import NativeCheckbox from '../components/controls/NativeCheckbox.svelte';
import NativeSlider from '../components/controls/NativeSlider.svelte';
import { nextControlId } from './control-id';

type Control = { input: HTMLInputElement; host: HTMLSpanElement; component: ReturnType<typeof mount> };
const controls = new WeakMap<HTMLInputElement, Control>();
const hosts = new WeakMap<Element, Control>();
export function enhanceNativeControl(input: HTMLInputElement) {
  if (input.hasAttribute('data-slot') && input.dataset.slot !== 'native-control') return;
  if (!['checkbox', 'range'].includes(input.type) || input.hidden || input.hasAttribute('data-native-proxy') || controls.has(input) || input.closest('.compat-native, .compat-slider-wrap, .app-native-control')) return;
  const parent = input.parentElement;
  if (!parent) return;
  if (!input.id) input.id = nextControlId('native-control');
  const host = document.createElement('span');
  host.className = 'app-native-control';
  host.style.display = 'contents';
  parent.insertBefore(host, input.nextSibling);
  const component = input.type === 'range'
    ? mount(NativeSlider, { target: host, props: { id: input.id, input } })
    : mount(NativeCheckbox, { target: host, props: { id: input.id, input } });
  const entry = { input, host, component };
  controls.set(input, entry);
  hosts.set(host, entry);
}
export function releaseDetachedNativeControls(removed: Iterable<Element>) {
  const candidates = new Set<Control>();
  function collect(node: Element) {
    const entry = node instanceof HTMLInputElement ? controls.get(node) : hosts.get(node);
    if (entry) candidates.add(entry);
  }
  for (const root of removed) {
    if (root.isConnected) continue;
    collect(root);
    root.querySelectorAll('input, .app-native-control').forEach(collect);
  }
  for (const entry of candidates) {
    if (entry.input.isConnected && entry.host.isConnected) continue;
    controls.delete(entry.input);
    hosts.delete(entry.host);
    void unmount(entry.component);
    entry.host.remove();
  }
}
