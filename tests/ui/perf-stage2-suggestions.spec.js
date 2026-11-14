import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';

// Cell-value autocomplete behaviour that the reduction in per-keystroke work
// must not change: the eight-row prefix-first cap, the highlighted typed part,
// pooled row buttons, and the "type it and press Enter" gesture.
//
// The list's ordering and matching are covered exhaustively in
// tests/unit/perf-stage2-suggestions.test.js against the production functions.
// This file is the integration contract around them.
const root = resolve('.test-dist');

test.beforeEach(async ({ page }) => {
  await page.route('http://tagrex.test/**', async (route) => {
    const path = resolve(root, '.' + new URL(route.request().url()).pathname);
    if (!path.startsWith(root + sep)) return route.abort();
    try {
      const contentType = path.endsWith('.js') ? 'text/javascript' : path.endsWith('.css') ? 'text/css' : path.endsWith('.woff2') ? 'font/woff2' : 'text/html';
      await route.fulfill({ body: await readFile(path), contentType });
    } catch { await route.fulfill({ status: 404, body: 'Missing fixture' }); }
  });
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(() => page.evaluate(() => typeof window.tagrex?.setMode)).toBe('function');
});

// Ten folders give ten distinct artist spellings (two files each), which is
// enough to exercise the eight-row cap without a real library.
async function folders(page, count = 10) {
  await page.evaluate(async (paths) => {
    await window.tagrex.invoke('open_drop', { paths });
    document.getElementById('root').value = '/dropped';
    await window.tagrex.openLibrary();
  }, Array.from({ length: count }, (_, index) => `folder-${index + 1}`));
  await expect(page.locator('#tracks-body tr[data-path]')).not.toHaveCount(0);
  await expect(page.locator('#tracks-body td[data-field="artist"]').filter({ hasText: /^Folder 2$/ }).first()).toBeVisible();
}

// Order-independent: whichever row currently carries Folder 2.
const folderTwoCell = (page) => page.locator('#tracks-body td[data-field="artist"]').filter({ hasText: /^Folder 2$/ }).first();

const cell = (page, field = 'artist') => page.locator(`#tracks-body td[data-field="${field}"]`).first();
const menu = (page) => page.locator('.sg-menu');
const rows = (page) => menu(page).locator('.sg-row:not([hidden])');

async function edit(page, field = 'artist') {
  const target = cell(page, field);
  await target.dblclick();
  await expect(target).toHaveAttribute('contenteditable', 'true');
  return target;
}

test('typing shows at most eight prefix rows, highlights the typed part and reuses the row buttons', async ({ page }) => {
  await folders(page);
  const target = await edit(page);
  await page.keyboard.press('Control+a');
  await page.keyboard.insertText('folder');
  await expect(rows(page)).toHaveCount(8);
  const visible = await rows(page).locator('.sg-value').allTextContents();
  expect(visible.every((value) => value.toLowerCase().startsWith('folder'))).toBe(true);
  await expect(rows(page).first().locator('.sg-value b')).toHaveText('Folder');
  await expect(rows(page).first().locator('.sg-value')).toHaveText('Folder 1');
  expect(await page.evaluate(() => document.querySelectorAll('.sg-menu .sg-row').length)).toBe(8);

  // The button a keystroke lands on is the same node the previous keystroke
  // used: the list updates labels instead of mounting a control per keypress.
  await menu(page).evaluate((node) => { window.auditSuggestionRow = node.querySelector('.sg-row'); });
  await page.keyboard.insertText(' 1');
  await expect(rows(page).first()).toBeVisible();
  expect(await page.evaluate(() => document.querySelector('.sg-menu .sg-row') === window.auditSuggestionRow)).toBe(true);
  await expect(target).toBeFocused();

  // A value that is already the only offer has no answer left: the list hides
  // and the cell keeps its text.
  await page.keyboard.press('Control+a');
  await page.keyboard.insertText('Folder 7');
  await expect(menu(page)).toBeHidden();
  await expect(target).toHaveAttribute('contenteditable', 'true');
});

test('arrow keys step into the list; Enter only takes a row once stepped in; Escape only closes the list', async ({ page }) => {
  await folders(page);
  // A cell that does not already hold the first suggestion, so accepting the
  // first row is visible.
  const path = await folderTwoCell(page).getAttribute('data-path');
  const target = page.locator(`#tracks-body td[data-field="artist"][data-path=${JSON.stringify(path)}]`);
  await target.dblclick();
  // An existing value filters its own list. Clear it to choose from all values.
  await page.keyboard.press('Control+a');
  await page.keyboard.press('Backspace');
  await page.keyboard.press('ArrowDown');
  await expect(menu(page)).toBeVisible();
  const first = (await rows(page).first().locator('.sg-value').textContent()) ?? '';
  expect(first).toBe('Folder 1');
  await expect(rows(page).first()).toHaveClass(/active/);
  await page.keyboard.press('Enter');
  await expect(menu(page)).toBeHidden();
  await expect(target).toHaveText('Folder 1');

  const second = page.locator('#tracks-body td[data-field="artist"]').nth(1);
  await second.dblclick();
  await page.keyboard.press('ArrowDown');
  await expect(menu(page)).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(menu(page)).toBeHidden();
  await expect(second).toHaveAttribute('contenteditable', 'true');

  const third = page.locator('#tracks-body td[data-field="artist"]').nth(2);
  await third.dblclick();
  await page.keyboard.press('Control+a');
  await page.keyboard.insertText('folder');
  await expect(menu(page)).toBeVisible();
  await page.keyboard.press('Enter');
  await expect(menu(page)).toBeHidden();
  // Nobody stepped into the list, so Enter committed exactly what was typed.
  await expect(third).toHaveText('folder');
});

test('fields where a repeat is meaningless never suggest', async ({ page }) => {
  await folders(page, 2);
  await edit(page, 'title');
  await page.keyboard.press('ArrowDown');
  await expect(menu(page)).toBeHidden();
  await expect(menu(page).locator('.sg-row')).toHaveCount(0);
});
