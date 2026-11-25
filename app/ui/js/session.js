// Pick up where you left off (#873): what was open and how it was shown,
// saved while the app runs and put back on the next launch.
//
// Only the VIEW is kept: the folder, the mode, TAGGER's side-panel tab, the
// sort, the filter text, the selected files and the scroll position. Staged
// edits, a staged plan and online results never are: a plan is applied or
// thrown away on purpose, and one coming back after a restart would be a
// surprise. A drop of loose files or a folder set built with Add folder is not
// saved either, since reopening their common parent would list more than was
// open.
//
// No imports, so the unit tests load it as it is; app.js does the opening and
// the restoring, this module only decides what is kept and checks what comes
// back, since a hand-edited or older entry must never steer the startup.

const SESSION_KEY = "tagrex.session";
const RESUME_KEY = "tagrex.resumeSession";

const MODES = ["tagger", "renamer", "generator", "deduplicator", "exporter"];
const SUBTABS = ["online", "editor", "filename", "fromtext"];
// A selection past this is not kept: a few thousand paths is already a large
// entry to rewrite while the app runs, and restoring part of a selection would
// silently leave files out of the next operation.
const MAX_SELECTION = 5000;
const MAX_FILTER = 2048;
const MAX_PATH = 4096;

function storageOf(storage) {
  if (storage) return storage;
  try {
    return globalThis.localStorage || null;
  } catch (e) {
    return null; // storage unavailable: nothing is kept
  }
}

// On unless switched off in Settings.
function resumeEnabled(storage) {
  try {
    return storageOf(storage)?.getItem(RESUME_KEY) !== "0";
  } catch (e) {
    return true;
  }
}

// Switching it off also forgets what was saved: an entry naming a folder
// should not outlive the choice not to have one.
function setResumeEnabled(on, storage) {
  const store = storageOf(storage);
  try {
    store?.setItem(RESUME_KEY, on ? "1" : "0");
    if (!on) store?.removeItem(SESSION_KEY);
  } catch (e) {
    /* storage unavailable: the preference just won't persist */
  }
}

// An absolute path on any platform: /…, C:\… or C:/…, or \\server\share.
function isAbsolutePath(path) {
  return /^(\/|[A-Za-z]:[\\/]|\\\\)/.test(path);
}

function boundedText(value, max) {
  return typeof value === "string" && value.length <= max ? value : "";
}

// The entry as it may be used, or null. Anything malformed falls back to the
// default for that part rather than throwing the whole entry away, except the
// folder: without a usable one there is nothing to resume.
function normalizeSession(value) {
  if (!value || typeof value !== "object") return null;
  const root = boundedText(value.root, MAX_PATH);
  if (!root || root !== root.trim() || !isAbsolutePath(root)) return null;
  const selection = Array.isArray(value.selection) && value.selection.length <= MAX_SELECTION
    ? [...new Set(value.selection.filter((p) => typeof p === "string" && p && p.length <= MAX_PATH))]
    : [];
  const scrollTop = Number.isFinite(value.scrollTop) && value.scrollTop > 0 ? Math.round(value.scrollTop) : 0;
  return {
    root,
    mode: MODES.includes(value.mode) ? value.mode : "tagger",
    subtab: SUBTABS.includes(value.subtab) ? value.subtab : "online",
    sortKey: boundedText(value.sortKey, 128) || null,
    sortDir: value.sortDir === -1 ? -1 : 1,
    filter: boundedText(value.filter, MAX_FILTER),
    selection,
    scrollTop,
  };
}

function readSession(storage) {
  try {
    const raw = storageOf(storage)?.getItem(SESSION_KEY);
    return raw ? normalizeSession(JSON.parse(raw)) : null;
  } catch (e) {
    return null; // unreadable entry: start empty, as before this existed
  }
}

// What is worth saving right now, from the live view. `root` is empty when
// nothing resumable is open (no folder, or a file set).
function sessionSnapshot({ root, mode, subtab, sortKey, sortDir, filter, selection, scrollTop }) {
  if (!root) return null;
  const paths = [...(selection || [])];
  return normalizeSession({
    root,
    mode,
    subtab,
    sortKey,
    sortDir,
    filter,
    selection: paths.length <= MAX_SELECTION ? paths : [],
    scrollTop,
  });
}

// Writes only when something changed, so a caller may ask as often as it likes.
// Returns the text now stored, for the next comparison.
function writeSession(snapshot, previous, storage) {
  const store = storageOf(storage);
  const text = snapshot ? JSON.stringify(snapshot) : "";
  if (text === previous) return previous;
  try {
    if (text) store?.setItem(SESSION_KEY, text);
    else store?.removeItem(SESSION_KEY);
  } catch (e) {
    /* storage unavailable or full: the next launch just starts empty */
  }
  return text;
}

export {
  MAX_SELECTION,
  normalizeSession,
  readSession,
  resumeEnabled,
  sessionSnapshot,
  setResumeEnabled,
  writeSession,
};
