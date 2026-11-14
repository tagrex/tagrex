import { test, expect } from '@playwright/test';
import { readFile, mkdir } from 'node:fs/promises';
import { resolve, sep } from 'node:path';

const root = resolve('.test-dist');
const pageErrors = new WeakMap();
test.use({ viewport: { width: 1440, height: 900 }, reducedMotion: 'reduce' });

test.beforeEach(async ({ page }) => {
  const errors = [];
  pageErrors.set(page, errors);
  page.on('pageerror', (error) => errors.push(error.message));
  await page.route('http://tagrex.test/**', async (route) => {
    const path = resolve(root, '.' + new URL(route.request().url()).pathname);
    if (!path.startsWith(root + sep)) return route.abort();
    try {
      const body = await readFile(path);
      const contentType = path.endsWith('.js') ? 'text/javascript' : path.endsWith('.css') ? 'text/css' : path.endsWith('.woff2') ? 'font/woff2' : 'text/html';
      await route.fulfill({ body, contentType });
    } catch {
      await route.fulfill({ status: 404, body: 'Missing fixture' });
    }
  });
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(() => page.evaluate(() => typeof window.tagrex?.openLibrary)).toBe('function');
  expect(await page.evaluate(() => !!window.__TAURI__)).toBe(false);
  // Read-only design iteration against an existing fixture can inject just the
  // owned stylesheet. Normal suite runs require its real application import.
  if (process.env.INSPECTOR_CSS_INJECT === '1') {
    await page.addStyleTag({ content: await readFile('app/ui/inspector.css', 'utf8') });
    await page.addStyleTag({ content: await readFile('app/ui/form-controls.css', 'utf8') });
  }
  await page.locator('#root-display').click();
  await page.locator('#root').fill('/music');
  await page.locator('#root').press('Enter');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
});

test.afterEach(async ({ page }) => {
  expect(pageErrors.get(page)).toEqual([]);
});

async function sizeInspector(page, width) {
  const splitter = page.getByRole('separator', { name: 'Resize workspace columns', exact: true });
  if (await splitter.count()) {
    const area = await page.locator('.dock-middle').boundingBox();
    const handle = await splitter.boundingBox();
    await page.mouse.move(handle.x + handle.width / 2, handle.y + handle.height / 2);
    await page.mouse.down();
    await page.mouse.move(area.x + area.width - width - handle.width / 2, handle.y + handle.height / 2, { steps: 4 });
    await page.mouse.up();
  } else {
    await page.evaluate((value) => { document.querySelector('.mode-col').style.flexBasis = `${value}px`; }, width);
  }
}

async function search(page) {
  await page.locator('.subtab[data-subtab="online"]').click();
  await page.evaluate(() => { document.getElementById('discogs-token').value = 'fixture'; });
  await page.locator('#discogs-query').fill('Helter Skelter');
  await page.locator('#discogs-search').click();
  await expect(page.locator('.release-card')).toHaveCount(5);
  await expect(page.locator('.release-card').first().locator('.tk-count')).toContainText('3 tracks');
}

