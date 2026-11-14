import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
import { setImmediate as flush } from 'node:timers/promises';

const text = readFileSync(process.env.TAGREX_ONLINE_SOURCE || new URL('../../app/ui/js/online.js', import.meta.url), 'utf8');
const ast = ts.createSourceFile('online.js', text, ts.ScriptTarget.Latest, true, ts.ScriptKind.JS);
const functions = ['applyImage', 'prefetchReleaseCounts'];
const bodies = ast.statements.filter(n => ts.isFunctionDeclaration(n) && functions.includes(n.name?.text)).map(n => n.getText(ast)).join('\n');
function setup(invoke) {
  const painted = [];
  const context = vm.createContext({
    releaseSource: 'musicbrainz', searchGen: 1,
    imageCache: new Map(), coverCache: new Map(), releaseCache: new Map(),
    THUMB_AFTER_LOOKUP: new Set(['musicbrainz']), PREFETCH_CONCURRENCY: 4,
    providerToken: async () => '', invoke,
    coverElOf: () => ({}), setCoverImage: (_, uri) => painted.push(uri),
    updateLoadMoreUi() {}, updateMediaBadge() {}, countPillOf: () => null,
    prefetching: false, rateLimitPauseMs: () => null,
  });
  vm.runInContext(bodies, context);
  return { context, painted };
}
const candidate = { id: 'shared-id', thumb_url: 'https://example.test/front-250' };
test('MusicBrainz thumbnails wait for cover confirmation and use the small URL', async () => {
  const calls = [];
  const { context } = setup(async (name, args) => { calls.push({ name, args }); return { mime: 'image/png', data_base64: 'small' }; });
  await context.applyImage(candidate);
  context.releaseCache.set(candidate.id, { cover_image_url: null });
  await context.applyImage(candidate);
  assert.equal(calls.length, 0);
  context.releaseCache.set(candidate.id, { cover_image_url: 'full' });
  await context.applyImage(candidate);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].args.url, candidate.thumb_url);
});
test('a late thumbnail cannot replace the selected full artwork', async () => {
  let resolve;
  const { context, painted } = setup(() => new Promise(r => { resolve = r; }));
  context.releaseCache.set(candidate.id, { cover_image_url: 'full' });
  const pending = context.applyImage(candidate);
  await flush();
  context.coverCache.set(candidate.id, { mime: 'image/png', data_base64: 'selected' });
  resolve({ mime: 'image/png', data_base64: 'small' });
  await pending;
  assert.equal(painted.length, 0);
});
test('superseded thumbnail downloads cannot repopulate the new search cache', async () => {
  let resolve;
  const { context, painted } = setup(() => new Promise(r => { resolve = r; }));
  context.releaseCache.set(candidate.id, { cover_image_url: 'full' });
  const pending = context.applyImage(candidate);
  await flush();
  context.searchGen++; context.imageCache.clear(); context.releaseSource = 'discogs';
  resolve({ mime: 'image/png', data_base64: 'old' });
  await pending;
  assert.equal(context.imageCache.size, 0);
  assert.equal(painted.length, 0);
});
test('superseded release lookups cannot seed caches or thumbnail work', async () => {
  let resolve;
  const calls = [];
  const { context } = setup((name) => { calls.push(name); return new Promise(r => { resolve = r; }); });
  const pending = context.prefetchReleaseCounts([candidate], 1);
  await flush();
  context.searchGen++; context.releaseCache.clear(); context.releaseSource = 'discogs';
  resolve({ cover_image_url: 'old-front' });
  await pending;
  assert.equal(context.releaseCache.size, 0);
  assert.deepEqual(calls, ['provider_fetch_release']);
});
