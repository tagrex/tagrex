import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';

// The preview player's polled mirror, measured as DOM work rather than time:
// the poll runs every 300 ms and most of what it used to write was already on
// screen. The assertions are exact counts of childList mutations on #pl-time and
// #pl-toggle, taken while the backend state is deliberately frozen (paused), so
// a slow or throttled machine changes how long the test waits, never the result.
//
// `player_status` calls are counted by the same route-rewrite the other player
// spec uses, which is what proves the polls actually happened before the count
// of zero mutations is read.
const root = resolve('.test-dist');
const errors = new WeakMap();

test.beforeEach(async ({ page }) => {
  const issues = [];
  errors.set(page, issues);
  page.on('pageerror', (error) => issues.push(error.message));
  await page.addInitScript(() => {
    window.playerCalls = {};
    window.__plMutations = { time: 0, toggle: 0 };
    const nativeObserver = window.MutationObserver;
    window.__watch = (id, key, options) => new nativeObserver((records) => {
      window.__plMutations[key] += records.filter((record) => record.type === 'childList').length;
    }).observe(document.getElementById(id), options);
  });
  await page.route('http://tagrex.test/**', async (route) => {
    const path = resolve(root, '.' + new URL(route.request().url()).pathname);
    if (!path.startsWith(root + sep)) return route.abort();
    try {
      let body = await readFile(path);
      if (path.endsWith('.js')) {
        body = Buffer.from(body.toString().replace(/case([`"'])(player_status|player_seek)\1:/g,
          (match, _quote, command) => `${match}window.playerCalls['${command}']=(window.playerCalls['${command}']||0)+1;`));
      }
      await route.fulfill({ body, contentType: path.endsWith('.js') ? 'text/javascript' : path.endsWith('.css') ? 'text/css' : path.endsWith('.woff2') ? 'font/woff2' : 'text/html' });
    } catch { await route.fulfill({ status: 404, body: 'Missing fixture' }); }
  });
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(() => page.evaluate(() => typeof window.tagrex?.setMode)).toBe('function');
});

test.afterEach(async ({ page }) => expect(errors.get(page)).toEqual([]));

async function library(page) {
  await page.locator('#root-display').click();
  await page.locator('#root').fill('/music');
  await page.locator('#root').press('Enter');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
  await expect(page.locator('#pl-toggle')).toBeEnabled();
}

const polls = (page) => page.evaluate(() => window.playerCalls.player_status || 0);
const mutations = (page) => page.evaluate(() => ({ ...window.__plMutations }));
const icon = (page) => page.locator('#pl-toggle use').getAttribute('href');

async function watch(page) {
  await page.evaluate(() => {
    window.__watch('pl-time', 'time', { childList: true });
    window.__watch('pl-toggle', 'toggle', { childList: true, subtree: true });
  });
}

async function pollsAtLeast(page, extra) {
  const start = await polls(page);
  await expect.poll(() => polls(page), { timeout: 30000 }).toBeGreaterThanOrEqual(start + extra);
}

test('a paused poll writes nothing to the clock or the transport glyph', async ({ page }) => {
  await library(page);
  await watch(page);
  await page.locator('#pl-toggle').click();
  await expect(page.locator('#player')).not.toHaveClass(/idle/);
  await page.locator('#pl-toggle').click();
  // Let the poll that sees the pause land, then start counting from a settled
  // state: the paused position is frozen in the mock backend.
  await pollsAtLeast(page, 1);
  await expect.poll(() => icon(page)).toBe('#i-play');
  await page.evaluate(() => { window.__plMutations.time = 0; window.__plMutations.toggle = 0; });
  await pollsAtLeast(page, 4);
  expect(await mutations(page)).toEqual({ time: 0, toggle: 0 });
  // The labels the UI promises are still there — nothing was skipped.
  await expect(page.locator('#pl-time')).toContainText('/ 10:00');
  await expect(page.locator('#pl-seek')).toHaveAttribute('aria-valuetext', /\/ 10:00$/);
});

test('a real state change updates the glyph once, and only then', async ({ page }) => {
  await library(page);
  await watch(page);
  await page.locator('#pl-toggle').click();
  await expect.poll(() => icon(page)).toBe('#i-pause');
  await page.locator('#pl-toggle').click();
  await pollsAtLeast(page, 1);
  await page.evaluate(() => { window.__plMutations.toggle = 0; });
  await pollsAtLeast(page, 1);
  await page.locator('#pl-toggle').click();
  await expect.poll(() => icon(page)).toBe('#i-pause');
  await pollsAtLeast(page, 2);
  expect((await mutations(page)).toggle).toBe(1);
  await pollsAtLeast(page, 3);
  expect((await mutations(page)).toggle).toBe(1);
});

test('a poll landing mid-drag cannot pull the clock back to the backend position', async ({ page }) => {
  await library(page);
  await page.locator('#pl-toggle').click();
  await expect(page.locator('#player')).not.toHaveClass(/idle/);
  await page.locator('#pl-toggle').click();
  await pollsAtLeast(page, 1);
  // Drag to 5:00 without releasing: `input` only.
  await page.locator('#pl-seek').evaluate((input) => {
    input.value = '500';
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await expect(page.locator('#pl-time')).toHaveText('5:00 / 10:00');
  await expect(page.locator('#pl-seek')).toHaveAttribute('aria-valuetext', '5:00 / 10:00');
  expect(await page.evaluate(() => window.playerCalls.player_seek || 0)).toBe(0);
  // Several polls pass while the drag is held: both readouts keep the target.
  await pollsAtLeast(page, 3);
  await expect(page.locator('#pl-time')).toHaveText('5:00 / 10:00');
  await expect(page.locator('#pl-seek')).toHaveAttribute('aria-valuetext', '5:00 / 10:00');
  // Releasing commits the seek, and the bar follows the backend again: one
  // string for the clock and the slider, at the position just sought.
  await page.locator('#pl-seek').evaluate((input) => input.dispatchEvent(new Event('change', { bubbles: true })));
  await expect.poll(() => page.evaluate(() => window.playerCalls.player_seek || 0)).toBe(1);
  await pollsAtLeast(page, 1);
  await expect(page.locator('#pl-seek')).toHaveAttribute('aria-valuetext', /^5:0\d \/ 10:00$/);
  expect(await page.locator('#pl-time').textContent())
    .toBe(await page.locator('#pl-seek').getAttribute('aria-valuetext'));
  // The clock keeps advancing from there: caching never freezes the label.
  const settled = await page.locator('#pl-time').textContent();
  await expect(page.locator('#pl-time')).not.toHaveText(settled, { timeout: 8000 });
});

test('focus stays on the seek slider while polls update it', async ({ page }) => {
  await library(page);
  await page.locator('#pl-toggle').click();
  await expect(page.locator('#player')).not.toHaveClass(/idle/);
  await page.locator('#pl-seek').focus();
  await pollsAtLeast(page, 4);
  await expect(page.locator('#pl-seek')).toBeFocused();
});

for (const ending of ['pointerup', 'pointercancel', 'blur', 'window-blur', 'keyup']) test(`a seek ending with ${ending} and no change resumes backend clock updates`, async ({ page }) => {
  await library(page);
  await page.locator('#pl-toggle').click();
  await page.locator('#pl-toggle').click();
  await pollsAtLeast(page, 1);
  const backendClock = await page.locator('#pl-time').textContent();
  await page.locator('#pl-seek').focus();
  await page.locator('#pl-seek').evaluate(input => {
    input.value = '500'; input.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await expect(page.locator('#pl-time')).toHaveText('5:00 / 10:00');
  if (ending === 'blur') await page.locator('#pl-toggle').focus();
  else if (ending === 'window-blur') await page.evaluate(() => window.dispatchEvent(new Event('blur')));
  else if (ending === 'keyup') await page.locator('#pl-seek').dispatchEvent('keyup', { key: 'ArrowRight' });
  else if (ending === 'pointerup') await page.locator('body').dispatchEvent('pointerup');
  else await page.locator('#pl-seek').dispatchEvent('pointercancel');
  await pollsAtLeast(page, 2);
  await expect(page.locator('#pl-time')).toHaveText(backendClock);
  await expect(page.locator('#pl-seek')).toHaveAttribute('aria-valuetext', backendClock);
  expect(await page.evaluate(() => window.playerCalls.player_seek || 0)).toBe(0);
});
