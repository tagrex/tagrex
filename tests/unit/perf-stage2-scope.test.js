import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';

// Execute exact production function bodies; no app boot, native bridge or media.
const root = resolve(process.env.TAGREX_SCOPE_SOURCE || '.');
function source(file, name) {
  const text = readFileSync(resolve(root, file), 'utf8');
  const match = text.match(new RegExp(`^(?:export )?(?:async )?function ${name}\\([^]*?^}`, 'm'));
  assert.ok(match, `Missing source function ${name}`);
  return match[0].replace(/^export /, '');
}
function harness(overrides = {}) {
  const nodes = new Map();
  const node = () => ({ dataset: {}, textContent: '', setAttribute(k, v) { this[k] = v; }, getAttribute(k) { return this[k] ?? null; } });
  const calls = { validity: 0, scope: 0, event: 0, panels: 0, save: 0, paints: [], sets: 0 };
  const c = vm.createContext({
    filterPending: false, filterError: false,
    calls, previewPlan: { changes: [{ path: 'a', tag_changes: [] }] }, automaticPlan: null,
    automaticVersion: 0, automaticTimer: null, automaticQueuedRun: null, clearTimeout() {},
    automaticBusy: false, automaticBlocked: false, automaticFailed: false,
    automaticKey: null, dismissedKey: null, liveBusy: false, live: false,
    invalid: new Set(), locks: [], lockedFieldKeys: () => c.locks,
    presetsFor: () => ['preset'], chainContext: () => 'editor',
    isLiveImportBusy: () => c.liveBusy, hasLiveInputs: () => c.live,
    paths: ['a', 'hidden'], selectedPaths: () => c.paths,
    navPathIndex: new Set(['a']), viewPathIndex: new Set(['a']), viewPaths: ['a'],
    diffByPath: new Map([['a', {}]]), applySelection: new Set(['a']),
    applyBtn: { ...node(), disabled: false }, selectAll: {}, editing: false,
    tracksBody: { querySelector: () => c.editing },
    el: id => { if (!nodes.has(id)) nodes.set(id, node()); return nodes.get(id); },
    msg: key => key, tn: (_key, count) => `${count} files`,
    paintRowSelection() {}, spreadsheet: { refresh() {} }, updateStatus() {}, updatePanelCounts() {},
    selectionPanelsPending: false, selectionPanelSubject: 'old', contentRevision: 0,
    selectionVersion: () => 1, isAutomaticTransformRefresh: () => c.refreshing,
    cancelSelectionPanels() {}, scheduleSelectionPanels: () => calls.panels++,
    scheduleSessionSave: () => calls.save++,
    CustomEvent: class { constructor(type) { this.type = type; } },
    document: { dispatchEvent(event) { assert.equal(event.type, 'tagrex:transform-scope'); calls.event++; c.updateTransformScope(); } },
    diskCoverSummary: { total: 2, with_cover: 0, distinct: false, samples: [] },
    coverSubject: { paths: ['a', 'hidden'] }, lastRenderedCoverSummary: null,
    current: true, isSelectionCurrent: () => c.current,
    renderCoverWell: summary => calls.paints.push(JSON.parse(JSON.stringify(summary))),
    Set: class extends Set { constructor(...args) { super(...args); calls.sets++; } },
    ...overrides,
  });
  for (const [file, names] of [
    ['app/ui/js/live-preview.js', ['hasInvalidPreview', 'hasInvalidLiveFields']],
    ['app/ui/js/generator.js', ['isAutomaticTransformPreview', 'automaticTransformScope', 'cancelAutomaticRequest', 'announceAutomaticScope', 'invalidateAutomaticTransformPreview']],
    ['app/ui/app.js', ['updateDiffBar', 'updateTransformScope', 'syncSelectionUI']],
    ['app/ui/js/cover.js', ['renderPendingCoverSummary']],
  ]) for (const name of names) vm.runInContext(source(file, name), c);
  vm.runInContext(`const validate = hasInvalidPreview; hasInvalidPreview = plan => { calls.validity++; return validate(plan); };
    const scope = automaticTransformScope; automaticTransformScope = () => { calls.scope++; return scope(); };`, c);
  return { c, calls, nodes };
}

const badChanges = [
  { invalid: true }, { tag_changes: [{ invalid: true }] },
  { cover_change: { invalid: true } },
  ...['old', 'new'].flatMap(side => [
    { cover_change: { [side]: [{ invalid: true, data_base64: 'inline' }] } },
    { cover_change: { [side]: [{ invalid: true, asset_id: 'ref' }] } },
    { block_changes: [{ [side]: { covers: [{ invalid: true }] } }] },
  ]),
  { block_changes: [{ invalid: true }] },
];

test('scope and Apply retain every nested validation gate in manual and automatic plans', () => {
  for (const automatic of [false, true]) for (const change of [{}, ...badChanges]) {
    const { c } = harness();
    c.previewPlan.changes.push(change);
    if (automatic) c.automaticPlan = c.previewPlan;
    c.updateTransformScope(); c.updateDiffBar();
    assert.equal(c.applyBtn.disabled, change !== undefined && Object.keys(change).length > 0);
  }
});

test('live field locks affect automatic scope but manual Apply only validates its own plan', () => {
  for (const automatic of [false, true]) for (const locked of [false, true]) {
    const { c, nodes } = harness();
    c.invalid.add('a\0title'); c.locks = locked ? ['title'] : [];
    if (automatic) c.automaticPlan = c.previewPlan;
    c.updateTransformScope(); c.updateDiffBar();
    assert.equal(c.applyBtn.disabled, automatic && !locked);
    assert.equal(nodes.get('status-transform-scope').textContent, locked ? `${Number(automatic)} modified · 1 selected hidden` : 'Fix invalid values');
  }
});

