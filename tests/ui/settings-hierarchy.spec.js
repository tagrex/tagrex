import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';
const root = resolve('.test-dist');
test.beforeEach(async ({ page }) => {
  await page.route('http://tagrex.test/**', async route => {
    const path = resolve(root, '.' + new URL(route.request().url()).pathname);
    if (!path.startsWith(root + sep)) return route.abort();
    try { await route.fulfill({ body: await readFile(path), contentType: path.endsWith('.js') ? 'text/javascript' : path.endsWith('.css') ? 'text/css' : path.endsWith('.woff2') ? 'font/woff2' : 'text/html' }); }
    catch { await route.fulfill({ status: 404, body: 'Missing fixture' }); }
  });
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(() => page.evaluate(() => typeof window.tagrex?.setMode)).toBe('function');
  await page.locator('#settings-open').click();
  await page.locator('#settings-nav-list button').first().click();
});
for (const theme of ['light', 'dark']) for (const width of [1280, 540]) {
  test(`settings keep flat hierarchy and controls contained in ${theme} at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 850 });
    await page.locator(`#set-theme [data-theme-mode="${theme}"]`).click();
    const sizes = await page.locator('.settings-group[data-reset-group="general"]').evaluate(group => {
      const heading = group.querySelector('h3'); const field = group.querySelector('.settings-field');
      return { title: parseFloat(getComputedStyle(heading).fontSize), label: parseFloat(getComputedStyle(field.querySelector('label,.settings-label')).fontSize), help: parseFloat(getComputedStyle(field.querySelector('.settings-hint')).fontSize), fieldBackground: getComputedStyle(field).backgroundColor, fieldBorder: getComputedStyle(field).borderTopWidth };
    });
    expect(sizes.title).toBeGreaterThan(sizes.label); expect(sizes.label).toBeGreaterThan(sizes.help);
    expect(sizes.fieldBackground).toBe('rgba(0, 0, 0, 0)'); expect(sizes.fieldBorder).toBe('0px');
    const bounds = await page.locator('.settings-body').boundingBox();
    for (const selector of ['#set-theme', '#set-palette-control', '#set-lang-control']) {
      const b = await page.locator(selector).boundingBox();
      expect(b.x).toBeGreaterThanOrEqual(bounds.x); expect(b.x + b.width).toBeLessThanOrEqual(bounds.x + bounds.width);
    }
    await expect(page.locator('#settings-save')).toBeInViewport();
    await expect(page.locator('#settings-cancel')).toBeInViewport();
  });
}
test('subsection search retains later matching controls and their heading', async ({ page }) => {
  await page.locator('#settings-search').fill('Appearance');
  await expect(page.locator('#set-theme')).toBeVisible(); await expect(page.locator('#set-palette-control')).toBeVisible(); await expect(page.locator('#set-lang-control')).toBeHidden();
  await page.locator('#settings-search').fill('Theme');
  await expect(page.locator('#set-palette-control')).toBeVisible(); await expect(page.locator('#set-theme')).toBeHidden();
  await expect(page.locator('h4[data-i18n="settings.appearance"]')).toBeVisible();
  await page.locator('#settings-search').fill('JPEG quality');
  await expect(page.locator('#set-cover-quality')).toBeVisible(); await expect(page.locator('h4[data-i18n="editor.coverArt"]')).toBeVisible();
  await expect(page.locator('#set-cover-max')).toBeHidden();
  await page.locator('#settings-search').press('Escape');
  await expect(page.locator('#settings-search')).toHaveValue(''); await expect(page.locator('#settings')).toBeVisible();
});
test('settings category keyboard navigation and language selection stay connected', async ({ page }) => {
  const nav = page.locator('#settings-nav-list button'); await nav.first().focus(); await page.keyboard.press('ArrowDown');
  await expect(nav.nth(1)).toBeFocused(); await expect(nav.nth(1)).toHaveAttribute('aria-selected', 'true');
  await page.keyboard.press('ArrowUp'); await expect(nav.first()).toHaveAttribute('aria-selected', 'true');
  await page.locator('#set-lang-control').click();
  await page.getByRole('option', { name: 'Deutsch', exact: true }).click();
  await expect(page.locator('h4[data-i18n="settings.appearance"]')).toHaveText('Darstellung');
  await expect(page.locator('#set-lang')).toHaveValue('de');
});

