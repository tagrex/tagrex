import { mount, unmount } from 'svelte';
import GenrePicker from '../components/GenrePicker.svelte';
import '../genre-picker.css';

/** Add suggestions without replacing the field or its existing staging path. */
export function attachGenrePicker(native: HTMLInputElement, getValues: () => string[], getSeparator: () => string = () => '; ') {
  const host = document.createElement('span');
  host.className = 'genre-picker-host';
  native.before(host);
  const component = mount(GenrePicker, { target: host, props: { native, getValues, getSeparator } });
  let disposed = false;
  const dispose = () => {
    if (disposed) return;
    disposed = true;
    observer.disconnect();
    void unmount(component);
  };
  // One constant-time lifetime check for the single Editor genre field. Moving
  // its module within the workspace retains both field and popup instance.
  const observer = new MutationObserver(() => { if (!host.isConnected) dispose(); });
  observer.observe(document, { childList: true, subtree: true });
  return dispose;
}
