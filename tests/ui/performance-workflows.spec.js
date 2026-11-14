import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';

const root = resolve('.test-dist');
const errors = new WeakMap();
test.beforeEach(async ({ page }) => {
  const issues = [];
  errors.set(page, issues);
  page.on('pageerror', error => issues.push(error.message));
  await page.addInitScript(() => { window.perfCalls = {}; window.perfColumnPaths = []; window.perfBatchColumns = []; });
  await page.route('http://tagrex.test/**', async route => {
    const path = resolve(root, '.' + new URL(route.request().url()).pathname);
    if (!path.startsWith(root + sep)) return route.abort();
    try {
      let body = await readFile(path);
      if (path.endsWith('.js')) {
        let source = body.toString();
        // Only the served mock is instrumented. Clone its list response to model
        // native IPC ownership, so a write cannot mutate the frontend DTO early.
        source = source.replace(/(case([`"'])list_tracks\2:\s*return Promise\.resolve\()([\w$]+)\.tracks(\))/g, '$1structuredClone($3.tracks)$4');
        source = source.replace(/case([`"'])(list_tracks|read_track|read_track_rows|preview_tag_edits|preview_live_edits|apply_plan|render_column|render_columns|probe_tags_from_names)\1:/g,
          (match, _quote, command, offset) => {
            let trace = `window.perfCalls['${command}']=(window.perfCalls['${command}']||0)+1;`;
            if (command === 'render_column') {
              const parameter = source.slice(offset, offset + 500).match(/return [\w$]+\(([\w$]+),/);
              if (!parameter) throw new Error('Could not instrument render_column mock arguments');
              trace += `window.perfColumnPaths.push([...${parameter[1]}.paths]);`;
            }
            if (command === 'render_columns') {
              const parameter = source.slice(offset, offset + 700).match(/([\w$]+)\.patterns/);
              if (!parameter) throw new Error('Could not instrument render_columns mock arguments');
              trace += `window.perfBatchColumns.push(structuredClone(${parameter[1]}));window.perfColumnPaths.push([...${parameter[1]}.paths]);window.perfColumnBatchDepth=true;`;
            }
            return match + trace;
          });
        body = Buffer.from(source);
      }
      await route.fulfill({ body, contentType: path.endsWith('.js') ? 'text/javascript' : path.endsWith('.css') ? 'text/css' : path.endsWith('.woff2') ? 'font/woff2' : 'text/html' });
    } catch (error) {
      if (path.endsWith('.js') && !String(error).includes('ENOENT')) throw error;
      await route.fulfill({ status: 404, body: 'Missing fixture' });
    }
  });
});
test.afterEach(async ({ page }) => expect(errors.get(page)).toEqual([]));

async function open(page, columns = [], fields = []) {
  await page.addInitScript(({ custom, fields }) => {
    localStorage.setItem('tagrex.groupBy', '');
    if (custom.length) {
      localStorage.setItem('tagrex.customColumns', JSON.stringify(custom));
      localStorage.setItem('tagrex.columns', JSON.stringify(['file', 'artist', 'title', ...fields, ...custom.map(column => `custom:${column.id}`)]));
    }
  }, { custom: columns, fields });
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(() => page.evaluate(() => typeof window.tagrex?.openLibrary)).toBe('function');
  await page.locator('#root-display').click();
  await page.locator('#root').fill('/music');
  await page.locator('#root').press('Enter');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
  if (columns.length) {
    await expect.poll(() => page.evaluate(() => window.perfCalls.render_columns || 0)).toBe(1);
    expect(await page.evaluate(() => window.perfCalls.render_column || 0)).toBe(0);
    expect(await page.evaluate(() => window.perfBatchColumns[0].patterns)).toEqual(columns.map(column => column.pattern));
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  }
  await page.evaluate(() => {
    window.perfCalls = {};
    window.perfColumnPaths = [];
    window.perfBatchColumns = [];
    window.perfPaints = 0;
    const replace = Element.prototype.replaceChildren;
    Element.prototype.replaceChildren = function (...nodes) {
      if (this.id === 'tracks-body') window.perfPaints++;
      return replace.apply(this, nodes);
    };
  });
}
const customCell = (page, row = 0, column = 0) => page.locator('#tracks-body tr[data-path]').nth(row).locator('td.position-cell').nth(column);
const cell = (page, field, row = 0) => page.locator(`#tracks-body tr[data-path] td[data-field="${field}"]`).nth(row);
async function edit(page, field, value) {
  await cell(page, field).dblclick();
  await page.keyboard.press('Control+a');
  await page.keyboard.insertText(value);
  await page.keyboard.press('Enter');
  await expect(page.locator('#diff-apply')).toBeEnabled();
  expect(await page.evaluate(() => window.perfCalls.apply_plan || 0)).toBe(0);
  expect(await page.evaluate(() => window.perfCalls.read_track_rows || 0)).toBe(0);
  await page.evaluate(() => { window.perfPaints = 0; });
  await page.locator('#diff-apply').click();
  await expect.poll(() => page.evaluate(() => window.perfCalls.read_track_rows || 0)).toBe(1);
  await expect(cell(page, field)).toHaveText(value);
}

test('one explicitly applied cell refreshes its DTO without relisting the library', async ({ page }) => {
  await open(page);
  const original = await page.evaluate(() => window.tagrex.tracks.slice(1).map(track => structuredClone(track)));
  await edit(page, 'title', 'A single-file edit');
  expect(await page.evaluate(() => window.perfCalls)).toMatchObject({ apply_plan: 1, read_track_rows: 1 });
  expect(await page.evaluate(() => window.perfCalls.list_tracks || 0)).toBe(0);
  expect(await page.evaluate(() => window.perfCalls.preview_live_edits || 0)).toBeGreaterThan(0);
  expect(await page.evaluate(() => window.tagrex.tracks.slice(1))).toEqual(original);
  await expect(page.locator('#undo')).toBeEnabled();
  await expect(page.locator('#diff-actionbar')).toBeHidden();
});

test('affected custom columns refresh only the edited path and preserve other cached rows', async ({ page }) => {
  await open(page, [
    { id: 'title', name: 'Display title', pattern: '%artist% — %title%', align: 'left' },
    { id: 'album', name: 'Album only', pattern: '%album%', align: 'left' },
  ]);
  await expect(customCell(page, 1)).toHaveText('Wish Mountain — Radio');
  await edit(page, 'title', 'Fresh title');
  await expect(customCell(page)).toHaveText('The X Factor — Fresh title');
  await expect(customCell(page, 1)).toHaveText('Wish Mountain — Radio');
  await expect(customCell(page, 0, 1)).toHaveText('La Bush');
  expect(await page.evaluate(() => window.perfCalls.render_columns)).toBe(1);
  expect(await page.evaluate(() => window.perfColumnPaths)).toEqual([['/music/01 - the x factor - desert rain.mp3']]);
});

test('unrelated custom masks do not refresh after a title edit', async ({ page }) => {
  await open(page, [{ id: 'album', name: 'Album only', pattern: '%album%', align: 'left' }]);
  await edit(page, 'title', 'Unrelated field');
  expect(await page.evaluate(() => (window.perfCalls.render_column || 0)+(window.perfCalls.render_columns || 0))).toBe(0);
  await expect(customCell(page)).toHaveText('La Bush');
});

test('one-path mask refresh preserves vinyl-side and explicit track padding semantics', async ({ page }) => {
  await open(page, [{ id: 'side', name: 'Side and track', pattern: '%side%%track:3%', align: 'left' }], ['media', 'disc', 'track']);
  for (const [field, value] of [['media', 'Vinyl'], ['disc', '1'], ['track', '7']]) {
    await page.evaluate(() => { window.perfCalls = {}; window.perfPaints = 0; window.perfColumnPaths = []; });
    await edit(page, field, value);
    expect(await page.evaluate(() => window.perfColumnPaths)).toEqual([['/music/01 - the x factor - desert rain.mp3']]);
  }
  await expect(customCell(page)).toHaveText('A007');
  await expect(customCell(page, 1)).toHaveText('');
});

test('filtered selection reuses model counts, including files inside collapsed groups', async ({ page }) => {
  await open(page);
  await page.evaluate(async () => {
    await window.tagrex.invoke('open_drop', { paths: Array.from({ length: 1000 }, (_, i) => `track-${String(i).padStart(4, '0')}.mp3`) });
    document.getElementById('root').value = '/dropped';
    await window.tagrex.openLibrary();
  });
  await page.locator('#filter-panel-toggle').click();
  await page.locator('#filter').fill('track-00');
  await expect(page.locator('#status-sel')).toContainText('100/');
  const reads = await page.evaluate(() => {
    let reads = 0;
    for (const track of window.tagrex.tracks) {
      track.tags = new Proxy(track.tags, { ownKeys(target) { reads++; return Reflect.ownKeys(target); } });
    }
    window.perfFilterReads = () => reads;
    return reads;
  });
  expect(reads).toBe(0);
  await page.locator('#tracks-body tr[data-path]').nth(1).click();
  await expect(page.locator('#status-sel')).toContainText('100/');
  // A row click may read the selected DTO for its inspector, but must not
  // enumerate every file's tags again just to recount filter matches.
  expect(await page.evaluate(() => window.perfFilterReads())).toBeLessThan(20);
  await page.locator('#view-btn').click();
  await page.locator('#group-btn').click();
  await page.locator('#group-menu [data-group="album"]').click();
  await page.locator('#toggle-groups').click();
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(0);
  await expect(page.locator('#status-sel')).toContainText('100/');
});

test('scroll measures only entering rows after learning a uniform height', async ({ page }) => {
  await open(page);
  await page.evaluate(async () => {
    await window.tagrex.invoke('open_drop', { paths: Array.from({ length: 1000 }, (_, i) => `track-${String(i).padStart(4, '0')}.mp3`) });
    document.getElementById('root').value = '/dropped';
    await window.tagrex.openLibrary();
  });
  await page.evaluate(() => {
    window.perfRowReads = [];
    const descriptor = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetHeight');
    Object.defineProperty(HTMLElement.prototype, 'offsetHeight', {
      configurable: true,
      get() {
        if (this.matches?.('#tracks-body tr[data-path]')) window.perfRowReads.push(this.dataset.path);
        return descriptor.get.call(this);
      },
    });
  });
  for (const top of [60, 90, 120, 150]) {
    await page.locator('#files-view').evaluate((element, y) => { element.scrollTop = y; }, top);
    await page.waitForTimeout(30);
  }
  const reads = await page.evaluate(() => window.perfRowReads);
  expect(reads.length).toBeGreaterThan(0);
  expect(new Set(reads).size).toBe(reads.length);
  expect(await page.locator('#tracks-body tr[data-path]').count()).toBeLessThan(100);
});

test('Apply shows a truthful preparation message until write progress is available', async ({ page }) => {
  await open(page);
  await page.evaluate(() => {
    const original = Promise.resolve;
    Promise.resolve = function (value) {
      if (window.perfPauseApply && value?.id && value?.description && 'applied_at' in value) {
        return new Promise(resolve => setTimeout(() => resolve(value), 900));
      }
      return original.call(this, value);
    };
  });
  await page.locator('.subtab[data-subtab="editor"]').click();
  await page.locator('#fields-body input[data-key="title"]').fill('Prepared edit');
  await expect(page.locator('#diff-actionbar')).toBeVisible();
  await expect(page.locator('#diff-apply')).toBeEnabled();
  await page.evaluate(() => { window.perfPauseApply = true; });
  await page.locator('#diff-apply').click();
  await expect(page.locator('#ab-file')).toHaveText('Checking files…');
  await expect(page.locator('#ab-bar-fill')).toHaveCSS('width', '0px');
  await expect(page.locator('#diff-actionbar')).toBeHidden();
});

test('visible mask columns share one batch and an invalid pattern only blanks its own column', async ({ page }) => {
  await open(page, [
    { id: 'artist', name: 'Artist', pattern: '%artist%', align: 'left' },
    { id: 'broken', name: 'Invalid', pattern: '%unknown_mask_field%', align: 'left' },
    { id: 'album', name: 'Album', pattern: '%album%', align: 'left' },
  ]);
  // open() checked the initial single batch before resetting counters.
  await expect(customCell(page, 0, 0)).toHaveText('The X Factor');
  await expect(customCell(page, 0, 1)).toHaveText('');
  await expect(customCell(page, 0, 2)).toHaveText('La Bush');
  expect(await page.evaluate(() => window.perfCalls.render_column || 0)).toBe(0);
});

test('a delayed mask-column batch cannot replace values from a newer library', async ({ page }) => {
  await open(page, [{ id: 'name', name: 'Name', pattern: '%filename%', align: 'left' }]);
  await page.evaluate(() => {
    window.perfHeldBatches = [];
    const all = Promise.all;
    Promise.all = function (values) {
      const result = all.call(this, values);
      if (!window.perfColumnBatchDepth) return result;
      window.perfColumnBatchDepth = false;
      if (!window.perfHoldBatch) return result;
      return new Promise(resolve => result.then(value => window.perfHeldBatches.push(() => resolve(value))));
    };
    window.perfColumnBatchDepth = false;
    window.perfHoldBatch = true;
  });
  await page.evaluate(async () => {
    await window.tagrex.invoke('open_drop', { paths: ['Alpha.mp3'] });
    document.getElementById('root').value = '/dropped';
    await window.tagrex.openLibrary();
  });
  await expect.poll(() => page.evaluate(() => window.perfHeldBatches.length)).toBe(1);
  await page.evaluate(async () => {
    window.perfHoldBatch = false;
    await window.tagrex.invoke('open_drop', { paths: ['Beta.mp3'] });
    await window.tagrex.openLibrary();
  });
  await expect(customCell(page)).toHaveText('Beta');
  await page.evaluate(() => window.perfHeldBatches.shift()());
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  await expect(customCell(page)).toHaveText('Beta');
  expect(await page.evaluate(() => window.perfCalls.render_columns)).toBe(2);
});

test('mask builder keeps one active preview and only the latest queued expression', async ({ page }) => {
  await open(page);
  await page.locator('.subtab[data-subtab="filename"]').click();
  // The inspector now shares this backend batch command. Let its debounced
  // sample settle before instrumenting only the builder's request queue.
  await expect(page.locator('#from-name-probe .probe-tracks')).toBeVisible();
  await page.evaluate(() => {
    window.perfCalls.probe_tags_from_names = 0;
    window.perfHeldProbes = [];
    const resolve = Promise.resolve;
    Promise.resolve = function (value) {
      if (Array.isArray(value?.files) && Array.isArray(value?.fields)) {
        return new Promise(done => window.perfHeldProbes.push({ fields: value.fields, release: () => done(value) }));
      }
      return resolve.call(this, value);
    };
  });
  await page.locator('#from-name-builder-open').click();
  await expect.poll(() => page.evaluate(() => window.perfHeldProbes.length)).toBe(1);
  await page.locator('#mb-expr').fill('%artist% - %title%');
  await page.waitForTimeout(250);
  await page.locator('#mb-expr').fill('%title%');
  await page.waitForTimeout(250);
  expect(await page.evaluate(() => window.perfCalls.probe_tags_from_names)).toBe(1);
  await page.evaluate(() => window.perfHeldProbes[0].release());
  await expect.poll(() => page.evaluate(() => window.perfHeldProbes.length)).toBe(2);
  expect(await page.evaluate(() => window.perfHeldProbes[1].fields)).toEqual(['title']);
  await page.evaluate(() => window.perfHeldProbes[1].release());
  await expect(page.locator('#mb-list th')).toHaveCount(2);
  expect(await page.evaluate(() => window.perfCalls.probe_tags_from_names)).toBe(2);
});
