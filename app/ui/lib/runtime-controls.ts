import { buttonVariants } from './components/ui/button/index.js';
import { enhanceSelect, releaseDetachedSelects } from './select-enhancer';
import { enhanceNativeControl, releaseDetachedNativeControls } from './native-controls-enhancer';

// File rows, provider cards and editor fields are produced by the existing
// workflow controllers. Share the same component recipes without moving or
// replacing their native nodes (and losing listeners, selection or focus).
export function initComponentControls() {
  const selector = 'button:not([data-slot]), input:not([data-slot]), input[type=checkbox]:not([data-native-proxy]), input[type=range]:not([data-native-proxy]), textarea:not([data-slot]), select';
  function enhance(node: Element) {
    if (node instanceof HTMLSelectElement) enhanceSelect(node);
    if (node instanceof HTMLInputElement) enhanceNativeControl(node);
    // Gesture canvases and artwork tiles have purpose-built hit areas, sizes
    // and pointer behavior. Their standard actions already use Button.
    if (node.matches('.aw-surface, .aw-thumb, .release-cover')) return;
    if (node instanceof HTMLButtonElement && !node.hasAttribute('data-slot')) {
      const icon = node.classList.contains('icon') || node.classList.contains('match-btn');
      const variant = node.classList.contains('primary') ? 'default'
        : node.classList.contains('danger') ? 'destructive'
        : icon || node.classList.contains('text-btn') || node.classList.contains('ctx-item') || node.classList.contains('seg-btn') ? 'ghost' : 'outline';
      node.dataset.slot = 'button';
      node.dataset.variant = variant;
      node.dataset.size = icon ? 'icon-sm' : 'sm';
      node.classList.add(...buttonVariants({ variant, size: icon ? 'icon-sm' : 'sm' }).split(' '));
    } else if (node instanceof HTMLInputElement && !node.hasAttribute('data-slot')) {
      node.dataset.slot = ['checkbox', 'radio', 'range', 'color', 'file'].includes(node.type) ? 'native-control' : 'input';
    } else if (node instanceof HTMLTextAreaElement && !node.hasAttribute('data-slot')) {
      node.dataset.slot = 'textarea';
    } else if (node instanceof HTMLSelectElement && !node.hasAttribute('data-slot')) {
      node.dataset.slot = 'native-select';
      node.classList.add('shadcn-native-select');
    }
  }
  function scan(node: Element) {
    if (node.matches(selector)) enhance(node);
    node.querySelectorAll(selector).forEach(enhance);
  }
  scan(document.body);
  const removed = new Set<Element>();
  let cleanupScheduled = false;
  const observer = new MutationObserver((records) => {
    const added = new Set<Element>();
    for (const record of records) {
      for (const node of record.addedNodes) if (node instanceof Element) added.add(node);
      for (const node of record.removedNodes) if (node instanceof Element) removed.add(node);
    }
    for (const node of added) {
      if (!node.isConnected) continue;
      let parent = node.parentElement;
      while (parent && !added.has(parent)) parent = parent.parentElement;
      if (!parent) scan(node);
    }
    if (!removed.size || cleanupScheduled) return;
    cleanupScheduled = true;
    queueMicrotask(() => {
      cleanupScheduled = false;
      // Limit disposal to removed subtrees. Text/readout updates and added
      // controls never walk the registry of the entire music library.
      releaseDetachedSelects(removed);
      releaseDetachedNativeControls(removed);
      removed.clear();
    });
  });
  observer.observe(document.body, { childList: true, subtree: true });
  window.addEventListener('pagehide', () => observer.disconnect(), { once: true });
}
