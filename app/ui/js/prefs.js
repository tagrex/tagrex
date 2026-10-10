// Display preferences (#143 split them out of app.js).
//
// The choices that only change how the app looks — the value font, the table
// and tracklist sizes, the badge face, the theme, the optional checkbox column,
// the filter-mode flags and the grouping key. They live in localStorage rather
// than the backend's settings.json because they are per-machine view state, not
// library data, and they apply themselves by toggling classes and CSS
// variables on <body>.
import { filterRegex, filterCase } from "./state.js";

// Value-font preference: which face every value surface uses — the file table,
// the release tracklist, deduplicator paths, rename/export pattern fields and
// editor inputs. "mono" is the default disambiguating monospace; "sans" and
// "condensed" swap in the bundled UI faces app-wide (the stylesheet redefines
// --font-mono-bundled off a body class). Grew out of the #100 condensed-table
// toggle, which was table-only — the old boolean key migrates below. A pure
// display choice, so it persists in localStorage, not the backend settings.
const VALUE_FONT_STORAGE_KEY = "tagrex.valueFont";
const CONDENSED_STORAGE_KEY = "tagrex.condensedTable"; // legacy, migrated once
const VALUE_FONTS = ["mono", "sans", "condensed"];
function valueFont() {
  try {
    const v = localStorage.getItem(VALUE_FONT_STORAGE_KEY);
    if (VALUE_FONTS.includes(v)) return v;
    // Migrate the old table-only boolean: it only ever meant "condensed".
    if (localStorage.getItem(CONDENSED_STORAGE_KEY) === "1") return "condensed";
  } catch (e) {
    return "mono";
  }
  return "mono";
}
function applyValueFont(mode) {
  const m = VALUE_FONTS.includes(mode) ? mode : "mono";
  document.body.classList.toggle("value-font-sans", m === "sans");
  document.body.classList.toggle("value-font-condensed", m === "condensed");
  try {
    localStorage.setItem(VALUE_FONT_STORAGE_KEY, m);
    localStorage.removeItem(CONDENSED_STORAGE_KEY);
  } catch (e) {
    /* localStorage unavailable — preference just won't persist */
  }
}

// Show the selection-checkbox column (#99 redesign). Off by default — rows
// select on click (Cmd/Shift+click for range/toggle), so the checkboxes are an
// optional convenience rather than the primary affordance.
const CHECKBOX_COL_STORAGE_KEY = "tagrex.checkboxCol";
function checkboxColEnabled() {
  try {
    return localStorage.getItem(CHECKBOX_COL_STORAGE_KEY) === "1";
  } catch (e) {
    return false;
  }
}
function applyCheckboxCol(on) {
  document.body.classList.toggle("show-checkbox", on);
  try {
    localStorage.setItem(CHECKBOX_COL_STORAGE_KEY, on ? "1" : "0");
  } catch (e) {
    /* localStorage unavailable — preference just won't persist */
  }
}

// Filter mode prefs (#44): regex on/off and case sensitivity. Pure view choices,
// persisted like the other display prefs. Read once at startup, then flipped by
// the toolbar toggles.
const FILTER_REGEX_STORAGE_KEY = "tagrex.filterRegex";
const FILTER_CASE_STORAGE_KEY = "tagrex.filterCase";
function regexModeEnabled() {
  try {
    return localStorage.getItem(FILTER_REGEX_STORAGE_KEY) === "1";
  } catch (e) {
    return false;
  }
}
function caseSensitiveEnabled() {
  try {
    return localStorage.getItem(FILTER_CASE_STORAGE_KEY) === "1";
  } catch (e) {
    return false;
  }
}
function saveFilterMode() {
  try {
    localStorage.setItem(FILTER_REGEX_STORAGE_KEY, filterRegex ? "1" : "0");
    localStorage.setItem(FILTER_CASE_STORAGE_KEY, filterCase ? "1" : "0");
  } catch (e) {
    /* localStorage unavailable — preference just won't persist */
  }
}

