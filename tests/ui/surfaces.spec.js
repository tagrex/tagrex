import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';

const root = resolve('.test-dist');
const errors = new WeakMap();
test.beforeEach(async ({ page }) => {
  const issues = [];
  errors.set(page, issues);
  page.on('pageerror', error => issues.push(error.message));
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
test.afterEach(async ({ page }) => expect(errors.get(page)).toEqual([]));

async function library(page) {
  await page.locator('#root-display').click();
  await page.locator('#root').fill('/music');
  await page.locator('#root').press('Enter');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
}

test('real dialog shells keep native children, focus, cancellation and exit presence', async ({ page }) => {
  await library(page);
  await page.locator('#tracks-body tr[data-path] td.file').first().click({ button: 'right' });
  await expect(page.locator('#file-ctx [data-slot="context-menu-content"]')).toBeVisible();
  await page.locator('#file-ctx [data-cmd="trash"]').click();
  const dialog = page.getByRole('alertdialog');
  await expect(dialog).toBeVisible();
  await expect(page.locator('#confirm-cancel')).toBeFocused();
  await page.keyboard.press('Escape');
  await expect.poll(() => page.evaluate(() => document.getElementById('confirm-modal').hidden)).toBe(true);
  await expect(page.locator('#confirm-modal')).toBeHidden();
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
  // Opening again proves the promise/listeners from the first prompt settled.
  await page.locator('#tracks-body tr[data-path] td.file').first().click({ button: 'right' });
  await page.locator('#file-ctx [data-cmd="trash"]').click();
  const closing = await page.evaluate(() => {
    const root = document.getElementById('confirm-modal');
    document.getElementById('confirm-cancel').click();
    return { logical: root.hidden, physical: root.hasAttribute('hidden'), animation: getComputedStyle(root.querySelector('[data-slot="dialog-content"]')).animationName };
  });
  expect(closing).toEqual({ logical: true, physical: false, animation: 'controller-exit' });
  await expect(page.locator('#confirm-modal')).toBeHidden();
  await expect(page.locator('#tracks-body tr.selected')).toHaveCount(1);
});

test('settings has a real focus scope and Escape restores its opener', async ({ page }) => {
  await page.locator('#settings-open').click();
  const dialog = page.getByRole('dialog', { name: 'Settings', exact: true });
  await expect(dialog).toBeVisible();
  await expect(dialog).toHaveAttribute('data-slot', 'dialog-content');
  await page.locator('#settings-search').focus();
  await page.keyboard.press('Shift+Tab');
  await expect.poll(() => page.evaluate(() => document.getElementById('settings').contains(document.activeElement))).toBe(true);
  await page.keyboard.press('Escape');
  await expect(page.locator('#settings')).toBeHidden();
  await expect(page.locator('#settings-open')).toBeFocused();
});

test('dynamic menu rebuilding preserves popover shell and column controller actions', async ({ page }) => {
  await library(page);
  await page.locator('#view-btn').click();
  await page.locator('#columns-btn').click();
  const menu = page.locator('#columns-menu');
  await expect(menu.locator('[data-slot="popover-content"]')).toBeVisible();
  await expect(menu.locator('input[type="checkbox"]')).not.toHaveCount(0);
  await page.keyboard.press('Escape');
  await expect(menu).toBeHidden();
  // Closing the child chooser leaves View open.
  await page.locator('#columns-btn').click();
  await expect(menu.locator('[data-slot="popover-content"]')).toHaveCount(1);
  await expect(menu.locator('input[type="checkbox"]')).not.toHaveCount(0);
  await page.locator('#columns-btn').click();
  await expect(menu).toBeHidden();
});

test('hover hints stay off while query suggestions and explicit help remain usable', async ({ page }) => {
  await library(page);
  const settings = page.locator('#settings-open');
  const settingsLabel = await settings.getAttribute('aria-label');
  await settings.focus();
  await settings.hover();
  await expect(settings).toHaveAttribute('aria-label', settingsLabel);
  await expect(settings).not.toHaveAttribute('title');

  await page.locator('#query-presets').click();
  const menu = page.locator('#query-preset-menu');
  const suggestion = menu.locator('.query-row').first();
  await expect(suggestion).toBeVisible();
  const query = await suggestion.locator('.query-text').textContent();
  await suggestion.hover();
  // Longer than both former custom hover delays; native bubbles have no title
  // attribute left to use, including on freshly generated suggestion buttons.
  await page.waitForTimeout(700);
  await expect(page.locator('#tooltip')).toBeHidden();
  await expect(page.locator('.cell-tip')).toHaveCount(0);
  await expect(suggestion).not.toHaveAttribute('title');
  await expect(suggestion).toHaveAttribute('aria-description', query);
  await expect(menu).toBeVisible();
  await suggestion.click();
  await expect(page.locator('#discogs-query')).toHaveValue(query);
  await expect(menu).toBeHidden();

  // A controller can rewrite titles after mount without bringing bubbles back.
  await settings.evaluate(button => button.title = 'Updated settings description');
  await expect(settings).not.toHaveAttribute('title');
  await expect(settings).toHaveAttribute('aria-description', 'Updated settings description');
  await expect(settings).toHaveAttribute('aria-label', settingsLabel);

  const cell = page.locator('#tracks-body td.editable').first();
  await cell.hover();
  await page.waitForTimeout(450);
  await expect(page.locator('.cell-tip')).toHaveCount(0);
  await cell.dblclick();
  await expect(cell).toHaveAttribute('contenteditable', 'true');
  await page.keyboard.press('Escape');

  await page.locator('.mode-tab[data-mode="renamer"]').click();
  await page.locator('[data-ph-target="mask"]').click();
  await expect(page.locator('#ph-menu')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('#ph-menu')).toBeHidden();
});

test('context menu keyboard selection preserves the text field editing path', async ({ page }) => {
  await page.locator('#filter-panel-toggle').click();
  await page.locator('#filter').fill('Keyboard selection');
  await page.locator('#filter').click({ button: 'right' });
  await expect(page.locator('#edit-ctx [data-slot="context-menu-content"]')).toBeFocused();
  await page.keyboard.press('End');
  await expect(page.locator('#edit-ctx [data-cmd="selectall"]')).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(page.locator('#edit-ctx')).toBeHidden();
  await expect(page.locator('#filter')).toBeFocused();
  expect(await page.locator('#filter').evaluate(input => [input.selectionStart, input.selectionEnd])).toEqual([0, 18]);
});

test('cell completion uses a real popover without blurring or writing the cell on open', async ({ page }) => {
  await library(page);
  const cell = page.locator('#tracks-body td[data-field="artist"]').first();
  await cell.dblclick();
  await page.keyboard.press('Control+a');
  await page.keyboard.insertText('W');
  const menu = page.locator('.sg-menu');
  await expect(menu.locator('[data-slot="popover-content"]')).toBeVisible();
  await expect(cell).toBeFocused();
  await expect(cell).toHaveAttribute('contenteditable', 'true');
  expect(await page.evaluate(() => window.tagrex.tracks[0].tags.artist)).toBe('The X Factor');
  await page.keyboard.press('Escape');
  await expect(menu).toBeHidden();
  await expect(cell).toHaveAttribute('contenteditable', 'true');
  await page.keyboard.insertText('i');
  await expect(menu.locator('.sg-row')).not.toHaveCount(0);
  const closing = await menu.evaluate(root => {
    root.querySelector('.sg-row').dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true }));
    return { hidden: root.hidden, logicalEmpty: root.innerHTML === '', visibleRows: root.querySelectorAll('.sg-row').length };
  });
  expect(closing.hidden).toBe(true);
  expect(closing.logicalEmpty).toBe(true);
  expect(closing.visibleRows).toBeGreaterThan(0);
  await expect(menu).toBeHidden();
});

