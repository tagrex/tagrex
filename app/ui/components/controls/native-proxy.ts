// One delegated reset listener per document instead of one per virtual row.
// Registration follows the control lifetime; an empty registry releases it.
const resetRegistries = new WeakMap<Document, Map<HTMLInputElement, () => void>>();
function subscribeFormReset(input: HTMLInputElement, synchronize: () => void) {
  const document = input.ownerDocument;
  let subscribers = resetRegistries.get(document);
  if (!subscribers) {
    subscribers = new Map();
    resetRegistries.set(document, subscribers);
    document.addEventListener('reset', onFormReset, true);
  }
  subscribers.set(input, synchronize);
  return () => {
    const current = resetRegistries.get(document);
    current?.delete(input);
    if (!current?.size) {
      document.removeEventListener('reset', onFormReset, true);
      resetRegistries.delete(document);
    }
  };
}
function onFormReset(event: Event) {
  const document = event.currentTarget as Document;
  queueMicrotask(() => {
    for (const [input, synchronize] of resetRegistries.get(document) || []) {
      if (input.form === event.target) synchronize();
    }
  });
}

/** Keep controller-owned native state and an accessible component in sync.
 * Property assignments intentionally do not emit user events. */
export function bindNativeControl(
  input: HTMLInputElement,
  properties: string[],
  synchronize: () => void,
  focusTarget: () => HTMLElement | null,
) {
  const previousAttributes = ['hidden', 'aria-hidden', 'tabindex', 'data-native-proxy'].map(name => [name, input.getAttribute(name)] as const);
  input.hidden = true;
  input.setAttribute('aria-hidden', 'true');
  input.tabIndex = -1;
  input.setAttribute('data-native-proxy', '');
  const saved = new Map<string, PropertyDescriptor | undefined>();
  const inputPrototype = HTMLInputElement.prototype;
  for (const key of properties) {
    const descriptor = Object.getOwnPropertyDescriptor(inputPrototype, key);
    if (!descriptor?.get || !descriptor.set) continue;
    saved.set(key, Object.getOwnPropertyDescriptor(input, key));
    Object.defineProperty(input, key, {
      configurable: true,
      enumerable: descriptor.enumerable,
      get: () => descriptor.get!.call(input),
      set: (value) => { descriptor.set!.call(input, value); synchronize(); },
    });
  }
  const previousFocus = Object.getOwnPropertyDescriptor(input, 'focus');
  Object.defineProperty(input, 'focus', {
    configurable: true,
    value: (options?: FocusOptions) => focusTarget()?.focus(options),
  });
  const labels = [...input.labels || []];
  const labelRestores = labels.map((label, index) => {
    const previousId = label.id;
    if (!label.id) label.id = `${input.id}-label-${index}`;
    const previousFor = label.htmlFor;
    const click = () => focusTarget()?.focus();
    // Leave implicit wrapping labels alone: their native activation still
    // reaches the proxy. Explicit labels target the visible control instead.
    if (label.htmlFor === input.id) label.htmlFor = `${input.id}-control`;
    label.addEventListener('click', click);
    return () => {
      label.htmlFor = previousFor;
      label.removeEventListener('click', click);
      if (!previousId) label.removeAttribute('id');
    };
  });
  const synchronizeWithLabel = () => {
    synchronize();
    const visible = focusTarget();
    if (!visible) return;
    if (input.getAttribute('aria-label')) visible.setAttribute('aria-label', input.getAttribute('aria-label')!);
    else if (labels.length) visible.setAttribute('aria-labelledby', labels.map(label => label.id).join(' '));
    if (input.title) visible.title = input.title;
  };
  const observer = new MutationObserver(synchronizeWithLabel);
  observer.observe(input, { attributes: true, attributeFilter: ['checked', 'disabled', 'indeterminate', 'value', 'min', 'max', 'step', 'title', 'aria-label'] });
  input.addEventListener('input', synchronizeWithLabel);
  input.addEventListener('change', synchronizeWithLabel);
  input.addEventListener('click', synchronizeWithLabel);
  const releaseReset = subscribeFormReset(input, synchronizeWithLabel);
  synchronizeWithLabel();
  return () => {
    observer.disconnect();
    input.removeEventListener('input', synchronizeWithLabel);
    input.removeEventListener('change', synchronizeWithLabel);
    input.removeEventListener('click', synchronizeWithLabel);
    releaseReset();
    for (const restore of labelRestores) restore();
    for (const [key, descriptor] of saved) {
      if (descriptor) Object.defineProperty(input, key, descriptor);
      else Reflect.deleteProperty(input, key);
    }
    for (const [name, value] of previousAttributes) {
      if (value === null) input.removeAttribute(name);
      else input.setAttribute(name, value);
    }
    if (previousFocus) Object.defineProperty(input, 'focus', previousFocus);
    else Reflect.deleteProperty(input, 'focus');
  };
}

export function dispatchNativeChange(input: HTMLInputElement, complete = true) {
  input.dispatchEvent(new Event('input', { bubbles: true }));
  if (complete) input.dispatchEvent(new Event('change', { bubbles: true }));
}
