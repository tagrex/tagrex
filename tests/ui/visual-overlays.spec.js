import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';

// Run against the parent's freshly rebuilt fixture, never against the native app.
const root = resolve('.test-dist');
test.use({ reducedMotion: 'reduce' });
test.beforeEach(async ({ page }) => {
  await page.route('http://tagrex.test/**', async route => {
    const path = resolve(root, '.' + new URL(route.request().url()).pathname);
    if (!path.startsWith(root + sep)) return route.abort();
    try {
      await route.fulfill({ body: await readFile(path), contentType: path.endsWith('.js') ? 'text/javascript' : path.endsWith('.css') ? 'text/css' : path.endsWith('.woff2') ? 'font/woff2' : 'text/html' });
    } catch { await route.fulfill({ status: 404, body: 'Missing fixture' }); }
  });
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(() => page.evaluate(() => typeof window.tagrex?.setMode)).toBe('function');
});

for (const theme of ['light', 'dark']) for (const width of [600, 360]) {
  test(`bounded placeholder keeps footer and keyboard scroll clearance (${theme}, ${width}px)`, async ({ page }) => {
    await page.setViewportSize({ width, height: 400 });
    await page.evaluate(theme => document.documentElement.dataset.theme = theme, theme);
    await page.locator('.mode-tab[data-mode="renamer"]').click();
    await page.locator('[data-ph-target="mask"]').click();
    const menu = page.locator('#ph-menu');
    const rows = menu.locator('button.ph-row');
    await expect(rows.first()).toBeVisible();
    // Exercise the real controller's inline placement contract on both updates.
    for (const available of [220, 180]) {
      await menu.evaluate((node, height) => { node.style.maxHeight = `${height}px`; }, available);
      await expect.poll(() => menu.evaluate(node => node.style.getPropertyValue('--controller-available-height'))).toBe(`${available}px`);
      const geometry = await menu.evaluate(node => {
        const surface = node.querySelector('.controller-popover-content');
        const body = node.querySelector('.ph-body');
        const footer = node.querySelector('.ph-foot');
        const s = surface.getBoundingClientRect(), b = body.getBoundingClientRect(), f = footer.getBoundingClientRect();
        return { height: s.height, scrolling: body.scrollHeight > body.clientHeight, overflow: getComputedStyle(body).overflowY, footerInside: f.bottom <= s.bottom && f.top >= b.bottom - 1, viewport: f.bottom <= innerHeight, radius: getComputedStyle(surface).borderRadius, padding: getComputedStyle(surface).paddingTop };
      });
      expect(geometry.height).toBeLessThanOrEqual(available + 1);
      expect(geometry).toMatchObject({ scrolling: true, overflow: 'auto', footerInside: true, viewport: true, radius: '16px', padding: '7px' });
    }
    for (const row of [rows.first(), rows.last()]) {
      await row.focus();
      await expect(row).toBeFocused();
      expect(await row.evaluate(node => {
        const b = node.closest('.ph-body').getBoundingClientRect(), r = node.getBoundingClientRect();
        return r.top >= b.top && r.bottom <= b.bottom;
      })).toBe(true);
      await expect(row).toHaveCSS('border-radius', '8px');
    }
    await page.keyboard.press('Tab');
    await expect(menu.locator('.ph-guide')).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(menu).toBeHidden();
  });
}

test('genre compound contour preserves keyboard selection and custom entry', async ({ page }) => {
  await page.locator('#root-display').click();
  await page.locator('#root').fill('/music');
  await page.locator('#root').press('Enter');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
  await page.locator('.subtab[data-subtab="editor"]').click();
  const input = page.locator('#fields-body input[data-key="genre"]');
  await input.fill('Ho');
  await input.press('ArrowDown');
  await expect(page.getByRole('option', { name: 'House', exact: true })).toBeVisible();
  await expect(page.locator('.genre-options')).toHaveCSS('border-radius', '16px');
  await expect(page.locator('.genre-options')).toHaveCSS('padding-top', '7px');
  await expect(page.locator('.genre-toggle')).toHaveCSS('border-top-right-radius', '10px');
  await expect(page.getByRole('option', { name: 'House', exact: true })).toHaveCSS('border-radius', '8px');
  await input.press('Enter');
  await expect(input).toHaveValue('House');
  await expect(input).toBeFocused();
  await input.fill('My custom genre');
  await expect(input).toHaveValue('My custom genre');
});

test('select menu keyboard endpoints keep list contours and return focus', async ({ page }) => {
  const trigger = page.locator('#online-source-control');
  await trigger.focus();
  await trigger.press('Enter');
  const menu = page.locator('.app-select-menu');
  await expect(menu).toBeVisible();
  await expect(menu).toHaveCSS('border-radius', '16px');
  await expect(menu).toHaveCSS('padding-top', '7px');
  for (const key of ['End', 'Home']) {
    await page.keyboard.press(key);
    const highlighted = menu.locator('[data-slot="select-item"][data-highlighted]');
    await expect(highlighted).toHaveCount(1);
    await expect(highlighted).toHaveCSS('border-radius', '8px');
  }
  await page.keyboard.press('Escape');
  await expect(menu).toHaveCount(0);
  await expect(trigger).toBeFocused();
  await expect(trigger).toHaveCSS('border-radius', '10px');
});
