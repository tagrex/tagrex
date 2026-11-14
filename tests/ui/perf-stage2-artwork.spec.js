import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';

// Artwork picker: a fetch that outlives its selection must not be decoded.
//
// Decode calls are counted directly on HTMLImageElement.prototype.decode, and
// "the fetch has settled" is signalled by a sentinel-delay timer wrapper rather
// than by waiting a wall-clock margin. Nothing here measures duration: the
// assertions are exact counts and exact state, so a slow machine cannot make
// them flaky or make an unoptimised build pass.
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
  await page.addInitScript(() => {
    window.__decodeCalls = 0;
    const decode = HTMLImageElement.prototype.decode;
    HTMLImageElement.prototype.decode = function (...args) {
      window.__decodeCalls++;
      return decode.apply(this, args);
    };
    // The fixture's fetch reads its delay from window.artworkTest.delays. A
    // delay of exactly SENTINEL is the test's completion signal: when its
    // callback runs, the fetch promise has resolved (or rejected) and the
    // picker's continuation is the next microtask.
    window.__fixtureTimersRun = 0;
    const nativeSetTimeout = window.setTimeout;
    window.setTimeout = function (callback, delay, ...args) {
      if (delay === 777) {
        return nativeSetTimeout.call(this, function () {
          window.__fixtureTimersRun++;
          return callback.apply(this, arguments);
        }, delay, ...args);
      }
      return nativeSetTimeout.call(this, callback, delay, ...args);
    };
  });
});

const dialog = (page) => page.getByRole('dialog', { name: 'Choose artwork' });
const tile = (page, number) => dialog(page).getByRole('button', { name: `Image ${number} of`, exact: false });
const cta = (page) => dialog(page).getByRole('button', { name: 'Use', exact: true });
const hero = (page) => dialog(page).locator('.aw-frame:not([inert]) .aw-hero');
const decodes = (page) => page.evaluate(() => window.__decodeCalls);
const timersRun = (page) => page.evaluate(() => window.__fixtureTimersRun);

async function open(page, query = '') {
  await page.goto(`http://tagrex.test/tests/ui/fixture/index.html${query}`);
  await page.getByRole('button', { name: 'Open artwork' }).click();
  await expect(dialog(page)).toBeVisible();
  await expect(cta(page)).toBeEnabled();
}

async function settled(page, count) {
  await expect.poll(() => timersRun(page)).toBeGreaterThanOrEqual(count);
}

test('a superseded fetch is not decoded once it resolves', async ({ page }) => {
  await open(page, '?count=40');
  await expect(hero(page)).toHaveAttribute('alt', 'Image 1 of 40');
  await page.evaluate(() => {
    window.__decodeCalls = 0;
    window.artworkTest.delays[35] = 777;
    window.artworkTest.delays[36] = 777;
    // Same task: the second selection supersedes the first before either fetch
    // can settle.
    document.querySelector('.aw-thumb[data-index="35"]').click();
    document.querySelector('.aw-thumb[data-index="36"]').click();
  });
  await expect(hero(page)).toHaveAttribute('alt', 'Image 37 of 40');
  await settled(page, 2);
  expect(await decodes(page)).toBe(1);
  await expect(cta(page)).toBeEnabled();
});

test('A to B and back to A decodes only the selection that is still current', async ({ page }) => {
  await open(page, '?count=40');
  await page.evaluate(() => {
    window.__decodeCalls = 0;
    window.artworkTest.delays[35] = 777;
    window.artworkTest.delays[36] = 777;
    document.querySelector('.aw-thumb[data-index="35"]').click();
    document.querySelector('.aw-thumb[data-index="36"]').click();
    document.querySelector('.aw-thumb[data-index="35"]').click();
  });
  await expect(hero(page)).toHaveAttribute('alt', 'Image 36 of 40');
  await settled(page, 2);
  expect(await decodes(page)).toBe(1);
  await expect(cta(page)).toBeEnabled();
  // The shared fetch promise served both visits to the same image.
  const calls = await page.evaluate(() => window.artworkTest.calls.filter((index) => index === 35).length);
  expect(calls).toBe(1);
});

test('closing during a pending fetch leaves it undecoded and returns focus', async ({ page }) => {
  await open(page, '?count=40');
  await page.evaluate(() => {
    window.__decodeCalls = 0;
    window.artworkTest.delays[35] = 777;
    document.querySelector('.aw-thumb[data-index="35"]').click();
    document.querySelector('.aw-close').click();
  });
  await expect(dialog(page)).toHaveCount(0);
  await settled(page, 1);
  expect(await decodes(page)).toBe(0);
  await expect.poll(() => page.evaluate(() => window.artworkResult)).toBe(null);
  await expect(page.locator('#launch')).toBeFocused();
});

test('a stale failure never replaces the current image or shows its error', async ({ page }) => {
  await open(page, '?count=40');
  await page.evaluate(() => {
    window.__decodeCalls = 0;
    window.artworkTest.delays[35] = 777;
    window.artworkTest.delays[36] = 777;
    window.artworkTest.fail = 35;
    document.querySelector('.aw-thumb[data-index="35"]').click();
    document.querySelector('.aw-thumb[data-index="36"]').click();
  });
  await settled(page, 2);
  await expect(hero(page)).toHaveAttribute('alt', 'Image 37 of 40');
  await expect(dialog(page).getByRole('button', { name: 'Retry' })).toHaveCount(0);
  expect(await decodes(page)).toBe(1);
  await expect(cta(page)).toBeEnabled();
});

test('a failed current image still offers Retry and decodes once on success', async ({ page }) => {
  await open(page, '?count=40');
  await page.evaluate(() => {
    window.__decodeCalls = 0;
    window.artworkTest.delays[35] = 777;
    window.artworkTest.fail = 35;
    document.querySelector('.aw-thumb[data-index="35"]').click();
  });
  await settled(page, 1);
  await expect(dialog(page).getByRole('button', { name: 'Retry' })).toBeVisible();
  await expect(cta(page)).toBeDisabled();
  expect(await decodes(page)).toBe(0);
  await page.evaluate(() => { window.artworkTest.fail = -1; window.artworkTest.delays[35] = 20; });
  await dialog(page).getByRole('button', { name: 'Retry' }).click();
  await expect(cta(page)).toBeEnabled();
  await expect(hero(page)).toHaveAttribute('alt', 'Image 36 of 40');
  expect(await decodes(page)).toBe(1);
});
