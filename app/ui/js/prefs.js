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
const TABLE_FONT_DEFAULT = 10;
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
  }
}
// Follow OS changes only while in Auto.
prefersDarkMq.addEventListener("change", () => {
  if (themeMode() === "auto") {
    document.documentElement.dataset.theme = resolveTheme("auto");
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
const LANG_MODES = ["auto", "en", "uk", "ru"];
// The languages there are catalogues for. "auto" resolves into one of these.
const LANGUAGES = ["en", "uk", "ru"];

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
  groupByPref,
  saveGroupBy,
};
