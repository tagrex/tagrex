import { mount, unmount } from 'svelte';
import { Button, buttonVariants } from './components/ui/button/index.js';
import { Input } from './components/ui/input/index.js';
import { Textarea } from './components/ui/textarea/index.js';

const mounted = new WeakMap();
const textInputTypes = new Set(["text", "search", "number", "email", "url", "password", "tel", "date", "time", "datetime-local", "month", "week"]);
let observing = false;

function observeLifetime() {
  if (observing) return;
  observing = true;
  const pending = new Set();
  let scheduled = false;
  function collect(node) {
    if (!(node instanceof Element) || node.isConnected) return;
    if (mounted.has(node)) pending.add(node);
    for (const element of node.querySelectorAll('[data-component-element]')) {
      if (mounted.has(element)) pending.add(element);
    }
  }
  const observer = new MutationObserver(records => {
    for (const record of records) {
      for (const node of record.removedNodes) collect(node);
    }
    if (!pending.size || scheduled) return;
    scheduled = true;
    queueMicrotask(() => {
      scheduled = false;
      for (const element of pending) {
        // Reparenting in the same turn must not destroy a live component.
        if (element.isConnected) continue;
        const record = mounted.get(element);
        if (!record) continue;
        mounted.delete(element);
        void unmount(record.component);
      }
      pending.clear();
    });
  });
  observer.observe(document, { childList: true, subtree: true });
  window.addEventListener('pagehide', () => observer.disconnect(), { once: true });
}

/** Create a shadcn text field/button before legacy callers attach listeners.
 * Non-text input types are native proxies for their dedicated control bridge;
 * pass their type at creation rather than changing a mounted Input later.
 * No flushSync is used, so calls are also safe from Svelte change callbacks.
 * The native node may be reparented and have its content/attributes updated by
 * its workflow controller; its component is disposed only after disconnection.
 * Components must render one stable native root. Do not pass managed children
 * or add conditional root markup: controllers own this node and its content.
 * @param {'button'|'input'|'textarea'} tag
 * @param {Record<string, unknown>} [props]
 */
export function createComponentElement(tag, props = {}) {
  // A checkbox/range is rendered by its dedicated bridge after insertion.
  // Mounting Input first would give its hidden proxy a second Svelte owner.
  if (tag === 'input' && props.type && !textInputTypes.has(String(props.type))) {
    const input = document.createElement('input');
    for (const [key, value] of Object.entries(props)) {
      if (key === 'class') input.className = String(value);
      else if (key.startsWith('aria-') || key.startsWith('data-')) input.setAttribute(key, String(value));
      else input[key] = value;
    }
    return input;
  }
  const Component = tag === 'button' ? Button : tag === 'input' ? Input : Textarea;
  const host = document.createElement('span');
  const component = mount(Component, {
    target: host,
    props: tag === 'button' ? { variant: 'outline', size: 'sm', ...props } : props,
  });
  const element = host.querySelector(tag);
  if (!element || host.children.length !== 1 || host.firstElementChild !== element) {
    void unmount(component);
    throw new Error(`The ${tag} component must create one stable native root synchronously`);
  }
  element.dataset.componentElement = tag;

  // Controllers commonly assign className after creation. Retain the mounted
  // component's recipe while accepting their workflow-specific class names.
  const classDescriptor = Object.getOwnPropertyDescriptor(Element.prototype, 'className');
  const originalClasses = element.className.split(/\s+/).filter(Boolean);
  if (classDescriptor?.get && classDescriptor.set) {
    Object.defineProperty(element, 'className', {
      configurable: true,
      get() { return classDescriptor.get.call(element); },
      set(value) {
        const classes = String(value).split(/\s+/).filter(Boolean);
        if (tag === 'button') {
          const variant = classes.includes('primary') ? 'default' : classes.includes('danger') ? 'destructive'
            : classes.some(name => ['icon', 'text-btn', 'seg-btn', 'ctx-item'].includes(name)) ? 'ghost' : 'outline';
          element.dataset.variant = variant;
          classDescriptor.set.call(element, [...new Set([...buttonVariants({ variant, size: props.size || 'sm' }).split(' '), ...classes])].join(' '));
        } else {
          classDescriptor.set.call(element, [...new Set([...originalClasses, ...classes])].join(' '));
        }
      },
    });
  }
  mounted.set(element, { component, host });
  observeLifetime();
  return element;
}

/** Upgrade freshly parsed markup before binding any per-node listeners.
 * Delegated listeners on ancestor containers remain untouched.
 * @param {Element} root
 * @param {{ exclude?: string }} [options]
 */
export function upgradeComponentMarkup(root, { exclude = '' } = {}) {
  for (const original of root.querySelectorAll('button, input, textarea')) {
    if (original instanceof HTMLInputElement && !textInputTypes.has(original.type)) continue;
    if ((exclude && original.matches(exclude)) || mounted.has(original) || original.hasAttribute('data-slot') || original.hasAttribute('data-native-proxy')) continue;
    const replacement = createComponentElement(original.tagName.toLowerCase());
    for (const attribute of original.attributes) {
      if (attribute.name === 'class') replacement.className = attribute.value;
      else replacement.setAttribute(attribute.name, attribute.value);
    }
    if (original instanceof HTMLInputElement) {
      replacement.value = original.value;
      replacement.checked = original.checked;
      replacement.disabled = original.disabled;
    } else if (original instanceof HTMLTextAreaElement) {
      replacement.defaultValue = original.defaultValue;
      replacement.value = original.value;
    } else {
      replacement.type = original.type;
      replacement.disabled = original.disabled;
      replacement.innerHTML = original.innerHTML;
    }
    original.replaceWith(replacement);
  }
}
