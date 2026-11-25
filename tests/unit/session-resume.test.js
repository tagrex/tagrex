import { test } from 'node:test';
import assert from 'node:assert/strict';

// session.js has no imports, so it loads as it is.
const {
  MAX_SELECTION,
  normalizeSession,
  readSession,
  resumeEnabled,
  sessionSnapshot,
  setResumeEnabled,
  writeSession,
} = await import('../../app/ui/js/session.js');

function memoryStorage() {
  const map = new Map();
  return {
    getItem: (k) => (map.has(k) ? map.get(k) : null),
    setItem: (k, v) => map.set(k, String(v)),
    removeItem: (k) => map.delete(k),
    map,
  };
}

const VIEW = {
  root: '/Music/House',
  mode: 'renamer',
  subtab: 'editor',
  sortKey: 'artist',
  sortDir: -1,
  filter: 'genre:house',
  selection: new Set(['/Music/House/a.flac', '/Music/House/b.flac']),
  scrollTop: 412.6,
};

test('a saved view comes back as it was (#873)', () => {
  const store = memoryStorage();
  writeSession(sessionSnapshot(VIEW), undefined, store);
  assert.deepEqual(readSession(store), {
    root: '/Music/House',
    mode: 'renamer',
    subtab: 'editor',
    sortKey: 'artist',
    sortDir: -1,
    filter: 'genre:house',
    selection: ['/Music/House/a.flac', '/Music/House/b.flac'],
    scrollTop: 413,
  });
});

test('nothing resumable open forgets the saved view (#873)', () => {
  const store = memoryStorage();
  const stored = writeSession(sessionSnapshot(VIEW), undefined, store);
  assert.equal(sessionSnapshot({ ...VIEW, root: '' }), null);
  writeSession(null, stored, store);
  assert.equal(readSession(store), null);
});

test('an unchanged view is not written again (#873)', () => {
  const store = memoryStorage();
  let writes = 0;
  const counting = { ...store, setItem: (k, v) => { writes += 1; store.setItem(k, v); } };
  const first = writeSession(sessionSnapshot(VIEW), undefined, counting);
  const second = writeSession(sessionSnapshot(VIEW), first, counting);
  assert.equal(first, second);
  assert.equal(writes, 1);
});

test('an entry without a usable folder is ignored (#873)', () => {
  assert.equal(normalizeSession(null), null);
  assert.equal(normalizeSession({ root: 'relative/path' }), null);
  assert.equal(normalizeSession({ root: ' /padded' }), null);
  assert.equal(normalizeSession({ root: 42 }), null);
  const store = memoryStorage();
  store.setItem('tagrex.session', '{not json');
  assert.equal(readSession(store), null);
});

test('Windows and UNC folders are accepted (#873)', () => {
  assert.equal(normalizeSession({ root: 'C:\\Music' }).root, 'C:\\Music');
  assert.equal(normalizeSession({ root: 'D:/Music' }).root, 'D:/Music');
  assert.equal(normalizeSession({ root: '\\\\nas\\music' }).root, '\\\\nas\\music');
});

test('malformed parts fall back to their defaults (#873)', () => {
  const s = normalizeSession({
    root: '/Music',
    mode: 'nonsense',
    subtab: 'gone',
    sortKey: 7,
    sortDir: 5,
    filter: { a: 1 },
    selection: ['/Music/a.mp3', 3, '', '/Music/a.mp3'],
    scrollTop: -10,
  });
  assert.deepEqual(s, {
    root: '/Music',
    mode: 'tagger',
    subtab: 'online',
    sortKey: null,
    sortDir: 1,
    filter: '',
    selection: ['/Music/a.mp3'],
    scrollTop: 0,
  });
});

test('a selection too large to keep is dropped, never cut short (#873)', () => {
  const many = new Set(Array.from({ length: MAX_SELECTION + 1 }, (_, i) => `/Music/${i}.mp3`));
  assert.deepEqual(sessionSnapshot({ ...VIEW, selection: many }).selection, []);
  const enough = new Set([...many].slice(0, MAX_SELECTION));
  assert.equal(sessionSnapshot({ ...VIEW, selection: enough }).selection.length, MAX_SELECTION);
});

test('switching resume off forgets the saved view (#873)', () => {
  const store = memoryStorage();
  assert.equal(resumeEnabled(store), true);
  writeSession(sessionSnapshot(VIEW), undefined, store);
  setResumeEnabled(false, store);
  assert.equal(resumeEnabled(store), false);
  assert.equal(readSession(store), null);
  setResumeEnabled(true, store);
  assert.equal(resumeEnabled(store), true);
});
