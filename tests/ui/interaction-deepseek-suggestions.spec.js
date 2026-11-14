import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';

// Cell autocomplete interaction: the pooled row nodes, dismissal on scroll, and
// the mouse path. The ordering/matching rules live in
// tests/unit/perf-stage2-suggestions.test.js and the Unicode marking invariant in
// tests/unit/interaction-deepseek-highlight.test.js; this file is the browser
// contract around the list element itself.
const root = resolve(process.env.TAGREX_INTERACTION_DIST || '.test-dist');

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

async function folders(page, count = 10) {
  await page.evaluate(async (paths) => {
    await window.tagrex.invoke('open_drop', { paths });
    document.getElementById('root').value = '/dropped';
    await window.tagrex.openLibrary();
  }, Array.from({ length: count }, (_, index) => `folder-${index + 1}`));
  await expect(page.locator('#tracks-body tr[data-path]')).not.toHaveCount(0);
  await expect(page.locator('#tracks-body td[data-field="artist"]').first()).toBeVisible();
}

const menu = (page) => page.locator('.sg-menu');
const rows = (page) => menu(page).locator('.sg-row:not([hidden])');
const cell = (page, field = 'artist') => page.locator(`#tracks-body td[data-field="${field}"]`).first();

async function edit(page, field = 'artist') {
  const target = cell(page, field);
  await target.dblclick();
  await expect(target).toHaveAttribute('contenteditable', 'true');
  return target;
}

test('a repaint reuses the pooled row and its label node', async ({ page }) => {
  await folders(page);
  const target = await edit(page);
  await page.keyboard.press('Control+a');
  await page.keyboard.insertText('folder');
  await expect(rows(page)).toHaveCount(8);
  await menu(page).evaluate((node) => {
    window.__row = node.querySelector('.sg-row');
    window.__label = node.querySelector('.sg-row .sg-value');
    window.__count = node.querySelector('.sg-row .sg-count');
  });
  await page.keyboard.insertText(' 1');
  await expect(rows(page).first().locator('.sg-value b')).toHaveText('Folder 1');
  expect(await page.evaluate(() => {
    const row = document.querySelector('.sg-menu .sg-row');
    return row === window.__row
      && row.querySelector('.sg-value') === window.__label
      && row.querySelector('.sg-count') === window.__count;
  })).toBe(true);
  await expect(target).toBeFocused();
});

test('scrolling dismisses the list, keeps the edit, and the next keystroke reopens it', async ({ page }) => {
  // Enough rows that the table really scrolls.
  await folders(page, 40);
  const path = await cell(page).getAttribute('data-path');
  const target = page.locator(`#tracks-body td[data-field="artist"][data-path=${JSON.stringify(path)}]`);
  await target.dblclick();
  await expect(target).toHaveAttribute('contenteditable', 'true');
  await page.keyboard.press('Control+a');
  await page.keyboard.insertText('folder');
  await expect(menu(page)).toBeVisible();
  await expect(rows(page)).toHaveCount(8);
  // A scroll inside the table, small enough to keep the edited row in the
  // renderer's overscan — the cell, not the list, is what this test is about.
  const table = page.locator('#files-view');
  await table.evaluate((node) => { node.scrollTop = 90; });
  await expect.poll(() => table.evaluate((node) => node.scrollTop)).toBeGreaterThan(0);
  await expect(menu(page)).toBeHidden();
  // The edit is untouched: same cell, same text, still editable.
  await expect(target).toHaveAttribute('contenteditable', 'true');
  await expect(target).toHaveText('folder');
  // A dismissed list leaves nothing behind to reappear later.
  expect(await page.evaluate(() => document.querySelectorAll('.sg-menu .sg-row').length)).toBe(0);
  // Scrolling again with no list up is a no-op, not an error or a reopen.
  await table.evaluate((node) => { node.scrollTop = 120; });
  await expect(menu(page)).toBeHidden();
  // The next keystroke re-filters and repaints from scratch.
  await page.keyboard.press('Control+a');
  await page.keyboard.insertText('folder');
  await expect(rows(page)).toHaveCount(8);
  await expect(rows(page).first().locator('.sg-value')).toHaveText('Folder 1');
  await expect(target).toBeFocused();
});

test('a mouse choice stages its value and leaves the cell in edit mode', async ({ page }) => {
  await folders(page);
  const target = await edit(page);
  await page.keyboard.press('Control+a');
  await page.keyboard.insertText('fold');
  await expect(rows(page)).toHaveCount(8);
  const chosenRow = rows(page).nth(2);
  const chosen = (await chosenRow.locator('.sg-value').textContent()) ?? '';
  expect(chosen).not.toBe('Folder 1');
  // A real press-and-release, so the cell's focus surviving is the browser's
  // own default being prevented, not a missing gesture.
  const box = await chosenRow.boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.up();
  await expect(menu(page)).toBeHidden();
  await expect(target).toHaveText(chosen);
  await expect(target).toHaveAttribute('contenteditable', 'true');
  await expect(target).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(target).not.toHaveAttribute('contenteditable', 'true');
});


for (const mode of ['isComposing', 'keyCode229']) test(`IME confirmation via ${mode} keeps the cell editable until plain Enter`, async ({ page }) => {
  await folders(page);
  const target = await edit(page, 'title');
  const path = await target.getAttribute('data-path');
  await page.keyboard.press('Control+a'); await page.keyboard.insertText('に');
  const prevented = await target.evaluate((node, mode) => {
    node.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true, data: 'に' }));
    const event = new KeyboardEvent('keydown', { key: 'Enter', code: 'Enter', bubbles: true, cancelable: true,
      isComposing: mode === 'isComposing', keyCode: mode === 'keyCode229' ? 229 : 13 });
    node.dispatchEvent(event); return event.defaultPrevented;
  }, mode);
  expect(prevented).toBe(false);
  await expect(target).toHaveAttribute('contenteditable', 'true'); await expect(target).toBeFocused();
  await target.evaluate(node => node.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: '日本語' })));
  await page.keyboard.press('Control+a'); await page.keyboard.insertText('日本語'); await page.keyboard.press('Enter');
  await expect(target).not.toHaveAttribute('contenteditable', 'true');
  await expect.poll(() => page.evaluate(path => window.tagrex.previewPlan?.changes.find(change => change.path === path)?.tag_changes.find(field => field.field === 'title')?.new, path)).toBe('日本語');
});
