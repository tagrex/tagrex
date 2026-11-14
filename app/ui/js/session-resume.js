import { normalizeConditions } from './filter-query.js';
// View state only: never persist edits, plans, credentials or artwork payloads.
export const SESSION_KEY = 'tagrex.session.v1';
export const RESUME_ATTEMPT_KEY = 'tagrex.session.resume-attempt.v1';
const MAX_BYTES = 256 * 1024;
const MAX_PATHS = 256;
const modes = new Set(['tagger', 'renamer', 'generator', 'exporter', 'deduplicator']);
const tabs = new Set(['online', 'editor', 'filename', 'fromtext']);
const text = (value, max) => typeof value === 'string' && value.length <= max && !value.includes('\0') ? value : '';
const paths = (value) => Array.isArray(value) ? [...new Set(value.slice(0, MAX_PATHS).map(path => text(path, 4096)).filter(Boolean))] : [];
const pixel = value => Number.isFinite(value) ? Math.max(0, Math.min(value, 10_000_000)) : 0;

export function normalizeSession(value) {
  if (!value || value.version !== 1) return null;
  const root = text(value.root, 4096);
  // Persist only concrete folders, never an arbitrary drop label or a URL.
  if (!root || !(/^(?:\/|[A-Za-z]:[\\/]|\\\\)/.test(root))) return null;
  let conditions;
  try { conditions = normalizeConditions(value.conditions); } catch { return null; }
  return {
    version: 1, root,
    conditions, conditionMode: value.conditionMode === "any" ? "any" : "all",
    mode: modes.has(value.mode) ? value.mode : 'tagger',
    subtab: tabs.has(value.subtab) ? value.subtab : 'online',
    filter: text(value.filter, 2048),
    regex: value.regex === true, caseSensitive: value.caseSensitive === true,
    sortKey: text(value.sortKey, 128) || null, sortDir: value.sortDir === -1 ? -1 : 1,
    groupBy: text(value.groupBy, 128),
    collapsed: paths(value.collapsed),
    // Never silently restore only part of a large selection.
    selection: Array.isArray(value.selection) && value.selection.length <= MAX_PATHS ? paths(value.selection) : [],
    selectionOmitted: value.selectionOmitted === true || (Array.isArray(value.selection) && value.selection.length > MAX_PATHS),
    scrollTop: pixel(value.scrollTop), scrollLeft: pixel(value.scrollLeft),
    filterOpen: value.filterOpen === true,
  };
}

export function readSession(storage) {
  try {
    const raw = storage.getItem(SESSION_KEY);
    if (!raw || raw.length > MAX_BYTES) return null;
    return normalizeSession(JSON.parse(raw));
  } catch { return null; }
}
export function saveSession(storage, value) {
  try {
    const session = normalizeSession(value);
    if (!session) return false;
    const raw = JSON.stringify(session);
    if (raw.length > MAX_BYTES) return false;
    storage.setItem(SESSION_KEY, raw);
    return true;
  } catch { return false; }
}
export function clearSession(storage) {
  try { storage.removeItem(SESSION_KEY); storage.removeItem(RESUME_ATTEMPT_KEY); } catch { /* optional persistence */ }
}
export function beginResume(storage, root) {
  let previous = null;
  try { previous = JSON.parse(storage.getItem(RESUME_ATTEMPT_KEY) || 'null'); } catch { /* invalid marker is replaceable */ }
  if (previous?.root === root) return false;
  try {
    // Leave this marker on interruption/failure. A manual successful open clears it.
    storage.setItem(RESUME_ATTEMPT_KEY, JSON.stringify({ root }));
  } catch { /* storage unavailable must not prevent a one-time open */ }
  return true;
}
export function clearResumeAttempt(storage) {
  try { storage.removeItem(RESUME_ATTEMPT_KEY); } catch { /* optional persistence */ }
}