for (const width of [360, 480]) {
  test(`the ${width}px inspector aligns metadata, fits number pairs and keeps its action footer reachable`, async ({ page }) => {
    await sizeInspector(page, width);
    await page.locator('.subtab[data-subtab="editor"]').click();
    const artist = page.locator('#fields-body input[data-key="artist"]');
    const title = page.locator('#fields-body input[data-key="title"]');
    const album = page.locator('#fields-body input[data-key="album"]');
    await artist.scrollIntoViewIfNeeded(); await expect(artist).toBeInViewport({ ratio: 1 });
    await title.scrollIntoViewIfNeeded(); await expect(title).toBeInViewport({ ratio: 1 });
    await album.scrollIntoViewIfNeeded(); await expect(album).toBeInViewport({ ratio: 1 });
    const fields = await Promise.all([artist, title, album].map((input) => input.boundingBox()));
    expect(Math.max(...fields.map((box) => box.x)) - Math.min(...fields.map((box) => box.x))).toBeLessThan(1);
    expect(fields.every((box) => box.width >= 150 && box.height === 32)).toBe(true);
    expect(await artist.evaluate(input => parseFloat(getComputedStyle(input).borderRadius))).toBe(10);
    for (const row of await page.locator('#fields-body .fe-row.duo').all()) {
      // Number labels use the same left rail as the other metadata fields.
      for (const unit of await row.locator('.fe-duo-unit').all()) expect((await unit.boundingBox()).height).toBeLessThanOrEqual(68);
      const label = row.locator('.fe-duo-label').first();
      expect((await label.boundingBox()).x + (await label.boundingBox()).width).toBeLessThan((await row.locator('input').first().boundingBox()).x);
    }
    const panel = await page.locator('.mode-col').boundingBox();
    for (const input of await page.locator('#fields-body .fe-row.duo input').all()) {
      await input.scrollIntoViewIfNeeded();
      const box = await input.boundingBox();
      expect(box.x).toBeGreaterThanOrEqual(panel.x);
      expect(box.x + box.width).toBeLessThanOrEqual(panel.x + panel.width + 1);
    }
    await expect(page.locator('#staged-edits')).toBeHidden();
    await artist.fill('Edited artist');
    await expect(page.locator('#diff-apply')).toBeEnabled();
    await expect(page.locator('#diff-actionbar')).toBeVisible();
    expect(await page.evaluate(() => window.tagrex.tracks[0].tags.artist)).toBe('The X Factor');
    await page.locator('#diff-discard').click();
    expect(await page.locator('.mode-col').evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true);
  });
}

test('release entries stay transparent when hovered or expanded, with plain useful metadata', async ({ page }) => {
  await sizeInspector(page, 360);
  await search(page);
  const card = page.locator('.release-card').first();
  await expect(card.locator('.tk-count')).toContainText('2 discs');
  await expect(card.locator('.rb-catno')).toHaveText('TOTH 001');
  const summary = await card.locator('.release-info').evaluate((node) => {
    const title = node.querySelector('.release-title');
    const artist = node.querySelector('.release-artist');
    const facts = node.querySelector('.release-facts');
    return { nodes: [...node.children].map((child) => child.className), title: Number(getComputedStyle(title).fontWeight), artist: Number(getComputedStyle(artist).fontWeight), facts: Number(getComputedStyle(facts).fontWeight) };
  });
  expect(summary.nodes.slice(0, 3)).toEqual(['release-title', 'release-artist', 'release-facts']);
  expect(summary.title).toBeGreaterThan(summary.artist);
  expect(summary.title).toBeGreaterThan(summary.facts);
  for (const state of ['rest', 'hover', 'expanded']) {
    if (state === 'hover') await card.locator('.release-head').hover();
    if (state === 'expanded') {
      await card.locator('.release-head').click();
      await expect(card).toHaveAttribute('aria-expanded', 'true');
    }
    const styles = await card.evaluate((node) => {
      const style = getComputedStyle(node), divider = getComputedStyle(node, '::after');
      return { background: style.backgroundColor, border: style.borderTopWidth, radius: style.borderRadius, shadow: style.boxShadow, divider: parseFloat(divider.width) / node.getBoundingClientRect().width };
    });
    expect(styles.background).toBe('rgba(0, 0, 0, 0)');
    expect(styles.border).toBe('0px');
    expect(styles.radius).toBe('0px');
    expect(styles.shadow).toBe('none');
    expect(styles.divider).toBeGreaterThan(0.3);
    expect(styles.divider).toBeLessThan(0.5);
    for (const text of await card.locator('.rb-catno, .tk-count').all()) {
      const chrome = await text.evaluate((node) => { const style = getComputedStyle(node); return [style.borderLeftWidth, style.borderTopWidth, style.backgroundColor, style.boxShadow]; });
      expect(chrome).toEqual(['0px', '0px', 'rgba(0, 0, 0, 0)', 'none']);
    }
  }
  await expect(card.locator('.release-import')).toBeVisible();
  await expect(card.locator('.release-cover')).toBeInViewport({ ratio: 1 });
  expect(await card.locator('.release-head a, .release-head button').count()).toBe(0);
});

