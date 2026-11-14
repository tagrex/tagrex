import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { createRequire } from 'node:module';
import vm from 'node:vm';

// The autocomplete's collection and matching run in a VM harness against the
// exact production function bodies (the same technique as
// perf-stage2-scope.test.js). No app boot, no DOM, no native bridge: the
// module itself creates a menu on import, and the only behaviour under test
// here is the pure part behind it.
//
// TAGREX_SUGGEST_SOURCE lets the same assertions run against the frozen
// baseline checkout, so a candidate and the source it came from can be
// compared with one command:
//   TAGREX_SUGGEST_SOURCE=<worktree> node --test tests/unit/perf-stage2-suggestions.test.js
const root = resolve(process.env.TAGREX_SUGGEST_SOURCE || '.');
const require = createRequire(resolve(root, 'package.json'));
const ts = require('typescript');

function extract(file, names) {
  const source = readFileSync(resolve(root, file), 'utf8');
  const ast = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.JS);
  return names.map((name) => {
    const nodes = ast.statements.filter((node) => ts.isFunctionDeclaration(node) && node.name?.text === name);
    assert.equal(nodes.length, 1, `expected exactly one ${name}() in ${file}`);
    return nodes[0].getText(ast).replace(/^export\s+/, '');
  }).join('\n');
}

const suggestNames = ['collectValues', 'matches', 'markMatch'];
// The Unicode span helper is optional when exercising the frozen pre-fix source.
if (readFileSync(resolve(root, 'app/ui/js/suggest.js'), 'utf8').includes('function originalSpan(')) suggestNames.push('originalSpan');
const suggestFunctions = extract('app/ui/js/suggest.js', suggestNames);
const escapeHtml = extract('app/ui/js/dom.js', ['escapeHtml']);

function harness(tracks, pending = []) {
  const context = vm.createContext({
    tracks,
    edits: new Map(pending),
    tag: (track, key) => track.tags[key] || '',
  });
  vm.runInContext('const MAX_ROWS = 8; let values = [];', context);
  vm.runInContext(suggestFunctions, context);
  vm.runInContext(escapeHtml, context);
  // Session boundaries mirror openCellSuggest()/endCellSuggest(): the value
  // list is rebuilt per edit, which is the only cache lifetime relied on.
  vm.runInContext(`
    globalThis.openSession = (field) => { values = collectValues(field); };
    globalThis.closeSession = () => { values = []; };
    globalThis.queryRaw = (typed) => matches(typed);
    globalThis.highlight = (entry, typed) => markMatch(entry, typed);
  `, context);
  // Projections are made in this realm: the harness answers with arrays and
  // objects from its own, and deep equality across realms would be noise.
  context.query = (typed) => Array.from(context.queryRaw(typed),
    (entry) => ({ value: entry.value, files: entry.files, at: entry.at }));
  return context;
}

// Straightforward reading of the contract, independent of the fold cache and
// precomputed case folds: count over the model, popularity first, then default
// locale order; prefix matches before containing ones, at most eight rows.
function oracleValues(tracks, pending, field) {
  const edits = new Map(pending);
  const counts = new Map();
  for (const track of tracks) {
    const staged = edits.get(track.path);
    const value = staged && staged.has(field) ? staged.get(field) : (track.tags[field] || '');
    const trimmed = value.trim();
    if (!trimmed) continue;
    counts.set(trimmed, (counts.get(trimmed) || 0) + 1);
  }
  return [...counts.entries()]
    .map(([value, files]) => ({ value, files }))
    .sort((a, b) => b.files - a.files || a.value.localeCompare(b.value));
}

function oracleMatches(values, typed) {
  const needle = typed.trim().toLowerCase();
  if (!needle) return values.slice(0, 8).map((entry) => ({ ...entry, at: undefined }));
  const starting = [];
  const containing = [];
  for (const entry of values) {
    const at = entry.value.toLowerCase().indexOf(needle);
    if (at === 0) {
      starting.push({ ...entry, at });
      if (starting.length === 8) return starting;
    } else if (at > 0 && containing.length < 8) {
      containing.push({ ...entry, at });
    }
  }
  return [...starting, ...containing].slice(0, 8);
}

function library(size, artist) {
  return Array.from({ length: size }, (_, index) => ({
    path: `/music/${index}.flac`,
    tags: { artist: artist(index) },
  }));
}

function expectEquivalent(context, tracks, pending, field, queries) {
  context.openSession(field);
  const expectedValues = oracleValues(tracks, pending, field);
  for (const typed of queries) {
    const actual = context.query(typed);
    const expected = oracleMatches(expectedValues, typed).map((entry) => ({
      value: entry.value, files: entry.files, at: entry.at,
    }));
    assert.deepEqual(actual, expected, `matching "${typed}"`);
    // The invariants the list's own behaviour depends on, asserted without the
    // oracle: eight rows at most, every row a real match, prefixes first.
    assert.ok(actual.length <= 8, `at most eight rows for "${typed}"`);
    const needle = typed.trim().toLowerCase();
    if (needle) {
      const positions = actual.map((entry) => entry.value.toLowerCase().indexOf(needle));
      assert.ok(positions.every((at) => at >= 0), `every row matches "${typed}"`);
      let seenMiddle = false;
      for (const at of positions) {
        if (at > 0) seenMiddle = true;
        else assert.ok(!seenMiddle, `prefixes precede middles for "${typed}"`);
      }
    }
  }
  // Re-asking in the same session must answer identically: the fold cache is
  // memoization, not state.
  const first = context.query(queries[0]);
  const again = context.query(queries[0]);
  assert.deepEqual(again, first, 'repeated queries are stable');
  return expectedValues;
}

