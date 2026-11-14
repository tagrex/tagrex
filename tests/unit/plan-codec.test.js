import { test } from 'node:test';
import assert from 'node:assert/strict';
import { compactPlan, hydratePlan, createPlanCodec } from '../../app/ui/js/plan-codec.js';

const art = { mime: 'image/jpeg', data_base64: 'YWJjZA==', kind: 'front', description: 'Original' };
const plan = () => ({ description: 'Artwork batch', changes: Array.from({ length: 3 }, (_, i) => ({
  path: `/music/${i}.flac`, tag_changes: [],
  cover_change: { old: [{ ...art }], new: [{ ...art, kind: 'back' }] },
  block_changes: [{ kind: 'vorbis', old_bytes_base64: 'EXACT', old: { covers: [{ ...art }], fields: [] }, new: null }],
})) });

test('older backends receive complete inline images until pool support is advertised', () => {
  const codec = createPlanCodec();
  const request = { plan: plan() };
  assert.equal(codec.prepare(request), request);
  codec.accept(plan());
  assert.equal(codec.prepare(request).plan.changes[0].cover_change.new[0].data_base64, art.data_base64);
  codec.accept({ changes: [], cover_assets: [] });
  assert.deepEqual(codec.prepare(request).plan.cover_assets, [art.data_base64]);
  assert.equal(codec.prepare(request).plan.changes[0].cover_change.new[0].data_ref, 0);
  const nested = createPlanCodec();
  nested.accept({ plan: compactPlan(plan()) });
  assert.deepEqual(nested.prepare(request).plan.cover_assets, [art.data_base64]);
});

test('a batch carries each exact artwork payload once across cover and block changes', () => {
  const original = plan();
  const saved = structuredClone(original);
  const wire = compactPlan(original);
  assert.deepEqual(wire.cover_assets, ['YWJjZA==']);
  for (const change of wire.changes) {
    assert.equal(change.cover_change.old[0].data_base64, '');
    assert.equal(change.cover_change.old[0].data_ref, 0);
    assert.equal(change.block_changes[0].old.covers[0].data_ref, 0);
    assert.equal(change.block_changes[0].old_bytes_base64, 'EXACT');
  }
  assert.deepEqual(original, saved, 'pending UI plan must not be mutated');
  const hydrated = hydratePlan(wire);
  assert.deepEqual(hydrated.changes, original.changes);
  assert.deepEqual(compactPlan(hydrated), wire, 'Apply and transform requests must stay compact');
});

test('legacy inline plans and cover-free plans preserve their contents', () => {
  assert.deepEqual(hydratePlan(plan()), plan());
  const empty = { description: 'Tags', changes: [{ path: 'x', tag_changes: [] }] };
  assert.deepEqual(hydratePlan(empty), empty);
  assert.deepEqual(hydratePlan({ ...empty, cover_assets: [] }), empty);
  assert.equal(hydratePlan(empty).changes, empty.changes, 'tag-only plans do not clone every file');
  assert.equal(compactPlan(empty).changes, empty.changes);
});

test('invalid asset references fail before they can turn into missing artwork', () => {
  for (const ref of [-1, 1, 0.5, '0']) {
    const wire = plan();
    wire.cover_assets = ['YWJjZA=='];
    wire.changes[0].cover_change.old[0] = { ...art, data_base64: '', data_ref: ref };
    assert.throws(() => hydratePlan(wire), /artwork reference/i);
  }
});

test('conflicting inline and referenced bytes are rejected and metadata stays per cover', () => {
  const wire = compactPlan(plan());
  wire.changes[0].cover_change.old[0].data_base64 = 'DIFFERENT';
  assert.throws(() => hydratePlan(wire), /artwork reference/i);
  const hydrated = hydratePlan(compactPlan(plan()));
  assert.equal(hydrated.changes[0].cover_change.new[0].kind, 'back');
  assert.equal(hydrated.changes[0].cover_change.old[0].kind, 'front');
});


test('live base plans pool repeated artwork before IPC without changing raw pending images', () => {
  const codec = createPlanCodec();
  const original = plan();
  const request = {basePlan:original,edits:[{path:'/music/0.flac',field:'title',value:'New title'}]};
  assert.equal(codec.prepare(request),request);
  codec.accept({changes:[],cover_assets:[]});
  const wire = codec.prepare(request);
  assert.deepEqual(wire.basePlan.cover_assets,[art.data_base64]);
  assert.equal(wire.basePlan.changes[1].cover_change.new[0].data_base64,'');
  assert.equal(original.changes[1].cover_change.new[0].data_base64,art.data_base64);
  assert.deepEqual(hydratePlan(wire.basePlan).changes,original.changes);
  assert.equal(wire.edits,request.edits);
});
