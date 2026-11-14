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
      const contentType = path.endsWith('.js') ? 'text/javascript' : path.endsWith('.css') ? 'text/css' : path.endsWith('.woff2') ? 'font/woff2' : 'text/html';
      await route.fulfill({ body: await readFile(path), contentType });
    } catch {
      await route.fulfill({ status: 404, body: 'Missing fixture' });
    }
  });
});

test.afterEach(async ({ page }) => {
  expect(errors.get(page)).toEqual([]);
});

async function openApp(page) {
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(() => page.evaluate(() => typeof window.tagrex?.setMode)).toBe('function');
}

async function openLibrary(page) {
  await page.locator('#root-display').click();
  await page.locator('#root').fill('/music');
  await page.locator('#root').press('Enter');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
}

async function openSettings(page) {
  await page.locator('#settings-open').click();
  await expect(page.getByRole('dialog', { name: 'Settings', exact: true })).toBeVisible();
}

test('every workspace mode and tagging subtab remains usable after mounting', async ({ page }) => {
  await openApp(page);
  for (const mode of ['renamer', 'generator', 'deduplicator', 'exporter', 'tagger']) {
    const tab = page.locator(`.mode-tab[data-mode="${mode}"]`);
    await tab.focus();
    await tab.press('Enter');
    await expect(page.locator(`#panel-${mode}`)).toBeVisible();
    await expect(page.locator('.mode-panel:visible')).toHaveCount(1);
    await expect(page.locator('#duplicates-view')).toBeVisible({ visible: mode === 'deduplicator' });
  }
  for (const subtab of ['editor', 'filename', 'fromtext', 'online']) {
    await page.locator(`.subtab[data-subtab="${subtab}"]`).click();
    await expect(page.locator(`#subtab-${subtab}`)).toBeVisible();
    await expect(page.locator('.subtab-panel:visible')).toHaveCount(1);
  }
});

test('opening and filtering a library preserves track selection and restores the list', async ({ page }) => {
  await openApp(page);
  await openLibrary(page);
  await expect(page.locator('#tracks-body tr.selected')).toHaveCount(1);
  await expect(page.locator('#root')).toBeHidden();
  await expect(page.locator('#root-display')).toContainText('music');
  await page.locator('#filter-panel-toggle').click();
  await page.locator('#filter').fill('Wish Mountain');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(1);
  await expect(page.locator('#tracks-body')).toContainText('Wish Mountain');
  await page.locator('#filter-clear').click();
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
  await expect(page.locator('#tracks-body tr.selected')).toHaveCount(1);
});

test('mask builder stages a rename before Apply changes the mock library', async ({ page }) => {
  await openApp(page);
  await openLibrary(page);
  await page.locator('.mode-tab[data-mode="renamer"]').click();
  await page.locator('#mask-builder-open').click();
  await expect(page.getByRole('dialog', { name: 'Mask builder', exact: true })).toBeVisible();
  await page.locator('#mb-expr').fill('%artist% — %title%');
  await expect(page.locator('#mb-list')).toContainText('The X Factor — Desert Rain.mp3');
  await expect(page.locator('#mb-ok')).toBeEnabled();
  await page.locator('#mb-ok').click();
  await expect(page.locator('#mask-builder')).toBeHidden();
  await expect(page.locator('#diff-actionbar')).toBeVisible();
  await expect(page.locator('#ab-count')).toHaveText('1');
  await expect(page.locator('#diff-apply')).toBeEnabled();
  expect(await page.evaluate(() => window.tagrex.tracks[0].path)).toBe('/music/01 - the x factor - desert rain.mp3');
  await page.locator('#diff-apply').click();
  await expect(page.locator('#diff-actionbar')).toBeHidden();
  await expect.poll(() => page.evaluate(() => window.tagrex.tracks.some((track) => track.path === '/music/The X Factor — Desert Rain.mp3'))).toBe(true);
});

