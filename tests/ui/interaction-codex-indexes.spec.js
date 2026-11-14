import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';
const root = resolve('.test-dist');
test.beforeEach(async ({ page }) => {
  page.interactionErrors = [];
  page.on('pageerror', error => page.interactionErrors.push(error.message));
  await page.addInitScript(() => localStorage.setItem('tagrex.groupBy', ''));
  await page.route('http://tagrex.test/**', async route => {
    const path = resolve(root, '.' + new URL(route.request().url()).pathname);
    if (!path.startsWith(root + sep)) return route.abort();
    try {
      await route.fulfill({ body: await readFile(path), contentType: path.endsWith('.js') ? 'text/javascript' : path.endsWith('.css') ? 'text/css' : path.endsWith('.woff2') ? 'font/woff2' : 'text/html' });
    } catch { await route.fulfill({ status: 404, body: 'Missing fixture' }); }
  });
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(() => page.evaluate(() => typeof window.tagrex?.openLibrary)).toBe('function');
  await page.locator('#root-display').click();
  await page.locator('#root').fill('/music'); await page.locator('#root').press('Enter');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
});
test.afterEach(async ({ page }) => expect(page.interactionErrors).toEqual([]));

test('auto-numbering stages mapping order after reverse Ctrl selection without modifying disk DTOs', async ({ page }) => {
  const before = await page.evaluate(() => structuredClone(window.tagrex.tracks));
  const rows = page.locator('#tracks-body tr[data-path]');
  await rows.nth(2).locator('td.file').click();
  await rows.nth(0).locator('td.file').click({ modifiers: ['Control'] });
  await page.locator('.mode-tab[data-mode="generator"]').click();
  await page.locator('#gen-job-number').click();
  await page.locator('#autonum-start').fill('7');
  await page.locator('#autonum-run').click();
  await expect(page.locator('#diff-apply')).toBeEnabled();
  const numbers = await page.evaluate(() => window.tagrex.previewPlan.changes.map(change => [change.path, change.tag_changes.find(field => field.field === 'track')?.new]));
  expect(numbers).toEqual([[before[0].path, '7'], [before[2].path, '8']]);
  expect(await page.evaluate(() => window.tagrex.tracks)).toEqual(before);
});

for (const count of [20000, 50000, 100000]) test(`select-all ${count} retains mapping order through filter and virtual scrolling`, async ({ page }) => {
  test.skip(!process.env.TAGREX_INTERACTION_LARGE, 'Parent serial scale slot only');
  test.setTimeout(30000);
  await page.evaluate(async count => {
    await window.tagrex.invoke('open_drop', { paths: Array.from({ length: count }, (_, i) => `track-${String(i).padStart(6, '0')}.mp3`) });
    document.getElementById('root').value = '/dropped'; await window.tagrex.openLibrary();
  }, count);
  // The selection checkbox column is hidden outside review; use the grid's
  // documented keyboard action, just as a user selects the whole library.
  await page.locator('#tracks-body tr[data-path] td.file').first().click();
  await page.keyboard.press('Control+a');
  const expected = await page.evaluate(() => window.tagrex.tracks.map(track => track.path));
  expect(await page.evaluate(() => window.tagrex.selectedPaths())).toEqual(expected);
  await page.locator('#filter-panel-toggle').click();
  await page.locator('#filter').fill('track-000000');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(1);
  expect(await page.evaluate(() => window.tagrex.selectedPaths())).toEqual(expected);
  await page.locator('#filter').fill('');
  await expect.poll(() => page.locator('#tracks-body tr[data-path]').count()).toBeGreaterThan(1);
  await page.locator('#files-view').evaluate(node => node.scrollTop = node.scrollHeight);
  await expect(page.locator(`#tracks-body tr[data-path="${expected.at(-1)}"]`)).toBeVisible();
  expect(await page.locator('#tracks-body tr[data-path]').count()).toBeLessThan(100);
  expect(await page.evaluate(() => window.tagrex.selectedPaths())).toEqual(expected);
});