test('each automatic synchronous rail update validates the plan once; independent calls revalidate mutable plans', () => {
  const { c, calls } = harness(); c.automaticPlan = c.previewPlan;
  c.updateTransformScope(); assert.equal(calls.validity, 1); assert.equal(calls.scope, 1);
  c.previewPlan.changes[0].invalid = true;
  c.updateDiffBar(); assert.equal(c.applyBtn.disabled, true); assert.equal(calls.validity, 2);
  c.previewPlan = { changes: [{}] }; c.automaticPlan = c.previewPlan;
  c.updateTransformScope(); assert.equal(c.applyBtn.disabled, false);
  c.previewPlan = null; c.automaticPlan = null;
  c.updateTransformScope(); assert.equal(calls.validity, 4);
});

test('busy, failed, dismissed, blocked, editing and empty Apply states preserve priority', () => {
  for (const [flag, text, disabled] of [
    ['automaticBusy', 'Previewing…', true], ['liveBusy', 'Previewing…', true],
    ['automaticFailed', 'Preview failed', true], ['automaticBlocked', 'Review current preview first', false],
    ['dismissed', 'Preview cleared', false], ['editing', '1 modified · 1 selected hidden', true],
    ['empty', '1 modified · 1 selected hidden', true],
  ]) {
    const { c, nodes } = harness(); c.automaticPlan = c.previewPlan;
    if (flag === 'dismissed') c.automaticKey = c.dismissedKey = 'same';
    else if (flag === 'empty') c.applySelection.clear(); else c[flag] = true;
    c.updateTransformScope();
    assert.equal(nodes.get('status-transform-scope').textContent, text);
    assert.equal(c.applyBtn.disabled, disabled);
  }
});

test('selection sync keeps one public announcement and fallback scope updates, with unchanged timer scheduling', () => {
  for (const deferPanels of [false, true]) for (const refreshing of [false, true]) for (const changed of [false, true]) {
    const { c, calls } = harness({ diffByPath: null, refreshing });
    c.selectionPanelSubject = changed ? 'old' : '1:0';
    c.syncSelectionUI({ deferPanels, refreshPanels: true });
    const announced = changed && !refreshing;
    assert.equal(calls.event, Number(announced));
    assert.equal(calls.scope, Number(announced || !deferPanels));
    assert.equal(calls.save, Number(!deferPanels));
    assert.equal(calls.panels, Number(!refreshing && !deferPanels));
  }
  const { c, calls } = harness(); // manual preview early return
  c.syncSelectionUI(); assert.equal(calls.event, 0); assert.equal(calls.save, 0);
});

test('cover summaries allocate no selection Set for tag-only/empty plans and repaint after discard/replacement', () => {
  const { c, calls } = harness();
  c.renderPendingCoverSummary(); assert.equal(calls.sets, 0);
  const image = { mime: 'image/png', data_base64: 'full-quality', kind: 'front' };
  c.previewPlan = { changes: [{ path: 'a', cover_change: { old: [], new: [image] } }] };
  c.renderPendingCoverSummary(); assert.equal(calls.sets, 1);
  assert.equal(calls.paints.at(-1).pendingArtwork, true);
  assert.equal(calls.paints.at(-1).samples[0].data_base64, 'full-quality');
  c.previewPlan = { changes: [{ path: 'elsewhere', cover_change: { old: [], new: [image] } }] };
  c.renderPendingCoverSummary(); assert.equal(calls.paints.at(-1).pendingArtwork, false);
  c.previewPlan = null; c.renderPendingCoverSummary();
  assert.equal(calls.paints.length, 3); // identical disk summary is not repainted
  c.current = false; c.renderPendingCoverSummary(); assert.equal(calls.paints.length, 3);
});


test('explicit Apply independently rejects a plan mutated after its last rail update', async () => {
  const { c } = harness(); c.automaticPlan = c.previewPlan;
  c.updateTransformScope(); assert.equal(c.applyBtn.disabled, false);
  // Execute the exact production guard prefix, stopping before transaction setup.
  const applySource = source('app/ui/app.js', 'apply');
  const boundary = applySource.indexOf('  const automaticRename =');
  assert.ok(boundary > 0, 'Apply guard boundary changed; review this test');
  vm.runInContext(applySource.slice(0, boundary) + "  return 'guards-passed';\n}", c);
  for (const change of badChanges) {
    c.previewPlan.changes.push(change);
    assert.equal(await c.apply(), undefined);
    c.previewPlan.changes.pop();
  }
  assert.equal(await c.apply(), 'guards-passed');
});


test('hidden counts need no filtered selection array, and missing diff/rail remain cheap', () => {
  const { c, nodes, calls } = harness();
  c.paths.filter = () => { throw Error('Selection filter allocation'); };
  c.updateTransformScope();
  assert.equal(nodes.get('status-transform-scope').textContent, '0 modified · 1 selected hidden');
  const before = calls.validity; c.diffByPath = null; c.updateDiffBar();
  assert.equal(calls.validity, before);
  const absent = harness({ el: () => null }); absent.c.updateTransformScope();
  assert.equal(absent.calls.validity, 0);
});

// Filtering may temporarily leave old rows on screen; never Apply that scope.
test('pending or invalid filters block Apply without bypassing plan validation', () => {
  for (const flags of [{ filterPending: true }, { filterError: 'invalid regex' }]) {
    const { c, nodes } = harness(flags);
    c.updateDiffBar();
    assert.equal(c.applyBtn.disabled, true);
    assert.equal(nodes.get('ab-state').textContent, flags.filterPending ? 'diff.updating' : 'diff.checkChanges');
  }
});