test('settings search, native fields, Save and Cancel retain their existing behavior', async ({ page }) => {
  await openApp(page);
  await openSettings(page);
  await page.locator('#settings-search').fill('proxy');
  await expect(page.locator('#set-proxy')).toBeVisible();
  await page.locator('#set-proxy').fill('http://localhost:8888');
  await page.locator('#settings-save').click();
  await expect(page.locator('#settings')).toBeHidden();
  await openSettings(page);
  await page.locator('#settings-search').fill('proxy');
  await expect(page.locator('#set-proxy')).toHaveValue('http://localhost:8888');
  await page.locator('#set-proxy').fill('http://discard-me:9999');
  await page.locator('#settings-cancel').click();
  await openSettings(page);
  await page.locator('#settings-search').fill('proxy');
  await expect(page.locator('#set-proxy')).toHaveValue('http://localhost:8888');
  await page.locator('#settings-search').fill('nothing matches this setting');
  await expect(page.locator('#settings-search-empty')).toBeVisible();
  await page.locator('#settings-cancel').click();
});

test('generated tag fields stage edits without changing files until Apply', async ({ page }) => {
  await openApp(page);
  await openLibrary(page);
  await page.locator('.subtab[data-subtab="editor"]').click();
  const title = page.locator('#fields-body input[data-key="title"]');
  await expect(title).toHaveValue('Desert Rain');
  await title.fill('Restored Desert Rain');
  await expect(page.locator('#diff-apply')).toBeEnabled();
  await expect(page.locator('#diff-actionbar')).toBeVisible();
  await expect(page.locator('#tracks-body')).toContainText('Restored Desert Rain');
  expect(await page.evaluate(() => window.tagrex.tracks[0].tags.title)).toBe('Desert Rain');
  await page.locator('#diff-discard').click();
  await expect(page.locator('#diff-actionbar')).toBeHidden();
  await expect(page.locator('#tracks-body')).toContainText('Desert Rain');
  await expect(page.locator('#tracks-body')).not.toContainText('Restored Desert Rain');
});

test('file context menu opens a cancellable confirmation without removing the selection', async ({ page }) => {
  await openApp(page);
  await openLibrary(page);
  await page.locator('#tracks-body tr[data-path] .file').first().click({ button: 'right' });
  await expect(page.locator('#file-ctx')).toBeVisible();
  await page.locator('#file-ctx [data-cmd="trash"]').click();
  await expect(page.locator('#file-ctx')).toBeHidden();
  await expect(page.locator('#confirm-modal')).toBeVisible();
  await expect(page.locator('#confirm-cancel')).toBeFocused();
  await page.locator('#confirm-cancel').press('Enter');
  await expect(page.locator('#confirm-modal')).toBeHidden();
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
  await expect(page.locator('#tracks-body tr.selected')).toHaveCount(1);
  await openSettings(page);
  await page.locator('#settings-cancel').click();
});

test('placeholder picker inserts a token and remains reusable after dismissal', async ({ page }) => {
  await openApp(page);
  await page.locator('.mode-tab[data-mode="renamer"]').click();
  await page.locator('#mask').fill('');
  await page.locator('[data-ph-target="mask"]').click();
  await expect(page.locator('#ph-menu')).toBeVisible();
  await page.locator('#ph-filter').fill('artist');
  await page.locator('#ph-menu [data-token="%artist%"]').click();
  await expect(page.locator('#mask')).toHaveValue('%artist%');
  await expect(page.locator('#ph-menu')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('#ph-menu')).toBeHidden();
  await page.locator('[data-ph-target="mask"]').click();
  await expect(page.locator('#ph-menu')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('#ph-menu')).toBeHidden();
});

