import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';

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
});

const dialog = (page) => page.getByRole('dialog', { name: 'Choose artwork' });
const tile = (page, number) => dialog(page).getByRole('button', { name: `Image ${number} of`, exact: false });
const cta = (page) => dialog(page).getByRole('button', { name: 'Use', exact: true });
async function open(page, query = '') {
  await page.goto(`http://tagrex.test/tests/ui/fixture/index.html${query}`);
  await page.getByRole('button', { name: 'Open artwork' }).click();
  await expect(dialog(page)).toBeVisible();
  await expect(cta(page)).toBeEnabled();
}

test('chooses exact image; arrow navigation, zoom and animated close preserve focus', async ({ page }) => {
  const errors = []; page.on('pageerror', (error) => errors.push(error.message));
  await open(page);
  await expect(tile(page, 1)).toBeFocused();
  await page.keyboard.press('ArrowRight');
  await expect(tile(page, 2)).toHaveAttribute('aria-pressed', 'true');
  await expect(cta(page)).toBeEnabled();
  await expect(dialog(page).locator('.aw-dimensions')).toHaveText('900 × 600');
  await dialog(page).locator('.aw-frame:not([inert])').getByRole('button', { name: 'Artwork canvas. Click to show controls.' }).click();
  await expect(dialog(page).getByRole('toolbar', { name: 'Artwork controls' })).toBeVisible();
  await dialog(page).getByRole('button', { name: 'Zoom in', exact: true }).click();
  await expect.poll(() => dialog(page).locator('.aw-frame:not([inert]) .aw-viewer').getAttribute('data-scale').then(Number)).toBeCloseTo(1, 2);
  await dialog(page).getByRole('button', { name: 'Zoom in', exact: true }).click();
  await expect.poll(() => dialog(page).locator('.aw-frame:not([inert]) .aw-viewer').getAttribute('data-scale').then(Number)).toBeGreaterThan(1);
  await cta(page).click();
  await expect(dialog(page)).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => window.artworkResult?.index)).toBe(1);
  expect(await page.evaluate(() => window.artworkResult.cover.data_base64 === window.artworkTest.fixtures[1].data_base64)).toBe(true);
  await expect(page.locator('#launch')).toBeFocused();
  expect(errors).toEqual([]);
});

test('right-click preview and apply use the targeted thumbnail; Escape closes only the menu first', async ({ page }) => {
  await open(page);
  await tile(page, 3).click({ button: 'right' });
  await expect(page.getByRole('menu')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('menu')).toHaveCount(0);
  await expect(dialog(page)).toBeVisible();
  await tile(page, 2).click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Preview' }).click();
  await expect(tile(page, 2)).toHaveAttribute('aria-pressed', 'true');
  await tile(page, 3).click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Use', exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.artworkResult?.index)).toBe(2);
});

test('late loads cannot replace a newer selection; errors retry; thumbnails are lazy', async ({ page }) => {
  await open(page, '?count=40');
  expect(await page.evaluate(() => window.artworkTest.calls.length)).toBeLessThan(20);
  await page.evaluate(() => { window.artworkTest.delays[20] = 600; });
  await page.keyboard.press('End');
  await expect(tile(page, 40)).toHaveAttribute('aria-pressed', 'true');
  await expect(cta(page)).toBeEnabled();
  // This image is outside the initial thumbnail window and has not been cached.
  await page.evaluate(() => { window.artworkTest.fail = 20; });
  await tile(page, 21).click();
  await expect(dialog(page).getByRole('button', { name: 'Retry' })).toBeVisible();
  await expect(cta(page)).toBeDisabled();
  await page.evaluate(() => { window.artworkTest.fail = -1; });
  await dialog(page).getByRole('button', { name: 'Retry' }).click();
  await expect(cta(page)).toBeEnabled();
  await tile(page, 22).click();
  await expect(cta(page)).toBeEnabled();
  await expect(dialog(page).locator('.aw-frame:not([inert]) .aw-hero')).toHaveAttribute('alt', 'Image 22 of 40');
  await page.evaluate(() => { window.artworkTest.delays[30] = 500; });
  await tile(page, 31).click();
  await tile(page, 32).click();
  await expect(cta(page)).toBeEnabled();
  await page.waitForTimeout(600);
  await expect(dialog(page).locator('.aw-frame:not([inert]) .aw-hero')).toHaveAttribute('alt', 'Image 32 of 40');
  await page.keyboard.press('Escape');
  await expect.poll(() => page.evaluate(() => window.artworkResult)).toBe(null);
});

