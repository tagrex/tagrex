// Keyboard shortcuts (#432): one registry instead of one-off keydown listeners.
//
// Every shortcut is an action id with a default combination and a handler, so
// they can all be listed, rebound in Settings › Shortcuts and named in the
// tooltips from one place. Most actions press the button that already does the
// job — only when it is on screen and enabled — so a shortcut never does
// anything the mouse couldn't, and "nothing to apply" is decided by the button.
//
// A combination is stored as modifiers + `event.code` ("Mod+Enter",
// "Alt+Space", "Mod+KeyZ"): the physical key, so a Cyrillic layout, where
// `event.key` for the A key is "ф", leaves letter shortcuts working. "Mod" is
// Command on macOS and Ctrl elsewhere; "Ctrl" only exists on macOS, where it is
// a key of its own.
import { el } from "./dom.js";
import { t } from "./i18n.js";

const STORAGE_KEY = "tagrex.shortcuts";
const IS_MAC = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent || "");

const MODIFIER_CODES = new Set([
  "MetaLeft", "MetaRight", "ControlLeft", "ControlRight",
  "AltLeft", "AltRight", "ShiftLeft", "ShiftRight", "CapsLock", "Fn",
]);

// Combinations a binding may not take: the ones the OS or the window owns, and
// the bare keys the table and every text field already use.
const RESERVED = new Set([
  "Mod+KeyC", "Mod+KeyV", "Mod+KeyX", "Mod+KeyQ", "Mod+KeyW", "Mod+Tab", "Alt+Tab", "Alt+F4",
  ...(IS_MAC
    ? ["Mod+KeyH", "Mod+Alt+KeyH", "Mod+KeyM", "Mod+Space", "Mod+Ctrl+KeyF", "Mod+Backquote"]
    : ["Alt+Space"]),
  "Escape", "Tab", "Enter", "Space", "Backspace", "Delete",
  "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Shift+Tab",
]);

// Press a button the way a click would, if it is there to be pressed.
function press(...ids) {
  for (const id of ids) {
    const btn = el(id);
    if (btn && !btn.disabled && btn.offsetParent !== null) {
      btn.click();
      return;
    }
  }
}

function pressMode(mode) {
  return () => document.querySelector(`.mode-tab[data-mode="${mode}"]`)?.click();
}

// The actions, in the order Settings lists them. `typing: true` lets a
// shortcut fire while a text field has focus; the rest leave the field its own
// keys (⌘⌫ deletes a line there, ⌘← moves the caret, ⌘Z undoes typing).
// `buttons` are what the tooltip hint and aria-keyshortcuts go on.
const ACTIONS = [
  { id: "apply", combo: "Mod+Enter", buttons: ["diff-apply"], run: () => press("diff-apply") },
  { id: "discard", combo: "Mod+Backspace", buttons: ["diff-discard"], run: () => press("diff-discard") },
  { id: "undo", combo: "Mod+KeyZ", buttons: ["undo"], run: () => press("undo") },
  { id: "open", combo: "Mod+KeyO", typing: true, buttons: ["lib-action"], run: () => press("lib-action") },
  { id: "reread", combo: "Mod+KeyR", typing: true, buttons: ["lib-refresh"], run: () => press("lib-refresh") },
  {
    id: "filter",
    combo: "Mod+KeyF",
    typing: true,
    run: () => {
      const input = el("filter");
      if (!input || input.offsetParent === null) return;
      input.focus();
      input.select();
    },
  },
  { id: "selectAll", combo: "Mod+KeyA" },
  {
    id: "playPause",
    combo: IS_MAC ? "Alt+Space" : "Mod+Shift+Space",
    buttons: ["pl-toggle", "sb-play"],
    run: () => press("pl-toggle", "sb-play"),
  },
  { id: "prev", combo: "Mod+ArrowLeft", buttons: ["pl-prev"], run: () => press("pl-prev") },
  { id: "next", combo: "Mod+ArrowRight", buttons: ["pl-next"], run: () => press("pl-next") },
  { id: "modeTagger", combo: "Mod+Digit1", typing: true, run: pressMode("tagger") },
  { id: "modeRenamer", combo: "Mod+Digit2", typing: true, run: pressMode("renamer") },
  { id: "modeGenerator", combo: "Mod+Digit3", typing: true, run: pressMode("generator") },
  { id: "modeDeduplicator", combo: "Mod+Digit4", typing: true, run: pressMode("deduplicator") },
  { id: "modeExporter", combo: "Mod+Digit5", typing: true, run: pressMode("exporter") },
  { id: "panel", combo: "Mod+Alt+KeyS", buttons: ["panel-toggle"], run: () => press("panel-toggle") },
  { id: "settings", combo: "Mod+Comma", typing: true, buttons: ["settings-open"], run: () => press("settings-open") },
];
const BY_ID = new Map(ACTIONS.map((a) => [a.id, a]));
const MODE_BUTTON = {
  modeTagger: "tagger",
  modeRenamer: "renamer",
  modeGenerator: "generator",
  modeDeduplicator: "deduplicator",
  modeExporter: "exporter",
};