test('live appearance and select settings persist through the component boundary', async ({ page }) => {
  await openApp(page);
  await openSettings(page);
  await page.locator('#set-theme [data-theme-mode="dark"]').click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await page.locator('#settings-search').fill('Read Latin-1');
  await expect(page.locator('#set-code-page-control')).toBeVisible();
  await page.locator('#set-code-page-control').click();
  await page.getByRole('option', { name: 'KOI8-R (Russian)', exact: true }).click();
  await page.locator('#settings-save').click();
  await openSettings(page);
  await page.locator('#settings-search').fill('Read Latin-1');
  await expect(page.locator('#set-code-page')).toHaveValue('koi8-r');
  await page.locator('#settings-cancel').click();
  await page.reload();
  await expect.poll(() => page.evaluate(() => typeof window.tagrex?.setMode)).toBe('function');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
});

test('provider selection, search and artwork choice remain connected to a release', async ({ page }) => {
  await openApp(page);
  await page.locator('#online-source').selectOption('musicbrainz');
  await page.locator('#discogs-query').fill('La Bush');
  await page.locator('#discogs-search').click();
  await expect(page.locator('.release-card')).not.toHaveCount(0);
  await expect(page.locator('.release-card').first()).toContainText('La Bush');
  await page.locator('#online-source').selectOption('discogs');
  // Token belongs to Settings; use its real input without opening a native browser.
  await page.evaluate(() => { document.getElementById('discogs-token').value = 'fixture'; });
  await page.locator('#discogs-query').fill('Helter Skelter');
  await page.locator('#discogs-search').click();
  await page.locator('.release-card').first().locator('.release-cover').click();
  const picker = page.getByRole('dialog', { name: 'Choose artwork', exact: true });
  await expect(picker).toBeVisible();
  await picker.getByRole('button', { name: 'Image 2 of', exact: false }).click();
  await picker.getByRole('button', { name: 'Use', exact: true }).click();
  await expect(picker).toHaveCount(0);
  await page.locator('.release-card').first().locator('.release-cover').click();
  await expect(picker.getByRole('button', { name: 'Image 2 of', exact: false })).toHaveAttribute('aria-pressed', 'true');
  await page.keyboard.press('Escape');
});

test('narrow mask builder keeps its editing field and confirmation controls on screen', async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 667 });
  await openApp(page);
  await openLibrary(page);
  await page.locator('.mode-tab[data-mode="renamer"]').click();
  await page.locator('#mask-builder-open').click();
  const builder = page.getByRole('dialog', { name: 'Mask builder', exact: true });
  await expect(builder).toBeVisible();
  const bounds = await builder.boundingBox();
  expect(bounds.x).toBeGreaterThanOrEqual(0);
  expect(bounds.x + bounds.width).toBeLessThanOrEqual(375);
  await expect(page.locator('#mb-close')).toBeInViewport({ ratio: 1 });
  await expect(page.locator('#mb-cancel')).toBeInViewport({ ratio: 1 });
  await expect(page.locator('#mb-ok')).toBeInViewport({ ratio: 1 });
  await page.locator('#mb-expr').scrollIntoViewIfNeeded();
  await expect(page.locator('#mb-expr')).toBeInViewport({ ratio: 1 });
  await page.locator('#mb-expr').fill('%artist% — %title%');
  await page.locator('#mb-ok').click();
  await expect(builder).toBeHidden();
  await expect(page.locator('#diff-actionbar')).toBeVisible();
});

test('narrow preset editor provides usable fields and separate rule actions', async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 667 });
  await openApp(page);
  await openLibrary(page);
  await page.locator('.mode-tab[data-mode="generator"]').click();
  await page.locator('#gen-job-transform').click();
  await page.locator('#presets-edit').click();
  await expect(page.locator('#pe-editor')).toBeVisible();
  await page.locator('#pe-new').click();
  await page.locator('#pe-name').fill('Mobile case change');
  await page.locator('#pe-name').press('Enter');
  await page.locator('#pe-empty-add').click();
  await page.locator('#pe-rule-picker [data-cmd="case"]').click();
  for (const field of [page.locator('#pe-sample-input'), page.locator('.is-expanded .pe-case-style').locator('..')]) {
    await field.scrollIntoViewIfNeeded();
    await expect(field).toBeInViewport({ ratio: 1 });
    expect((await field.boundingBox()).width).toBeGreaterThanOrEqual(120);
  }
  await page.locator('#pe-open-add').scrollIntoViewIfNeeded();
  await expect(page.locator('#pe-open-add')).toBeInViewport({ ratio: 1 });
  await page.locator('#pe-editor-close').click();
  await expect(page.locator('#pe-editor')).toBeHidden();
});

