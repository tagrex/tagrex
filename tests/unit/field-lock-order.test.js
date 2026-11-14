import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';

async function locksWith(invoke) {
  // Load the real module with only its browser/IPC imports replaced. Data-URL
  // ES modules use Node's stable loader; each instance gets its own lock state.
  const stubKey = `tagrex-lock-test-${randomUUID()}`;
  let source = await readFile(new URL('../../app/ui/js/locks.js', import.meta.url), 'utf8');
  globalThis[stubKey] = invoke;
  const dependencies = {
    '../lib/component-elements.js': 'export const createComponentElement = () => null;',
    './dom.js': 'export const ico = () => "";',
    './i18n.js': 'export const t = () => "";',
    './hooks.js': 'export const hooks = { renderTableHead() {}, renderTracks() {} };',
    './invoke.js': `export const invoke = globalThis[${JSON.stringify(stubKey)}];`,
  };
  const moduleUrl = body => `data:text/javascript;base64,${Buffer.from(body).toString('base64')}`;
  try {
    for (const [specifier, body] of Object.entries(dependencies)) {
      const quoted = JSON.stringify(specifier);
      assert.equal(source.split(quoted).length, 2, `expected one import of ${specifier}`);
      source = source.replace(quoted, JSON.stringify(moduleUrl(body)));
    }
    return await import(moduleUrl(`${source}\n// isolated test: ${stubKey}`));
  } finally {
    delete globalThis[stubKey];
  }
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
