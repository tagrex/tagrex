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
        body = Buffer.from(body.toString().replace(/case([`"'])((?:preview_tag_edits|preview_live_edits|apply_plan|undo))\1:/g,
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

const cell = (page, field, row = 0) => page.locator(`#tracks-body tr[data-path] td[data-field="${field}"]`).nth(row);
const calls = (page, command) => page.evaluate((key) => window.sheetCalls?.[key] || 0, command);
async function paste(page, text) {
  await page.evaluate((value) => {
    const data = new DataTransfer();
    data.setData('text/plain', value);
    document.activeElement.dispatchEvent(new ClipboardEvent('paste', { bubbles: true, cancelable: true, clipboardData: data }));
  }, text);
}
async function copy(page) {
  return page.evaluate(() => {
    const data = new DataTransfer();
    document.activeElement.dispatchEvent(new ClipboardEvent('copy', { bubbles: true, cancelable: true, clipboardData: data }));
    return data.getData('text/plain');
  });
}

test('mode and subtab arrow keys activate the existing controller panes', async ({ page }) => {
  await page.locator('.mode-tab[data-mode="tagger"]').focus();
  await page.keyboard.press('ArrowRight');
  await expect(page.locator('#panel-renamer')).toBeVisible();
  await expect(page.locator('.mode-tab[data-mode="renamer"]')).toHaveAttribute('aria-selected', 'true');
  await page.keyboard.press('End');
  await expect(page.locator('#panel-exporter')).toBeVisible();
  await page.keyboard.press('Home');
  await expect(page.locator('#panel-tagger')).toBeVisible();
  await page.locator('.subtab[data-subtab="online"]').focus();
  await page.keyboard.press('ArrowRight');
  await expect(page.locator('#subtab-editor')).toBeVisible();
  await page.keyboard.press('End');
  await expect(page.locator('#subtab-fromtext')).toBeVisible();
  await page.keyboard.press('Home');
  await expect(page.locator('#subtab-online')).toBeVisible();
});

test('the keyboard splitter resizes the workspace and remembers the layout on reload', async ({ page }) => {
  const splitter = page.getByRole('separator', { name: 'Resize workspace columns', exact: true });
  const before = Number(await splitter.getAttribute('aria-valuenow'));
  const originalWidth = (await page.locator('.mode-col').boundingBox()).width;
  await splitter.focus();
  // Move beyond the inspector's desktop minimum before measuring growth.
  for (let step=0;step<6;step++) await page.keyboard.press('ArrowLeft');
  await expect.poll(async () => Number(await splitter.getAttribute('aria-valuenow'))).toBeLessThan(before);
  expect((await page.locator('.mode-col').boundingBox()).width).toBeGreaterThan(originalWidth);
  const changed = Number(await splitter.getAttribute('aria-valuenow'));
  await page.reload();
  await expect.poll(() => page.evaluate(() => typeof window.tagrex?.setMode)).toBe('function');
  await expect.poll(async () => Number(await splitter.getAttribute('aria-valuenow'))).toBe(changed);
  await splitter.focus();
  for (let step=0;step<6;step++) await page.keyboard.press('ArrowRight');
  await expect.poll(async () => Number(await splitter.getAttribute('aria-valuenow'))).toBe(before);
});

test('cell keyboard navigation, live preview, cancellation and row selection remain separate', async ({ page }) => {
  await cell(page, 'artist').click();
  await expect(cell(page, 'artist')).toBeFocused();
  await page.keyboard.press('ArrowRight');
  await expect(cell(page, 'title')).toBeFocused();
  await page.keyboard.press('F2');
  await expect(cell(page, 'title')).toHaveAttribute('contenteditable', 'true');
  await cell(page, 'title').fill('Direct cell title');
  await page.keyboard.press('Enter');
  await expect(page.locator('#diff-apply')).toBeEnabled();
  expect(await page.evaluate(() => window.tagrex.tracks[0].tags.title)).toBe('Desert Rain');
  expect(await calls(page, 'preview_live_edits')).toBeGreaterThan(0);
  expect(await calls(page, 'apply_plan')).toBe(0);
  await expect(cell(page, 'title')).toBeFocused();
  await page.locator('#diff-apply').click();
  await expect.poll(() => page.evaluate(() => window.tagrex.tracks[0].tags.title)).toBe('Direct cell title');
  await expect(page.locator('#diff-actionbar')).toBeHidden();
  expect(await calls(page, 'apply_plan')).toBe(1);
  await cell(page, 'title').click();
  await page.keyboard.press('Tab');
  await expect(cell(page, 'album')).toBeFocused();
  await page.keyboard.press('Shift+Tab');
  await expect(cell(page, 'title')).toBeFocused();
  await page.keyboard.press('F2');
  await cell(page, 'title').fill('Cancelled title');
  await page.keyboard.press('Escape');
  await expect(cell(page, 'title')).toHaveText('Direct cell title');
  expect(await calls(page, 'apply_plan')).toBe(1);
  await page.keyboard.press('ArrowDown');
  await expect(cell(page, 'title', 1)).toBeFocused();
  await expect(page.locator('#tracks-body tr.selected')).toHaveCount(1);
  await page.keyboard.press('Space');
  await expect(page.locator('#tracks-body tr.selected')).toHaveCount(2);
});

test('Shift selects a rectangle and copies model values as TSV', async ({ page }) => {
  await cell(page, 'artist').click();
  await page.keyboard.press('Shift+ArrowRight');
  await page.keyboard.press('Shift+ArrowDown');
  await expect(page.locator('#tracks-body td.sheet-range')).toHaveCount(4);
  await expect(page.locator('#spreadsheet-tools')).toContainText('2 × 2 cells');
  expect(await copy(page)).toBe('The X Factor\tDesert Rain\r\nWish Mountain\tRadio');
  await page.keyboard.press('Escape');
  await expect(page.locator('#tracks-body td.sheet-range')).toHaveCount(0);
  await expect(page.locator('#spreadsheet-tools')).toContainText('selected');
});

test('a same-order model rebuild preserves the rectangle while a changed sort resets it', async ({ page }) => {
  await cell(page, 'artist').click();
  await page.keyboard.press('Shift+ArrowRight');
  await page.keyboard.press('Shift+ArrowDown');
  const original = await copy(page);
  await page.evaluate(() => window.tagrex.renderTracks());
  await expect(page.locator('#tracks-body td.sheet-range')).toHaveCount(4);
  expect(await copy(page)).toBe(original);
  await page.locator('#lib-refresh').click();
  await expect(page.locator('#tracks-body td.sheet-range')).toHaveCount(4);
  await page.locator('#tracks th[data-sort="artist"]').click();
  await expect(page.locator('#tracks-body td.sheet-range')).toHaveCount(1);
  await expect(page.locator('#spreadsheet-tools')).toContainText('1 cell');
});

async function openGroupedPack(page) {
  await page.evaluate(async () => {
    await window.tagrex.invoke('open_drop', { paths: ['Pack'] });
    document.getElementById('root').value = '/dropped/Pack';
    await window.tagrex.openLibrary();
  });
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(4);
  await expect(page.locator('#tracks-body tr.group-head')).toHaveCount(3);
}

test('the ARIA grid publishes every grouped model row in one contiguous index space', async ({ page }) => {
  await openGroupedPack(page);
  const rows = page.locator('#tracks-body > tr:not(.spacer)');
  await expect(page.locator('#tracks')).toHaveAttribute('aria-rowcount', '8');
  await expect(page.locator('#tracks thead tr')).toHaveAttribute('aria-rowindex', '1');
  expect(await rows.evaluateAll((nodes) => nodes.map((node) => Number(node.getAttribute('aria-rowindex'))))).toEqual([2, 3, 4, 5, 6, 7, 8]);
  await page.locator('#tracks-body tr.group-head .group-caret').first().click();
  await expect(page.locator('#tracks')).toHaveAttribute('aria-rowcount', '6');
  expect(await rows.evaluateAll((nodes) => nodes.map((node) => Number(node.getAttribute('aria-rowindex'))))).toEqual([2, 3, 4, 5, 6]);
});

test('batch validation identifies the same grid row and file as the grouped table', async ({ page }) => {
  await openGroupedPack(page);
  const target = cell(page, 'year', 2);
  const index = await target.locator('..').getAttribute('aria-rowindex');
  await target.click();
  await paste(page, 'invalid-year');
  await expect(page.locator('#toast')).toContainText(`grid row ${index} (01 c.mp3)`);
  expect(await calls(page, 'preview_tag_edits')).toBe(0);
});

for (const kind of ['Paste cells', 'Fill down']) {
  test(`${kind} is a client label while the backend description survives into undo history`, async ({ page }) => {
    await cell(page, 'artist').click();
    if (kind === 'Paste cells') await paste(page, 'Restored artist');
    else {
      await page.keyboard.press('Shift+ArrowDown');
      await page.keyboard.press('Control+d');
    }
    await expect(page.locator('#diff-actionbar')).toBeVisible();
    expect(await page.evaluate(() => window.tagrex.previewPlan.description)).toBe('Edit tags');
    await expect(page.locator('#ab-plan')).toContainText(kind);
    await expect(page.locator('#ab-plan')).toContainText('Edit tags');
    await page.locator('#diff-apply').click();
    await expect(page.locator('#diff-actionbar')).toBeHidden();
    expect(await page.evaluate(async () => (await window.tagrex.invoke('history', {}))[0].description)).toBe('Edit tags');
  });
}

test('Tab can leave the grid at either edge and filtering resets the rectangle', async ({ page }) => {
  await cell(page, 'artist').click();
  await page.keyboard.press('Shift+Tab');
  expect(await page.evaluate(() => !!document.activeElement.closest('#tracks-body'))).toBe(false);
  await cell(page, 'year', 2).click();
  await page.keyboard.press('Tab');
  expect(await page.evaluate(() => !!document.activeElement.closest('#tracks-body'))).toBe(false);
  await cell(page, 'artist').click();
  await page.keyboard.press('Shift+ArrowRight');
  await page.keyboard.press('Shift+ArrowDown');
  await expect(page.locator('#tracks-body td.sheet-range')).toHaveCount(4);
  await page.locator('#filter-panel-toggle').click();
  await page.locator('#filter').fill('Wish Mountain');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(1);
  await expect(page.locator('#tracks-body td.sheet-range')).toHaveCount(1);
  await cell(page, 'title').click();
  await paste(page, 'Filtered title');
  await expect(page.locator('#diff-actionbar')).toBeVisible();
  expect(await page.evaluate(() => window.tagrex.previewPlan.changes.length)).toBe(1);
  expect(await page.evaluate(() => window.tagrex.previewPlan.changes[0].path)).toContain('wish mountain');
});

test('the cell context menu reads the clipboard into a preview and never writes immediately', async ({ page }) => {
  await page.evaluate(() => {
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { readText: async () => 'From toolbar clipboard' } });
  });
  await cell(page, 'title').click();
  await page.keyboard.press('Shift+F10');
  await page.getByRole('menuitem', { name: /^Paste/ }).click();
  await expect(page.locator('#diff-actionbar')).toBeVisible();
  expect(await calls(page, 'preview_tag_edits')).toBe(1);
  expect(await calls(page, 'apply_plan')).toBe(0);
  expect(await page.evaluate(() => window.tagrex.tracks[0].tags.title)).toBe('Desert Rain');
});

test('TSV paste creates one review plan and one Apply/Undo journal batch', async ({ page }) => {
  const original = await page.evaluate(() => window.tagrex.tracks.slice(0, 2).map((track) => ({ ...track.tags })));
  await cell(page, 'artist').click();
  await paste(page, 'Artist A\tTitle A\nArtist B\tTitle B');
  await expect(page.locator('#diff-actionbar')).toBeVisible();
  const changes = await page.evaluate(() => window.tagrex.previewPlan.changes);
  expect(changes).toHaveLength(2);
  expect(changes.map((change) => change.tag_changes.length)).toEqual([2, 2]);
  expect(await calls(page, 'preview_tag_edits')).toBe(1);
  expect(await calls(page, 'apply_plan')).toBe(0);
  expect(await page.evaluate(() => window.tagrex.tracks.slice(0, 2).map((track) => track.tags))).toEqual(original);
  await page.locator('#diff-apply').click();
  await expect(page.locator('#diff-actionbar')).toBeHidden();
  expect(await calls(page, 'apply_plan')).toBe(1);
  expect(await page.evaluate(async () => (await window.tagrex.invoke('history', {})).length)).toBe(1);
  expect(await page.evaluate(() => window.tagrex.tracks.slice(0, 2).map((track) => [track.tags.artist, track.tags.title]))).toEqual([['Artist A', 'Title A'], ['Artist B', 'Title B']]);
  await page.locator('#undo').click();
  await expect.poll(() => calls(page, 'undo')).toBe(1);
  expect(await page.evaluate(async () => (await window.tagrex.invoke('history', {})).length)).toBe(0);
  // The shared browser mock only consumes history on undo. Native value
  // restoration is covered by the core journal tests, not asserted here.
});

test('manual review applies only highlighted files without row checkboxes', async ({ page }) => {
  const original = await page.evaluate(() => window.tagrex.tracks[0].tags.artist);
  await cell(page, 'artist').click();
  await paste(page, 'Artist A\nArtist B');
  await expect(page.locator('#diff-actionbar')).toBeVisible();
  await expect(page.locator('#tracks-body tr[data-path].selected')).toHaveCount(2);
  await page.locator('#tracks-body tr[data-path] td.file').first().click({ modifiers: ['Control'] });
  await expect(page.locator('#tracks-body tr[data-path].selected')).toHaveCount(1);
  await expect(page.locator('#tracks-body input[type="checkbox"]')).toHaveCount(0);
  await page.locator('#diff-apply').click();
  await expect(page.locator('#diff-actionbar')).toBeHidden();
  expect(await page.evaluate(() => window.tagrex.tracks.slice(0, 2).map(track => track.tags.artist))).toEqual([original, 'Artist B']);
  expect(await calls(page, 'apply_plan')).toBe(1);
});

for (const [name, value, field, expected] of [
  ['ragged rows', 'A\tB\nC', 'artist', 'rectangular'],
  ['invalid numeric value', 'New artist\tNew title\tNew album\tbad-year', 'artist', 'Nothing was staged'],
  ['read-only column', '2025\t10:00', 'year', 'read-only'],
  ['beyond visible rows', 'A\nB\nC\nD', 'artist', 'beyond'],
  ['unclosed quoted field', '"A\tB', 'artist', 'unclosed'],
  ['cell count limit', Array(10001).fill('x').join('\t'), 'artist', '10,000'],
  ['text limit', 'x'.repeat(1024 * 1024 + 1), 'artist', '1 MiB'],
]) {
  test(`paste rejects ${name} without partial staging or writes`, async ({ page }) => {
    const original = await page.evaluate(() => window.tagrex.tracks.map((track) => ({ ...track.tags })));
    await cell(page, field).click();
    await paste(page, value);
    await expect(page.locator('#toast')).toContainText(expected);
    expect(await calls(page, 'preview_tag_edits')).toBe(0);
    expect(await calls(page, 'apply_plan')).toBe(0);
    expect(await page.evaluate(() => window.tagrex.tracks.map((track) => track.tags))).toEqual(original);
    await expect(page.locator('#diff-actionbar')).toBeHidden();
  });
}

test('quoted tabs and newlines survive paste, while mismatched range dimensions are refused', async ({ page }) => {
  await cell(page, 'artist').click();
  await paste(page, '"Artist\tA"\t"Title\nA"\r\n"Artist ""B"""\tTitle B\r\n');
  await expect(page.locator('#diff-actionbar')).toBeVisible();
  expect(await page.evaluate(() => window.tagrex.previewPlan.changes.map((change) => change.tag_changes.map((edit) => edit.new)))).toEqual([['Artist\tA', 'Title\nA'], ['Artist "B"', 'Title B']]);
  await page.locator('#diff-discard').click();
  await cell(page, 'artist').click();
  await page.keyboard.press('Shift+ArrowRight');
  await page.keyboard.press('Shift+ArrowDown');
  await paste(page, 'A\tB');
  await expect(page.locator('#toast')).toContainText('dimensions');
  expect(await calls(page, 'preview_tag_edits')).toBe(1);
});

test('fill down previews the first range row and contextual actions return focus', async ({ page }) => {
  await cell(page, 'artist').click();
  await page.keyboard.press('Shift+ArrowDown');
  await page.keyboard.press('Shift+ArrowDown');
  await cell(page, 'artist', 1).click({ button: 'right' });
  await expect(page.getByRole('menuitem', { name: /Fill down/ })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(cell(page, 'artist', 2)).toBeFocused();
  await page.keyboard.press('Control+d');
  await expect(page.locator('#diff-actionbar')).toBeVisible();
  const changes = await page.evaluate(() => window.tagrex.previewPlan.changes);
  expect(changes).toHaveLength(2); // the first row already has its source value
  expect(changes.every((change) => change.tag_changes[0].new === 'The X Factor')).toBe(true);
  expect(await calls(page, 'preview_tag_edits')).toBe(1);
  expect(await calls(page, 'apply_plan')).toBe(0);
});

test('range selection and fill work across rows outside the virtual window', async ({ page }) => {
  await page.evaluate(async () => {
    await window.tagrex.invoke('open_drop', { paths: Array.from({ length: 350 }, (_, i) => `track-${String(i).padStart(3, '0')}.mp3`) });
    document.getElementById('root').value = '/dropped';
    await window.tagrex.openLibrary();
  });
  await expect.poll(() => page.evaluate(() => window.tagrex.tracks.length)).toBe(350);
  expect(await page.locator('#tracks-body tr[data-path]').count()).toBeLessThan(100);
  await cell(page, 'artist').first().click();
  await page.locator('#files-view').evaluate((element) => { element.scrollTop = 3000; });
  await expect.poll(() => page.locator('#tracks-body tr[data-path]').first().getAttribute('data-path')).not.toBe('/dropped/track-000.mp3');
  const target = page.locator('#tracks-body td[data-field="artist"]').last();
  const lastPath = await target.getAttribute('data-path');
  const end = Number(lastPath.match(/track-(\d+)/)[1]);
  // Mouse Shift-click ranges tracks; keyboard Shift+arrows ranges cells.
  await page.keyboard.down('Shift');
  for (let index=0;index<end;index++) await page.keyboard.press('ArrowDown');
  await page.keyboard.up('Shift');
  expect((await copy(page)).split('\r\n')).toHaveLength(end + 1);
  await paste(page, 'Unified artist');
  await expect(page.locator('#diff-actionbar')).toBeVisible();
  expect(await page.evaluate(() => window.tagrex.previewPlan.changes.length)).toBe(end + 1);
  expect(await calls(page, 'preview_tag_edits')).toBe(1);
  expect(await calls(page, 'apply_plan')).toBe(0);
  expect(await page.locator('#tracks-body tr[data-path]').count()).toBeLessThan(100);
});

for (const invalidKind of ['date', 'cover']) {
  test(`a backend-invalid ${invalidKind} rejects the entire spreadsheet batch before staging`, async ({ page }) => {
    const original = await page.evaluate(() => window.tagrex.tracks.map((track) => ({ ...track.tags })));
    await page.evaluate((kind) => {
      const originalResolve = Promise.resolve;
      Promise.resolve = function (value) {
        // The browser mock omits backend format-specific validation. Add the
        // response flag only: 1996-13 passes the client rule but real ID3v2
        // date validation rejects it. No production/mock source is replaced.
        if (value?.description === 'Edit tags' && value.changes?.length) {
          if (kind === 'date') {
            for (const change of value.changes) for (const field of change.tag_changes) {
              if (field.field === 'year' && field.new === '1996-13') field.invalid = true;
            }
          } else {
            // Defensive boundary case: CoverChangeDto currently has no invalid
            // flag, but a future/extended response must not partly apply it.
            value.changes[0].cover_change = { old: [], new: [], invalid: true };
          }
        }
        return originalResolve.call(this, value);
      };
    }, invalidKind);
    await cell(page, 'year').click();
    await paste(page, '1996-13\n1997');
    await expect(page.locator('#toast')).toContainText('Nothing was staged');
    await expect(page.locator('#toast')).toContainText('01 - the x factor - desert rain.mp3');
    await expect(page.locator('#toast')).toContainText(invalidKind === 'date' ? 'Year' : 'Artwork');
    await expect(page.locator('#diff-actionbar')).toBeHidden();
    expect(await calls(page, 'preview_tag_edits')).toBe(1);
    expect(await calls(page, 'apply_plan')).toBe(0);
    expect(await page.evaluate(() => window.tagrex.tracks.map((track) => track.tags))).toEqual(original);
  });
}

test('wheel scrolling can unmount the focused row while keyboard navigation remains in the grid', async ({ page }) => {
  await page.locator('#filter-panel-toggle').click();
  await page.evaluate(async () => {
    await window.tagrex.invoke('open_drop', { paths: Array.from({ length: 350 }, (_, i) => `track-${String(i).padStart(3, '0')}.mp3`) });
    document.getElementById('root').value = '/dropped';
    await window.tagrex.openLibrary();
  });
  await cell(page, 'artist').click();
  const wrap = page.locator('#files-view');
  const bounds = await wrap.boundingBox();
  await page.mouse.move(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2);
  await page.mouse.wheel(0, 5000);
  await expect.poll(() => wrap.evaluate((element) => element.scrollTop)).toBeGreaterThan(4000);
  await expect(page.locator('#tracks-body tr[data-path="/dropped/track-000.mp3"]')).toHaveCount(0);
  await expect(page.locator('#tracks')).toBeFocused();
  const afterWheel = await wrap.evaluate((element) => element.scrollTop);
  await page.locator('#filter').focus();
  await page.keyboard.press('ArrowDown');
  expect(await wrap.evaluate((element) => element.scrollTop)).toBe(afterWheel);
  await page.locator('#tracks').focus();
  await page.keyboard.press('ArrowDown');
  await expect(page.locator('#tracks-body tr[data-path="/dropped/track-001.mp3"] td[data-field="artist"]')).toBeFocused();
  expect(await wrap.evaluate((element) => element.scrollTop)).toBeLessThan(1000);
  expect(await page.evaluate(() => window.tagrex.selection.size)).toBe(1);
});

for (const selectionMode of ['hidden checkboxes', 'optional checkboxes', 'staged preview']) {
  test(`artwork and selection stay pinned while Filename scrolls with ${selectionMode}`, async ({ page }) => {
    await page.setViewportSize({ width: 900, height: 800 });
    if (selectionMode === 'optional checkboxes') await page.evaluate(() => document.body.classList.add('show-checkbox'));
    else if (selectionMode === 'staged preview') {
      await cell(page, 'year').click();
      await paste(page, '1998');
      await expect(page.locator('#diff-actionbar')).toBeVisible();
    }
    // Sample the geometry and actual clamped scroll in one frame. Viewport
    // resizing and review focus can otherwise shift columns between reads.
    const movement = await page.locator('#files-view').evaluate(wrap => {
      wrap.scrollLeft = 0;
      const file = wrap.querySelector('tr[data-path] td.file');
      const before = file.getBoundingClientRect().left;
      wrap.scrollLeft = 120;
      return {file:before - file.getBoundingClientRect().left, scroll:wrap.scrollLeft};
    });
    expect(movement.scroll).toBeGreaterThan(50);
    expect(movement.file).toBeCloseTo(movement.scroll, 0);
    await expect.poll(() => page.evaluate(() => {
      const wrap = document.getElementById('files-view').getBoundingClientRect();
      const row = document.querySelector('#tracks-body tr[data-path]');
      const selectionCell = row.querySelector('td.sel');
      const selection = selectionCell.getBoundingClientRect();
      const artwork = row.querySelector('td.artwork-col').getBoundingClientRect();
      const file = row.querySelector('td.file').getBoundingClientRect();
      const leadingEdge = getComputedStyle(selectionCell).display === 'none' ? wrap.left : selection.right;
      return artwork.left >= wrap.left && Math.abs(artwork.left - leadingEdge) < 0.25 && getComputedStyle(row.querySelector("td.file")).position !== "sticky";
    })).toBe(true);
  });
}

test('read-only and locked range reasons remain visible as lock state changes', async ({ page }) => {
  await page.locator('#tracks-body tr[data-path] td.file').first().click();
  await expect(page.locator('#spreadsheet-tools')).toContainText('File is read-only');
  await page.keyboard.press('Shift+F10');
  await expect(page.getByRole('menuitem', { name: /^Paste/ })).toHaveAttribute('aria-disabled', 'true');
  await expect(page.locator('.sheet-menu-reason')).toContainText('File is read-only');
  await page.keyboard.press('Escape');
  await cell(page, 'artist').click();
  await page.locator('.subtab[data-subtab="editor"]').click();
  const lock = page.locator('#fields-body .fe-row').filter({ has: page.locator('input[data-key="artist"]') }).locator('.fe-lock');
  await lock.click();
  await expect(cell(page, 'artist')).toHaveAttribute('aria-readonly', 'true');
  await expect(page.locator('#spreadsheet-tools')).toContainText('Artist is locked');
  await lock.click();
  await expect(cell(page, 'artist')).toHaveAttribute('aria-readonly', 'false');
  await cell(page, 'artist').focus();
  await page.keyboard.press('Shift+F10');
  await expect(page.getByRole('menuitem', { name: /^Paste/ })).not.toHaveAttribute('aria-disabled', 'true');
  await page.keyboard.press('Escape');
  await expect(page.locator('#spreadsheet-tools')).not.toContainText('Artist is locked');
});

test('an unchanged refresh avoids rewriting grid attributes or remeasuring structural headers', async ({ page }) => {
  await cell(page, 'artist').click();
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  const evidence = await page.evaluate(async () => {
    const grid = document.getElementById('tracks');
    const headers = [...grid.querySelectorAll('th.sel, th.artwork-col')];
    const originalRects = headers.map(header => header.getBoundingClientRect.bind(header));
    let reads = 0, writes = 0;
    headers.forEach((header, index) => { header.getBoundingClientRect = () => { reads++; return originalRects[index](); }; });
    const observer = new MutationObserver((records) => { writes += records.length; });
    observer.observe(grid, { subtree: true, attributes: true, attributeFilter: ['role', 'aria-rowindex', 'aria-colindex', 'aria-readonly', 'aria-selected', 'aria-rowcount', 'aria-colcount', 'tabindex'] });
    document.body.classList.add('sheet-unchanged-paint-probe');
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    observer.disconnect();
    headers.forEach((header, index) => { header.getBoundingClientRect = originalRects[index]; });
    document.body.classList.remove('sheet-unchanged-paint-probe');
    return { reads, writes };
  });
  expect(evidence).toEqual({ reads: 0, writes: 0 });
});

for (const viewport of [{ width: 375, height: 667 }, { width: 844, height: 390 }]) {
  test(`every mode keeps its primary actions reachable at ${viewport.width}×${viewport.height}`, async ({ page }) => {
    await page.setViewportSize(viewport);
    const actions = {
      tagger: [], renamer: ['preview'],
      generator: ['presets-edit', 'autonum-run', 'vinyl-split'],
      deduplicator: ['dup-scan'], exporter: ['export-run'],
    };
    for (const [mode, ids] of Object.entries(actions)) {
      await page.locator(`.mode-tab[data-mode="${mode}"]`).click();
      if (mode === 'tagger') await page.locator('.subtab[data-subtab="editor"]').click();
      const panel = page.locator(`#panel-${mode}`);
      await expect(panel).toBeVisible();
      await expect(page.locator('.mode-panel:visible')).toHaveCount(1);
      const box = await panel.boundingBox();
      expect(box.x).toBeGreaterThanOrEqual(0);
      expect(box.x + box.width).toBeLessThanOrEqual(viewport.width + 1);
      expect(box.y + box.height).toBeLessThanOrEqual(viewport.height + 1);
      for (const id of ids) {
        if (mode === 'generator') await page.getByRole('tab', { name: 'Inspector', exact: true }).click();
        if (mode === 'generator' && id !== 'presets-edit') await page.locator(`#gen-job-${id === 'autonum-run' ? 'number' : 'sides'}`).click();
        if (mode === 'generator' && id === 'presets-edit') await page.getByRole('tab', { name: 'Transformations', exact: true }).click();
        const button = page.locator(`#${id}`);
        await button.scrollIntoViewIfNeeded();
        await expect(button).toBeInViewport({ ratio: 1 });
        await button.focus();
        await expect(button).toBeFocused();
      }
    }
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(viewport.width);
  });
}


test('the table is a flat file list with no control strip or boxed cell selection', async ({ page }) => {
  await cell(page, 'artist').click();
  const state = await page.evaluate(() => {
    const pane = document.querySelector('.files-col');
    const active = document.querySelector('#tracks td.sheet-active');
    const tools = document.querySelector('#spreadsheet-tools');
    return {
      paneRadius: getComputedStyle(pane).borderTopLeftRadius,
      cellShadow: getComputedStyle(active).boxShadow,
      filenameCursor: getComputedStyle(document.querySelector('#tracks td.file')).cursor,
      leftBorder: getComputedStyle(active).borderLeftWidth,
      rightBorder: getComputedStyle(active).borderRightWidth,
      controlHeight: tools.getBoundingClientRect().height,
      controls: tools.querySelectorAll('.spreadsheet-toolbar, #sheet-paste, #sheet-actions').length,
    };
  });
  expect(state).toEqual({ paneRadius: '14px', cellShadow: 'none', filenameCursor: 'default', leftBorder: '0px', rightBorder: '0px', controlHeight: 0, controls: 0 });
  await page.keyboard.press('Shift+F10');
  await expect(page.getByRole('menuitem', { name: /^Copy/ })).toBeVisible();
  await expect(page.getByRole('menuitem', { name: /^Paste/ })).toBeVisible();
});
