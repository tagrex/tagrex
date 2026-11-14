import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';

// Parent builds once, then serves that artifact through the usual route fixture.
// TAGREX_SCOPE_DIST can point to either separately built baseline or candidate.
const root = resolve(process.env.TAGREX_SCOPE_DIST || '.test-dist');
test.beforeEach(async ({ page }) => {
  page.scopeErrors = [];
  page.on('pageerror', error => page.scopeErrors.push(error.message));
  await page.addInitScript(() => {
    localStorage.setItem('tagrex.groupBy', '');
    window.scopeWrites = 0;
    window.scopeEvents = 0;
    document.addEventListener('tagrex:transform-scope', () => window.scopeEvents++);
  });
  await page.route('http://tagrex.test/**', async route => {
    const path = resolve(root, '.' + new URL(route.request().url()).pathname);
    if (!path.startsWith(root + sep)) return route.abort();
    try {
      let body = await readFile(path);
      if (path.endsWith('.js')) {
        const source = body.toString();
        const instrumented = source.replace(/case([`"'])apply_plan\1:/g, match => `${match}window.scopeWrites++;`);
        body = Buffer.from(instrumented === source ? source : `window.scopeWriteCounterInstalled=true;\n${instrumented}`);
      }
      await route.fulfill({ body, contentType: path.endsWith('.js') ? 'text/javascript' : path.endsWith('.css') ? 'text/css' : path.endsWith('.woff2') ? 'font/woff2' : 'text/html' });
    } catch (error) {
      if (!String(error).includes('ENOENT')) throw error;
      await route.fulfill({ status: 404, body: 'Missing fixture' });
    }
  });
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(() => page.evaluate(() => typeof window.tagrex?.openLibrary)).toBe('function');
});
test.afterEach(async ({ page }) => expect(page.scopeErrors).toEqual([]));
async function open(page) {
  await page.locator('#root-display').click();
  await page.locator('#root').fill('/music');
  await page.locator('#root').press('Enter');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
  await page.locator('.subtab[data-subtab="editor"]').click();
  await expect.poll(() => page.evaluate(() => window.scopeWriteCounterInstalled)).toBe(true);
}

test('mutable inline/reference artwork and block validity stays fresh through scope events and explicit Apply', async ({ page }) => {
  await open(page);
  await page.locator('#fields-body [data-key="title"]').fill('Pending title');
  await expect(page.locator('#diff-apply')).toBeEnabled();
  for (const bad of [
    { invalid: true }, { tag_changes: [{ invalid: true }] },
    { cover_change: { old: [], new: [{ invalid: true, data_base64: 'inline' }] } },
    { cover_change: { old: [], new: [{ invalid: true, asset_id: 'reference' }] } },
    { block_changes: [{ new: { covers: [{ invalid: true }] } }] },
  ]) {
    await page.evaluate(bad => {
      const plan = window.tagrex.previewPlan;
      window.scopeOriginalChanges = plan.changes;
      plan.changes = [...plan.changes, { path: plan.changes[0].path, ...bad }];
      // The button is still enabled: exercise the independent fresh Apply guard.
      document.getElementById('diff-apply').click();
      document.dispatchEvent(new CustomEvent('tagrex:transform-scope'));
    }, bad);
    await expect(page.locator('#diff-apply')).toBeDisabled();
    await expect(page.locator('#status-transform-scope')).toHaveText('Fix invalid values');
    expect(await page.evaluate(() => window.scopeWrites)).toBe(0);
    await page.evaluate(() => {
      window.tagrex.previewPlan.changes = window.scopeOriginalChanges;
      document.dispatchEvent(new CustomEvent('tagrex:transform-scope'));
    });
    await expect(page.locator('#diff-apply')).toBeEnabled();
  }
  await page.locator('#diff-discard').click();
  await expect(page.locator('#diff-actionbar')).toBeHidden();
  expect(await page.evaluate(() => window.scopeWrites)).toBe(0);
});

test('selection keeps drafts and scope announcements without writes', async ({ page }) => {
  await open(page);
  const selected = await page.evaluate(() => window.tagrex.selectedPaths()[0]);
  await page.locator('#fields-body [data-key="title"]').fill('Retained draft');
  await expect(page.locator('#diff-apply')).toBeEnabled();
  await page.locator('#tracks-body tr[data-path]').nth(1).click();
  await expect.poll(() => page.evaluate(() => window.tagrex.selectedPaths()[0])).not.toBe(selected);
  await expect.poll(() => page.evaluate(path => window.tagrex.previewPlan?.changes.some(change => change.path === path && change.tag_changes.some(field => field.new === 'Retained draft')), selected)).toBe(true);
  expect(await page.evaluate(() => window.scopeWrites)).toBe(0);
});

for (const size of [20000, 50000, 100000]) test(`scope-bench ${size} select-all, announcement and row interaction`, async ({ page }, info) => {
  test.skip(!process.env.TAGREX_SCOPE_BENCH, 'Parent-controlled serial benchmark only');
  test.setTimeout(180000);
  await page.evaluate(async size => {
    await window.tagrex.invoke('open_drop', { paths: Array.from({ length: size }, (_, i) => `scope-${i}.mp3`) });
    document.getElementById('root').value = '/dropped';
    await window.tagrex.openLibrary();
  }, size);
  await expect.poll(() => page.evaluate(() => window.tagrex.tracks.length)).toBe(size);
  expect(await page.evaluate(() => window.scopeWriteCounterInstalled)).toBe(true);
  const samples = await page.evaluate(async () => {
    const paint = () => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const select = () => { for (const track of window.tagrex.tracks) window.tagrex.selection.add(track.path); window.tagrex.renderTracks(); };
    const samples = [];
    for (let iteration = 0; iteration < 18; iteration++) {
      select(); await paint();
      const eventsBefore = window.scopeEvents;
      const start = performance.now();
      for (let count = 0; count < 50; count++) document.dispatchEvent(new CustomEvent('tagrex:transform-scope'));
      const dispatchMs = (performance.now() - start) / 50;
      // Keep the dispatch batch's duration from shifting the click's RAF phase.
      await paint();
      const clickStart = performance.now();
      document.querySelector('#tracks-body tr[data-path] td[data-field="title"]').click();
      const clickMs = performance.now() - clickStart;
      await paint();
      const clickToPaintMs = performance.now() - clickStart;
      if (iteration >= 3) samples.push({ dispatchMs, clickMs, clickToPaintMs, events: window.scopeEvents - eventsBefore });
    }
    const note = document.getElementById('status-transform-scope');
    return { samples, note: { text: note.textContent, hidden: note.hidden, busy: note.getAttribute('aria-busy') }, selected: window.tagrex.selectedPaths(), writes: window.scopeWrites };
  });
  expect(samples.writes).toBe(0);
  await info.attach(`scope-${size}.json`, { body: JSON.stringify({ source: process.env.TAGREX_SCOPE_LABEL || 'unlabelled', size, ...samples }, null, 2), contentType: 'application/json' });
});
