// Interface strings (#50).
//
// No build step: `en` is the source of truth and every other language falls
// back to it key by key, so a partial translation shows English where it has
// nothing rather than a blank or a raw identifier. A key that is in no
// catalogue renders as itself — visible on screen, which is how a typo gets
// found.
//
// The language is resolved once, at module load, from the saved preference
// (see `prefs.js`, where it sits beside the theme for the same reason: both
// have to be settled before the first paint). Changing it re-renders the
// static text in place; anything a panel builds at runtime picks the new
// language up the next time it renders.
import { en } from "./i18n/en.js";
import { ru } from "./i18n/ru.js";
import { uk } from "./i18n/uk.js";
import { langMode, resolveLang, saveLangMode } from "./prefs.js";

const CATALOGUES = { en, ru, uk };

let lang = resolveLang(langMode());

// One rule set per language, kept because the plural category has to be asked
// of the language the *chosen string* is written in — not the one the user
// picked. With Russian selected and an English string falling back, Russian
// rules would call 21 `one` and produce "21 track".
const PLURAL_RULES = {};
function rulesFor(code) {
  if (!PLURAL_RULES[code]) PLURAL_RULES[code] = new Intl.PluralRules(code);
  return PLURAL_RULES[code];
}

// `{name}` → the matching var. A placeholder with nothing to fill it is left
// alone rather than blanked: seeing `{tracks}` on screen says which call is
// missing an argument, an empty gap says nothing.
function fill(template, vars) {
  if (!vars) return template;
  return template.replace(/\{(\w+)\}/g, (whole, name) =>
    Object.prototype.hasOwnProperty.call(vars, name) ? String(vars[name]) : whole
  );
}

// The catalogue entry for `key` in the active language, else in English —
// together with the language it actually came from, which is what decides the
// plural rules applied to it.
function entry(key) {
  const active = CATALOGUES[lang];
  if (active && Object.prototype.hasOwnProperty.call(active, key)) {
    return { value: active[key], lang };
  }
  if (Object.prototype.hasOwnProperty.call(en, key)) return { value: en[key], lang: "en" };
  return null;
}

// A translated string. `vars` fills `{name}` placeholders.
export function t(key, vars) {
  const found = entry(key);
  if (found === null) return key;
  const { value } = found;
  // A plural entry asked for without a count: take the form that reads as a
  // bare noun rather than returning "[object Object]".
  if (typeof value === "object") return fill(value.other || value.one || key, vars);
  return fill(value, vars);
}

// A translated string whose wording depends on `n` — "1 track" / "2 tracks",
// "1 трек" / "2 трека" / "5 треков". The catalogue entry is an object of plural
// categories; `Intl.PluralRules` says which one this language uses for this
// number, so the caller never has to know how many forms a language has.
//
// `{n}` in the chosen form is the count itself.
export function tn(key, n, vars) {
  const found = entry(key);
  if (found === null) return `${n} ${key}`;
  const { value } = found;
  if (typeof value !== "object") return fill(value, { n, ...vars });
  const form = value[rulesFor(found.lang).select(n)] || value.other || value.one;
  return fill(form || key, { n, ...vars });
}

// "A and B" / "A, B and C" — in the active language, which is not the same
// glue everywhere and not always a comma. `Intl.ListFormat` knows; a `join`
// here would be English syntax hidden inside a helper.
const LIST_FORMATS = {};
export function list(items) {
  if (!LIST_FORMATS[lang]) LIST_FORMATS[lang] = new Intl.ListFormat(lang, { type: "conjunction" });
  return LIST_FORMATS[lang].format(items);
}

// The catalogue's text for `key`, or `fallback` when it has none — for the
// places where the backend sends a key *and* the English it composed, so an
// older frontend against a newer backend shows a sentence rather than an
// identifier (#268).
export function tOr(key, fallback) {
  if (!key || !Object.prototype.hasOwnProperty.call(en, key)) return fallback;
  return t(key);
}

// A message the backend composed as a code and its values (#268), rendered in
// the current language: `{ code, args, count }`, where `count` is present only
// when the wording turns on a number.
//
// A message whose code this catalogue has never heard of renders as nothing, so
// the caller can fall back to the English the backend sent alongside — a code
// from a newer backend must not put an identifier on screen.
export function message(entry) {
  if (!entry || !entry.code) return "";
  if (!Object.prototype.hasOwnProperty.call(en, entry.code)) return "";
  return entry.count === null || entry.count === undefined
    ? t(entry.code, entry.args)
    : tn(entry.code, entry.count, entry.args);
}

// What a plan or a journalled batch calls itself: its message and any notes,
// joined the way the backend joins them. Falls back to the English the backend
// sent — which is all a batch recorded before #268 has.
export function planText(plan) {
  if (!plan) return "";
  const head = message(plan.message);
  if (!head) return plan.description || "";
  const notes = (plan.notes || []).map(message).filter(Boolean);
  return [head, ...notes].join(" · ");
}

// The language in use — a resolved catalogue name, never "auto".
export function currentLang() {
  return lang;
}

// Translate the static markup under `root` (the whole document by default).
//
// The text of an element carries `data-i18n`; an attribute carries
// `data-i18n-<attribute>`, so `data-i18n-title` and `data-i18n-placeholder`
// work without a list of attributes here needing to know about them. Values
// that hold markup — a hint with a `<b>` in it — are marked `data-i18n-html`,
// which is the only path that writes innerHTML, and only ever from a
// catalogue, never from user or file data.
export function applyStaticText(root) {
  const scope = root || document;
  scope.querySelectorAll("[data-i18n]").forEach((node) => {
    const value = t(node.dataset.i18n);
    if (node.hasAttribute("data-i18n-html")) node.innerHTML = value;
    else node.textContent = value;
  });
  // A full walk, because CSS cannot select "has an attribute whose *name*
  // starts with data-i18n-" and enumerating the attributes here is the
  // bookkeeping this scheme exists to avoid. It runs twice in a session — once
  // at startup, once per language change — so the sweep is not on any path that
  // repeats.
  scope.querySelectorAll("*").forEach((node) => {
    for (const name of Object.keys(node.dataset)) {
      // dataset gives `i18nAriaLabel` for `data-i18n-aria-label`.
      if (!name.startsWith("i18n") || name === "i18n" || name === "i18nHtml") continue;
      const attribute = name
        .slice(4)
        .replace(/([A-Z])/g, "-$1")
        .toLowerCase()
        .replace(/^-/, "");
      node.setAttribute(attribute, t(node.dataset[name]));
    }
  });
}

// Switch language and repaint the static text. Panels that build their own
// content re-render on their own next pass, so `onChange` is where a caller
// asks for the ones that are on screen right now.
export function setLanguage(mode, onChange) {
  saveLangMode(mode);
  lang = resolveLang(mode);
  document.documentElement.lang = lang;
  applyStaticText();
  if (onChange) onChange();
}

document.documentElement.lang = lang;