test('standard and advanced field disclosures support native Enter and Space activation', async ({ page }) => {
  await library(page);
  await page.evaluate(() => { window.tagrex.tracks[0].tags['custom:RAW_TEST_FRAME'] = 'Raw metadata'; });
  await page.locator('#tracks-body tr[data-path]').nth(1).click();
  await page.locator('#tracks-body tr[data-path]').first().click();
  await page.locator('.subtab[data-subtab="editor"]').click();
  const standard = page.locator('button[aria-controls="editor-group-standard"]');
  const advanced = page.locator('button[aria-controls="editor-group-advanced"]');
  await expect(standard).toHaveAttribute('data-slot', 'button');
  await expect(standard).toHaveAttribute('aria-expanded', 'false');
  await expect(page.locator('#editor-group-standard')).toBeHidden();
  await standard.focus();
  await page.keyboard.press('Enter');
  await expect(standard).toHaveAttribute('aria-expanded', 'true');
  await page.keyboard.press('Space');
  await expect(standard).toHaveAttribute('aria-expanded', 'false');
  await expect(page.locator('#editor-group-standard')).toBeHidden();
  await page.keyboard.press('Enter');
  await expect(standard).toHaveAttribute('aria-expanded', 'true');
  await expect(page.locator('#editor-group-standard')).toBeVisible();
  await expect(advanced).toHaveAttribute('aria-expanded', 'false');
  await advanced.focus();
  await page.keyboard.press('Enter');
  await expect(advanced).toHaveAttribute('aria-expanded', 'true');
  await expect(page.locator('#editor-group-advanced')).toBeVisible();
  await page.keyboard.press('Space');
  await expect(advanced).toHaveAttribute('aria-expanded', 'false');
  await expect(page.locator('#editor-group-advanced')).toBeHidden();
});

