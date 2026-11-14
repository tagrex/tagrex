import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';

// Artwork picker: which selections actually need a decode.
//
// Decode calls are counted on HTMLImageElement.prototype.decode, and every
// assertion is an exact count or an exact state — no wall-clock margins, so a
// slow machine cannot turn a pass into a failure. The picker fixture serves a
// data-URI image per tile and records the fetches it was asked for.
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
    // Zero by default; a test raises it to keep decodes in flight long enough
    // to observe a second selection reusing the first one.
    window.__decodeDelay = 0;
    const decode = HTMLImageElement.prototype.decode;
    HTMLImageElement.prototype.decode = function (...args) {
      window.__decodeCalls++;
      const delay = window.__decodeDelay;
      if (!delay) return decode.apply(this, args);
      return new Promise((done, fail) => setTimeout(() => {
        Promise.resolve(decode.apply(this, args)).then(done, fail);
      }, delay));
    };
  });
});

const dialog = (page) => page.getByRole('dialog', { name: 'Choose artwork' });
const tile = (page, number) => dialog(page).getByRole('button', { name: `Image ${number} of`, exact: false });
const cta = (page) => dialog(page).getByRole('button', { name: 'Use', exact: true });
const hero = (page) => dialog(page).locator('.aw-frame:not([inert]) .aw-hero');
const decodes = (page) => page.evaluate(() => window.__decodeCalls);
const frame = (page) => page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));

async function open(page, query = '') {
  await page.goto(`http://tagrex.test/tests/ui/fixture/index.html${query}`);
  await page.getByRole('button', { name: 'Open artwork' }).click();
  await expect(dialog(page)).toBeVisible();
  await expect(cta(page)).toBeEnabled();
}

test('choosing the image already on show does not fetch or decode it again', async ({ page }) => {
  await open(page, '?count=1');
  await expect(hero(page)).toHaveAttribute('alt', 'Image 1 of 1');
  expect(await decodes(page)).toBe(1);
  await tile(page, 1).click();
  // Arrow navigation wraps onto the same single image.
  await page.keyboard.press('ArrowRight');
  await page.keyboard.press('Home');
  await frame(page);
  expect(await decodes(page)).toBe(1);
  expect(await page.evaluate(() => window.artworkTest.calls.length)).toBe(1);
  await expect(tile(page, 1)).toBeFocused();
  await expect(tile(page, 1)).toHaveAttribute('aria-pressed', 'true');
  await expect(cta(page)).toBeEnabled();
});

test('using a rotated image does not decode the pixels a second time', async ({ page }) => {
  await open(page, '?count=12');
  expect(await decodes(page)).toBe(1);
  await tile(page, 2).click();
  await expect(hero(page)).toHaveAttribute('alt', 'Image 2 of 12');
  await expect(dialog(page).locator('.aw-dimensions')).toHaveText('900 × 600');
  expect(await decodes(page)).toBe(2);
  await dialog(page).locator('.aw-frame:not([inert]) .aw-surface').click();
  await dialog(page).getByRole('button', { name: 'Rotate right', exact: true }).click();
  await expect(dialog(page).locator('.aw-dimensions')).toHaveText('600 × 900');
  // The preview rotates through CSS; only Apply touches pixels.
  expect(await decodes(page)).toBe(2);
  await cta(page).click();
  await expect(dialog(page)).toHaveCount(0);
  expect(await decodes(page)).toBe(2);
  await expect.poll(() => page.evaluate(() => window.artworkResult?.index)).toBe(1);
  expect(await page.evaluate(() => window.artworkResult.rotation)).toBe(90);
  // The committed cover really is the rotated original, not the preview.
  expect(await page.evaluate(async () => {
    const cover = window.artworkResult.cover;
    const image = new Image();
    image.src = `data:${cover.mime};base64,${cover.data_base64}`;
    await image.decode();
    return [image.naturalWidth, image.naturalHeight];
  })).toEqual([600, 900]);
});

test('returning to an image whose decode is still running reuses that decode', async ({ page }) => {
  await open(page, '?count=40');
  await page.evaluate(() => { window.__decodeDelay = 1500; window.__decodeCalls = 0; });
  // Each step waits until the previous selection has started decoding, so the
  // third click lands while the first image's decode is genuinely in flight —
  // the case a rapid A→B→A browsing gesture produces.
  await page.evaluate(() => document.querySelector('.aw-thumb[data-index="35"]').click());
  await expect.poll(() => decodes(page)).toBe(1);
  await page.evaluate(() => document.querySelector('.aw-thumb[data-index="36"]').click());
  await expect.poll(() => decodes(page)).toBe(2);
  await page.evaluate(() => document.querySelector('.aw-thumb[data-index="35"]').click());
  await expect(hero(page)).toHaveAttribute('alt', 'Image 36 of 40');
  await expect(cta(page)).toBeEnabled();
  expect(await decodes(page)).toBe(2);
  expect(await page.evaluate(() => window.artworkTest.calls.filter((index) => index === 35).length)).toBe(1);
});

test('Retry reselects the displayed image after a transient rotated-Apply failure', async ({ page }) => {
  await open(page, '?count=1');
  await page.evaluate(() => {
    const encode = HTMLCanvasElement.prototype.toDataURL;
    let fail = true;
    HTMLCanvasElement.prototype.toDataURL = function (...args) {
      if (fail) { fail = false; throw new Error('Transient rotation encoding failure'); }
      return encode.apply(this, args);
    };
  });
  await dialog(page).locator('.aw-frame:not([inert]) .aw-surface').click();
  await dialog(page).getByRole('button', { name: 'Rotate right', exact: true }).click();
  await cta(page).click();
  await expect(dialog(page).getByRole('button', { name: 'Retry', exact: true })).toBeVisible();
  await dialog(page).getByRole('button', { name: 'Retry', exact: true }).click();
  await expect(cta(page)).toBeEnabled();
  expect(await decodes(page)).toBe(2);
  await cta(page).click();
  await expect(dialog(page)).toHaveCount(0);
  expect(await page.evaluate(() => window.artworkResult.rotation)).toBe(90);
  expect(await decodes(page)).toBe(2);
});