// ---- persistence: only what differs from the defaults is stored ----

function readOverrides() {
  try {
    const raw = JSON.parse(localStorage.getItem(STORAGE_KEY) || "{}");
    return raw && typeof raw === "object" ? raw : {};
  } catch (e) {
    return {};
  }
}

let overrides = readOverrides();

function writeOverrides() {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(overrides));
  } catch (e) {
    /* storage unavailable — the binding just won't persist */
  }
}

function comboOf(id) {
  const action = BY_ID.get(id);
  if (!action) return "";
  return typeof overrides[id] === "string" ? overrides[id] : action.combo;
}

function isDefault(id) {
  return comboOf(id) === BY_ID.get(id)?.combo;
}

// ---- combinations ----

function comboFromEvent(e) {
  if (!e.code || MODIFIER_CODES.has(e.code)) return null;
  const code = e.code === "NumpadEnter" ? "Enter" : e.code;
  const parts = [];
  if (IS_MAC ? e.metaKey : e.ctrlKey) parts.push("Mod");
  if (IS_MAC && e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  parts.push(code);
  return parts.join("+");
}

// A binding without Mod, Ctrl or Alt is a plain key — it never fires while
// typing, whatever its action allows.
function hasCommandModifier(combo) {
  return /(^|\+)(Mod|Ctrl|Alt)\+/.test(combo);
}

const KEY_NAMES = {
  Enter: ["↩", "Enter"],
  Backspace: ["⌫", "Backspace"],
  Delete: ["⌦", "Del"],
  Escape: ["Esc", "Esc"],
  Tab: ["⇥", "Tab"],
  Space: ["Space", "Space"],
  ArrowLeft: ["←", "Left"],
  ArrowRight: ["→", "Right"],
  ArrowUp: ["↑", "Up"],
  ArrowDown: ["↓", "Down"],
  Home: ["↖", "Home"],
  End: ["↘", "End"],
  PageUp: ["⇞", "PgUp"],
  PageDown: ["⇟", "PgDn"],
  Comma: [",", ","],
  Period: [".", "."],
  Slash: ["/", "/"],
  Backslash: ["\\", "\\"],
  Semicolon: [";", ";"],
  Quote: ["'", "'"],
  Backquote: ["`", "`"],
  Minus: ["-", "-"],
  Equal: ["=", "="],
  BracketLeft: ["[", "["],
  BracketRight: ["]", "]"],
};

function keyName(code) {
  if (KEY_NAMES[code]) return KEY_NAMES[code][IS_MAC ? 0 : 1];
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit\d$/.test(code)) return code.slice(5);
  if (/^Numpad\d$/.test(code)) return code.slice(6);
  return code;
}

// ⌃⌥⇧⌘K on macOS, in Apple's modifier order; Ctrl+Alt+Shift+K elsewhere.
function formatCombo(combo) {
  if (!combo) return "";
  const parts = combo.split("+");
  const code = parts.pop();
  const mods = new Set(parts);
  if (IS_MAC) {
    return (
      (mods.has("Ctrl") ? "⌃" : "") +
      (mods.has("Alt") ? "⌥" : "") +
      (mods.has("Shift") ? "⇧" : "") +
      (mods.has("Mod") ? "⌘" : "") +
      keyName(code)
    );
  }
  const names = [];
  if (mods.has("Mod")) names.push("Ctrl");
  if (mods.has("Alt")) names.push("Alt");
  if (mods.has("Shift")) names.push("Shift");
  names.push(keyName(code));
  return names.join("+");
}