test('switches remain aligned with their label and retain native label activation', async ({ page }) => {
  await page.locator('#settings-search').fill('Safe file names');
  const label = page.locator('label[for="set-portable-names-control"]');
  const toggle = page.locator('#set-portable-names-control');
  const before = await toggle.getAttribute('aria-checked'); await label.click();
  await expect(toggle).toHaveAttribute('aria-checked', before === 'true' ? 'false' : 'true');
  const field = await toggle.locator('xpath=ancestor::div[contains(@class,"settings-field")]').boundingBox();
  const bounds = await toggle.boundingBox(); expect(Math.abs(bounds.x + bounds.width - field.x - field.width)).toBeLessThan(2);
});


test('multiple help paragraphs remain separate at narrow widths', async ({ page }) => {
  await page.setViewportSize({ width: 540, height: 850 });
  for (const term of ['JPEG quality', 'Skip files longer than']) {
    await page.locator('#settings-search').fill(term);
    const help = page.locator('.settings-field:not(.search-hidden) .settings-help');
    // Measure together so an entering dialog cannot move between reads.
    const { first, second, control } = await help.evaluate(e => ({
      first: e.children[0].getBoundingClientRect().toJSON(),
      second: e.children[1].getBoundingClientRect().toJSON(),
      control: e.parentElement.querySelector('input').getBoundingClientRect().toJSON(),
    }));
    expect(second.y).toBeGreaterThanOrEqual(first.y + first.height);
    expect(control.y).toBeGreaterThanOrEqual(second.y + second.height);
  }
});

for (const theme of ['light', 'dark']) test(`fields have one focus outline in ${theme}`, async ({ page }) => {
  await page.locator(`#set-theme [data-theme-mode="${theme}"]`).click();
  await page.keyboard.press('Tab');
  for (const selector of ['#settings-search', '#set-lang-control']) {
    const control = page.locator(selector);
    await control.focus();
    await expect(control).toHaveCSS('outline-style', 'solid');
    await expect(control).toHaveCSS('outline-width', '2px');
    await expect(control).toHaveCSS('outline-offset', '0px');
    await expect(control).toHaveCSS('box-shadow', 'none');
    await expect(control).toHaveCSS('border-color', 'rgba(0, 0, 0, 0)');
  }
  await page.locator('#set-lang-control').click();
  await expect(page.locator('.app-select-menu[data-state="open"]')).toHaveCSS('box-shadow', /rgba\(0, 0, 0, 0\.24\)/);
  await page.keyboard.press('Escape');
  await expect(page.locator('.settings-nav')).toHaveCSS('border-right-width', '0px');
  await expect(page.locator('.settings-body')).toHaveCSS('border-radius', '14px');
  await expect(page.locator('#settings-search')).toHaveCSS('border-radius', '10px');
  await page.locator('#settings-cancel').click();
  await page.locator('.mode-tab[data-mode="renamer"]').click();
  await page.keyboard.press('Tab');
  await page.locator('#mask').focus();
  const group = page.locator('.field').filter({ has: page.locator('#mask') });
  await expect(group).toHaveCSS('outline-style', 'solid');
  await expect(group).toHaveCSS('outline-width', '2px');
  await expect(group).toHaveCSS('box-shadow', 'none');
  await expect(group).toHaveCSS('border-color', 'rgba(0, 0, 0, 0)');
  await expect(page.locator('#mask')).toHaveCSS('outline-style', 'none');
  const accent = await group.evaluate(e => getComputedStyle(e).outlineColor);
  await group.evaluate(e => e.classList.add('has-error'));
  await expect(group).not.toHaveCSS('outline-color', accent);
});
