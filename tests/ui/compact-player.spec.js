import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';

const root = resolve('.test-dist');
const errors = new WeakMap();
test.beforeEach(async ({ page }) => {
  const issues = [];
  errors.set(page, issues);
  page.on('pageerror', error => issues.push(error.message));
  await page.addInitScript(() => { window.playerCalls = {}; });
  await page.route('http://tagrex.test/**', async route => {
    const path = resolve(root, '.' + new URL(route.request().url()).pathname);
    if (!path.startsWith(root + sep)) return route.abort();
    try {
      let body = await readFile(path);
      if (path.endsWith('.js')) {
        body = Buffer.from(body.toString().replace(/case([`"'])(player_play|player_pause|player_resume|player_stop|player_seek|player_set_volume|read_cover_summary)\1:/g,
          (match, _quote, command) => `${match}window.playerCalls['${command}']=(window.playerCalls['${command}']||0)+1;`));
      }
      await route.fulfill({ body, contentType: path.endsWith('.js') ? 'text/javascript' : path.endsWith('.css') ? 'text/css' : path.endsWith('.woff2') ? 'font/woff2' : 'text/html' });
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
  await expect(page.locator('#pl-toggle')).toBeEnabled();
}

for (const width of [1280, 375]) {
  test(`player stays visible and keeps its height through idle, play, pause and stop at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 800 });
    const player = page.locator('#player');
    await expect(player).toBeVisible();
    await expect(page.locator('#pl-seek')).toBeDisabled();
    await expect(page.locator('#pl-toggle')).toBeDisabled();
    await expect(player.locator('canvas')).toHaveCount(0);
    const initial = await player.boundingBox();
    expect(initial.height).toBeLessThanOrEqual(36);
    await library(page);
    const idle = await player.boundingBox();
    await page.locator('#pl-toggle').click();
    await expect(player).not.toHaveClass(/idle/);
    await expect(page.locator('#pl-title')).toContainText('Desert Rain');
    await expect(page.locator('#pl-seek')).toBeEnabled();
    await expect(page.locator('#pl-time')).toContainText('/ 10:00');
    await page.locator('#pl-toggle').click();
    await expect.poll(() => page.evaluate(() => window.playerCalls.player_pause || 0)).toBe(1);
    await page.locator('#pl-toggle').click();
    await expect.poll(() => page.evaluate(() => window.playerCalls.player_resume || 0)).toBe(1);
    await page.locator('#pl-stop').click();
    await expect(player).toHaveClass(/idle/);
    await expect(page.locator('#pl-seek')).toBeDisabled();
    await expect(page.locator('#pl-time')).toHaveText('0:00 / 0:00');
    const stopped = await player.boundingBox();
    expect(stopped.height).toBe(initial.height);
    expect(stopped.height).toBe(idle.height);
    expect(stopped.x).toBeGreaterThanOrEqual(0);
    expect(stopped.x + stopped.width).toBeLessThanOrEqual(width);
    await expect(page.locator('#pl-repeat')).toBeVisible();
    await expect(page.locator('#pl-volume')).toBeVisible();
  });
}

test('native seek, track navigation, repeat and inline volume remain operable', async ({ page }) => {
  await library(page);
  await page.evaluate(() => { window.playerCalls = {}; });
  await page.locator('#pl-toggle').click();
  await expect(page.locator('#pl-seek')).toBeEnabled();
  await expect(page.locator('#pl-time')).toContainText('/ 10:00');
  await page.locator('#pl-seek').focus();
  await page.keyboard.press('ArrowRight');
  await expect.poll(() => page.evaluate(() => window.playerCalls.player_seek || 0)).toBeGreaterThan(0);
  await page.locator('#pl-seek').evaluate(input => { input.value = '500'; input.dispatchEvent(new Event('input', { bubbles: true })); input.dispatchEvent(new Event('change', { bubbles: true })); });
  await expect(page.locator('#pl-time')).toContainText('5:00 / 10:00');
  await expect(page.locator('#pl-seek')).toHaveAttribute('aria-valuetext', '5:00 / 10:00');
  await page.locator('#pl-next').click();
  await expect(page.locator('#pl-title')).toContainText('Radio');
  await page.locator('#pl-prev').click();
  await expect(page.locator('#pl-title')).toContainText('Desert Rain');
  await page.locator('#pl-repeat').click();
  await expect(page.locator('#pl-repeat')).toHaveAttribute('aria-label', 'Repeat all');
  await page.locator('#pl-repeat').click();
  await expect(page.locator('#pl-repeat')).toHaveAttribute('aria-label', 'Repeat this track');
  await page.locator('#pl-volume').evaluate(input => { input.value = '35'; input.dispatchEvent(new Event('input', { bubbles: true })); });
  await expect.poll(() => page.evaluate(() => localStorage.getItem('tagrex.volume'))).toBe('0.35');
  await page.locator('#pl-mute').click();
  await expect(page.locator('#pl-volume')).toHaveValue('0');
  await page.locator('#pl-mute').click();
  await expect(page.locator('#pl-volume')).toHaveValue('35');
  await expect(page.locator('#player canvas')).toHaveCount(0);
});

test('idle text follows the UI language without replacing a playing track title', async ({ page }) => {
  async function language(value) {
    await page.evaluate(value => {
      const select = document.getElementById('set-lang');
      select.value = value;
      select.dispatchEvent(new Event('change', { bubbles: true }));
    }, value);
  }
  await language('de');
  await expect(page.locator('#pl-title')).not.toHaveText('No track loaded');
  await library(page);
  await page.locator('#pl-toggle').click();
  await expect(page.locator('#pl-title')).toContainText('Desert Rain');
  await language('en');
  await expect(page.locator('#pl-title')).toContainText('Desert Rain');
  await expect(page.locator('#pl-title')).not.toHaveAttribute('data-i18n', 'player.noTrack');
  await page.locator('#pl-stop').click();
  await expect(page.locator('#pl-title')).toHaveText('No track loaded');
});