// The same combination in the form `aria-keyshortcuts` wants.
function ariaCombo(combo) {
  const parts = combo.split("+");
  const code = parts.pop();
  const names = parts.map((m) => (m === "Mod" ? (IS_MAC ? "Meta" : "Control") : m === "Ctrl" ? "Control" : m));
  const key = KEY_NAMES[code]?.[1] ?? keyName(code);
  return [...names, key === "Space" ? "Space" : key].join("+");
}

function actionLabel(id) {
  return t(`shortcuts.action.${id}`);
}

// ---- dispatch ----

function typingIn(node) {
  if (!node || !(node instanceof Element)) return false;
  if (node.isContentEditable) return true;
  if (node.tagName === "TEXTAREA" || node.tagName === "SELECT") return true;
  if (node.tagName !== "INPUT") return false;
  return !["checkbox", "radio", "range", "button", "submit", "color", "file"].includes(node.type);
}

// Settings, a dialog or the preset editor owns the keyboard while it is open.
function modalOpen() {
  if (el("settings") && !el("settings").hidden) return true;
  return !!document.querySelector(".modal-backdrop:not([hidden])");
}

const handlers = new Map(); // id → function, for actions with no button to press
let capturing = false; // Settings is recording a new combination

function findAction(combo) {
  return ACTIONS.find((a) => comboOf(a.id) === combo) || null;
}

function onKeyDown(e) {
  if (capturing || e.defaultPrevented || e.isComposing) return;
  const combo = comboFromEvent(e);
  if (!combo) return;
  const action = findAction(combo);
  if (!action) return;
  const typing = typingIn(e.target) || typingIn(document.activeElement);
  const firesWhileTyping = action.typing && hasCommandModifier(combo);
  if (typing && !firesWhileTyping) return; // the field keeps its own key
  // Taken even when nothing runs: Ctrl+R and Ctrl+F belong to the webview
  // otherwise, and would reload the page or open its find bar.
  e.preventDefault();
  if (modalOpen()) return;
  const run = handlers.get(action.id) || action.run;
  if (run) run(e);
}

// ---- tooltips and accessible names ----

function decorate() {
  for (const action of ACTIONS) {
    const combo = comboOf(action.id);
    const ids = action.buttons || [];
    const nodes = ids.map((id) => el(id)).filter(Boolean);
    if (MODE_BUTTON[action.id]) {
      const tab = document.querySelector(`.mode-tab[data-mode="${MODE_BUTTON[action.id]}"]`);
      if (tab) nodes.push(tab);
    }
    for (const node of nodes) {
      node.dataset.shortcut = action.id;
      if (combo) node.setAttribute("aria-keyshortcuts", ariaCombo(combo));
      else node.removeAttribute("aria-keyshortcuts");
    }
  }
}

// What the tooltip shows after a control's title — "⌘↩", "Ctrl+Enter".
function shortcutHint(node) {
  const id = node?.dataset?.shortcut;
  return id ? formatCombo(comboOf(id)) : "";
}

// ---- rebinding ----

// Bind `combo` to `id`, or refuse with the reason.
function rebind(id, combo) {
  if (!BY_ID.has(id) || !combo) return { ok: false };
  if (RESERVED.has(combo)) {
    return { ok: false, error: t("shortcuts.reserved", { combo: formatCombo(combo) }) };
  }
  const holder = findAction(combo);
  if (holder && holder.id !== id) {
    return {
      ok: false,
      error: t("shortcuts.taken", { combo: formatCombo(combo), action: actionLabel(holder.id) }),
    };
  }
  if (combo === BY_ID.get(id).combo) delete overrides[id];
  else overrides[id] = combo;
  writeOverrides();
  decorate();
  return { ok: true };
}

// Back to the default — refused when another action has since taken it.
function resetShortcut(id) {
  const action = BY_ID.get(id);
  if (!action) return { ok: false };
  return rebind(id, action.combo);
}

// Every default back. An override that would now clash cannot exist: all of
// them go at once.
function resetAllShortcuts() {
  overrides = {};
  writeOverrides();
  decorate();
}

function setCapturing(on) {
  capturing = on;
}

// The registry's own listener, plus a handler for each action with no button.
function initShortcuts(extra = {}) {
  for (const [id, fn] of Object.entries(extra)) handlers.set(id, fn);
  document.addEventListener("keydown", onKeyDown);
  decorate();
}

export {
  ACTIONS,
  actionLabel,
  comboFromEvent,
  comboOf,
  formatCombo,
  initShortcuts,
  isDefault,
  rebind,
  resetAllShortcuts,
  resetShortcut,
  setCapturing,
  shortcutHint,
};
