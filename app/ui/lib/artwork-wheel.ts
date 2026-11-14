type WheelHandler = (event: WheelEvent) => void;
const documents = new WeakMap<Document, Set<WheelHandler>>();

// Install at startup, before opening compositor-backed dialogs. In the native
// WebKitGTK app, popup-time registration missed physical wheel input even
// though synthetic tests passed. Keep the listener early; subscribe viewers later.
export function initArtworkWheel(document: Document) {
  const existing = documents.get(document);
  if (existing) return existing;
  const handlers = new Set<WheelHandler>();
  documents.set(document, handlers);
  document.addEventListener('wheel', (event) => {
    for (const handler of handlers) handler(event);
  }, { capture: true, passive: false });
  return handlers;
}

export function subscribeArtworkWheel(document: Document, handler: WheelHandler) {
  const handlers = initArtworkWheel(document);
  handlers.add(handler);
  return () => { handlers.delete(handler); };
}