for (const [width, height] of [[375, 667], [844, 390], [1280, 800]]) {
  test(`workspace and settings stay within the viewport at ${width}×${height}`, async ({ page }) => {
    await page.setViewportSize({ width, height });
    await openApp(page);
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
    for (const selector of ['.topbar', '.workarea']) {
      const bounds = await page.locator(selector).boundingBox();
      expect(bounds.x).toBeGreaterThanOrEqual(0);
      expect(bounds.x + bounds.width).toBeLessThanOrEqual(width + 1);
    }
    await openSettings(page);
    const settings = page.getByRole('dialog', { name: 'Settings', exact: true });
    const bounds = await settings.boundingBox();
    expect(bounds.x).toBeGreaterThanOrEqual(0);
    expect(bounds.x + bounds.width).toBeLessThanOrEqual(width + 1);
    expect(bounds.y + bounds.height).toBeLessThanOrEqual(height + 1);
    await page.locator('#settings-search').fill('proxy');
    await expect(page.locator('#set-proxy')).toBeInViewport();
    await page.locator('#settings-cancel').click();
    await expect(settings).toBeHidden();
  });
}

test('native artwork wheel routing is registered before any popup opens', async ({ page }) => {
  await page.addInitScript(() => {
    window.artworkWheelRegistrations = [];
    const add = Document.prototype.addEventListener;
    Document.prototype.addEventListener = function(type, listener, options) {
      if (type === 'wheel' && options?.capture && options?.passive === false) {
        window.artworkWheelRegistrations.push({ popupOpen: !!this.querySelector('[data-artwork-ui]') });
      }
      return add.call(this, type, listener, options);
    };
  });
  await openApp(page);
  expect(await page.evaluate(() => window.artworkWheelRegistrations)).toContainEqual({ popupOpen: false });
});

test('a controller import failure explains recovery and Reload starts the app', async ({ page }) => {
  const rejectController = route => route.abort('failed');
  await page.route('**/assets/app-*.js', rejectController);
  await page.goto('http://tagrex.test/app/ui/index.html');
  const notice = page.getByRole('alert');
  await expect(notice).toContainText('TagRex couldn’t start', { timeout: 3000 });
  await expect(notice.getByRole('button', { name: 'Reload' })).toBeFocused();
  await expect(page.locator('#app')).toHaveAttribute('inert', '');
  await page.unroute('**/assets/app-*.js', rejectController);
  await notice.getByRole('button', { name: 'Reload' }).click();
  await expect.poll(() => page.evaluate(() => typeof window.tagrex?.setMode)).toBe('function');
  await expect(page.getByRole('alert')).toHaveCount(0);
});

test('a closing dialog does not swallow the next Escape while its exit is visible', async ({ page }) => {
  await openApp(page);
  await openLibrary(page);
  await page.locator('.mode-tab[data-mode="renamer"]').click();
  await page.locator('#mask-builder-open').click();
  await page.locator('#mb-expr').fill('%artist% — %title%');
  await page.locator('#mb-ok').click();
  await expect(page.locator('#diff-actionbar')).toBeVisible();
  await page.locator('#mask-builder-open').click();
  const result = await page.evaluate(() => {
    const modal = document.getElementById('mask-builder');
    document.getElementById('mb-cancel').click();
    document.getElementById('diff-discard').focus();
    const physicallyPresent = !modal.hasAttribute('hidden');
    document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    return { logicallyClosed: modal.hidden, physicallyPresent, discarded: window.tagrex.previewPlan === null };
  });
  expect(result).toEqual({ logicallyClosed: true, physicallyPresent: true, discarded: true });
});
