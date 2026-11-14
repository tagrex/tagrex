import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';
const root = resolve('.test-dist');
const errors = new WeakMap();
test.beforeEach(async ({ page }) => {
  const issues = [];
  errors.set(page, issues);
  page.on('pageerror', error => issues.push(error.message));
  await page.addInitScript(() => localStorage.setItem('tagrex.groupBy', ''));
  await page.route('http://tagrex.test/**', async route => {
    const path = resolve(root, '.' + new URL(route.request().url()).pathname);
    if (!path.startsWith(root + sep)) return route.abort();
    try {
      await route.fulfill({ body: await readFile(path), contentType:path.endsWith('.js') ? 'text/javascript' : path.endsWith('.css') ? 'text/css' : path.endsWith('.woff2') ? 'font/woff2' : 'text/html' });
    } catch { await route.fulfill({ status: 404, body: 'Missing fixture' }); }
  });
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(() => page.evaluate(() => typeof window.tagrex?.openLibrary)).toBe('function');
  await page.locator('#root-display').click();
  await page.locator('#root').fill('/music');
  await page.locator('#root').press('Enter');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
});
test.afterEach(async ({ page }) => expect(errors.get(page)).toEqual([]));
const rows = page => page.locator('#tracks-body tr[data-path]');
const selected = page => page.evaluate(() => [...window.tagrex.selection]);
async function paths(page) { return page.evaluate(() => window.tagrex.tracks.map(track => track.path)); }
async function dragRows(page, from, to, { ctrl = false } = {}) {
  const start = await rows(page).nth(from).locator('td.file').boundingBox();
  const end = await rows(page).nth(to).locator('td.file').boundingBox();
  if (ctrl) await page.keyboard.down('Control');
  await page.mouse.move(start.x + Math.min(90, start.width - 8), start.y + start.height / 2);
  await page.mouse.down();
  await page.mouse.move(end.x + Math.min(120, end.width - 5), end.y + end.height / 2, { steps: 8 });
  await expect(page.locator('.row-selection-rectangle')).toBeVisible();
  await page.mouse.up();
  if (ctrl) await page.keyboard.up('Control');
}
async function many(page, entries, group = '') {
  await page.evaluate(async ({ entries, group }) => {
    await window.tagrex.invoke('open_drop', { paths: entries });
    localStorage.setItem('tagrex.groupBy', group);
    document.getElementById('root').value = '/dropped';
    await window.tagrex.openLibrary();
  }, { entries, group });
  await expect.poll(() => page.evaluate(() => window.tagrex.tracks.length)).toBe(entries.length);
}

test('plain row clicks select one; Ctrl clicks add and toggle individual rows', async ({ page }) => {
  const all = await paths(page);
  await rows(page).nth(1).locator('td[data-field="artist"]').click();
  expect(await selected(page)).toEqual([all[1]]);
  await rows(page).nth(0).locator('td[data-field="artist"]').click({ modifiers: ['Control'] });
  expect(new Set(await selected(page))).toEqual(new Set([all[1], all[0]]));
  await rows(page).nth(1).locator('td[data-field="artist"]').click({ modifiers: ['Control'] });
  expect(await selected(page)).toEqual([all[0]]);
  await rows(page).nth(2).locator('td[data-field="artist"]').click();
  expect(await selected(page)).toEqual([all[2]]);
});

test('dragging the filename selects intersected rows without reordering or entering edit mode', async ({ page }) => {
  const all = await paths(page);
  await expect(page.locator('[data-row-reorder-grip]')).toHaveCount(0);
  await rows(page).first().locator('td.file').click();
  await dragRows(page, 0, 2);
  expect(new Set(await selected(page))).toEqual(new Set(all));
  expect(await paths(page)).toEqual(all);
  await expect(page.locator('#tracks-body [contenteditable="true"]')).toHaveCount(0);
  await expect(page.locator('.row-selection-rectangle')).toBeHidden();
  await expect(page.locator('body')).not.toHaveClass(/selecting-rows/);
});

test('track surfaces show an arrow on hover and a text cursor only while editing', async ({ page }) => {
  const cells = rows(page).first().locator('td:not(.sel)');
  for (const cell of await cells.all()) {
    await cell.hover();
    expect(await cell.evaluate(node => getComputedStyle(node).cursor)).toBe('default');
    expect(await cell.evaluate(node => getComputedStyle(node).webkitUserDrag)).toBe('none');
  }
  const artist = rows(page).first().locator('td[data-field="artist"]');
  await artist.dblclick();
  await expect(artist).toHaveAttribute('contenteditable', 'true');
  expect(await artist.evaluate(node => getComputedStyle(node).cursor)).toBe('text');
  await page.keyboard.press('Escape');
});

