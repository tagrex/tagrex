import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';
const root = resolve('.test-dist');

test.beforeEach(async ({ page }) => {
  await page.route('http://tagrex.test/**', async route => {
    const path = resolve(root, '.' + new URL(route.request().url()).pathname);
    if (!path.startsWith(root + sep)) return route.abort();
    try {
      let body = await readFile(path);
      if (path.endsWith('.js')) {
        // The test provider supplies a known list with one legitimate slash.
        body = Buffer.from(body.toString().replace(/styles:\s*\[(["'`])Trance\1,\s*\1Tribal\1,\s*\1Techno\1\]/g, 'styles:["Hardcore","Happy Hardcore","Pop/Rock"]'));
      }
      await route.fulfill({body, contentType:path.endsWith('.js') ? 'text/javascript' : path.endsWith('.css') ? 'text/css' : path.endsWith('.woff2') ? 'font/woff2' : 'text/html'});
    } catch { await route.fulfill({status:404,body:'Missing fixture'}); }
  });
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(() => page.evaluate(() => !!window.tagrex)).toBe(true);
});

for (const [separator, expected] of [['', 'Hardcore; Happy Hardcore; Pop/Rock'], [';', 'Hardcore;Happy Hardcore;Pop/Rock'], [' | ', 'Hardcore | Happy Hardcore | Pop/Rock']]) {
  test(`provider genre lists use ${separator ? `the explicit ${JSON.stringify(separator)} codec` : 'the semicolon default'} through preview and Apply`, async ({ page }) => {
    if (separator) {
      await page.locator('#settings-open').click();
      await page.locator('#settings-search').fill('Multi-value separator');
      await page.locator('#set-multi-sep').fill(separator);
      await page.locator('#settings-save').click();
    }
    await page.locator('#root-display').click();
    await page.locator('#root').fill('/music');
    await page.locator('#root').press('Enter');
    await page.evaluate(() => { document.getElementById('discogs-token').value = 'fixture'; });
    await page.locator('#discogs-query').fill('Helter Skelter');
    await page.locator('#discogs-search').click();
    const card = page.locator('.release-card').first();
    await card.locator('.release-head').click();
    await expect(card).toHaveClass(/tracklist-loaded/);
    await card.locator('.release-import').click();
    await expect.poll(() => page.evaluate(() => window.tagrex.previewPlan?.changes?.flatMap(change => change.tag_changes).find(change => change.field === 'genre')?.new)).toBe(expected);
    await page.locator('#diff-apply').click();
    await expect.poll(() => page.evaluate(() => window.tagrex.tracks[0].tags.genre)).toBe(expected);
  });
}