test('missing catalogue placeholders disappear while counts and complete edition details remain available', async ({ page }) => {
  await page.evaluate(() => {
    const originalResolve = Promise.resolve;
    Promise.resolve = function (value) {
      if (Array.isArray(value) && value[0]?.catalog_number && value[0]?.thumb_url !== undefined) {
        value[0].catalog_number = 'none';
        value[0].format = 'Vinyl, LP, Limited Edition, Mixed, Unofficial Release';
      }
      return originalResolve.call(this, value);
    };
  });
  await search(page);
  const card = page.locator('.release-card').first();
  await expect(card.locator('.rb-catno')).toHaveCount(0);
  await expect(card.locator('.tk-count')).toContainText('3 tracks');
  await expect(card.locator('.tk-count')).toContainText('2 discs');
  await expect(card.locator('.release-meta')).toContainText('Vinyl');
  await expect(card.locator('.release-meta')).not.toContainText('Limited Edition');
  await expect(card.locator('.release-meta')).toHaveAttribute('aria-description', /Limited Edition, Mixed, Unofficial Release/);
  await expect(card.locator('.release-meta')).not.toHaveAttribute('title');
});


for (const theme of ['light', 'dark']) {
  test(`online source, media and query fields share normal input metrics in ${theme} theme`, async ({ page }) => {
    await page.evaluate(value => { localStorage.setItem('tagrex.theme', value); document.documentElement.dataset.theme = value; }, theme);
    const typography = ['fontFamily', 'fontSize', 'fontWeight', 'lineHeight', 'color'];
    const frame = ['height', 'borderTopWidth', 'borderTopColor', 'backgroundColor', 'borderRadius'];
    const padding = ['paddingTop', 'paddingBottom', 'paddingLeft', 'paddingRight'];
    const styles = (selector, properties) => page.locator(selector).evaluate((node, keys) => Object.fromEntries(keys.map(key => [key, getComputedStyle(node)[key]])), properties);
    const ink = await page.evaluate(() => getComputedStyle(document.body).color);
    await expect.poll(async () => (await styles('#online-source-control', typography)).color).toBe(ink);
    const online = await Promise.all(['#online-source-control', '#search-format-control'].map(async selector => ({
      text:await styles(selector, typography), frame:await styles(selector, frame), padding:await styles(selector, padding),
    })));
    const query = {text:await styles('#discogs-query', typography), frame:await styles('#query-box', frame), padding:await styles('#discogs-query', padding)};
    await page.locator('.subtab[data-subtab="editor"]').click();
    const editor = {text:await styles('#fields-body input[data-key="title"]', typography), frame:await styles('#fields-body input[data-key="title"]', frame), padding:await styles('#fields-body input[data-key="title"]', padding)};
    expect(editor.frame.height).toBe('32px');
    expect(editor.frame.borderRadius).toBe('10px');
    for (const field of [...online, query]) {
      expect(field.text).toEqual(editor.text);
      expect(field.frame).toEqual(editor.frame);
      expect(field.padding).toEqual(editor.padding);
    }
    await page.locator('#settings-open').click();
    await page.locator('#settings-search').fill('proxy');
    expect(await styles('#set-proxy', typography)).toEqual(editor.text);
    const settingsFill = await page.locator('.settings-sheet').evaluate(node => getComputedStyle(node).backgroundColor);
    expect(await styles('#set-proxy', frame)).toEqual({ ...editor.frame, borderTopColor: 'rgba(0, 0, 0, 0)', backgroundColor: settingsFill });
    expect(await styles('#set-proxy', padding)).toEqual(editor.padding);
  });
}