// Table font size (#100), 10–20px, applied live to both the monospace and the
// condensed face through a CSS var. A pure display choice → localStorage.
const TABLE_FONT_STORAGE_KEY = "tagrex.tableFontPx";
const TABLE_FONT_MIN = 10;
const TABLE_FONT_MAX = 20;
// First-run default (#274): a touch above the 10px floor so the table is a
// little more legible out of the box while keeping its dense character. Anyone
// who wants tighter drags it back down in Settings › Display.
const TABLE_FONT_DEFAULT = 11;
function clampTableFont(px) {
  return Math.min(TABLE_FONT_MAX, Math.max(TABLE_FONT_MIN, px || TABLE_FONT_DEFAULT));
}
function tableFontPx() {
  try {
    const v = parseInt(localStorage.getItem(TABLE_FONT_STORAGE_KEY), 10);
    if (Number.isFinite(v)) return clampTableFont(v);
  } catch (e) {
    /* fall through to default */
  }
  return TABLE_FONT_DEFAULT;
}
function applyTableFont(px) {
  const v = clampTableFont(px);
  document.documentElement.style.setProperty("--table-font-size", `${v}px`);
  try {
    localStorage.setItem(TABLE_FONT_STORAGE_KEY, String(v));
  } catch (e) {
    /* localStorage unavailable — preference just won't persist */
  }
}

// ---- LAB typography knobs (Settings › LAB) ----
// Release-card tracklist size and badge face, on the same localStorage-only
// footing as the table-font control: pure display choices still being trialled.
const TRACKLIST_FONT_STORAGE_KEY = "tagrex.tracklistFontPx";
const TRACKLIST_FONT_MIN = 10;
const TRACKLIST_FONT_MAX = 16;
const TRACKLIST_FONT_DEFAULT = 12;
function clampTracklistFont(px) {
  return Math.min(TRACKLIST_FONT_MAX, Math.max(TRACKLIST_FONT_MIN, px || TRACKLIST_FONT_DEFAULT));
}
function tracklistFontPx() {
  try {
    const v = parseInt(localStorage.getItem(TRACKLIST_FONT_STORAGE_KEY), 10);
    if (Number.isFinite(v)) return clampTracklistFont(v);
  } catch (e) {
    /* fall through to the default */
  }
  return TRACKLIST_FONT_DEFAULT;
}
function applyTracklistFont(px) {
  const v = clampTracklistFont(px);
  document.documentElement.style.setProperty("--tracklist-font-size", `${v}px`);
  try {
    localStorage.setItem(TRACKLIST_FONT_STORAGE_KEY, String(v));
  } catch (e) {
    /* localStorage unavailable — preference just won't persist */
  }
}

// Which media-glyph family the Online cover badge draws (LAB experiment): the
// shipped hand-drawn set, or one of the permissively-licensed families in
// mediaglyphs.js. A display choice, so it lives in localStorage. The set keys
// are validated by online.js against MEDIA_GLYPH_SETS; an unknown value there
// falls back to "ours".
const MEDIA_GLYPH_SET_STORAGE_KEY = "tagrex.mediaGlyphSet";
const MEDIA_GLYPH_SET_DEFAULT = "ours";
function mediaGlyphSet() {
  try {
    return localStorage.getItem(MEDIA_GLYPH_SET_STORAGE_KEY) || MEDIA_GLYPH_SET_DEFAULT;
  } catch (e) {
    return MEDIA_GLYPH_SET_DEFAULT;
  }
}
function setMediaGlyphSet(name) {
  try {
    localStorage.setItem(MEDIA_GLYPH_SET_STORAGE_KEY, name);
  } catch (e) {
    /* localStorage unavailable — preference just won't persist */
  }
}