test('Ctrl marquee adds to the captured selection while a plain marquee replaces it', async ({ page }) => {
  const all = await paths(page);
  await rows(page).nth(2).locator('td[data-field="artist"]').click();
  await dragRows(page, 0, 1, { ctrl: true });
  expect(new Set(await selected(page))).toEqual(new Set(all));
  await dragRows(page, 0, 1);
  expect(new Set(await selected(page))).toEqual(new Set(all.slice(0, 2)));
});

test('Escape cancels a marquee and restores the previous selection', async ({ page }) => {
  const all = await paths(page);
  await rows(page).nth(2).locator('td[data-field="artist"]').click();
  const first = await rows(page).first().locator('td.file').boundingBox();
  const second = await rows(page).nth(1).locator('td.file').boundingBox();
  await page.mouse.move(first.x + 90, first.y + first.height / 2);
  await page.mouse.down();
  await page.mouse.move(second.x + 120, second.y + second.height / 2, { steps: 6 });
  await expect(page.locator('.row-selection-rectangle')).toBeVisible();
  await page.keyboard.press('Escape');
  await page.mouse.up();
  expect(await selected(page)).toEqual([all[2]]);
  await expect(page.locator('.row-selection-rectangle')).toBeHidden();
});

test('group left-click folds, right-click replaces selection and Ctrl-click removes only that group', async ({ page }) => {
  const entries=[...Array.from({length:200},(_,i)=>`A/track-${String(i).padStart(3,'0')}.mp3`),'B/other-1.mp3','B/other-2.mp3'];
  await many(page,entries,'folder');const group=page.locator('#tracks-body tr.group-head').first();const label=group.locator('.group-label');const initial=await selected(page);
  await label.click();await expect(group).toHaveClass(/collapsed/);expect(await selected(page)).toEqual(initial);
  await label.click({button:'right'});expect((await selected(page)).length).toBe(200);expect((await selected(page)).every(path=>path.startsWith('/dropped/A/'))).toBe(true);await expect(group).toHaveClass(/collapsed/);
  await rows(page).first().locator('td.file').click({modifiers:['Control']});const other=(await selected(page)).find(path=>path.startsWith('/dropped/B/'));expect(other).toBeTruthy();
  await label.click({modifiers:['Control']});expect(await selected(page)).toEqual([other]);await expect(group).toHaveClass(/collapsed/);
  await label.click({button:'right'});expect((await selected(page)).length).toBe(200);expect((await selected(page)).includes(other)).toBe(false);await expect(page.locator('#file-ctx')).toBeHidden();
});

test('marquee autoscroll spans rows outside the original virtual window', async ({ page }) => {
  await many(page, Array.from({ length: 400 }, (_, i) => `track-${String(i).padStart(3, '0')}.mp3`));
  const initialPaths = await rows(page).evaluateAll(nodes => nodes.map(node => node.dataset.path));
  const first = await rows(page).first().locator('td.file').boundingBox();
  const bounds = await page.locator('#files-view').boundingBox();
  await page.mouse.move(first.x + 90, first.y + first.height / 2);
  await page.mouse.down();
  await page.mouse.move(first.x + 120, bounds.y + bounds.height - 2, { steps: 8 });
  await expect.poll(() => page.locator('#files-view').evaluate(node => node.scrollTop)).toBeGreaterThan(450);
  await page.mouse.up();
  const chosen = await selected(page);
  expect(chosen.some(path => !initialPaths.includes(path))).toBe(true);
  expect(chosen).toContain('/dropped/track-000.mp3');
  expect(await rows(page).count()).toBeLessThan(100);
  expect(await paths(page)).toEqual(Array.from({ length: 400 }, (_, i) => `/dropped/track-${String(i).padStart(3, '0')}.mp3`));
});

test('double-click tag editing still works and its text drag does not become a row marquee', async ({ page }) => {
  const cell = rows(page).first().locator('td[data-field="title"]');
  await cell.dblclick();
  await expect(cell).toHaveAttribute('contenteditable', 'true');
  const bounds = await cell.boundingBox();
  await page.mouse.move(bounds.x + 8, bounds.y + bounds.height / 2);
  await page.mouse.down();
  await page.mouse.move(bounds.x + Math.min(bounds.width - 8, 70), bounds.y + bounds.height / 2, { steps: 5 });
  await page.mouse.up();
  await expect(page.locator('.row-selection-rectangle')).toBeHidden();
  await expect(cell).toHaveAttribute('contenteditable', 'true');
  await page.keyboard.press('Escape');
});