test('Sonner retains report nodes and native dismiss listeners across repeated messages', async ({ page }) => {
  await library(page);
  const toast = page.locator('#toast');
  await expect(toast).toBeVisible();
  await expect(page.locator('[data-sonner-toast] #toast')).toHaveCount(1);
  await page.evaluate(() => {
    const root = document.getElementById('toast');
    root.hidden = true;
    document.getElementById('toast-title').textContent = 'Some files need attention';
    document.getElementById('toast-parts').innerHTML = '<li class="toast-part problem">One file could not be read</li>';
    document.getElementById('toast-parts').hidden = false;
    document.getElementById('toast-close').hidden = false;
    root.hidden = false;
  });
  await expect(toast).toContainText('One file could not be read');
  await page.locator('#toast-close').click();
  await expect.poll(() => page.evaluate(() => document.getElementById('toast').hidden)).toBe(true);
  await expect(page.locator('[data-sonner-toast]')).toHaveCount(0);
  await page.evaluate(() => { document.getElementById('toast').hidden = false; });
  await expect(page.locator('[data-sonner-toast] #toast')).toHaveCount(1);
  await page.locator('#toast-close').click();
  await expect(page.locator('[data-sonner-toast]')).toHaveCount(0);
});

for (const viewport of [{ width: 375, height: 667 }, { width: 844, height: 390 }, { width: 1280, height: 800 }]) {
  test(`mask dialog remains reachable at ${viewport.width}x${viewport.height}`, async ({ page }) => {
    await page.setViewportSize(viewport);
    await library(page);
    await page.locator('.mode-tab[data-mode="renamer"]').click();
    await page.locator('#mask-builder-open').click();
    await expect(page.locator('#mb-expr')).toBeVisible();
    const bounds = await page.locator('#mask-builder [data-slot="dialog-content"]').boundingBox();
    expect(bounds.x).toBeGreaterThanOrEqual(0);
    expect(bounds.x + bounds.width).toBeLessThanOrEqual(viewport.width + 1);
    await page.locator('#mb-cancel').click();
    await expect(page.locator('#mask-builder')).toBeHidden();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
  });
}
