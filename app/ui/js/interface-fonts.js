// Interface appearance is independent of the user's data/badge font choices.
// Assets are bundled locally; switching never needs a provider or font CDN.
export const INTERFACE_FONT_DEFAULT = 'inter';
export const INTERFACE_FONTS = ['inter', 'plex', 'source', 'mono'];
const STORAGE_KEY = 'tagrex.interfaceFont';
let revision = 0;

export function interfaceFont() {
  try {
    const value = localStorage.getItem(STORAGE_KEY);
    return INTERFACE_FONTS.includes(value) ? value : INTERFACE_FONT_DEFAULT;
  } catch { return INTERFACE_FONT_DEFAULT; }
}

export function applyInterfaceFont(value) {
  const choice = INTERFACE_FONTS.includes(value) ? value : INTERFACE_FONT_DEFAULT;
  document.documentElement.dataset.uiFont = choice;
  try { localStorage.setItem(STORAGE_KEY, choice); } catch { /* appearance still works */ }
  const generation = ++revision;
  const notify = () => {
    if (generation === revision) document.dispatchEvent(new CustomEvent('tagrex:interfacefont'));
  };
  notify();
  // Re-measure text-based layouts once this face is available. A late response
  // from an earlier choice must not refresh the current one.
  if (document.fonts) {
    const family = { inter:'TagRex Inter', plex:'TagRex Sans', source:'TagRex Source Sans', mono:'TagRex Mono' }[choice];
    document.fonts.load(`400 13px "${family}"`).then(notify, notify);
  }
  return choice;
}

// Loaded before the Svelte roots, so the saved face applies on the first paint.
document.documentElement.dataset.uiFont = interfaceFont();
