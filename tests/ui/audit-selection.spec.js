import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';

const root = resolve('.test-dist');
test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    window.__audit = { reads: [], externalReads: [], pending: [], externalPending: [], wait: false, externalWait: false, historyReads: 0, listReads: 0, rowReads: [], undoSnapshots: [] };
    window.__audit.summary = (name) => {
      const image = { mime: 'image/svg+xml', data_base64: btoa('<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>'), kind: 'front', description: name };
      return { total: 1, with_cover: 1, distinct: false, samples: [image], shared_set: [image] };
    };
    window.__audit.external = () => {
      window.__audit.externalReads.push([]);
      if (!window.__audit.externalWait) return Promise.resolve(null);
      return new Promise(resolve => window.__audit.externalPending.push(resolve));
    };
    window.__audit.read = (paths) => {
      window.__audit.reads.push(paths);
      if (!window.__audit.wait) return Promise.resolve(window.__audit.summary('Current artwork'));
      return new Promise(resolve => window.__audit.pending.push({ paths, resolve }));
    };
  });
  await page.route('http://tagrex.test/**', async route => {
    const path = resolve(root, '.' + new URL(route.request().url()).pathname);
    if (!path.startsWith(root + sep)) return route.abort();
    try {
      let body = await readFile(path);
      if (path.endsWith('.js')) {
        body = body.toString();
        const state = body.match(/case([`"'])list_tracks\1:\s*return Promise\.resolve\(([\w$]+)\.tracks\)/)?.[2];
        const rowsCase = body.match(/case([`"'])read_track_rows\1:/);
        const rowArgs = rowsCase && body.slice(rowsCase.index, rowsCase.index + 600).match(/([\w$]+)\.paths/)?.[1];
        if (rowsCase && !rowArgs) throw new Error('Cannot instrument scoped readback arguments');
        if (rowArgs) body = body.replace(/case([`"'])read_track_rows\1:/, `$& window.__audit.rowReads.push([...${rowArgs}.paths]);`);
        if (state) {
          body = body.replace(/case([`"'])apply_plan\1:/, `$& window.__audit.undoSnapshots.push(structuredClone(${state}.tracks));`)
            .replace(/case([`"'])undo\1:/, `$& if(window.__audit.undoSnapshots.length) ${state}.tracks=window.__audit.undoSnapshots.pop();`)
            .replace(/(case([`"'])list_tracks\2:\s*return Promise\.resolve\()([\w$]+)\.tracks(\))/g, '$1structuredClone($3.tracks)$4');
        }
        body = body.replace(/case\s*["'`]read_cover_summary["'`]\s*:\s*\{\s*let\s+\w+=(\w+)\.paths/, (match, args) => match.replace('{', `{ if(window.__audit) return window.__audit.read(${args}.paths);`));
        body = body.replace(/case\s*["'`]read_external_cover["'`]\s*:/, '$& if(window.__audit) return window.__audit.external();');
        body = body.replace(/case\s*["'`]history["'`]\s*:/, '$& if(window.__audit) window.__audit.historyReads++;');
        body = body.replace(/case\s*["'`]list_tracks["'`]\s*:/, '$& if(window.__audit) window.__audit.listReads++;');
      }
      await route.fulfill({ body, contentType: path.endsWith('.js') ? 'text/javascript' : path.endsWith('.css') ? 'text/css' : 'text/html' });
    } catch (error) {
      if (path.endsWith('.js') && !String(error).includes('ENOENT')) throw error;
      await route.fulfill({ status: 404, body: 'Missing fixture' });
    }
  });
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(() => page.evaluate(() => typeof window.tagrex?.openLibrary)).toBe('function');
  await page.locator('#root-display').click();
  await page.locator('#root').fill('/music');
  await page.locator('#root').press('Enter');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
});

const row = (page, at) => page.locator('#tracks-body tr[data-path] .file').nth(at);

test('Online selection does no hidden Editor artwork I/O and Editor catches up once', async ({ page }) => {
  await row(page, 1).click();
  await row(page, 2).click();
  expect(await page.evaluate(() => window.__audit.reads.length)).toBe(0);
  await page.locator('.subtab[data-subtab="editor"]').click();
  await expect.poll(() => page.evaluate(() => window.__audit.reads.length)).toBe(1);
  await expect(page.locator('#fields-body input[data-key="title"]')).toHaveValue('Feel It');
});

test('filter, sorting and regrouping preserve an unstaged draft and its input', async ({ page }) => {
  await page.locator('.subtab[data-subtab="editor"]').click();
  const title = page.locator('#fields-body input[data-key="title"]');
  await title.fill('UNSTAGED DRAFT');
  await title.evaluate(node => window.__draftInput = node);
  await page.locator('#filter-panel-toggle').click();
  await page.locator('#filter').fill('mp3');
  await page.waitForTimeout(220);
  await expect(title).toHaveValue('UNSTAGED DRAFT');
  expect(await title.evaluate(node => node === window.__draftInput)).toBe(true);
  await page.locator('th[data-sort="title"]').click();
  await expect(title).toHaveValue('UNSTAGED DRAFT');
  expect(await title.evaluate(node => node === window.__draftInput)).toBe(true);
  await page.evaluate(() => window.tagrex.renderTracks());
  await expect(title).toHaveClass(/dirty/);
});

test('late artwork cannot replace the current selection or stage its images', async ({ page }) => {
  await page.locator('.subtab[data-subtab="editor"]').click();
  await expect(page.locator('.cover-main-image img')).toHaveAttribute('alt', 'Current artwork');
  await page.evaluate(() => window.__audit.wait = true);
  await row(page, 1).click();
  await expect.poll(() => page.evaluate(() => window.__audit.pending.length)).toBe(1);
  await row(page, 2).click();
  await expect.poll(() => page.evaluate(() => window.__audit.pending.length)).toBe(2);
  await page.evaluate(() => window.__audit.pending[1].resolve(window.__audit.summary('B artwork')));
  await expect(page.locator('.cover-main-image img')).toHaveAttribute('alt', 'B artwork');
  await page.evaluate(() => window.__audit.pending[0].resolve(window.__audit.summary('A stale artwork')));
  await page.waitForTimeout(40);
  await expect(page.locator('.cover-main-image img')).toHaveAttribute('alt', 'B artwork');
  await page.locator('.cover-main-image').click();
  await expect(page.locator('.cover-lightbox .aw-hero')).toHaveAttribute('alt', 'Cover art');
  expect(await page.evaluate(() => window.tagrex.previewPlan)).toBeNull();
});


test('deselection invalidates pending artwork and detached edit actions', async ({ page }) => {
  await page.locator('.subtab[data-subtab="editor"]').click();
  await expect(page.locator('.cover-main-image img')).toHaveAttribute('alt', 'Current artwork');
  await page.locator('#cover-well .cover-edit').click();
  await page.locator('[data-cover="remove"]').evaluate(node => window.__oldRemove = node);
  await page.keyboard.press('Escape');
  await page.evaluate(() => window.__audit.wait = true);
  await row(page, 1).click();
  await expect.poll(() => page.evaluate(() => window.__audit.pending.length)).toBe(1);
  await page.evaluate(() => { window.tagrex.selection.clear(); window.tagrex.renderTracks(); });
  await page.evaluate(() => {
    window.__oldRemove.click();
    window.__audit.pending[0].resolve(window.__audit.summary('Obsolete artwork'));
  });
  await expect(page.locator('#cover-set')).toBeHidden();
  expect(await page.evaluate(() => window.tagrex.previewPlan)).toBeNull();
});

test('folder artwork responses stay bound to their source selection', async ({ page }) => {
  await page.evaluate(() => window.__audit.externalWait = true);
  await page.locator('.subtab[data-subtab="editor"]').click();
  await expect.poll(() => page.evaluate(() => window.__audit.externalPending.length)).toBe(1);
  await row(page, 2).click();
  await expect.poll(() => page.evaluate(() => window.__audit.externalPending.length)).toBe(2);
  await page.evaluate(() => window.__audit.externalPending[1](window.__audit.summary('Current folder artwork').samples[0]));
  await page.locator('#cover-well .cover-edit').click();
  await expect(page.locator('[data-cover="external"]')).toBeVisible();
  await page.evaluate(() => window.__audit.externalPending[0](window.__audit.summary('Obsolete folder artwork').samples[0]));
  await page.locator('[data-cover="external"]').click();
  await expect.poll(() => page.evaluate(() => (window.tagrex.previewPlan?.changes[0]?.cover_change?.new?.description ?? window.tagrex.previewPlan?.changes[0]?.cover_change?.new?.[0]?.description))).toBe('Current folder artwork');
});

test('a new selected subject never inherits another files unstaged draft', async ({ page }) => {
  await page.locator('.subtab[data-subtab="editor"]').click();
  await page.locator('#fields-body input[data-key="title"]').fill('A ONLY DRAFT');
  await row(page, 1).click();
  await expect(page.locator('#fields-body input[data-key="title"]')).toHaveValue('Radio');
  await expect(page.locator('#fields-body input[data-key="title"]')).not.toHaveClass(/dirty/);
});

test('rapid Editor selection bounds native reads and keeps only the latest waiting subject', async ({ page }) => {
  await page.locator('.subtab[data-subtab="editor"]').click();
  await expect(page.locator('.cover-main-image img')).toHaveAttribute('alt', 'Current artwork');
  await page.evaluate(() => window.__audit.wait = true);
  for (const at of [1, 2, 0, 1]) await row(page, at).click();
  expect(await page.evaluate(() => window.__audit.pending.length)).toBe(2);
  await page.evaluate(() => window.__audit.pending[0].resolve(window.__audit.summary('Old first response')));
  await expect.poll(() => page.evaluate(() => window.__audit.pending.length)).toBe(3);
  await page.evaluate(() => {
    window.__audit.pending[1].resolve(window.__audit.summary('Old second response'));
    window.__audit.pending[2].resolve(window.__audit.summary('Latest subject artwork'));
  });
  await expect(page.locator('.cover-main-image img')).toHaveAttribute('alt', 'Latest subject artwork');
  expect(await page.evaluate(() => window.__audit.pending[2].paths)).toEqual(['/music/02 - wish mountain - radio.mp3']);
});

test('large filter queries reuse normalized metadata and publish the latest complete result', async ({ page }) => {
  await page.evaluate(async () => {
    await window.tagrex.invoke('open_drop', { paths: Array.from({ length: 5000 }, (_, i) => `track-${String(i).padStart(4, '0')}.mp3`) });
    document.getElementById('root').value = '/dropped';
    await window.tagrex.openLibrary();
    window.__filterReads = 0;
    for (const track of window.tagrex.tracks) {
      track.tags = new Proxy(track.tags, { ownKeys(target) { window.__filterReads++; return Reflect.ownKeys(target); } });
    }
  });
  await page.locator('#filter-panel-toggle').click();
  await page.locator('#filter').fill('track-0');
  await expect(page.locator('#status-sel')).toContainText('1000/5000');
  expect(await page.evaluate(() => window.__filterReads)).toBe(5000);
  await page.evaluate(() => window.__filterReads = 0);
  await page.locator('#filter').fill('track-00');
  await expect(page.locator('#status-sel')).toContainText('100/5000');
  expect(await page.evaluate(() => window.__filterReads)).toBe(0);
});


test('Undo reuses the toolbar history head and requests summaries only after completion', async ({ page }) => {
  await page.locator('.subtab[data-subtab="editor"]').click();
  await page.locator('#fields-body input[data-key="title"]').fill('New title');
  await expect(page.locator('#diff-apply')).toBeEnabled();
  await expect(page.locator('#diff-actionbar')).toBeVisible();
  await page.locator('#diff-apply').click();
  await expect(page.locator('#diff-actionbar')).toBeHidden();
  await expect(page.locator('#undo')).toBeEnabled();
  const before = await page.evaluate(() => window.__audit.historyReads);
  await page.locator('#undo').click();
  await expect.poll(() => page.evaluate(() => window.__audit.historyReads)).toBe(before + 1);
});


test('tag-only Apply and Undo read back only changed rows and preserve unrelated DTO identities', async ({ page }) => {
  await page.locator('.subtab[data-subtab="editor"]').click();
  await page.evaluate(() => { window.__untouchedTrack = window.tagrex.tracks[1]; window.__audit.listReads = 0; window.__audit.rowReads = []; });
  await page.locator('#fields-body input[data-key="title"]').fill('Scoped new title');
  await expect(page.locator('#diff-apply')).toBeEnabled();
  await expect(page.locator('#diff-actionbar')).toBeVisible();
  expect(await page.evaluate(() => window.tagrex.tracks[0].tags.title)).toBe('Desert Rain');
  await page.locator('#diff-apply').click();
  await expect(page.locator('#diff-actionbar')).toBeHidden();
  await expect.poll(() => page.evaluate(() => window.tagrex.tracks[0].tags.title)).toBe('Scoped new title');
  expect(await page.evaluate(() => window.tagrex.tracks[1] === window.__untouchedTrack)).toBe(true);
  expect(await page.evaluate(() => window.__audit.listReads)).toBe(0);
  expect(await page.evaluate(() => window.__audit.rowReads)).toEqual([['/music/01 - the x factor - desert rain.mp3']]);
  await expect(page.locator('#undo')).toBeEnabled();
  await page.locator('#undo').click();
  await expect.poll(() => page.evaluate(() => window.tagrex.tracks[0].tags.title)).toBe('Desert Rain');
  expect(await page.evaluate(() => window.tagrex.tracks[1] === window.__untouchedTrack)).toBe(true);
  expect(await page.evaluate(() => window.__audit.listReads)).toBe(0);
  expect(await page.evaluate(() => window.__audit.rowReads)).toEqual([
    ['/music/01 - the x factor - desert rain.mp3'], ['/music/01 - the x factor - desert rain.mp3']
  ]);
});
