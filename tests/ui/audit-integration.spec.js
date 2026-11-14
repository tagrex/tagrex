import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';

const root = resolve('.test-dist');
const errors = new WeakMap();
test.beforeEach(async ({ page }) => {
  const pageErrors = [];
  errors.set(page, pageErrors);
  page.on('pageerror', error => pageErrors.push(error.message));
  await page.route('http://tagrex.test/**', async route => {
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

test.afterEach(async ({ page }) => {
  expect(errors.get(page)).toEqual([]);
});

async function settings(page) {
  await page.locator('#settings-open').click();
  await expect(page.getByRole('dialog', { name: 'Settings', exact: true })).toBeVisible();
}

test('dynamic checkbox proxies have one component owner', async ({ page }) => {
  await settings(page);
  const box = page.locator('#set-import-fields input[type="checkbox"]').first();
  await expect(box).toHaveAttribute('data-native-proxy');
  await expect(box).not.toHaveAttribute('data-component-element', 'input');
  await expect(box).not.toHaveAttribute('data-slot', 'input');
  const id = await box.getAttribute('id');
  await expect(page.locator(`[data-proxy-id="${id}"]`)).toHaveCount(1);
});

test('settings reopening and shortcut capture preserve mounted control identities', async ({ page }) => {
  await settings(page);
  await page.evaluate(() => {
    window.auditSettingsNodes = [
      document.querySelector('#set-shortcuts .shortcut-combo'),
      document.querySelector('#settings-nav-list button'),
      document.querySelector('#set-import-fields input'),
    ];
  });
  await page.locator('#settings-close').click();
  await expect(page.getByRole('dialog', { name: 'Settings', exact: true })).toBeHidden();
  await settings(page);
  expect(await page.evaluate(() => window.auditSettingsNodes.every(node => node?.isConnected))).toBe(true);
  await page.locator('#settings-search').fill('Shortcuts');
  const combo = page.locator('#set-shortcuts .shortcut-combo').first();
  await combo.click();
  await expect(combo).toHaveClass(/capturing/);
  await page.keyboard.press('Escape');
  await expect(combo).not.toHaveClass(/capturing/);
  expect(await page.evaluate(() => document.querySelector('#set-shortcuts .shortcut-combo') === window.auditSettingsNodes[0])).toBe(true);
});

async function library(page) {
  await page.locator('#root-display').click();
  await page.locator('#root').fill('/music');
  await page.locator('#root').press('Enter');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
}

test('unchanged column menus retain their controls', async ({ page }) => {
  await library(page);
  await page.locator('#view-btn').click();
  await page.locator('#columns-btn').click();
  const menu = page.locator('#columns-menu');
  await expect(menu.locator('input[type="checkbox"]').first()).toHaveAttribute('data-native-proxy');
  await menu.evaluate(node => { window.auditColumnControl = node.querySelector('input[type="checkbox"]'); });
  await page.keyboard.press('Escape');
  await expect(menu).toBeHidden();
  // Escape closes the child chooser while retaining its View tray.
  await page.locator('#columns-btn').click();
  expect(await page.evaluate(() => document.querySelector('#columns-menu input[type="checkbox"]') === window.auditColumnControl)).toBe(true);
});

test('typing a cell completion updates existing suggestion controls', async ({ page }) => {
  await library(page);
  const cell = page.locator('#tracks-body td[data-field="artist"]').first();
  await cell.dblclick();
  await page.keyboard.press('Control+a');
  await page.keyboard.insertText('W');
  const menu = page.locator('.sg-menu');
  await expect(menu.locator('.sg-row').first()).toBeVisible();
  await menu.evaluate(node => { window.auditSuggestion = node.querySelector('.sg-row'); });
  await page.keyboard.insertText('i');
  await expect(menu.locator('.sg-row').first()).toBeVisible();
  expect(await page.evaluate(() => document.querySelector('.sg-menu .sg-row') === window.auditSuggestion)).toBe(true);
  await expect(cell).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(menu).toBeHidden();
  await expect(cell).toHaveAttribute('contenteditable', 'true');
});

test('virtual rows mount no selection checkboxes or per-control reset listeners', async ({ page }) => {
  await page.evaluate(() => {
    window.auditResetListeners = 0;
    const add = document.addEventListener.bind(document);
    document.addEventListener = function (type, ...args) {
      if (type === 'reset') window.auditResetListeners++;
      return add(type, ...args);
    };
  });
  await page.evaluate(async () => {
    await window.tagrex.invoke('open_drop', { paths: Array.from({ length: 100 }, (_, i) => `track-${i}.mp3`) });
    document.getElementById('root').value = '/dropped';
    await window.tagrex.openLibrary();
  });
  await expect(page.locator('#tracks-body tr[data-path]')).not.toHaveCount(0);
  await expect(page.locator('#tracks-body [data-slot="checkbox"]')).toHaveCount(0);
  expect(await page.evaluate(() => window.auditResetListeners)).toBeLessThanOrEqual(1);
});