test('closing during a slow fetch cancels cleanly and leaves the background usable', async ({ page }) => {
  await page.goto('http://tagrex.test/tests/ui/fixture/index.html?count=1');
  await page.evaluate(() => { window.artworkTest.delays[0] = 1000; });
  await page.locator('#launch').click();
  await expect(dialog(page)).toBeVisible();
  await expect(cta(page)).toBeDisabled();
  await page.keyboard.press('Escape');
  await expect.poll(() => page.evaluate(() => window.artworkResult)).toBe(null);
  await expect(page.locator('#launch')).toBeFocused();
  await page.locator('#background').click();
  await expect(page.locator('#background')).toBeFocused();
});

for (const [width, height] of [[375, 667], [844, 390], [1280, 800]]) {
  test(`responsive layout and focus containment at ${width}×${height}`, async ({ page }) => {
    await page.setViewportSize({ width, height });
    await page.emulateMedia({ colorScheme: 'dark', reducedMotion: 'reduce' });
    await open(page);
    const bounds = await dialog(page).boundingBox();
    expect(bounds.x).toBeGreaterThanOrEqual(0);
    expect(bounds.y).toBeGreaterThanOrEqual(0);
    expect(bounds.x + bounds.width).toBeLessThanOrEqual(width);
    expect(bounds.y + bounds.height).toBeLessThanOrEqual(height);
    const stage = await dialog(page).locator('.aw-stage').boundingBox();
    expect(stage.height).toBeGreaterThan(50);
    await surface(page).click();
    const tools = await dialog(page).getByRole('toolbar', { name: 'Artwork controls' }).boundingBox();
    expect(tools.x).toBeGreaterThanOrEqual(stage.x);
    expect(tools.x + tools.width).toBeLessThanOrEqual(stage.x + stage.width);
    expect(tools.y + tools.height).toBeLessThanOrEqual(stage.y + stage.height);
    await cta(page).focus(); await page.keyboard.press('Tab');
    expect(await dialog(page).evaluate((node) => node.contains(document.activeElement))).toBe(true);
    await page.keyboard.press('Escape');
    await expect(page.locator('#launch')).toBeFocused();
  });
}

test('production editor opens the Svelte picker directly from release artwork', async ({ page }) => {
  const errors = []; page.on('pageerror', (error) => errors.push(error.message));
  await page.goto('http://tagrex.test/app/ui/index.html');
  await page.evaluate(() => { document.getElementById('discogs-token').value = 'fixture'; });
  await page.locator('#discogs-query').fill('Helter Skelter');
  await page.locator('#discogs-search').click();
  const card = page.locator('.release-card').first();
  await card.locator('.release-cover').click();
  await expect(dialog(page)).toBeVisible();
  const crumbs = dialog(page).getByRole('navigation', { name: 'Release details' });
  await expect(crumbs.getByRole('link')).toHaveCount(3);
  await expect(crumbs.getByRole('link').first()).toHaveText('Antler-Subway');
  await expect(crumbs).not.toContainText('Belgium');
  await expect(crumbs).not.toContainText('1996');
  await expect(crumbs).not.toContainText('Vinyl');
  await expect(card).toHaveAttribute('aria-expanded', 'false');
  await tile(page, 2).click(); await expect(cta(page)).toBeEnabled();
  await surface(page).click();
  await dialog(page).getByRole('button', { name: 'Rotate right', exact: true }).click();
  await cta(page).click(); await expect(dialog(page)).toHaveCount(0);
  await card.locator('.release-cover').click();
  await expect(tile(page, 2)).toHaveAttribute('aria-pressed', 'true');
  await expect.poll(() => numeric(page, 'data-angle')).toBeCloseTo(90, 2);
  await expect.poll(() => numeric(page, 'data-pan-x')).toBeCloseTo(0, 2);
  await page.keyboard.press('Escape');
  expect(errors).toEqual([]);
});

test('keeps the current artwork fully visible until the next image is decoded', async ({ page }) => {
  await open(page, '?count=40');
  await page.evaluate(() => { window.artworkTest.delays[20] = 1000; });
  await tile(page, 21).click();
  await expect(cta(page)).toBeDisabled();
  await page.waitForTimeout(220);
  const opacity = await dialog(page).locator('.aw-hero').first().evaluate((image) => Number(getComputedStyle(image).opacity));
  expect(opacity).toBeGreaterThanOrEqual(0.99);
});