test('editor labels keep a fixed rail while inputs resize, with compact metadata rows', async ({ page }) => {
  await page.locator('.subtab[data-subtab="editor"]').click();
  const artist = page.locator('#fields-body input[data-key="artist"]');
  const disc = page.locator('#fields-body input[data-key="disc"]');
  const track = page.locator('#fields-body input[data-key="track"]');
  await artist.fill('Keep this draft');
  await artist.evaluate(node => { window.retainedEditorField = node; });
  const metrics = [];
  for (const width of [320, 440, 580]) {
    await sizeInspector(page, width);
    metrics.push(await artist.evaluate(node => {
      const row = node.closest('.fe-row'), label = row.querySelector('.fe-label');
      return { rail:label.getBoundingClientRect().width, input:node.getBoundingClientRect().width, offset:node.getBoundingClientRect().left-row.getBoundingClientRect().left };
    }));
    await expect(artist).toHaveValue('Keep this draft');
    expect(await artist.evaluate(node => node === window.retainedEditorField)).toBe(true);
    for (const row of await page.locator('#fields-body .fe-row').all()) {
      expect(await row.evaluate(node => node.scrollWidth <= node.clientWidth + 1)).toBe(true);
    }
  }
  expect(Math.max(...metrics.map(value => value.rail))-Math.min(...metrics.map(value => value.rail))).toBeLessThan(1);
  expect(Math.max(...metrics.map(value => value.offset))-Math.min(...metrics.map(value => value.offset))).toBeLessThan(1);
  expect(metrics[2].input-metrics[0].input).toBeGreaterThan(100);
  expect(Math.abs((await disc.boundingBox()).y-(await track.boundingBox()).y)).toBeLessThan(1);
  await page.locator('label[for="editor-field-artist"]').click();
  await expect(artist).toBeFocused();
  for (const field of [track, disc, artist]) expect(await field.evaluate(node => getComputedStyle(node).borderRadius)).toBe('10px');
  if (process.env.ITERATION_CAPTURE_EVIDENCE === '1') {
    await mkdir('docs/ui-iteration', { recursive:true });
    await page.screenshot({path:'docs/ui-iteration/editor-compact-fields.png'});
  }
});


test('Core keeps long-form comments and compact BPM/Key editable without losing drafts on resize', async ({ page }) => {
  await page.locator('.subtab[data-subtab="editor"]').click();
  await sizeInspector(page, 400);
  const core = page.locator('#editor-group-core');
  const comment = core.locator('textarea[data-key="comment"]');
  await expect(comment).toBeVisible();
  const value = 'First line\nSecond line\nThird line';
  await comment.fill(value);
  expect(await comment.evaluate(node => getComputedStyle(node).resize)).toBe('vertical');
  await comment.evaluate(node => node.style.height='140px');
  await sizeInspector(page, 360);
  await expect(comment).toHaveValue(value);
  expect((await comment.boundingBox()).height).toBe(140);
  const bpm = core.locator('input[data-key="bpm"]'), key = core.locator('input[data-key="key"]');
  // Small docks can stack compact fields so their values remain readable.
  expect((await bpm.boundingBox()).width).toBeGreaterThanOrEqual(48);
  expect((await key.boundingBox()).width).toBeGreaterThanOrEqual(48);
  await bpm.fill('145'); await key.fill('Am');
  await expect(page.locator('#diff-apply')).toBeEnabled();
  await expect(page.locator('#diff-actionbar')).toBeVisible();
  const changes = await page.evaluate(() => window.tagrex.previewPlan.changes);
  expect(JSON.stringify(changes)).toContain('First line');
  expect(JSON.stringify(changes)).toContain('145');
  expect(JSON.stringify(changes)).toContain('Am');
});
