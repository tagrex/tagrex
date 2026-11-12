import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';

async function locksWith(invoke) {
  const context = vm.createContext({ invokeStub: invoke });
  const source = await readFile(new URL('../../app/ui/js/locks.js', import.meta.url), 'utf8');
  const module = new vm.SourceTextModule(source, { context });
  const dependencies = {
    './dom.js': 'export const ico = () => "";',
    './i18n.js': 'export const t = () => "";',
    './hooks.js': 'export const hooks = { renderTableHead() {}, renderTracks() {} };',
    './invoke.js': 'export const invoke = globalThis.invokeStub;',
  };
  await module.link(specifier => new vm.SourceTextModule(dependencies[specifier], { context }));
  await module.evaluate();
  return module.namespace;
}
const tick = () => new Promise(resolve => setImmediate(resolve));

test('rapid lock clicks submit snapshots in order even while the first IPC is pending', async () => {
  const calls = [];
  const locks = await locksWith((cmd, args) => new Promise(resolve => calls.push({cmd, fields: [...args.fields], resolve})));
  const first = locks.toggleFieldLock(['artist']);
  const second = locks.toggleFieldLock(['title']);
  await tick();
  assert.equal(calls.length, 1, 'the second snapshot must wait before invoking IPC');
  assert.equal(calls[0].cmd, 'set_locked_fields');
  assert.deepEqual(calls[0].fields, ['artist']);
  calls[0].resolve();
  await tick();
  assert.equal(calls.length, 2);
  assert.deepEqual(calls[1].fields, ['artist', 'title']);
  calls[1].resolve();
  await Promise.all([first, second]);
});

test('a refused submission does not prevent the next library lock snapshot', async () => {
  const calls = [];
  const locks = await locksWith((cmd, args) => {
    calls.push([...args.fields]);
    return calls.length === 1 ? Promise.reject(new Error('no library open')) : Promise.resolve();
  });
  await locks.toggleFieldLock(['artist']);
  await locks.pushFieldLocks();
  assert.deepEqual(calls, [['artist'], ['artist']]);
});