const viewer = (page) => dialog(page).locator('.aw-frame:not([inert]) .aw-viewer');
const surface = (page) => viewer(page).locator('.aw-surface');
const numeric = (page, attr) => viewer(page).getAttribute(attr).then(Number);

test('wheel overzoom, left-drag and reset tween; switching images always recentres', async ({ page }) => {
  await open(page);
  const bounds = await surface(page).boundingBox();
  const fitted = await surface(page).evaluate((node) => Math.min((node.clientWidth - 36) / 600, (node.clientHeight - 36) / 600, 1));
  await expect.poll(() => numeric(page, 'data-scale')).toBeCloseTo(fitted, 2);
  await page.mouse.move(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2);
  await page.mouse.wheel(0, -180);
  await expect.poll(() => numeric(page, 'data-scale')).toBeGreaterThan(fitted * 1.4);
  await page.mouse.wheel(0, -180);
  await expect.poll(() => numeric(page, 'data-scale')).toBeGreaterThan(1);
  await page.mouse.down({ button: 'left' });
  await page.mouse.move(bounds.x + bounds.width / 2 + 180, bounds.y + bounds.height / 2 + 90, { steps: 8 });
  await page.mouse.up({ button: 'left' });
  await expect.poll(() => numeric(page, 'data-pan-x')).toBeGreaterThan(150);
  const reset = dialog(page).getByRole('button', { name: 'Reset view', exact: true });
  await page.evaluate(() => {
    window.resetSamples = [];
    window.resetObserver = new MutationObserver(() => {
      const canvas = document.querySelector('.aw-frame:not([inert]) .aw-viewer');
      if (canvas) window.resetSamples.push(Number(canvas.dataset.panX));
    });
    window.resetObserver.observe(document.querySelector('.aw-frame:not([inert]) .aw-viewer'), { attributes: true, attributeFilter: ['data-pan-x'] });
  });
  await reset.click();
  await expect.poll(() => numeric(page, 'data-pan-x')).toBeCloseTo(0, 2);
  expect(await page.evaluate(() => { window.resetObserver.disconnect(); return window.resetSamples.some((x) => x > 1 && x < 150); })).toBe(true);
  await expect.poll(() => numeric(page, 'data-pan-y')).toBeCloseTo(0, 2);
  await expect.poll(() => numeric(page, 'data-scale')).toBeCloseTo(fitted, 2);
  // Pan far away again, then switch and come back: each new canvas starts fit.
  await page.mouse.move(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2);
  await page.mouse.down({ button: 'left' });
  await page.mouse.move(bounds.x + bounds.width / 2 + 200, bounds.y + bounds.height / 2 + 120);
  await page.mouse.up({ button: 'left' });
  await tile(page, 2).click(); await expect(cta(page)).toBeEnabled();
  await expect.poll(() => numeric(page, 'data-pan-x')).toBeCloseTo(0, 2);
  await expect.poll(() => numeric(page, 'data-pan-y')).toBeCloseTo(0, 2);
  await tile(page, 1).click(); await expect(cta(page)).toBeEnabled();
  await expect.poll(() => numeric(page, 'data-pan-x')).toBeCloseTo(0, 2);
  await expect.poll(() => numeric(page, 'data-scale')).toBeCloseTo(fitted, 2);
});

test('rotation affects the selected image pixels, preserving every edge', async ({ page }) => {
  await open(page);
  await tile(page, 2).click(); await expect(cta(page)).toBeEnabled();
  await surface(page).click();
  await dialog(page).getByRole('button', { name: 'Rotate right', exact: true }).click();
  // Panning during a rotation must not leave the view at an arbitrary angle.
  const area = await surface(page).boundingBox();
  await page.mouse.move(area.x + area.width / 2, area.y + area.height / 2);
  await page.mouse.down({ button: 'left' });
  await page.mouse.move(area.x + area.width / 2 + 40, area.y + area.height / 2 + 20);
  await page.mouse.up({ button: 'left' });
  await expect.poll(() => numeric(page, 'data-angle')).toBeCloseTo(90, 2);
  await expect(dialog(page).locator('.aw-dimensions')).toHaveText('600 × 900');
  await cta(page).click();
  await expect.poll(() => page.evaluate(() => window.artworkResult?.index)).toBe(1);
  const decoded = await page.evaluate(async () => {
    const cover = window.artworkResult.cover;
    const image = new Image(); image.src = `data:${cover.mime};base64,${cover.data_base64}`;
    await image.decode();
    const canvas = document.createElement('canvas'); canvas.width = image.naturalWidth; canvas.height = image.naturalHeight;
    const ctx = canvas.getContext('2d'); ctx.drawImage(image, 0, 0);
    return { width: image.naturalWidth, height: image.naturalHeight, pixel: [...ctx.getImageData(image.naturalWidth - 10, 10, 1, 1).data] };
  });
  expect(decoded).toEqual({ width: 600, height: 900, pixel: [0, 255, 0, 255] });
});

