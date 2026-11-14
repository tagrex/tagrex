import test from 'node:test';
import assert from 'node:assert/strict';
import { compileQuery, normalizeConditions } from '../../app/ui/js/filter-query.js';

const tracks = [
  { path: 'one.mp3', tags: { artist: 'Aphex Twin', album: '', title: 'First' }, has_cover: true },
  { path: 'two.mp3', tags: { artist: 'Aphex Twin', album: 'Selected', title: 'Second' }, has_cover: false },
  { path: 'three.mp3', tags: { artist: 'Other', album: 'Selected', title: 'Third' }, unreadable: true, has_cover: false },
];
const context = {
  fields: ['artist', 'album', 'title', 'file'],
  fieldValue: (track, key) => key === 'file' ? track.path : track.tags[key] || '',
  searchText: (track, folded) => { const text = [track.path, ...Object.values(track.tags)].join(' '); return folded ? text.toLowerCase() : text; },
  coverLabel: state => ({ embedded: 'Embedded artwork', missing: 'No embedded artwork', unknown: 'Artwork status unavailable' })[state],
};
const model = extras => ({ version: 1, text: '', regex: false, caseSensitive: false, conditions: [], mode: 'all', ...extras });
const condition = (id, field, operator, value = '') => ({ id, field, operator, value });
const matches = query => { const compiled = compileQuery(model(query), context); assert.equal(compiled.error, ''); return tracks.filter(compiled.match).map(track => track.path); };

test('quick search narrows all/any structured conditions without modifying source rows', () => {
  const before = structuredClone(tracks);
  const conditions = [condition('art', 'cover', 'equals', 'missing'), condition('title', 'title', 'starts', 'Third')];
  assert.deepEqual(matches({ conditions }), []);
  assert.deepEqual(matches({ mode: 'any', conditions }), ['two.mp3', 'three.mp3']);
  assert.deepEqual(matches({ text: 'aphex', mode: 'any', conditions }), ['two.mp3']);
  assert.deepEqual(tracks, before);
});
test('field queries, filename, literal colons, empty/present and case flags retain their meanings', () => {
  assert.deepEqual(matches({ text: 'artist:APHEX' }), ['one.mp3', 'two.mp3']);
  assert.deepEqual(matches({ text: 'artist:APHEX', caseSensitive: true }), []);
  assert.deepEqual(matches({ conditions: [condition('x', 'album', 'empty')] }), ['one.mp3']);
  assert.deepEqual(matches({ conditions: [condition('x', 'album', 'present')] }), ['two.mp3', 'three.mp3']);
  assert.deepEqual(matches({ conditions: [condition('x', 'file', 'equals', 'two.mp3')] }), ['two.mp3']);
  assert.deepEqual(matches({ text: 'unknown:thing' }), []);
});
test('cover aliases do not confuse embedded with missing or unreadable metadata', () => {
  for (const [value, paths] of [['embedded', ['one.mp3']], ['missing', ['two.mp3']], ['unknown', ['three.mp3']], ['no cover', ['two.mp3']]]) {
    assert.deepEqual(matches({ text: `cover:${value}` }), paths);
  }
  assert.deepEqual(matches({ text: 'cover:^(missing|unknown)$', regex: true }), ['two.mp3', 'three.mp3']);
});
test('invalid queries return no predicate so the controller can retain the previous result', () => {
  for (const query of [{ text: '[', regex: true }, { version: 2 }, { text: 'x'.repeat(2049) }, { mode: 'bad' }]) {
    const result = compileQuery(model(query), context);
    assert.ok(result.error);
    assert.equal(result.match, null);
  }
});
test('saved conditions are bounded, validated and copied without arbitrary payloads', () => {
  const input = condition('x', 'cover', 'equals', 'missing');
  assert.deepEqual(normalizeConditions([{ ...input, token: 'excluded' }]), [input]);
  for (const invalid of [null, {}, [input, input], [condition('x', 'cover', 'contains', 'missing')], [condition('x', 'arbitrary', 'equals', 'value')], [condition('x', 'artist', 'contains')], Array.from({ length: 25 }, (_, i) => ({ ...input, id: String(i) }))]) {
    assert.throws(() => normalizeConditions(invalid));
  }
});
test('one compiled metadata predicate evaluates 20000 rows without reading artwork or tag values', () => {
  const compiled = compileQuery(model({ conditions: [condition('art', 'cover', 'equals', 'missing')] }), { ...context, fieldValue() { throw Error('Unexpected tag read'); } });
  let count = 0;
  for (let i = 0; i < 20000; i++) if (compiled.match({ has_cover: i % 2 === 0 })) count++;
  assert.equal(count, 10000);
});
