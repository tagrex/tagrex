import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';

const root = resolve('.test-dist');
const issues = new WeakMap();
const groups = [
  { name: 'Upper titles', scope: 'title', rules: [{ kind: 'case', style: 'upper', scope: 'title', from: '', to: '', regex: false }] },
  { name: 'Append title mark', scope: 'title', rules: [{ kind: 'replace', scope: 'title', from: '$', to: '!', regex: true, style: '' }] },
  { name: 'Upper filenames', scope: 'filename', rules: [{ kind: 'case', style: 'upper', scope: 'filename', from: '', to: '', regex: false }] },
];

test.beforeEach(async ({ page }) => {
  const errors = []; issues.set(page, errors);
  page.on('pageerror', error => errors.push(error.stack || error.message));
  await page.addInitScript(presets => {
    localStorage.setItem('tagrex.activePresetsByJob', JSON.stringify({ generator: [], online: [], fromname: [], renamer: [] }));
    window.transformCalls = {}; window.transformRequests = []; window.transformUndo = [];
    window.transformLocked = []; window.transformHeld = []; window.transformHold = false;
    const originalResolve = Promise.resolve;
    Promise.resolve = function (value) {
      // The fixture supplies real configurable presets and models native IPC
      // ownership. No native app or real library is ever accessed.
      if (value && typeof value === 'object' && 'proxy' in value && 'portable_names' in value) value = { ...value, action_groups: structuredClone(presets) };
      if (value?.description === 'Transform' && Array.isArray(value.changes)) {
        value = structuredClone(value);
        // Model the native backend's field-lock gate (the browser mock has no
        // transform lock gate). Keep its actual transform computation intact.
        const skipped = new Map();
        value.changes = value.changes.filter(change => {
          change.tag_changes = change.tag_changes.filter(field => {
            if (!window.transformLocked.includes(field.field)) return true;
            skipped.set(field.field, (skipped.get(field.field) || 0) + 1); return false;
          });
          return change.rename_to || change.tag_changes.length;
        });
        value.locked_skipped = [...skipped].map(([field, files]) => ({ field, files }));
        if (window.transformHold) return new Promise(resolve => window.transformHeld.push({ plan: value, resolve }));
      }
      return originalResolve.call(this, value);
    };
    window.releaseTransform = index => window.transformHeld[index].resolve(window.transformHeld[index].plan);
  }, groups);
  await page.route('http://tagrex.test/**', async route => {
    const path = resolve(root, '.' + new URL(route.request().url()).pathname);
    if (!path.startsWith(root + sep)) return route.abort();
    try {
      let body = await readFile(path);
      if (path.endsWith('.js')) {
        let source = body.toString();
        const state = source.match(/case([`"'])list_tracks\1:\s*return Promise\.resolve\(([\w$]+)\.tracks\)/)?.[2];
        const transform = source.match(/case([`"'])preview_live_edits\1:/);
        const args = transform && source.slice(transform.index, transform.index + 1200).match(/([\w$]+)\.paths/)?.[1];
        if (transform && (!state || !args)) throw new Error('Could not instrument transformation mock');
        if (transform) {
          source = source.replace(/case([`"'])(preview_live_edits|preview_transform_over_plan|preview_tag_edits|apply_plan|undo|set_locked_fields)\1:/g, (match, _quote, command) => {
            let trace = `window.transformCalls['${command}']=(window.transformCalls['${command}']||0)+1;`;
            if (command === 'preview_live_edits') trace += `window.transformRequests.push(structuredClone(${args}));`;
            if (command === 'apply_plan') trace += `window.transformUndo.push(structuredClone(${state}.tracks));`;
            if (command === 'undo') trace += `if(window.transformUndo.length)${state}.tracks=window.transformUndo.pop();`;
            if (command === 'set_locked_fields') trace += `window.transformLocked=[...${args}.fields];`;
            return match + trace;
          });
          source = source.replace(/(case([`"'])list_tracks\2:\s*return Promise\.resolve\()([\w$]+)\.tracks(\))/g, '$1structuredClone($3.tracks)$4');
          body = Buffer.from(source);
        }
      }
      await route.fulfill({ body, contentType: path.endsWith('.js') ? 'text/javascript' : path.endsWith('.css') ? 'text/css' : path.endsWith('.woff2') ? 'font/woff2' : 'text/html' });
    } catch (error) {
      if (!String(error).includes('ENOENT')) throw error;
      await route.fulfill({ status: 404, body: 'Missing fixture' });
    }
  });
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(() => page.evaluate(() => typeof window.tagrex?.openLibrary)).toBe('function');
  expect(await page.evaluate(() => !!window.__TAURI__)).toBe(false);
  await page.locator('#root-display').click(); await page.locator('#root').fill('/music'); await page.locator('#root').press('Enter');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
  await page.locator('.subtab[data-subtab="editor"]').click();
  await expect(page.locator('#groups-menu')).toContainText('Upper titles');
});
test.afterEach(async ({ page }) => expect(issues.get(page)).toEqual([]));
const calls = (page, command) => page.evaluate(command => window.transformCalls[command] || 0, command);
const plan = page => page.evaluate(() => structuredClone(window.tagrex.previewPlan));
async function tick(page, name, on = true) {
  const row = page.locator('#groups-menu .preset-check').filter({ has: page.locator('.preset-apply', { hasText: name }) });
  const input = row.locator('input.group-tick');
  if (await input.isChecked() !== on) await row.locator('.preset-apply').click();
  await expect(input).toBeChecked({ checked: on });
}
async function changed(page, count = 1) {
  await expect.poll(async () => (await plan(page))?.changes.length || 0).toBe(count);
  await expect(page.locator('#status-transform-scope')).toContainText(`${count} modified`);
  await expect(page.locator('#diff-apply')).toBeEnabled();
}

for (const mode of ['tagger', 'renamer', 'generator', 'deduplicator', 'exporter']) {
  test(`ticking a visible preset automatically previews the selection in ${mode}`, async ({ page }) => {
    await page.locator(`.mode-tab[data-mode="${mode}"]`).click();
    await tick(page, mode === 'renamer' ? 'Upper filenames' : 'Upper titles');
    await changed(page);
    await expect(page.locator('#files-view')).toBeVisible();
    await expect(page.locator('#diff-actionbar')).toBeVisible();
    await expect(page.locator('#transform-preview')).toBeHidden();
    if (mode === 'renamer') expect((await plan(page)).changes[0].rename_to).toContain('THE X FACTOR');
    else expect((await plan(page)).changes[0].tag_changes.find(change => change.field === 'title').new).toBe('DESERT RAIN');
    expect(await calls(page, 'preview_tag_edits')).toBe(0);
    expect(await calls(page, 'apply_plan')).toBe(0);
  });
}

test('the same active filename rules run after a chosen Online import', async ({ page }) => {
  await page.locator('.subtab[data-subtab="online"]').click();
  await tick(page, 'Upper filenames'); await changed(page);
  expect((await plan(page)).changes[0].rename_to).toContain('THE X FACTOR');
  await page.evaluate(() => { document.getElementById('discogs-token').value = 'fixture'; });
  await page.locator('#discogs-query').fill('La Bush'); await page.locator('#discogs-search').click();
  const card = page.locator('.release-card').first(); await card.locator('.release-head').click();
  await expect(card).toHaveClass(/tracklist-loaded/); await card.locator('.release-import').click();
  await expect.poll(() => calls(page, 'preview_live_edits')).toBeGreaterThan(0);
  await expect.poll(async () => (await plan(page))?.changes.some(change => change.tag_changes.length > 0)).toBe(true);
  await expect.poll(async()=> (await plan(page))?.changes.some(change=>!!change.rename_to)).toBe(true);
  expect(await calls(page, 'preview_transform_over_plan')).toBe(0);
  expect(await calls(page, 'apply_plan')).toBe(0);
});

test('automatic filename Apply and Undo follow the same selected file through renamed paths in Online', async ({ page }) => {
  const original = await page.evaluate(() => window.tagrex.selectedPaths()[0]);
  await page.locator('.subtab[data-subtab="online"]').click();
  await tick(page, 'Upper filenames'); await changed(page);
  const renamed = (await plan(page)).changes[0].rename_to;
  await page.locator('#diff-apply').click();
  await expect.poll(() => page.evaluate(() => window.tagrex.selectedPaths())).toEqual([renamed]);
  await expect.poll(() => plan(page)).toBe(null);
  await expect(page.locator('#tracks-body tr.selected')).toHaveAttribute('data-path', renamed);
  expect(await calls(page, 'apply_plan')).toBe(1);
  await page.locator('#undo').click();
  await expect.poll(() => page.evaluate(() => window.tagrex.selectedPaths())).toEqual([original]);
  await expect.poll(() => plan(page)).toBe(null);
  await expect(page.locator('#tracks-body tr.selected')).toHaveAttribute('data-path', original);
  expect(await calls(page, 'undo')).toBe(1);
  await expect(page.locator('#groups-menu .preset-check').filter({ hasText: 'Upper filenames' }).locator('input.group-tick')).toBeChecked();
});

test('toggling a non-idempotent preset is A/B and selecting another diff row retargets the preview', async ({ page }) => {
  await tick(page, 'Append title mark'); await changed(page);
  expect((await plan(page)).changes[0].tag_changes[0].new).toBe('Desert Rain!');
  await tick(page, 'Append title mark', false);
  await expect.poll(() => plan(page)).toBe(null);
  await expect(page.locator('#diff-actionbar')).toBeHidden();
  await tick(page, 'Append title mark'); await changed(page);
  expect((await plan(page)).changes[0].tag_changes[0].new).toBe('Desert Rain!');
  await page.locator('#tracks-body tr[data-path]').nth(1).locator('td.file').click();
  await expect.poll(async () => (await plan(page))?.changes[0]?.tag_changes[0]?.new).toBe('Radio!');
  expect((await plan(page)).changes[0].path).toContain('02 -');
  expect(await page.evaluate(() => window.tagrex.selectedPaths())).toEqual([(await plan(page)).changes[0].path]);
  expect(await page.evaluate(() => window.tagrex.tracks[0].tags.title)).toBe('Desert Rain');
  expect(await calls(page, 'apply_plan')).toBe(0);
});

test('Apply and Undo each use one batch and leave saved values visible with presets still checked', async ({ page }) => {
  await tick(page, 'Append title mark'); await changed(page);
  await page.locator('#diff-apply').click();
  await expect.poll(() => page.evaluate(() => window.tagrex.tracks[0].tags.title)).toBe('Desert Rain!');
  await expect.poll(() => plan(page)).toBe(null);
  await expect(page.locator('#diff-actionbar')).toBeHidden();
  expect(await calls(page, 'apply_plan')).toBe(1);
  await expect(page.locator('#undo')).toBeEnabled(); await page.locator('#undo').click();
  await expect.poll(() => page.evaluate(() => window.tagrex.tracks[0].tags.title)).toBe('Desert Rain');
  await expect.poll(() => plan(page)).toBe(null);
  await expect(page.locator('#diff-actionbar')).toBeHidden();
  expect(await calls(page, 'undo')).toBe(1);
  await expect(page.locator('#groups-menu .preset-check').filter({ hasText: 'Append title mark' }).locator('input.group-tick')).toBeChecked();
  await tick(page, 'Append title mark', false); await tick(page, 'Append title mark'); await changed(page);
  expect((await plan(page)).changes[0].tag_changes[0].new).toBe('Desert Rain!');
});

test('slow previews coalesce to the newest selection and never enable a stale Apply', async ({ page }) => {
  await page.evaluate(() => { window.transformHold = true; });
  await tick(page, 'Append title mark');
  await expect.poll(() => page.evaluate(() => window.transformHeld.length)).toBe(1);
  await page.locator('#tracks-body tr[data-path]').nth(1).locator('td.file').click();
  // Selection B queues behind the single active native job. Its obsolete A
  // response must never become an applicable plan while B is waiting.
  await page.waitForTimeout(100);
  expect(await page.evaluate(() => window.transformHeld.length)).toBe(1);
  await page.evaluate(() => window.releaseTransform(0));
  await expect.poll(() => page.evaluate(() => window.transformHeld.length)).toBe(2);
  expect(await plan(page)).toBeNull();
  await page.evaluate(() => window.releaseTransform(1)); await changed(page);
  const newest = await plan(page);
  expect(newest.changes[0].path).toContain('02 -');
  await page.locator('#tracks-body tr[data-path]').nth(2).locator('td.file').click();
  await expect(page.locator('#diff-apply')).toBeDisabled();
  await page.keyboard.press('Control+Enter');
  expect(await calls(page, 'apply_plan')).toBe(0);
  await expect.poll(() => page.evaluate(() => window.transformHeld.length)).toBe(3);
  await page.evaluate(() => window.releaseTransform(2)); await changed(page);
  expect((await plan(page)).changes[0].path).toContain('03 -');
});

test('manual values and active rules compose after late transform responses', async ({ page }) => {
  await page.evaluate(() => { window.transformHold = true; });
  await tick(page, 'Upper titles');
  await expect.poll(() => page.evaluate(() => window.transformHeld.length)).toBe(1);
  await page.locator('#fields-body input[data-key="artist"]').fill('Pending manual artist');
  await expect(page.locator('#diff-apply')).toBeDisabled();
  await page.evaluate(() => { window.transformHold = false; window.releaseTransform(0); });
  await expect(page.locator('#diff-apply')).toBeEnabled();
  expect((await plan(page)).changes[0].tag_changes.find(change => change.field === 'artist').new).toBe('Pending manual artist');
  expect((await plan(page)).changes[0].tag_changes.find(change => change.field === 'title').new).toBe('DESERT RAIN');
  await tick(page, 'Upper titles', false);
  await expect(page.locator('#diff-apply')).toBeEnabled();
  await expect.poll(async()=> (await plan(page)).changes[0].tag_changes.some(change=>change.field==='title')).toBe(false);
  expect(await calls(page, 'apply_plan')).toBe(0);
});

test('typing while rules are active retains raw input and previews transformations after it', async ({ page }) => {
  await tick(page, 'Append title mark'); await changed(page);
  const title = page.locator('#fields-body input[data-key="title"]');
  await title.fill('Keep my typing');
  await expect(title).toHaveValue('Keep my typing');
  await expect(title).toHaveClass(/dirty/);
  await expect.poll(async()=> (await plan(page))?.changes[0]?.tag_changes.find(field=>field.field==='title')?.new).toBe('Keep my typing!');
  await tick(page, 'Append title mark', false);
  await expect.poll(async()=> (await plan(page))?.changes[0]?.tag_changes.find(field=>field.field==='title')?.new).toBe('Keep my typing');
  await expect(title).toHaveValue('Keep my typing');
  expect(await calls(page, 'apply_plan')).toBe(0);
});

test('field locks are honored and changing a lock recomputes the checked preset', async ({ page }) => {
  const lock = page.locator('#fields-body .fe-row').filter({ has: page.locator('input[data-key="title"]') }).locator('.fe-lock');
  await lock.click(); await tick(page, 'Upper titles');
  await expect.poll(() => calls(page, 'preview_live_edits')).toBe(1);
  await expect.poll(() => page.evaluate(() => document.getElementById('status-transform-scope').getAttribute('aria-busy'))).toBe('false');
  expect(await plan(page)).toBe(null);
  expect(await page.evaluate(() => window.tagrex.tracks[0].tags.title)).toBe('Desert Rain');
  await lock.click(); await changed(page);
  expect((await plan(page)).changes[0].tag_changes[0].new).toBe('DESERT RAIN');
  expect(await calls(page, 'apply_plan')).toBe(0);
});

test('selection preview covers virtual rows while filters keep hidden selections explicit', async ({ page }) => {
  await page.evaluate(async () => {
    await window.tagrex.invoke('open_drop', { paths: Array.from({ length: 350 }, (_, i) => `track-${String(i).padStart(3, '0')}.mp3`) });
    document.getElementById('root').value = '/dropped'; await window.tagrex.openLibrary();
  });
  await tick(page, 'Append title mark'); await changed(page);
  await page.locator('#tracks-body tr[data-path]').first().focus(); await page.keyboard.press('Control+a');
  await changed(page, 350);
  expect(await page.locator('#tracks-body tr[data-path]').count()).toBeLessThan(350);
  await page.locator('#filter-panel-toggle').click();
  await page.locator('#filter').fill('track-000');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(1);
  await expect(page.locator('#status-transform-scope')).toContainText('349 selected hidden');
  expect((await plan(page)).changes).toHaveLength(350);
  expect(await calls(page, 'apply_plan')).toBe(0);
});

test('the compact375px layout keeps scope and explicit Apply reachable after selecting a transformation', async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 667 });
  await page.getByRole('tab', { name: 'Transformations', exact: true }).click();
  await tick(page, 'Upper titles'); await changed(page);
  await page.getByRole('tab', { name: 'Files', exact: true }).click();
  await expect(page.locator('#diff-apply')).toBeInViewport({ ratio: 1 });
  await expect(page.locator('#diff-apply')).toHaveAccessibleName('Apply to 1 file');
  await expect(page.locator('#status-transform-scope')).toHaveText('1 modified');
  await page.locator('#diff-apply').focus(); await expect(page.locator('#diff-apply')).toBeFocused();
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(375);
  expect(await calls(page, 'apply_plan')).toBe(0);
});
