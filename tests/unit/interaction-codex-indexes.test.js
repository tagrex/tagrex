import test from 'node:test';
import assert from 'node:assert/strict';
import { coverHarness, numberHarness, loadFunction } from './interaction-codex-harness.js';
import * as state from '../../app/ui/js/state.js';
const cover = loadFunction('app/ui/js/cover.js', 'previewCoverRemove');
const number = loadFunction('app/ui/js/generator.js', 'numberTracks');
const plain = value => JSON.parse(JSON.stringify(value));
const image = { mime: 'image/png', data_base64: 'fixture' };
const change = (path, images = [image]) => ({ path, cover_change: { old: [], new: images } });

test('cover removal keeps native order, appends selection order and first pending duplicate', async () => {
  const plan = { changes: [change('native')] };
  const pending = { changes: [change('empty', []), change('empty'), change('b'), change('a')] };
  const h = coverHarness(cover, ['b', 'native', 'empty', 'a', 'b', 'missing'], pending, plan);
  await h.run();
  assert.deepEqual(plan.changes.map(c => c.path), ['native', 'b', 'a']);
  assert.deepEqual(plain(plan.changes[1].cover_change), { old: [], new: [] });
  assert.equal(h.calls.adopt.length, 1);
  assert.equal(h.calls.adopt[0][0], plan);
  assert.deepEqual(h.calls.adopt[0][1], ['b', 'native', 'empty', 'a', 'b', 'missing']);
  assert.deepEqual(plain(h.calls.invoke), [['preview_cover_remove', { paths: h.calls.adopt[0][1] }]]);
});

test('cover removal handles no pending plan and does not stage stale selection', async () => {
  for (const current of [true, false]) {
    const h = coverHarness(cover, ['a'], null, { changes: [] }, current);
    await h.run();
    assert.equal(h.calls.adopt.length, Number(current));
    assert.equal(h.calls.toast.length, Number(current));
  }
  const h = coverHarness(cover, [], null, { changes: [] }); await h.run();
  assert.equal(h.calls.invoke.length, 0);
});

test('cover merge reads DTO paths linearly rather than searching each plan per selected path', async () => {
  let reads = 0;
  const changes = Array.from({ length: 80 }, (_, i) => ({ get path() { reads++; return `p${i}`; }, cover_change: { new: [image] } }));
  const paths = Array.from({ length: 80 }, (_, i) => `p${i}`);
  const h = coverHarness(cover, paths, { changes }, { changes: [] }); await h.run();
  assert.equal(h.calls.adopt[0][0].changes.length, 80);
  assert.ok(reads <= 4 * paths.length, `path reads: ${reads}`);
});

test('auto-numbering touches selected index entries without walking unselected tracks', async () => {
  const tracks = Array.from({ length: 80 }, (_, i) => ({ path: `p${i}`, tags: { track: String(i) } }));
  const h = numberHarness(number, tracks, ['p60', 'p2']); await h.run();
  assert.equal(h.calls.libraryVisits, 0);
  assert.equal(h.calls.indexReads, 2);
  assert.deepEqual(plain([...h.edits].map(([p, fields]) => [p, [...fields]])), [
    ['p60', [['track', '1'], ['tracktotal', '2']]], ['p2', [['track', '2'], ['tracktotal', '2']]],
  ]);
  assert.equal(h.calls.render, 1); assert.equal(h.calls.preview, 1);
});

test('auto-numbering keeps vinyl sides, grouped restart/totals and last duplicate path semantics', async () => {
  const h = numberHarness(number, [
    { path: 'a', group: 'old', tags: { track: '3' } },
    { path: 'b', group: 'x', tags: { track: 'A1' } },
    { path: 'a', group: 'x', tags: { track: '4' } },
    { path: 'c', group: 'y', tags: { track: '2' } },
    { path: 'd', group: 'x', tags: { track: '5' } },
  ], ['missing', 'a', 'b', 'c', 'd'], {
    'autonum-start': { value: '7' }, 'autonum-per-group': { checked: true },
    'autonum-keep-sides': { checked: true }, 'autonum-disc': { value: '02' },
  }); await h.run();
  assert.equal(h.edits.has('b'), false);
  assert.deepEqual([...h.edits].map(([p, f]) => [p, [...f.values()]]), [ ['a', ['7', '2', '2']], ['c', ['7', '1', '2']], ['d', ['8', '2', '2']] ]);
});

test('selectedPaths retains mapping order, duplicates, hidden membership and snapshot invalidation', () => {
  state.selection.clear();
  const tracks = Array.from({ length: 40 }, (_, i) => ({ path: `p${i}`, tags: {} }));
  state.setTracks(tracks);
  for (const count of [0, 1, 9, 10, 40]) {
    state.selection.clear(); state.selection.add('unknown');
    for (let i = count - 1; i >= 0; i--) state.selection.add(`p${i}`);
    const expected = state.tracks.filter(t => state.selection.has(t.path)).map(t => t.path);
    assert.deepEqual(state.selectedPaths(), expected);
    assert.equal(state.selectedPaths(), state.selectedPaths());
  }
  tracks.reverse(); state.reindexTracks();
  assert.deepEqual(state.selectedPaths(), tracks.map(t => t.path));
  state.setTracks([{ path: 'p1' }, { path: 'p1' }, { path: 'p2' }]);
  assert.deepEqual(state.selectedPaths(), ['p1', 'p1', 'p2']);
  state.selection.clear(); state.setTracks([]);
});

test('auto-numbering keeps empty selection, invalid disc and all-vinyl early exits', async () => {
  const tracks = [{ path: 'a', group: 'x', tags: { track: 'A1' } }];
  for (const [paths, options] of [
    [[], {}], [['a'], { 'autonum-disc': { value: '2.5' } }],
    [['a'], { 'autonum-keep-sides': { checked: true } }],
  ]) {
    const h = numberHarness(number, tracks, paths, options); await h.run();
    assert.equal(h.edits.size, 0); assert.equal(h.calls.render, 0); assert.equal(h.calls.preview, 0);
    assert.equal(h.calls.toast.length, 1);
  }
});


test('native cover removals never inspect the unused pending plan', async () => {
  const pending = { get changes() { throw new Error('pending plan is unnecessary'); } };
  const plan = { changes: [change('a'), change('b')] };
  const h = coverHarness(cover, ['a', 'b'], pending, plan);
  await h.run();
  assert.equal(h.calls.adopt.length, 1);
  assert.equal(h.calls.adopt[0][0], plan);
});

test('one pending cover removal retains an early-exit lookup', async () => {
  let laterReads = 0;
  const pending = { changes: [change('a'), ...Array.from({ length: 80 }, (_, i) => ({
    get path() { laterReads++; return `later${i}`; }, cover_change: { new: [image] },
  }))] };
  const h = coverHarness(cover, ['a'], pending, { changes: [] });
  await h.run();
  assert.equal(h.calls.adopt[0][0].changes.length, 1);
  assert.equal(laterReads, 0);
});
