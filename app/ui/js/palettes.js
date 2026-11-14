// Complete paired themes; the accent belongs to the chosen preset.
export const PALETTE_DEFAULT = "navy";
export const PALETTES = [
  {
    "id": "navy",
    "name": "Navy",
    "accent": "#5b8cff",
    "lightAccent": "#245fce"
  },
  {
    "id": "graphite",
    "name": "Graphite",
    "accent": "#6993f7",
    "lightAccent": "#2563eb"
  },
  {
    "id": "cupertino",
    "name": "Cupertino",
    "accent": "#0a84ff",
    "lightAccent": "#007aff"
  },
  {
    "id": "catppuccin",
    "name": "Catppuccin",
    "accent": "#cba6f7",
    "lightAccent": "#8839ef"
  },
  {
    "id": "nord",
    "name": "Nord",
    "accent": "#88c0d0",
    "lightAccent": "#5e81ac"
  },
  {
    "id": "tokyonight",
    "name": "Tokyo Night",
    "accent": "#7aa2f7",
    "lightAccent": "#2e7de9"
  },
  {
    "id": "plum",
    "name": "Rosé Pine",
    "accent": "#c4a7e7",
    "lightAccent": "#907aa9"
  },
  {
    "id": "gruvbox",
    "name": "Gruvbox",
    "accent": "#fabd2f",
    "lightAccent": "#b57614"
  },
  {
    "id": "solarized",
    "name": "Solarized",
    "accent": "#2aa198",
    "lightAccent": "#2aa198"
  },
  {
    "id": "forest",
    "name": "Everforest",
    "accent": "#a7c080",
    "lightAccent": "#8da101"
  }
];
export const DEFAULT_ACCENT = PALETTES[0].accent;
export function normalizePalette(value) {
  return PALETTES.some(palette => palette.id === value) ? value : PALETTE_DEFAULT;
}
export function paletteAccent(value, mode = 'dark') {
  const palette = PALETTES.find(palette => palette.id === normalizePalette(value));
  return mode === 'light' ? palette.lightAccent : palette.accent;
}