const BADGE_FONT_STORAGE_KEY = "tagrex.badgeFont";
const BADGE_FONTS = ["mono", "sans"];
function badgeFont() {
  try {
    const v = localStorage.getItem(BADGE_FONT_STORAGE_KEY);
    if (BADGE_FONTS.includes(v)) return v;
  } catch (e) {
    /* fall through to the default */
  }
  return "mono";
}
function applyBadgeFont(mode) {
  const m = BADGE_FONTS.includes(mode) ? mode : "mono";
  // The badge carries a catalogue number — an identifier — so mono is the
  // default; --badge-font lets LAB try the UI face instead. It governs the
  // whole badge: mixing faces inside one pill leaves the two halves at
  // different x-heights (#176).
  document.documentElement.style.setProperty(
    "--badge-font",
    m === "sans" ? "var(--font-ui)" : "var(--font-mono-bundled)",
  );
  try {
    localStorage.setItem(BADGE_FONT_STORAGE_KEY, m);
  } catch (e) {
    /* localStorage unavailable — preference just won't persist */
  }
}

// Accent colour (#431): the brand green by default, or one of a few presets or
// any colour the user picks. Only the base `--accent` is a free choice; the two
// companions are derived so a custom colour stays readable: `--accent-text` is
// the label on an accent fill (white, with the fill darkened until it clears
// WCAG AA against it) and `--accent-ink` is the accent used as text on the page
// surface (shifted darker in the light theme, lighter in the dark one). The
// focus ring, row selection and dirty tints already derive from `--accent` in
// the stylesheet, so they follow without being touched here. The semantic
// colours (--add, --del) are deliberately left alone. The override is set as an
// inline style on <html>, which wins over both theme palettes, and is recomputed
// whenever the resolved theme changes because the ink depends on the surface.
const ACCENT_STORAGE_KEY = "tagrex.accent";
// `hex: null` is the brand green — no override at all. The swatch colour shown
// for it is the stylesheet's light-theme fill.
const ACCENT_BRAND_SWATCH = "#0b6b53";
const ACCENT_PRESETS = [
  { id: "green", hex: null },
  { id: "teal", hex: "#0d9488" },
  { id: "blue", hex: "#2563eb" },
  { id: "indigo", hex: "#4f46e5" },
  { id: "violet", hex: "#7c3aed" },
  { id: "pink", hex: "#db2777" },
  { id: "orange", hex: "#ea580c" },
  { id: "amber", hex: "#d97706" },
  { id: "slate", hex: "#475569" },
];
const ACCENT_MIN_CONTRAST = 4.5;
// Least contrast a fill needs against the page surface to still read as a shape.
const ACCENT_MIN_FILL_CONTRAST = 1.5;

