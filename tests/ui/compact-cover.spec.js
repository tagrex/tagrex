import { test, expect } from '@playwright/test';
import { readFile, mkdir } from 'node:fs/promises';
import { resolve, sep } from 'node:path';
const root = resolve('.test-dist');
test.use({ viewport: { width: 1440, height: 900 }, reducedMotion: 'reduce' });

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    const image = (name, color = '#138d75') => ({ mime: 'image/svg+xml', data_base64: btoa(`<svg xmlns="http://www.w3.org/2000/svg" width="600" height="600"><rect width="600" height="600" fill="${color}"/><text x="300" y="310" text-anchor="middle" fill="white" font-size="42">${name}</text></svg>`), kind: 'front', description: name });
    window.__cover = { mode: 'single', calls: [], image, summary(paths) {
      const images = this.mode === 'empty' ? [] : this.mode === 'multiple' ? [image('Front'), { ...image('Back', '#435c8c'), kind: 'back' }] : [image('Front')];
      return { total: paths.length, with_cover: images.length ? paths.length : 0, distinct: this.mode === 'mixed', samples: images, shared_set: this.mode === 'mixed' ? [] : images };
    } };
  });
  await page.route('http://tagrex.test/**', async route => {
    const path = resolve(root, '.' + new URL(route.request().url()).pathname);
    if (!path.startsWith(root + sep)) return route.abort();
    try {
      let body = await readFile(path);
      if (path.endsWith('.js')) {
        body = body.toString().replace(/case\s*["'`]read_cover_summary["'`]\s*:\s*\{\s*let\s+\w+=(\w+)\.paths/, (match, args) => match.replace('{', `{ return Promise.resolve(window.__cover.summary(${args}.paths));`));
        body = body.replace(/case\s*["'`]read_external_cover["'`]\s*:/, '$& return Promise.resolve(null);');
        body = body.replace(/case\s*["'`]read_cover_image["'`]\s*:/, '$& window.__cover.calls.push("read_cover_image");');
      }
      await route.fulfill({ body, contentType: path.endsWith('.js') ? 'text/javascript' : path.endsWith('.css') ? 'text/css' : 'text/html' });
    } catch { await route.fulfill({ status: 404, body: 'Missing fixture' }); }
  });
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  page.__coverErrors = errors;
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(() => page.evaluate(() => typeof window.tagrex?.openLibrary)).toBe('function');
  await page.locator('#root-display').click();
  await page.locator('#root').fill('/music');
  await page.locator('#root').press('Enter');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
  await page.locator('.subtab[data-subtab="editor"]').click();
  await expect(page.locator('.cover-main-image img')).toHaveAttribute('alt', 'Front');
});
test.afterEach(async ({ page }) => expect(page.__coverErrors).toEqual([]));
const nextRow = page => page.locator('#tracks-body tr[data-path] .file').nth(1).click();
const openMenu = page => page.locator('#cover-well .cover-edit').click();

test('one centered cover replaces duplicated rows, with actions behind Edit', async ({ page }) => {
  await expect(page.locator('#cover-set')).toBeHidden();
  await expect(page.locator('#cover-well .cover-main-image')).toBeVisible();
  await expect(page.locator('#cover-well .cover-actions')).toHaveCount(0);
  await openMenu(page);
  for (const name of ['Replace', 'Export', 'Remove']) await expect(page.getByRole('menuitem', { name, exact: true })).toBeVisible();
  await expect(page.getByRole('menuitem', { name: 'Manage images', exact: true })).toHaveCount(0);
  await expect(page.locator('.cover-row, .cover-mini')).toHaveCount(0);
  expect(await page.evaluate(() => window.tagrex.previewPlan)).toBeNull();
});

test('lightbox reuses wheel zoom and direct pan, closes with Escape and restores focus', async ({ page }) => {
  await page.locator('.cover-main-image').click();
  const dialog = page.getByRole('dialog', { name: 'Cover art', exact: true });
  await expect(dialog).toBeVisible();
  const canvas = dialog.locator('.aw-viewer');
  await expect(canvas.locator('img')).toBeVisible();
  const old = Number(await canvas.getAttribute('data-scale'));
  await dialog.locator('.aw-surface').hover();
  await page.mouse.wheel(0, -180);
  await expect.poll(async () => Number(await canvas.getAttribute('data-scale'))).toBeGreaterThan(old);
  const box = await canvas.boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down(); await page.mouse.move(box.x + box.width / 2 + 55, box.y + box.height / 2 + 25); await page.mouse.up();
  await expect.poll(async () => Number(await canvas.getAttribute('data-pan-x'))).toBeGreaterThan(40);
  await page.keyboard.press('Escape');
  await expect(dialog).toBeHidden();
  await expect(page.locator('.cover-main-image')).toBeFocused();
});

test('empty state opens native picker beside tracks and rejects a stale selected subject', async ({ page }) => {
  await page.evaluate(() => {
    window.__cover.mode = 'empty';
    window.__TAURI__ = { dialog: { open: options => { window.__cover.pickerOptions = options; return new Promise(resolve => window.__cover.pick = resolve); } } };
  });
  await nextRow(page);
  await expect(page.locator('.cover-empty-target')).toBeEnabled();
  expect(await page.locator('.cover-empty-target').evaluate(node => [getComputedStyle(node).borderRadius,getComputedStyle(node).borderTopStyle])).toEqual(['8px','dashed']);
  await page.locator('.cover-empty-target').click();
  await expect.poll(() => page.evaluate(() => window.__cover.pickerOptions?.defaultPath)).toBe('/music/');
  await page.locator('#tracks-body tr[data-path] .file').nth(2).click();
  await page.evaluate(() => window.__cover.pick('/music/cover.png'));
  await page.waitForTimeout(30);
  expect(await page.evaluate(() => window.__cover.calls)).toEqual([]);
  expect(await page.evaluate(() => window.tagrex.previewPlan)).toBeNull();
});

test('native cancellation is inert and a valid image only stages a preview', async ({ page }) => {
  await page.evaluate(() => {
    window.__cover.mode = 'empty';
    window.__TAURI__ = { dialog: { open: options => { window.__cover.pickerOptions = options; return Promise.resolve(window.__cover.picked ?? null); } } };
  });
  await nextRow(page);
  await page.locator('.cover-empty-target').click();
  await expect.poll(() => page.evaluate(() => window.__cover.pickerOptions?.defaultPath)).toBe('/music/');
  expect(await page.evaluate(() => window.tagrex.previewPlan)).toBeNull();
  await page.evaluate(() => window.__cover.picked = '/music/cover.png');
  await page.locator('.cover-empty-target').click();
  await expect.poll(() => page.evaluate(() => window.__cover.calls.length)).toBe(1);
  await expect.poll(() => page.evaluate(() => !!window.tagrex.previewPlan)).toBe(true);
  expect(await page.evaluate(() => window.tagrex.tracks[1].covers?.length || 0)).toBe(0);
});

test('extra embedded pictures stay out of the main cover UI and mixed selections remain explicit', async ({ page }) => {
  await page.evaluate(() => window.__cover.mode = 'multiple'); await nextRow(page);
  await expect(page.locator('.cover-mini')).toHaveCount(0);
  await expect(page.locator('.cover-main-image img')).toHaveAttribute('alt', 'Front');
  await expect(page.locator('#cover-set')).toBeHidden();
  await page.evaluate(() => window.__cover.mode = 'mixed');
  await page.locator('#tracks-body tr[data-path] .file').nth(2).click();
  await expect(page.locator('.cover-compact-note')).toContainText('Multiple covers');
  await openMenu(page);
  await expect(page.getByRole('menuitem', { name: 'Manage images', exact: true })).toHaveCount(0);
});

for (const [width, theme] of [[1440, 'dark'], [1440, 'light'], [375, 'dark']]) {
  test(`compact artwork capture ${width} ${theme}`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.evaluate(value => { document.documentElement.dataset.theme = value; }, theme);
    if (width < 600) await page.locator('.dock-module-tab').filter({ hasText: 'Inspector' }).click();
    await page.locator('.cover-main-image').scrollIntoViewIfNeeded();
    expect(await page.locator('.cover-main-image').evaluate(node => getComputedStyle(node).borderRadius)).toBe('8px');
    expect(await page.locator('.mode-col').evaluate(node => node.scrollWidth <= node.clientWidth)).toBe(true);
    if (process.env.ITERATION_CAPTURE_EVIDENCE === '1') { await mkdir('docs/ui-iteration', { recursive: true }); await page.screenshot({ path: `docs/ui-iteration/cover-${width}-${theme}.png` }); }
    await page.locator('.cover-main-image').click();
    await expect(page.locator('.cover-lightbox .aw-hero')).toBeVisible();
    if (process.env.ITERATION_CAPTURE_EVIDENCE === '1') await page.screenshot({ path: `docs/ui-iteration/cover-lightbox-${width}-${theme}.png` });
    const box = await page.locator('.cover-lightbox').boundingBox();
    expect(box.x).toBeGreaterThanOrEqual(0); expect(box.x + box.width).toBeLessThanOrEqual(width + 1);
  });
}

test('artwork hover and drop feedback remain visible for filled and empty covers', async ({ page }) => {
  const edit=page.locator('#cover-well .cover-edit');
  const before=await edit.evaluate(node=>getComputedStyle(node).backgroundColor);
  await edit.hover();
  await expect.poll(()=>edit.evaluate(node=>getComputedStyle(node).backgroundColor)).not.toBe(before);
  for(const empty of [false,true]) {
    if(empty){await page.evaluate(()=>{window.__cover.mode='empty';});await nextRow(page);}
    const target=page.locator(empty?'.cover-empty-target':'.cover-main-image');
    await expect(target).toBeVisible();
    await page.locator('#cover-well').evaluate(node=>node.classList.add('dragover'));
    expect(await target.evaluate(node=>{const s=getComputedStyle(node);return [s.outlineStyle,s.outlineWidth];})).toEqual(['dashed','2px']);
    await page.locator('#cover-well').evaluate(node=>node.classList.remove('dragover'));
  }
  expect(await page.evaluate(()=>window.tagrex.previewPlan)).toBeNull();
});
