import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';

export function functionSource(text, name) {
  const match = text.match(new RegExp(`^(?:export )?(?:async )?function ${name}\\([^]*?^}`, 'm'));
  if (!match) throw new Error(`Missing source function ${name}`);
  return match[0].replace(/^export /, '');
}
export function loadFunction(file, name, root = process.cwd()) {
  if (process.env.TAGREX_INTERACTION_BASELINE) return JSON.parse(readFileSync(process.env.TAGREX_INTERACTION_BASELINE, 'utf8'))[name];
  return functionSource(readFileSync(resolve(root, file), 'utf8'), name);
}
export function coverHarness(source, paths, pending, plan, current = true) {
  const calls = { adopt: [], toast: [], invoke: [] };
  const subject = { paths };
  const context = vm.createContext({
    captureSelection: () => subject, previewPlan: pending,
    invoke: async (...args) => { calls.invoke.push(args); return plan; },
    isSelectionCurrent: () => typeof current === 'function' ? current() : current,
    adoptLiveCoverPlan: (...args) => calls.adopt.push(args),
    toast: (...args) => calls.toast.push(args), t: key => key, tn: (_, n) => n,
  });
  vm.runInContext(source, context);
  return { run: () => context.previewCoverRemove(), calls };
}
export function numberHarness(source, tracks, paths, options = {}) {
  const calls = { indexReads: 0, libraryVisits: 0, render: 0, preview: 0, toast: [] };
  const index = new Map(tracks.map(track => [track.path, track]));
  const observedTracks = new Proxy(tracks, { get(target, key, receiver) {
    if (/^\d+$/.test(String(key))) calls.libraryVisits++;
    return Reflect.get(target, key, receiver);
  } });
  const edits = new Map();
  const values = { 'autonum-start': { value: '1' }, 'autonum-total': { checked: true },
    'autonum-per-group': { checked: false }, 'autonum-keep-sides': { checked: false },
    'autonum-disc': { value: '' }, ...options };
  const context = vm.createContext({ selectedPaths: () => paths, tracks: observedTracks,
    trackAt: path => { calls.indexReads++; return index.get(path); },
    el: id => values[id], edits, groupBy: 'folder', groupKeyOf: track => track.group,
    isVinylSide: value => !!value?.trim() && !/^\d+$/.test(value.trim()),
    hooks: { renderTracks: () => calls.render++, previewEdits: async () => calls.preview++ },
    toast: (...args) => calls.toast.push(args), t: key => key, tn: (_, n) => n,
  });
  vm.runInContext(source, context);
  return { run: () => context.numberTracks(), calls, edits };
}
