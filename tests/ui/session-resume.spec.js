import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';

const fixture = resolve('.test-dist');
const sessionKey = 'tagrex.session.v1';
const attemptKey = 'tagrex.session.resume-attempt.v1';
const base = { version: 1, root: '/music', mode: 'tagger', subtab: 'editor', filter: '', groupBy: 'folder', selection: ['/music/02 - the x factor - radio.mp3'] };

async function start(page, { saved = null, attempt = null, launchPaths = [], slow = false, fastTimeout = false } = {}) {
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.addInitScript(({ saved, attempt, launchPaths, slow, fastTimeout }) => {
    // AddInitScript seeds only the first load, leaving reload to test real persistence.
    if (!sessionStorage.getItem('resume-test-seeded')) {
      if (saved !== null) localStorage.setItem('tagrex.session.v1', typeof saved === 'string' ? saved : JSON.stringify(saved));
      if (attempt !== null) localStorage.setItem('tagrex.session.resume-attempt.v1', JSON.stringify(attempt));
      sessionStorage.setItem('resume-test-seeded', '1');
    }
    window.__resume = { opens: [], applies: 0, launchPaths, slow, pending: [] };
    if (fastTimeout) {
      const original = window.setTimeout;
      window.setTimeout = (callback, delay, ...args) => original(callback, delay === 10000 ? 30 : delay, ...args);
    }
  }, { saved, attempt, launchPaths, slow, fastTimeout });
  await page.route('http://tagrex.test/**', async route => {
    const path = resolve(fixture, '.' + new URL(route.request().url()).pathname);
    if (!path.startsWith(fixture + sep)) return route.abort();
    try {
      let body = await readFile(path);
      if (path.endsWith('.js')) {
        body = body.toString();
        body = body.replace(/case([`"'])take_launch_paths\1:\s*return Promise\.resolve\(\[\]\)/, 'case"take_launch_paths":return Promise.resolve(window.__resume.launchPaths.splice(0))');
        const hit = body.match(/case([`"'])open_library\1:/);
        if (hit) {
          const arg = body.slice(hit.index, hit.index + 250).match(/([\w$]+)\.root/)?.[1];
          if (!arg) throw Error('Cannot find mocked open arguments');
          body = body.replace(hit[0], `${hit[0]}window.__resume.opens.push(${arg}.root);if(window.__resume.slow)return new Promise(resolve=>window.__resume.pending.push(resolve));`);
        }
        body = body.replace(/case([`"'])apply_plan\1:/, '$&window.__resume.applies++;');
      }
      await route.fulfill({ body, contentType: path.endsWith('.js') ? 'text/javascript' : path.endsWith('.css') ? 'text/css' : path.endsWith('.woff2') ? 'font/woff2' : 'text/html' });
    } catch (error) {
      if (!String(error).includes('ENOENT')) throw error;
      await route.fulfill({ status: 404, body: 'Missing fixture' });
    }
  });
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(() => page.evaluate(() => typeof window.tagrex?.openLibrary)).toBe('function');
  return errors;
}
async function open(page, path) {
  if (await page.locator('#root').isHidden()) await page.locator('#root-display').click();
  await page.locator('#root').fill(path);
  await page.locator('#root').press('Enter');
}

test('successful folder open persists view and resumes without restoring an unapplied draft', async ({ page }) => {
  const errors = await start(page);
  await open(page, '/music');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
  await page.locator('#tracks-body tr[data-path] .file').nth(1).click();
  const selected = await page.evaluate(() => window.tagrex.selectedPaths());
  await page.locator('.subtab[data-subtab="editor"]').click();
  await page.locator('#fields-body input[data-key="title"]').fill('DO NOT RESTORE THIS DRAFT');
  await page.locator('#filter-panel-toggle').click();
  await page.locator('#filter').fill('Radio');
  await expect.poll(() => page.evaluate(() => JSON.parse(localStorage.getItem('tagrex.session.v1') || '{}').filter)).toBe('Radio');
  await page.reload();
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(1);
  await expect(page.locator('#subtab-editor')).toBeVisible();
  await expect(page.locator('#filter')).toHaveValue('Radio');
  expect(await page.evaluate(() => window.tagrex.selectedPaths())).toEqual(selected);
  await expect(page.locator('#fields-body input[data-key="title"]')).toHaveValue('Radio');
  expect(await page.evaluate(() => window.tagrex.previewPlan)).toBeNull();
  expect(await page.evaluate(() => window.__resume.applies)).toBe(0);
  expect(errors).toEqual([]);
});

test('missing folder leaves explicit retry and preserves recents; next startup does not loop', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('tagrex.recentRoots', JSON.stringify(['/does-not-exist'])));
  const errors = await start(page, { saved: { ...base, root: '/does-not-exist' } });
  await expect(page.locator('#root')).toHaveValue('/does-not-exist');
  await expect(page.locator('#root')).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.__resume.opens.length)).toBe(1);
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('tagrex.recentRoots')))).toContain('/does-not-exist');
  await page.reload();
  await expect(page.locator('#root')).toBeVisible();
  expect(await page.evaluate(() => window.__resume.opens)).toEqual([]);
  await open(page, '/music');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
  await expect.poll(() => page.evaluate(() => localStorage.getItem('tagrex.session.resume-attempt.v1'))).toBeNull();
  expect(errors).toEqual([]);
});

test('explicit launch paths take precedence over saved folder', async ({ page }) => {
  const errors = await start(page, { saved: base, launchPaths: ['/priority'] });
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(4);
  expect(await page.evaluate(() => window.__resume.opens)).toEqual([]);
  await expect(page.locator('#root-display')).toContainText('priority');
  expect(errors).toEqual([]);
});

test('manual open supersedes late startup restore without stale rows or labels', async ({ page }) => {
  const errors = await start(page, { saved: base, slow: true });
  await expect.poll(() => page.evaluate(() => window.__resume.pending.length)).toBe(1);
  await page.evaluate(() => window.__resume.slow = false);
  await open(page, '/manual');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
  await expect(page.locator('#root-display')).toContainText('manual');
  await page.evaluate(() => window.__resume.pending[0]());
  await page.waitForTimeout(80);
  await expect(page.locator('#root-display')).toContainText('manual');
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('tagrex.session.v1')).root)).toBe('/manual');
  expect(errors).toEqual([]);
});

test('timed-out read cannot publish tracks and retry remains available', async ({ page }) => {
  const errors = await start(page, { saved: base, slow: true, fastTimeout: true });
  await expect(page.locator('#root')).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.__resume.pending.length)).toBe(1);
  await page.evaluate(() => window.__resume.pending[0]());
  await page.waitForTimeout(80);
  expect(await page.evaluate(() => window.tagrex.tracks)).toHaveLength(0);
  expect(await page.evaluate(() => window.tagrex.selectedPaths())).toEqual([]);
  expect(await page.evaluate(() => window.tagrex.previewPlan)).toBeNull();
  await page.evaluate(() => window.__resume.slow = false);
  await open(page, '/music');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
  expect(errors).toEqual([]);
});

test('corrupt saved data is ignored; removed selected files never select replacement files', async ({ page }) => {
  const errors = await start(page, { saved: '{broken' });
  expect(await page.evaluate(() => window.__resume.opens)).toEqual([]);
  await page.evaluate(({ sessionKey, attemptKey, base }) => {
    localStorage.setItem(sessionKey, JSON.stringify({ ...base, selection: ['/music/gone.mp3'] }));
    localStorage.removeItem(attemptKey);
  }, { sessionKey, attemptKey, base });
  await page.reload();
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
  expect(await page.evaluate(() => window.tagrex.selectedPaths())).toEqual([]);
  expect(errors).toEqual([]);
});