function normalizeHex(value) {
  const m = /^#?([0-9a-f]{6})$/i.exec(String(value || "").trim());
  return m ? `#${m[1].toLowerCase()}` : null;
}
function hexToRgb(hex) {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}
function rgbToHex([r, g, b]) {
  return `#${[r, g, b].map((c) => Math.round(c).toString(16).padStart(2, "0")).join("")}`;
}
function rgbToHsl([r, g, b]) {
  const rn = r / 255;
  const gn = g / 255;
  const bn = b / 255;
  const max = Math.max(rn, gn, bn);
  const min = Math.min(rn, gn, bn);
  const l = (max + min) / 2;
  if (max === min) return [0, 0, l];
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  let h;
  if (max === rn) h = (gn - bn) / d + (gn < bn ? 6 : 0);
  else if (max === gn) h = (bn - rn) / d + 2;
  else h = (rn - gn) / d + 4;
  return [h * 60, s, l];
}
function hslToRgb([h, s, l]) {
  const k = (n) => (n + h / 30) % 12;
  const a = s * Math.min(l, 1 - l);
  const f = (n) => l - a * Math.max(-1, Math.min(k(n) - 3, Math.min(9 - k(n), 1)));
  return [f(0) * 255, f(8) * 255, f(4) * 255];
}
function relativeLuminance(hex) {
  const [r, g, b] = hexToRgb(hex).map((c) => {
    const v = c / 255;
    return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}
function contrastRatio(a, b) {
  const la = relativeLuminance(a);
  const lb = relativeLuminance(b);
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}
// Walk the colour's lightness one percent at a time in `direction` (-1 darker,
// +1 lighter) until it clears `min` against `against`; the last step reached if
// it never does (a pure white or black is as far as it can go).
function shiftToContrast(hex, against, direction, min) {
  const [h, s, l] = rgbToHsl(hexToRgb(hex));
  let out = hex;
  for (let step = 0; step <= 100; step += 1) {
    const next = Math.min(1, Math.max(0, l + (direction * step) / 100));
    out = rgbToHex(hslToRgb([h, s, next]));
    if (contrastRatio(out, against) >= min) break;
  }
  return out;
}
// The three accent tokens for a chosen colour on a given surface.
function deriveAccent(hex, surface, dark) {
  const ink = shiftToContrast(hex, surface, dark ? 1 : -1, ACCENT_MIN_CONTRAST);
  const fill = shiftToContrast(hex, "#ffffff", -1, ACCENT_MIN_CONTRAST);
  // A near-black pick in the dark theme darkens to a fill that vanishes into the
  // surface; there the readable ink becomes the fill and takes a black or white
  // label, whichever reads better on it.
  if (contrastRatio(fill, surface) < ACCENT_MIN_FILL_CONTRAST) {
    const text = contrastRatio(ink, "#000000") >= contrastRatio(ink, "#ffffff") ? "#000000" : "#ffffff";
    return { accent: ink, text, ink };
  }
  return { accent: fill, text: "#ffffff", ink };
}
function accentColor() {
  try {
    return normalizeHex(localStorage.getItem(ACCENT_STORAGE_KEY));
  } catch (e) {
    return null;
  }
}
// Re-derive and stamp the accent for the current theme; the brand green (no
// stored colour) just clears the override so the stylesheet's own palette shows.
function applyAccent() {
  const root = document.documentElement;
  const hex = accentColor();
  if (!hex) {
    ["--accent", "--accent-text", "--accent-ink"].forEach((n) => root.style.removeProperty(n));
    return;
  }
  const dark = root.dataset.theme === "dark";
  const surface = normalizeHex(getComputedStyle(root).getPropertyValue("--bg")) || (dark ? "#16181d" : "#ffffff");
  const { accent, text, ink } = deriveAccent(hex, surface, dark);
  root.style.setProperty("--accent", accent);
  root.style.setProperty("--accent-text", text);
  root.style.setProperty("--accent-ink", ink);
}
// `null` goes back to the brand green.
function saveAccent(hex) {
  const value = normalizeHex(hex);
  try {
    if (value) localStorage.setItem(ACCENT_STORAGE_KEY, value);
    else localStorage.removeItem(ACCENT_STORAGE_KEY);
  } catch (e) {
    /* localStorage unavailable — preference just won't persist */
  }
  applyAccent();
}

// Theme: Auto (follow OS) / Light / Dark. "auto" resolves to light/dark from the
// OS preference and re-resolves when it changes; light/dark force a palette via
// a data-theme attribute the stylesheet keys off. Persisted in localStorage.
const THEME_STORAGE_KEY = "tagrex.theme";
const THEME_MODES = ["auto", "light", "dark"];
const prefersDarkMq = window.matchMedia("(prefers-color-scheme: dark)");
function themeMode() {
  try {
    const v = localStorage.getItem(THEME_STORAGE_KEY);
    return THEME_MODES.includes(v) ? v : "auto";
  } catch (e) {
    return "auto";
  }
}
function resolveTheme(mode) {
  if (mode === "light" || mode === "dark") return mode;
  return prefersDarkMq.matches ? "dark" : "light";
}
function applyTheme(mode) {
  document.documentElement.dataset.theme = resolveTheme(mode);
  try {
    localStorage.setItem(THEME_STORAGE_KEY, mode);
  } catch (e) {
    /* localStorage unavailable — preference just won't persist */
  }  applyAccent();
}
// Follow OS changes only while in Auto.
prefersDarkMq.addEventListener("change", () => {
  if (themeMode() === "auto") {
    document.documentElement.dataset.theme = resolveTheme("auto");
    applyAccent();
  }
});
// Apply as early as app.js runs, before the settings sheet is ever opened.
applyTheme(themeMode());

// Interface language (#50, #269): Auto (follow the OS) / English / Ukrainian /
// Russian. Shaped exactly like the theme above and for the same reason — it
// is a display preference that has to be resolved before the first paint, and
// a round trip to the backend for it would show English for a frame and then
// swap. `settings.json` stays out of it until something in the backend needs
// to know.
const LANG_STORAGE_KEY = "tagrex.lang";
const LANG_MODES = ["auto", "en", "de", "es", "fr", "it", "uk", "ru"];
// The languages there are catalogues for. "auto" resolves into one of these.
const LANGUAGES = ["en", "de", "es", "fr", "it", "uk", "ru"];

function langMode() {
  try {
    const saved = localStorage.getItem(LANG_STORAGE_KEY);
    return LANG_MODES.includes(saved) ? saved : "auto";
  } catch (e) {
    return "auto";
  }
}

// The catalogue a mode resolves to. Auto takes the browser/OS languages in
// order of preference and picks the first one there is a catalogue for —
// matching on the base tag, so `ru-RU` finds `ru`. English otherwise.
function resolveLang(mode) {
  if (LANGUAGES.includes(mode)) return mode;
  const preferred = navigator.languages && navigator.languages.length
    ? navigator.languages
    : [navigator.language || "en"];
  for (const tag of preferred) {
    const base = String(tag).toLowerCase().split("-")[0];
    if (LANGUAGES.includes(base)) return base;
  }
  return "en";
}

function saveLangMode(mode) {
  try {
    localStorage.setItem(LANG_STORAGE_KEY, mode);
  } catch (e) {
    /* localStorage unavailable — preference just won't persist */
  }
}

// Grouping is purely a view concern (#20): "" | "folder" | "artist" | "album".
// It regroups rows visually but never reorders the `tracks` array, so the file
// order used by mapping (rename masks, Discogs import) is unaffected. Collapsed
// group keys persist across renders. The choice is a display preference,
// persisted in localStorage and defaulting to Folder (#108).
const GROUP_STORAGE_KEY = "tagrex.groupBy";
function groupByPref() {
  try {
    const v = localStorage.getItem(GROUP_STORAGE_KEY);
    // Any stored string is accepted here; populateGroupMenu() validates it
    // against the built option list once EXTENDED_FIELDS is available (#43).
    return v === null ? "folder" : v;
  } catch (e) {
    return "folder";
  }
}
function saveGroupBy(value) {
  try {
    localStorage.setItem(GROUP_STORAGE_KEY, value);
  } catch (e) {
    /* localStorage unavailable — preference just won't persist */
  }
}

export {
  clampTableFont,
  clampTracklistFont,
  valueFont,
  applyValueFont,
  checkboxColEnabled,
  applyCheckboxCol,
  regexModeEnabled,
  caseSensitiveEnabled,
  saveFilterMode,
  tableFontPx,
  applyTableFont,
  TABLE_FONT_MIN,
  TABLE_FONT_MAX,
  tracklistFontPx,
  applyTracklistFont,
  TRACKLIST_FONT_MIN,
  TRACKLIST_FONT_MAX,
  mediaGlyphSet,
  setMediaGlyphSet,
  badgeFont,
  applyBadgeFont,
  BADGE_FONTS,
  VALUE_FONTS,
  THEME_MODES,
  themeMode,
  LANG_MODES,
  LANGUAGES,
  langMode,
  resolveLang,
  saveLangMode,
  applyTheme,
  resolveTheme,
  ACCENT_PRESETS,
  ACCENT_BRAND_SWATCH,
  accentColor,
  saveAccent,
  normalizeHex,
  groupByPref,
  saveGroupBy,
};