test('subtitle includes exact artist, release and label inspection links', async ({ page }) => {
  await open(page);
  await expect(dialog(page).getByRole('link', { name: 'Helter Skelter', exact: true })).toHaveAttribute('href', 'https://www.discogs.com/label/456');
  await dialog(page).getByRole('link', { name: 'Various', exact: true }).click();
  await dialog(page).getByRole('link', { name: 'Helter Skelter', exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.artworkTest.opened.map((link) => `${link.entity}/${link.id}`))).toEqual(['artist/194', 'label/456']);
  await expect(dialog(page)).toBeVisible();
});

test('image changes fade over a preserved canvas without dark dips or colour lift', async ({ page }) => {
  await open(page);
  await tile(page, 2).click(); await expect(cta(page)).toBeEnabled();
  const samples = await page.evaluate(async () => {
    for (let attempt = 0; attempt < 60; attempt++) {
      const frames = [...document.querySelectorAll('.aw-frame')];
      const incoming = frames.find((frame) => !frame.inert);
      const animation = incoming?.getAnimations().find((animation) => animation.effect.getTiming().duration === 140);
      if (frames.length === 2 && animation) {
        animation.pause(); animation.currentTime = 70;
        return frames.map((frame) => ({
          duration: animation.effect.getTiming().duration,
          hasTransform: animation.effect.getKeyframes().some((key) => 'transform' in key),
          blend: getComputedStyle(frame).mixBlendMode,
          opacity: Number(getComputedStyle(frame).opacity),
          incoming: frame === incoming,
        }));
      }
      await new Promise(requestAnimationFrame);
    }
    return [];
  });
  expect(samples).toHaveLength(2);
  for (const sample of samples) {
    expect(sample.duration).toBe(140);
    expect(sample.hasTransform).toBe(false);
    expect(sample.blend).toBe('normal');
    expect(sample.opacity).toBeCloseTo(sample.incoming ? .5 : 1, 1);
  }
  // Check the actual composited pixels, catching dark dips or colour lifting.
  const bounds = await dialog(page).locator('.aw-stage').boundingBox();
  const screenshot = (await page.screenshot()).toString('base64');
  const pixel = await page.evaluate(async ({ screenshot, x, y }) => {
    const image = new Image(); image.src = `data:image/png;base64,${screenshot}`; await image.decode();
    const canvas = document.createElement('canvas'); canvas.width = canvas.height = 1;
    const context = canvas.getContext('2d'); context.drawImage(image, -Math.floor(x), -Math.floor(y));
    return [...context.getImageData(0, 0, 1, 1).data];
  }, { screenshot, x: bounds.x + bounds.width / 2, y: bounds.y + bounds.height / 2 });
  for (let i = 0; i < 3; i++) expect(Math.abs(pixel[i] - [116, 74, 116][i])).toBeLessThanOrEqual(3);
});

test('panning updates the image in the input event without waiting for framework rendering', async ({ page }) => {
  await open(page);
  await surface(page).click();
  await page.waitForTimeout(300);
  const bounds = await surface(page).boundingBox();
  await page.mouse.move(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2);
  await surface(page).evaluate((node) => node.addEventListener('pointerdown', (event) => { window.dragPointerId = event.pointerId; }, { once: true }));
  await page.mouse.down({ button: 'left' });
  const movement = await surface(page).evaluate((node) => {
    const image = node.querySelector('img');
    const before = image.getBoundingClientRect();
    const bounds = node.getBoundingClientRect();
    node.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, pointerId: window.dragPointerId, pointerType: 'mouse', buttons: 1, clientX: bounds.x + bounds.width / 2 + 120, clientY: bounds.y + bounds.height / 2 + 60 }));
    const after = image.getBoundingClientRect();
    return { x: after.x - before.x, y: after.y - before.y };
  });
  await page.mouse.up({ button: 'left' });
  expect(movement.x).toBeCloseTo(120, 0);
  expect(movement.y).toBeCloseTo(60, 0);
});