test('collection keeps popularity, trimming, pending edits and default-locale ties', () => {
  const tracks = [
    { path: 'a', tags: { artist: 'The Artist' } },
    { path: 'b', tags: { artist: 'The Artist' } },
    { path: 'c', tags: { artist: 'artist two' } },
    { path: 'd', tags: { artist: 'Various' } },
    { path: 'e', tags: { artist: '   ' } },
    { path: 'f', tags: { artist: 'Artist 3' } },
    { path: 'g', tags: { artist: 'Ärtist 3' } },
    { path: 'h', tags: { artist: 'artist 3' } },
  ];
  const pending = [
    ['c', new Map([['artist', '  Artist Two  ']])],
    ['d', new Map([['artist', '']])],
  ];
  const context = harness(tracks, pending);
  const values = expectEquivalent(context, tracks, pending, 'artist',
    ['', ' ', 'a', 'A', 'artist', 'artist 3', 'two', 'various', 'no such artist', 'Ä', 'é']);

  const byValue = new Map(values.map((entry) => [entry.value, entry.files]));
  assert.equal(byValue.get('The Artist'), 2, 'the popular spelling leads');
  assert.equal(byValue.get('Artist Two'), 1, 'the pending spelling is the value, trimmed');
  assert.equal(byValue.get('Artist 3'), 1);
  assert.equal(byValue.get('artist 3'), 1, 'differing case is a different spelling to offer');
  assert.equal(values[0].value, 'The Artist');
  assert.equal(byValue.size, 5);
  // The pending edit replaced 'artist two' and cleared 'Various' — an applied
  // change and a staged one are the same input to the list.
  assert.ok(!byValue.has('artist two'));
  assert.ok(!byValue.has('Various'));
});

test('a rare or unmatched query returns the same empty list and no duplicates', () => {
  const tracks = library(2000, (index) => `Artist ${index}`);
  const pending = [];
  const context = harness(tracks, pending);
  const queries = ['zzzz not present', 'Artist 1999', 'artist 1999 ', '🎵', '', 'Artist 19'];
  const values = expectEquivalent(context, tracks, pending, 'artist', queries);
  assert.equal(values.length, 2000);
  assert.deepEqual(context.query('zzzz not present'), []);
  assert.deepEqual(context.query('artist 1999'), [{ value: 'Artist 1999', files: 1, at: 0 }]);
  assert.equal(context.query('Artist 19').length, 8, 'the list is capped at eight rows');
});

test('eight prefix matches win over more popular containing matches', () => {
  const tracks = [];
  for (let index = 0; index < 25; index++) {
    for (let copy = 0; copy < 3; copy++) {
      tracks.push({ path: `c${index}-${copy}`, tags: { artist: `The artist ${index}` } });
    }
  }
  for (let index = 0; index < 12; index++) {
    tracks.push({ path: `p${index}`, tags: { artist: `Artist prefix ${index}` } });
  }
  const pending = [];
  const context = harness(tracks, pending);
  const values = expectEquivalent(context, tracks, pending, 'artist', ['artist', 'prefix', 'the artist']);
  assert.equal(values[0].value, 'The artist 0', 'popularity orders the collected list');
  const rows = context.query('artist');
  assert.equal(rows.length, 8);
  assert.ok(rows.every((entry) => entry.value.startsWith('Artist prefix')), 'all eight are prefix matches');
});

test('Unicode, case and collation ties keep the default locale order', () => {
  const spellings = [
    'Éclair', 'éclair', 'Eclair', 'e\u0301clair', 'İstanbul', 'istanbul', 'ßeta', 'SS',
    '🎵 Song', 'Ω omega', 'ø', 'Ø', 'Å', 'å', 'Straße', 'Strasse', 'artist', 'Artist',
  ];
  const tracks = spellings.map((artist, index) => ({ path: `u${index}`, tags: { artist } }));
  const pending = [['u0', new Map([['artist', '  Éclair  ']])]];
  const context = harness(tracks, pending);
  const values = expectEquivalent(context, tracks, pending, 'artist',
    ['', 'é', 'É', 'e', 'ss', 'ß', 'istanbul', '🎵', 'ø', 'strasse', 'artist', 'artist ']);
  // Highlight indices are the folded index applied to the original spelling,
  // which is what makes the bold slice line up on screen.
  const eclair = values.find((entry) => entry.value === 'Éclair');
  assert.equal(context.highlight({ value: eclair.value, at: 0 }, 'é'), '<b>É</b>clair');
  const istanbul = values.find((entry) => entry.value === 'İstanbul');
  assert.equal(context.highlight({ value: istanbul.value, at: 0 }, 'İ'), '<b>İ</b>stanbul');
});

test('a new edit session sees pending values changed since the last one', () => {
  const tracks = [
    { path: 'a', tags: { artist: 'Original' } },
    { path: 'b', tags: { artist: 'Shared' } },
  ];
  const context = harness(tracks, [['a', new Map([['artist', 'Alpha']])]]);
  context.openSession('artist');
  assert.deepEqual(context.query('al').map((entry) => entry.value), ['Alpha']);

  context.closeSession();
  context.edits.set('a', new Map([['artist', 'Beta']]));
  context.openSession('artist');
  assert.deepEqual(context.query('al').map((entry) => entry.value), []);
  assert.deepEqual(context.query('be').map((entry) => entry.value), ['Beta']);

  context.closeSession();
  context.edits.delete('a');
  context.openSession('artist');
  assert.deepEqual(context.query('or').map((entry) => entry.value), ['Original']);
});
