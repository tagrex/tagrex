import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readdir } from 'node:fs/promises';
import { en } from '../../app/ui/js/i18n/en.js';

const directory = new URL('../../app/ui/js/i18n/', import.meta.url);
const catalogues = await Promise.all((await readdir(directory))
  .filter(file => file.endsWith('.js') && file !== 'en.js')
  .map(async file => ({ language: file.slice(0, -3), entries: (await import(new URL(file, directory).href))[file.slice(0, -3)] })));

test('every translation exports the complete English catalogue key set', () => {
  assert.ok(catalogues.length > 0);
  const expected = Object.keys(en).sort();
  for (const { language, entries } of catalogues) {
    assert.deepEqual(Object.keys(entries).sort(), expected, `${language} has missing or unknown catalogue keys`);
  }
});

test('translated preset messages are nonempty and retain their named placeholders', () => {
  const placeholders = value => [...new Set([...value.matchAll(/\{(\w+)\}/g)].map(match => match[1]))].sort();
  for (const [key, value] of Object.entries(en).filter(([key]) => key.startsWith('presets.'))) {
    for (const { language, entries } of catalogues) {
      assert.equal(typeof entries[key], 'string', `${language}: missing ${key}`);
      assert.ok(entries[key].trim(), `${language}: empty ${key}`);
      assert.deepEqual(placeholders(entries[key]), placeholders(value), `${language}: placeholders differ for ${key}`);
    }
  }
});