test('settled canvas avoids continuous crossfade blending and backdrop blur over moving art', async ({ page }) => {
  await open(page);
  await surface(page).click();
  await page.waitForTimeout(300);
  expect(await dialog(page).locator('.aw-frame:not([inert])').evaluate((node) => getComputedStyle(node).mixBlendMode)).toBe('normal');
  expect(await dialog(page).locator('.aw-tools').evaluate((node) => getComputedStyle(node).backdropFilter)).toBe('none');
  await tile(page, 2).click(); await expect(cta(page)).toBeEnabled();
  await page.waitForTimeout(200);
  expect(await dialog(page).locator('.aw-frame:not([inert])').evaluate((node) => getComputedStyle(node).mixBlendMode)).toBe('normal');
});


test('dragging uses a hand cursor without toggling controls; double-click recentres smoothly', async ({ page }) => {
  await open(page);
  await page.waitForTimeout(300);
  const canvas = surface(page);
  const bounds = await canvas.boundingBox();
  const x = bounds.x + bounds.width / 2, y = bounds.y + bounds.height / 2;
  expect(await canvas.evaluate(node => getComputedStyle(node).cursor)).toBe('grab');
  await expect(canvas).toHaveAttribute('aria-expanded', 'false');
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x + 160, y + 80, { steps: 5 });
  expect(await canvas.evaluate(node => getComputedStyle(node).cursor)).toBe('grabbing');
  await page.mouse.up();
  await expect(canvas).toHaveAttribute('aria-expanded', 'true');
  await expect.poll(() => numeric(page, 'data-pan-x')).toBeCloseTo(160, 0);
  await canvas.dblclick({ position: { x: bounds.width / 2, y: bounds.height / 2 } });
  await expect.poll(() => numeric(page, 'data-pan-x')).toBeCloseTo(0, 2);
  await expect.poll(() => numeric(page, 'data-pan-y')).toBeCloseTo(0, 2);
  await expect(canvas).toHaveAttribute('aria-expanded', 'true');
  await canvas.click();
  await expect(canvas).toHaveAttribute('aria-expanded', 'false');
});


test('rapid browsing releases old layers and always creates a centred current canvas', async ({ page }) => {
  await open(page);
  await page.waitForTimeout(300);
  await page.evaluate(async () => {
    for (const index of [1, 2, 0, 2, 1, 0]) {
      document.querySelector(`.aw-thumb[data-index="${index}"]`).click();
      await new Promise(resolve => setTimeout(resolve, 35));
    }
  });
  await expect(dialog(page).locator('.aw-frame')).toHaveCount(1);
  await expect(viewer(page).locator('.aw-hero')).toHaveAttribute('alt', 'Image 1 of 12');
  await expect.poll(() => numeric(page, 'data-pan-x')).toBeCloseTo(0, 2);
  await surface(page).click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Rotate right', exact: true }).click();
  await expect.poll(() => numeric(page, 'data-angle')).toBeCloseTo(90, 2);
});


test('wheel zoom keeps working over the visible compact controls', async ({ page }) => {
  await open(page);
  await surface(page).click();
  await page.waitForTimeout(300);
  const scale = await numeric(page, 'data-scale');
  const readout = await viewer(page).locator('.aw-scale').boundingBox();
  await page.mouse.move(readout.x + readout.width / 2, readout.y + readout.height / 2);
  await page.mouse.wheel(0, -180);
  await expect.poll(() => numeric(page, 'data-scale')).toBeGreaterThan(scale * 1.4);
  await page.mouse.wheel(0, 180);
  await expect.poll(() => numeric(page, 'data-scale')).toBeCloseTo(scale, 2);
});

test('document wheel capture zooms only the live preview and survives reopening', async ({ page }) => {
  await open(page);
  await page.waitForTimeout(300);
  const fitted = await numeric(page, 'data-scale');
  const rail = await dialog(page).locator('.aw-rail').boundingBox();
  await page.mouse.move(rail.x + rail.width / 2, rail.y + rail.height / 2);
  await page.mouse.wheel(0, -180);
  await page.waitForTimeout(300);
  expect(await numeric(page, 'data-scale')).toBeCloseTo(fitted, 2);
  await page.keyboard.press('Escape');
  await expect(dialog(page)).toHaveCount(0);
  await page.locator('#launch').click();
  await expect(cta(page)).toBeEnabled();
  await page.waitForTimeout(300);
  const bounds = await surface(page).boundingBox();
  await page.mouse.move(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2);
  await page.mouse.wheel(0, -180);
  await expect.poll(() => numeric(page, 'data-scale')).toBeCloseTo(fitted * Math.exp(.45), 2);
});
