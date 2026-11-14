import test from 'node:test';
import assert from 'node:assert/strict';
import { SESSION_KEY, RESUME_ATTEMPT_KEY, readSession, saveSession, normalizeSession, beginResume, clearResumeAttempt, clearSession } from '../../app/ui/js/session-resume.js';
const memory = () => { const map = new Map(); return { getItem: key => map.get(key) ?? null, setItem: (key, value) => map.set(key, value), removeItem: key => map.delete(key) }; };
const snapshot = extras => ({ version: 1, root: '/music', mode: 'tagger', subtab: 'editor', selection: ['/music/a.mp3'], ...extras });

test('resume stores only validated view state and excludes edits, plans and credentials', () => {
  const storage = memory();
  assert.equal(saveSession(storage, snapshot({ edits: { title: 'UNAPPLIED' }, previewPlan: { changes: ['bad'] }, token: 'SECRET', scrollTop: 123 })), true);
  const state = readSession(storage);
  assert.deepEqual(state.selection, ['/music/a.mp3']);
  assert.equal(state.scrollTop, 123);
  assert.equal(state.subtab, 'editor');
  assert.equal(storage.getItem(SESSION_KEY).includes('SECRET'), false);
  assert.equal(storage.getItem(SESSION_KEY).includes('UNAPPLIED'), false);
});

test('invalid, oversized and unknown-version snapshots cannot start an open', () => {
  const storage = memory();
  for (const raw of ['{', JSON.stringify({ version: 2, root: '/music' }), JSON.stringify(snapshot({ root: 'https://example.test' })), 'x'.repeat(256 * 1024 + 1)]) {
    storage.setItem(SESSION_KEY, raw);
    assert.equal(readSession(storage), null);
  }
  assert.equal(normalizeSession(snapshot({ root: 'relative/path' })), null);
  assert.equal(normalizeSession(snapshot({ root: '/bad\0path' })), null);
});

test('large selections are omitted as a whole instead of restoring a partial operation scope', () => {
  const state = normalizeSession(snapshot({ selection: Array.from({ length: 257 }, (_, i) => `/music/${i}.mp3`) }));
  assert.deepEqual(state.selection, []);
  assert.equal(state.selectionOmitted, true);
});

test('interrupted or failed restore is circuit-broken until successful manual open', () => {
  const storage = memory();
  assert.equal(beginResume(storage, '/music'), true);
  assert.equal(beginResume(storage, '/music'), false);
  assert.ok(storage.getItem(RESUME_ATTEMPT_KEY));
  clearResumeAttempt(storage);
  assert.equal(beginResume(storage, '/music'), true);
  clearSession(storage);
  assert.equal(storage.getItem(RESUME_ATTEMPT_KEY), null);
});

test('unavailable storage leaves persistence optional', () => {
  const storage = { getItem() { throw Error('blocked'); }, setItem() { throw Error('quota'); }, removeItem() { throw Error('blocked'); } };
  assert.equal(readSession(storage), null);
  assert.equal(saveSession(storage, snapshot()), false);
  assert.equal(beginResume(storage, '/music'), true);
  assert.doesNotThrow(() => clearSession(storage));
});

test('unknown modes and invalid dimensions fall back safely; Windows paths are accepted', () => {
  const state = normalizeSession(snapshot({ root: 'C:\\Music', mode: 'invented', subtab: 'invented', scrollTop: Infinity, scrollLeft: -1, sortDir: 20 }));
  assert.equal(state.mode, 'tagger');
  assert.equal(state.subtab, 'online');
  assert.equal(state.scrollTop, 0);
  assert.equal(state.scrollLeft, 0);
  assert.equal(state.sortDir, 1);
});


test('structured filters survive view restore and invalid conditions reject the snapshot', () => {
  const conditions = [{ id: 'art', field: 'cover', operator: 'equals', value: 'missing' }];
  const normalized = normalizeSession(snapshot({ conditions, conditionMode: 'any' }));
  assert.deepEqual(normalized.conditions, conditions);
  assert.equal(normalized.conditionMode, 'any');
  assert.equal(normalizeSession(snapshot({ conditions: [{ ...conditions[0], field: 'arbitrary' }] })), null);
});
