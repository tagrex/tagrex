import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';

const root = resolve('.test-dist');
const errors = new WeakMap();

test.beforeEach(async ({ page }) => {
  const pageErrors = [];
  errors.set(page, pageErrors);
  page.on('pageerror', (error) => pageErrors.push(error.message));
  await page.route('http://tagrex.test/**', async (route) => {
    const path = resolve(root, '.' + new URL(route.request().url()).pathname);
    if (!path.startsWith(root + sep)) return route.abort();
    try {
      let body = await readFile(path);
      if (path.endsWith('.js')) {
        // Instrument only the browser fixture's mock switch, in memory. This
        // counts real controller requests without changing production sources
        // or ever invoking a native filesystem command.
        body = Buffer.from(body.toString().replace(/case([`"'])((?:preview_tag_edits|apply_plan|undo))\1:/g,
          (match, _quote, command) => `${match}window.sheetCalls??={};window.sheetCalls['${command}']=(window.sheetCalls['${command}']||0)+1;`));
      }
      const contentType = path.endsWith('.js') ? 'text/javascript' : path.endsWith('.css') ? 'text/css' : path.endsWith('.woff2') ? 'font/woff2' : 'text/html';
      await route.fulfill({ body, contentType });
    } catch {
      await route.fulfill({ status: 404, body: 'Missing fixture' });
    }
  });
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(() => page.evaluate(() => typeof window.tagrex?.openLibrary)).toBe('function');
  expect(await page.evaluate(() => !!window.__TAURI__)).toBe(false);
  await page.locator('#root-display').click();
  await page.locator('#root').fill('/music');
  await page.locator('#root').press('Enter');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
});

test.afterEach(async ({ page }) => {
  expect(errors.get(page)).toEqual([]);
});


test('embedded artwork indicators use file metadata and header sort finds missing artwork', async ({ page }) => {
  const first = page.locator('#tracks-body tr[data-path]').filter({ hasText: 'Desert Rain' });
  const missing = page.locator('#tracks-body tr[data-path]').filter({ hasText: 'Radio' });
  await expect(first.locator('.track-artwork')).toHaveAttribute('data-cover-state', 'embedded');
  await expect(first.getByRole('img', { name: 'Embedded artwork', exact: true })).toBeVisible();
  await expect(missing.locator('.track-artwork')).toHaveAttribute('data-cover-state', 'missing');
  await expect(missing.getByRole('img', { name: 'No embedded artwork', exact: true })).toBeVisible();
  const sort = page.getByRole('button', { name: 'Sort by embedded artwork', exact: true });
  await sort.click();
  await expect(sort).toHaveAttribute('aria-pressed', 'true');
  await expect(page.locator('#tracks-body tr[data-path]').first()).toContainText('Radio');
  await expect(page.locator('#tracks-body tr[data-path]').last().locator('.track-artwork')).toHaveAttribute('data-cover-state', 'unknown');
  await sort.click();
  await expect(page.locator('#tracks-body tr[data-path]').first()).toContainText('Desert Rain');
  await expect(page.locator('#tracks-body tr[data-path]').last().locator('.track-artwork')).toHaveAttribute('data-cover-state', 'unknown');
});

test('artwork has a dedicated column before File without entering editable ranges', async ({ page }) => {
  const headers = page.locator('#tracks thead tr > th');
  await expect(headers.nth(0)).toHaveClass(/sel/);
  await expect(headers.nth(1)).toHaveClass(/artwork-col/);
  await expect(headers.nth(1)).toHaveAttribute('data-sort', 'artwork');
  await expect(headers.nth(2)).toHaveAttribute('data-sort', 'file');
  await expect(headers.nth(3)).toHaveAttribute('data-sort', 'artist');
  const row = page.locator('#tracks-body tr[data-path]').first();
  await expect(row.locator(':scope > td').nth(1)).toHaveClass('artwork-col');
  await expect(row.locator(':scope > td').nth(2)).toHaveClass(/file/);
  await expect(row.locator('td.file .track-artwork')).toHaveCount(0);
  await expect(page.locator('.track-reorder-grip')).toHaveCount(0);
  expect(await row.locator(':scope > td').count()).toBe(await headers.count());
  await expect(page.locator('#tracks')).toHaveAttribute('aria-colcount', String(await headers.count()));
  await expect(row.locator('td.file')).toHaveAttribute('aria-colindex', '3');
  const artist = row.locator('td[data-field="artist"]');
  await expect(artist).toHaveAttribute('aria-colindex', '4');
  await artist.click();
  await page.keyboard.press('ArrowLeft');
  await expect(row.locator('td.file')).toBeFocused();
  await page.keyboard.press('ArrowLeft');
  await expect(row.locator('td.file')).toBeFocused();
  await page.keyboard.press('ArrowRight');
  await expect(artist).toBeFocused();
  await page.keyboard.press('Shift+ArrowRight');
  const copied = await page.evaluate(() => {
    const data = new DataTransfer();
    document.activeElement.dispatchEvent(new ClipboardEvent('copy', { bubbles: true, cancelable: true, clipboardData: data }));
    return data.getData('text/plain');
  });
  expect(copied).toBe(await artist.textContent() + '\t' + await row.locator('td[data-field="title"]').textContent());
  const widths = await page.evaluate(() => [...document.querySelectorAll('#tracks .artwork-col')].map(cell => cell.getBoundingClientRect().width));
  // Full-width native table layout can distribute a fractional CSS pixel.
  for (const width of widths) expect(Math.abs(width - 24)).toBeLessThan(1);
  expect(Math.max(...widths) - Math.min(...widths)).toBeLessThan(0.1);
  await page.setViewportSize({ width: 1920, height: 800 });
  await expect.poll(() => page.evaluate(() => {
    const cells = document.querySelector('#tracks-body tr[data-path]').cells;
    const artwork = cells[1].getBoundingClientRect();
    const file = cells[2].getBoundingClientRect();
    const header = document.querySelector('#tracks th.artwork-col').getBoundingClientRect();
    return Math.abs(file.left - artwork.right) < 0.25 && Math.abs(header.width - artwork.width) < 0.1;
  })).toBe(true);
  await page.locator('#tracks th[data-sort="artist"] .col-resize').dblclick();
  await expect.poll(() => page.evaluate(() => {
    const row = document.querySelector('#tracks-body tr[data-path]');
    return Math.abs(row.querySelector('td.file').getBoundingClientRect().left - row.querySelector('td.artwork-col').getBoundingClientRect().right);
  })).toBeLessThan(0.25);
});

test('group, unreadable and virtual spacer rows span the complete structural grid', async ({ page }) => {
  await page.evaluate(() => { window.tagrex.tracks[1].unreadable = true; window.tagrex.renderTracks(); });
  const count = await page.locator('#tracks thead tr > th').count();
  expect(await page.locator('#tracks-body tr.unreadable > td').evaluateAll(cells => cells.reduce((sum, cell) => sum + cell.colSpan, 0))).toBe(count);
  await page.evaluate(async () => {
    await window.tagrex.invoke('open_drop', { paths: ['Pack'] });
    document.getElementById('root').value = '/dropped/Pack';
    await window.tagrex.openLibrary();
  });
  await expect(page.locator('#tracks-body tr.group-head')).toHaveCount(3);
  for (const span of await page.locator('#tracks-body tr.group-head > td').evaluateAll(cells => cells.map(cell => cell.colSpan))) expect(span).toBe(count);
  await page.evaluate(async () => {
    await window.tagrex.invoke('open_drop', { paths: Array.from({ length: 350 }, (_, i) => `art-${i}.mp3`) });
    document.getElementById('root').value = '/dropped';
    await window.tagrex.openLibrary();
  });
  await expect(page.locator('#tracks-body tr.spacer > td').first()).toHaveAttribute('colspan', String(count));
});

test('alternating row tints are plain full-width bands and indicators survive a staged preview', async ({ page }) => {
  await page.evaluate(() => { window.tagrex.selection.clear(); window.tagrex.renderTracks(); });
  const colours = await page.locator('#tracks-body tr[data-path] td.file').evaluateAll(cells => cells.slice(0, 3).map(cell => getComputedStyle(cell).backgroundColor));
  expect(colours[0]).not.toBe(colours[1]);
  expect(colours[0]).toBe(colours[2]);
  await page.locator('#tracks-body tr[data-path] td[data-field="title"]').first().click();
  await page.evaluate(() => {
    const data = new DataTransfer(); data.setData('text/plain', 'Staged title');
    document.activeElement.dispatchEvent(new ClipboardEvent('paste', { bubbles: true, cancelable: true, clipboardData: data }));
  });
  await expect(page.locator('#diff-actionbar')).toBeVisible();
  await expect(page.locator('#tracks-body tr[data-path]').first().locator('.track-artwork')).toHaveAttribute('data-cover-state', 'embedded');
});

test('group labels and collapse carets remain visible when the file table scrolls horizontally', async ({ page }) => {
  await page.setViewportSize({width:900,height:800});
  const wrap = page.locator('#files-view');
  await wrap.evaluate(node => { node.scrollLeft = 400; });
  await expect.poll(() => wrap.evaluate(node => node.scrollLeft)).toBeGreaterThan(100);
  const group = page.locator('#tracks-body tr.group-head').first();
  // Resizing schedules a virtual-window repaint. Read the header and its
  // scrollport in one browser turn, retrying while that replacement settles.
  await expect.poll(() => group.evaluate(node => {
    const label = node.querySelector('.group-label')?.getBoundingClientRect();
    const caret = node.querySelector('.group-caret')?.getBoundingClientRect();
    const bounds = node.closest('.table-wrap').getBoundingClientRect();
    return !!label?.width && !!caret?.width && caret.x >= bounds.x &&
      label.x >= bounds.x && label.x < bounds.x + bounds.width - 50;
  })).toBe(true);
  await group.locator('.group-caret').click();
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(0);
});
