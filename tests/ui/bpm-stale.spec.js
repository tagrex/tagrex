import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';

const fixture = resolve('.test-dist');

test.beforeEach(async ({ page }) => {
  page.__bpmErrors = [];
  page.on('pageerror', error => page.__bpmErrors.push(error.message));
  await page.addInitScript(() => {
    localStorage.setItem('tagrex.activePresetsByJob', JSON.stringify({ generator: [], online: [], fromname: [], renamer: [] }));
    window.__bpm = { pending: [], calls: 0, cancels: 0, applies: 0, slowOpen: false, opens: [] };
  });
  await page.route('http://tagrex.test/**', async route => {
    const path = resolve(fixture, '.' + new URL(route.request().url()).pathname);
    if (!path.startsWith(fixture + sep)) return route.abort();
    try {
      let body = await readFile(path);
      if (path.endsWith('.js')) {
        body = body.toString();
        const detection = body.match(/case([`"'])detect_bpm\1:/);
        if (detection) {
          const arg = body.slice(detection.index, detection.index + 1100).match(/([\w$]+)\.paths/)?.[1];
          if (!arg) throw Error('Cannot find BPM mock arguments');
          body = body.replace(detection[0], `${detection[0]}window.__bpm.calls++;return new Promise(resolve=>window.__bpm.pending.push({resolve,paths:[...${arg}.paths]}));`);
        }
        body = body.replace(/case([`"'])cancel_detect_bpm\1:/, '$&window.__bpm.cancels++;');
        body = body.replace(/case([`"'])apply_plan\1:/, '$&window.__bpm.applies++;');
        body = body.replace(/case([`"'])open_library\1:/, '$&if(window.__bpm.slowOpen)return new Promise(resolve=>window.__bpm.opens.push(resolve));');
      }
      await route.fulfill({ body, contentType: path.endsWith('.js') ? 'text/javascript' : path.endsWith('.css') ? 'text/css' : path.endsWith('.woff2') ? 'font/woff2' : 'text/html' });
    } catch (error) {
      if (!String(error).includes('ENOENT')) throw error;
      await route.fulfill({ status: 404, body: 'Missing fixture' });
    }
  });
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(() => page.evaluate(() => typeof window.tagrex?.openLibrary)).toBe('function');
  await open(page, '/music');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
});

test.afterEach(async ({ page }) => {
  expect(page.__bpmErrors).toEqual([]);
  expect(await page.evaluate(() => window.__bpm.applies)).toBe(0);
});

async function open(page, root) {
  if (await page.locator('#root').isHidden()) await page.locator('#root-display').click();
  await page.locator('#root').fill(root);
  await page.locator('#root').press('Enter');
}
async function startScan(page) {
  await page.getByRole('button', { name: 'More', exact: true }).click();
  await page.locator('#detect-bpm').click();
  await expect.poll(() => page.evaluate(() => window.__bpm.pending.length)).toBe(1);
}
async function finishScan(page) {
  await page.evaluate(() => {
    const pending = window.__bpm.pending.shift();
    pending.resolve({ plan: { description: 'Detect BPM', changes: pending.paths.map(path => ({ path, rename_to: null, tag_changes: [{ field: 'bpm', old: null, new: '127' }], cover_change: null })), notes: [], locked_skipped: [] }, analysed: pending.paths.length, no_tempo: 0, too_long: 0, failed: [], cancelled: false });
  });
  await expect(page.locator('#detect-bpm')).not.toHaveClass(/busy/);
}

test('unchanged scan stages BPM through the normal preview without writing', async ({ page }) => {
  await startScan(page);
  await finishScan(page);
  await expect.poll(() => page.evaluate(() => window.tagrex.previewPlan?.description)).toBe('Detect BPM');
});

test('late BPM results cannot replace a newly opened library preview', async ({ page }) => {
  await startScan(page);
  await open(page, '/other-library');
  await expect.poll(() => page.evaluate(() => window.__bpm.cancels)).toBe(1);
  await finishScan(page);
  expect(await page.evaluate(() => window.tagrex.previewPlan)).toBeNull();
});

test('starting a folder open invalidates BPM before the new library arrives', async ({ page }) => {
  await startScan(page);
  await page.evaluate(() => window.__bpm.slowOpen = true);
  await open(page, '/slow-library');
  await expect.poll(() => page.evaluate(() => window.__bpm.opens.length)).toBe(1);
  await finishScan(page);
  expect(await page.evaluate(() => window.tagrex.previewPlan)).toBeNull();
});

test('late BPM results preserve a title drafted while the scan was running', async ({ page }) => {
  await startScan(page);
  await page.locator('.subtab[data-subtab="editor"]').click();
  await page.locator('#fields-body [data-key="title"]').fill('Keep this newer draft');
  await expect(page.locator('#diff-apply')).toBeEnabled();
  await finishScan(page);
  await expect(page.locator('#fields-body [data-key="title"]')).toHaveValue('Keep this newer draft');
  const changes = await page.evaluate(() => window.tagrex.previewPlan?.changes || []);
  expect(changes.some(change => change.tag_changes.some(field => field.field === 'title' && field.new === 'Keep this newer draft'))).toBe(true);
  expect(changes.some(change => change.tag_changes.some(field => field.field === 'bpm'))).toBe(false);
});

test('late BPM results preserve a different preview created during detection', async ({ page }) => {
  await startScan(page);
  await page.locator('.mode-tab[data-mode="renamer"]').click();
  await page.locator('#mask').fill('Renamed - %title%');
  await page.locator('#preview').click();
  await expect.poll(() => page.evaluate(() => window.tagrex.previewPlan?.changes.some(change => change.rename_to?.includes('Renamed')))).toBe(true);
  const newer = await page.evaluate(() => window.tagrex.previewPlan);
  await finishScan(page);
  expect(await page.evaluate(() => window.tagrex.previewPlan)).toEqual(newer);
});

test('the desktop footer keeps BPM Stop reachable alongside staged edits', async ({ page }, testInfo) => {
  await page.setViewportSize({width:1024,height:800});
  await page.locator('#tracks-body tr[data-path]').first().click();
  await page.locator('.subtab[data-subtab="editor"]').click();
  await page.locator('#editor-field-title').fill('Keep this staged title');
  await expect(page.locator('#diff-apply')).toBeEnabled();
  await expect(page.locator('#staged-edits')).toBeVisible();
  await startScan(page);
  const stop=page.locator('#busy-stop');
  await expect(stop).toBeVisible();
  await expect(stop).toBeInViewport({ratio:1});
  expect(await stop.evaluate(node=>{const b=node.getBoundingClientRect();const hit=document.elementFromPoint(b.x+b.width/2,b.y+b.height/2);return node===hit||node.contains(hit);})).toBe(true);
  await page.screenshot({path:testInfo.outputPath("bpm-busy-footer.png")});
  await stop.click();
  await expect.poll(()=>page.evaluate(()=>window.__bpm.cancels)).toBe(1);
  await finishScan(page);
  await expect(page.locator('#editor-field-title')).toHaveValue('Keep this staged title');
});
