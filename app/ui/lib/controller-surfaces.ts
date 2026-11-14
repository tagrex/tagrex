import { flushSync, mount, unmount } from 'svelte';
import { upgradeComponentMarkup } from './component-elements.js';
import ControllerDialog from '../components/ControllerDialog.svelte';
import { toast as sonnerToast } from 'svelte-sonner';
import ControllerToast from '../components/ControllerToast.svelte';
import ControllerPopover from '../components/ControllerPopover.svelte';

/** Keep the imperative controllers' hidden contract while Bits owns presence/focus. */
export function connectControllerSurface(root: HTMLElement, update: (open: boolean) => void) {
  let hidden = root.hasAttribute('hidden');
  let closeTimer: ReturnType<typeof setTimeout> | undefined;
  root.dataset.controllerSurface = '';
  const setOpen = (open: boolean) => {
    if (hidden === !open) return;
    hidden = !open;
    clearTimeout(closeTimer);
    root.dataset.surfaceState = open ? 'open' : 'closed';
    if (open) root.removeAttribute('hidden');
    // Controllers often measure/focus immediately after assigning hidden=false.
    flushSync(() => update(open));
    if (!open) {
      const delay = matchMedia('(prefers-reduced-motion: reduce)').matches ? 0 : 140;
      closeTimer = setTimeout(() => root.setAttribute('hidden', ''), delay);
    }
  };
  update(!hidden);
  Object.defineProperty(root, 'hidden', {
    configurable: true,
    get: () => hidden,
    set: (value: boolean) => setOpen(!value),
  });
  return {
    setOpen,
    destroy() {
      clearTimeout(closeTimer);
      delete (root as unknown as Record<string, unknown>).hidden;
      root.toggleAttribute('hidden', hidden);
    },
  };
}

/** Existing controller nodes remain in the same root, including all listeners and IDs. */
export function retainControllerNodes(host: HTMLElement, nodes: Node[]) {
  host.append(...nodes);
}

type TooltipWriter = (anchor: HTMLElement, text: string, hint: string) => void;
let tooltipWriter: TooltipWriter | undefined;
export function registerControllerTooltip(writer: TooltipWriter) {
  tooltipWriter = writer;
  return () => { tooltipWriter = undefined; };
}
export function showControllerTooltip(anchor: HTMLElement, text: string, hint: string) {
  tooltipWriter?.(anchor, text, hint);
}

export function positionControllerToast(bar: DOMRect | null) {
  const style = document.documentElement.style;
  style.setProperty('--controller-toast-bottom', bar ? `${window.innerHeight - bar.top + 8}px` : '20px');
  style.setProperty('--controller-toast-left', bar ? `${bar.left + bar.width / 2}px` : '50%');
  style.setProperty('--controller-toast-width', bar ? `${Math.max(bar.width, 320)}px` : 'min(480px, calc(100vw - 32px))');
}

function initControllerToast() {
  const root = document.getElementById('toast');
  if (!root) return;
  const home = document.createElement('div');
  home.className = 'controller-toast-home';
  root.before(home);
  home.append(root);
  connectControllerSurface(root, (open) => {
    if (open) sonnerToast.custom(ControllerToast, {
      id: 'controller-report', componentProps: { node: root, home },
      duration: Infinity, dismissible: false, unstyled: true,
    });
    else sonnerToast.dismiss('controller-report');
  });
}

let initialized = false;
export function initControllerSurfaces() {
  if (initialized) return;
  initialized = true;
  initControllerToast();
  const settings = document.getElementById('settings');
  if (settings) {
    const sheet = settings.querySelector<HTMLElement>('.settings-sheet');
    const scrim = document.getElementById('settings-scrim');
    if (sheet) {
      sheet.removeAttribute('role');
      sheet.removeAttribute('aria-modal');
      if (scrim) scrim.style.display = 'none';
      const target = document.createElement('div');
      settings.append(target);
      mount(ControllerDialog, { target, props: {
        nativeRoot: settings,
        nodes: [sheet],
        label: sheet.getAttribute('aria-label') || 'Settings',
        closeId: 'settings-close',
        settings: true,
      } });
    }
  }
  // Static chrome plus menus created while editing/searching keep the exact
  // original roots as their controller-facing content containers.
  const mounted = new Map<HTMLElement, ReturnType<typeof mount>>();
  function enhance(root: HTMLElement) {
    if (mounted.has(root)) return;
    // Release save actions use ancestor-delegated events, so fresh rows can be
    // upgraded before the shell takes ownership without replacing listeners.
    if (root.classList.contains('tk-save-menu')) upgradeComponentMarkup(root);
    const nodes = Array.from(root.childNodes);
    const target = document.createElement('div');
    root.append(target);
    const component = mount(ControllerPopover, { target, props: {
      nativeRoot: root, nodes, bridgeContent: true,
      preserveFocus: root.classList.contains('sg-menu'),
    } });
    mounted.set(root, component);
  }
  for (const id of ['group-menu', 'columns-menu', 'presets-menu', 'health-menu', 'root-recent-menu', 'query-preset-menu']) {
    const root = document.getElementById(id);
    if (root) enhance(root);
  }
  const selector = '.sg-menu, .tk-save-menu';
  const observe = new MutationObserver((records) => {
    const added = new Set<HTMLElement>();
    const removed = new Set<HTMLElement>();
    for (const record of records) {
      for (const node of record.addedNodes) if (node instanceof HTMLElement) added.add(node);
      for (const node of record.removedNodes) if (node instanceof HTMLElement) removed.add(node);
    }
    for (const node of added) {
      if (!node.isConnected) continue;
      let parent = node.parentElement;
      while (parent && !added.has(parent)) parent = parent.parentElement;
      if (parent) continue;
      if (node.matches(selector)) enhance(node);
      node.querySelectorAll<HTMLElement>(selector).forEach(enhance);
    }
    const dispose = (root: HTMLElement) => {
      const component = mounted.get(root);
      if (!component || root.isConnected) return;
      mounted.delete(root);
      void unmount(component);
    };
    for (const node of removed) {
      if (node.isConnected) continue;
      dispose(node);
      node.querySelectorAll<HTMLElement>(selector).forEach(dispose);
    }
  });
  observe.observe(document.body, { childList: true, subtree: true });
  flushSync();
}
