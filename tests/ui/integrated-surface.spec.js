import { test, expect } from '@playwright/test';
import { readFile, mkdir } from 'node:fs/promises';
import { resolve, sep } from 'node:path';

const root = resolve('.test-dist');
const evidence = resolve('docs/ui-integration');
const pageErrors = new WeakMap();
test.use({ viewport: { width: 1440, height: 900 }, reducedMotion: 'reduce' });

test.beforeEach(async ({ page }) => {
  const errors = []; pageErrors.set(page, errors);
  page.on('pageerror', error => errors.push(error.message));
  await page.addInitScript(() => { window.integrationCalls = []; });
  await page.route('http://tagrex.test/**', async route => {
    const path = resolve(root, '.' + new URL(route.request().url()).pathname);
    if (!path.startsWith(root + sep)) return route.abort();
    try {
      let body = await readFile(path);
      if (path.endsWith('.js')) body = body.toString().replace(/case([`"'])(read_cover_summary|read_external_cover|preview_transform_groups|preview_tag_edits|apply_plan|render_columns|render_column|read_track_rows|list_tracks|get_settings|save_settings)\1:/g,
        (match, _quote, command) => `${match}window.integrationCalls.push('${command}');`);
      await route.fulfill({ body, contentType: path.endsWith('.js') ? 'text/javascript' : path.endsWith('.css') ? 'text/css' : path.endsWith('.woff2') ? 'font/woff2' : 'text/html' });
    } catch (error) {
      if (path.endsWith('.js') && !String(error).includes('ENOENT')) throw error;
      await route.fulfill({ status: 404, body: 'Missing fixture' });
    }
  });
});
test.afterEach(async ({ page }) => expect(pageErrors.get(page)).toEqual([]));

async function open(page, theme = 'dark') {
  await page.addInitScript(value => localStorage.setItem('tagrex.theme', value), theme);
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(() => page.evaluate(() => typeof window.tagrex?.openLibrary)).toBe('function');
  await page.locator('#root-display').click();
  await page.locator('#root').fill('/music');
  await page.locator('#root').press('Enter');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
}
const transforms = page => page.locator('#dock-module-transformations');
const toggle = page => transforms(page).getByRole('button', { name: 'Transformations', exact: true });
async function editor(page) {
  const compact = page.getByRole('tablist', { name: 'Workspace module' });
  if (await compact.isVisible()) await page.getByRole('tab', { name: 'Inspector', exact: true }).click();
  await page.locator('.subtab[data-subtab="editor"]').click();
}
async function screenshot(page, file) {
  await mkdir(evidence, { recursive: true });
  await page.screenshot({ path: resolve(evidence, file), fullPage: true, animations: 'disabled' });
}

// These initial assertions were run against checkpoint719c96c's exact172-test
// fixture build before the implementation build lease was released.
test('Transformations header collapses to its header and gives Inspector the reclaimed height', async ({ page }) => {
  await open(page);
  if (process.env.INTEGRATION_CAPTURE_BASELINE === '1') await screenshot(page, 'baseline-dark-wide.png');
  await expect(toggle(page)).toHaveAttribute('aria-expanded', 'true', { timeout: 1200 });
  await expect(toggle(page)).toHaveAttribute('aria-labelledby', 'dock-title-transformations', { timeout: 1200 });
  await expect(page.locator('#dock-title-transformations')).toHaveText('Transformations', { useInnerText: true });
  const top = await transforms(page).boundingBox();
  const below = await page.locator('#dock-module-inspector').boundingBox();
  await toggle(page).click();
  await expect(toggle(page)).toHaveAttribute('aria-expanded', 'false');
  await expect(transforms(page)).toHaveAttribute('data-dock-collapsed', 'true');
  await expect(page.locator('#dock-body-transformations')).toHaveAttribute('inert', '');
  await expect.poll(async () => (await transforms(page).boundingBox()).height).toBeLessThan(46);
  expect((await page.locator('#dock-module-inspector').boundingBox()).height).toBeGreaterThan(below.height + top.height - 50);
  await toggle(page).focus();
  await page.keyboard.press('Enter');
  await expect(toggle(page)).toHaveAttribute('aria-expanded', 'true');
  await expect.poll(async () => Math.abs((await transforms(page).boundingBox()).height - top.height)).toBeLessThan(2);
  await page.keyboard.press('Space');
  await expect(toggle(page)).toHaveAttribute('aria-expanded', 'false');
  await expect(toggle(page)).toBeFocused();
});

test('everyday fields stay open while More tags stays an accessible collapsible', async ({ page }) => {
  await open(page);
  await editor(page);
  await expect(page.locator('#fields-body .fe-group[data-group="core"] > button.fe-group-head')).toHaveCount(0);
  await expect(page.locator('#subtab-editor [data-i18n="editor.tagFields"], #subtab-editor [data-i18n="editor.fieldsShort"]')).toHaveCount(0);
  await expect(page.locator('#fields-add-toggle, #fields-add-row, #fields-new-name, #fields-new-value, #fields-add, #fields-add-cancel, #fields-known')).toHaveCount(0);
  await expect(page.getByRole('button', { name: /^Add tags$/i })).toHaveCount(0);
  for (const kind of ['standard']) {
    const head = page.locator(`.fe-group[data-group="${kind}"] > button.fe-group-head`);
    await expect(head).toHaveAttribute('aria-controls', `editor-group-${kind}`);
    const labelOffset = await head.evaluate((node, name) => {
      const walker = document.createTreeWalker(node, NodeFilter.SHOW_TEXT);
      let text; while ((text = walker.nextNode())) if (text.textContent.trim().toLowerCase() === name) break;
      if (!text) return Infinity;
      const range = document.createRange(); range.selectNodeContents(text);
      return range.getBoundingClientRect().left - node.getBoundingClientRect().left;
    }, kind === 'standard' ? 'more tags' : kind);
    expect(labelOffset).toBeLessThan(32);
    if (await head.getAttribute('aria-expanded') === 'false') await head.click();
    await head.focus();
    await page.keyboard.press('Space');
    await expect(head).toHaveAttribute('aria-expanded', 'false');
    await expect(page.locator(`#editor-group-${kind}`)).toHaveAttribute('inert', '');
    await page.keyboard.press('Tab');
    expect(await page.evaluate(id => document.activeElement?.closest(`#editor-group-${id}`) === null, kind)).toBe(true);
    await head.focus();
    await page.keyboard.press('Enter');
    await expect(head).toHaveAttribute('aria-expanded', 'true');
  }
});

test('dark table well stays distinct while sidebar and chrome share a quiet continuous surface', async ({ page }) => {
  await open(page);
  const surfaces = await page.evaluate(() => {
    const rgb = value => { const parts = value.match(/[\d.]+/g)?.map(Number) || []; const scale = value.startsWith('color(srgb ') ? 255 : 1; return { rgb: parts.slice(0, 3).map(channel => channel * scale), alpha: parts[3] ?? 1 }; };
    const surface = selector => {
      let node = document.querySelector(selector), color = [0, 0, 0];
      const ancestors = []; while (node) { ancestors.unshift(node); node = node.parentElement; }
      for (const node of ancestors) { const layer = rgb(getComputedStyle(node).backgroundColor); color = color.map((c, i) => layer.rgb[i] * layer.alpha + c * (1 - layer.alpha)); }
      return color;
    };
    return { table: surface('#table-wrap'), sidebar: surface('#dock-module-inspector'), toolbar: surface('.topbar'), frame: surface('.workarea'), seam: getComputedStyle(document.querySelector('.dock-column-splitter')).backgroundColor };
  });
  const luminance = color => color.reduce((total, value, i) => total + value * [0.2126, 0.7152, 0.0722][i], 0);
  expect(luminance(surfaces.sidebar) - luminance(surfaces.table)).toBeGreaterThan(2);
  expect(Math.abs(luminance(surfaces.sidebar) - luminance(surfaces.toolbar))).toBeLessThan(3);
  expect(Math.abs(luminance(surfaces.sidebar) - luminance(surfaces.frame))).toBeLessThan(3);
  const seam = await page.locator('.dock-column-splitter').evaluate(node => ({ background: getComputedStyle(node).backgroundColor, before: getComputedStyle(node, '::before').backgroundColor, after: getComputedStyle(node, '::after').backgroundColor }));
  const opaque = value => !/rgba?\([^)]*,\s*0\s*\)/.test(value) && value !== 'transparent';
  expect(opaque(seam.background)).toBe(false);
});

test('folding Transformations retains the exact checkbox, draft and manual plan without IPC', async ({ page }) => {
  await open(page);
  await editor(page);
  const title = page.locator('#fields-body input[data-key="title"]');
  await title.fill('Pending title');
  await page.locator('#groups-menu .preset-apply').first().click();
  await expect(page.locator('#groups-menu input.group-tick').first()).toBeChecked();
  await expect(page.locator('#diff-apply')).toBeEnabled();
  await expect(page.locator('#diff-actionbar')).toBeVisible();
  await title.fill('KEEP UNSTAGED DRAFT');
  await page.waitForTimeout(200);
  await page.evaluate(() => {
    window.integrationCalls = [];
    window.integrationOriginal = { plan: window.tagrex.previewPlan, title: document.querySelector('#fields-body input[data-key="title"]'), checkbox: document.querySelector('#groups-menu input.group-tick') };
  });
  for (let i = 0; i < 3; i++) { await toggle(page).click(); await toggle(page).click(); }
  await expect(title).toHaveValue('KEEP UNSTAGED DRAFT');
  await expect(page.locator('#groups-menu input.group-tick').first()).toBeChecked();
  expect(await page.evaluate(() => ({
    title: window.integrationOriginal.title === document.querySelector('#fields-body input[data-key="title"]'),
    checkbox: window.integrationOriginal.checkbox === document.querySelector('#groups-menu input.group-tick'),
    plan: window.integrationOriginal.plan === window.tagrex.previewPlan,
    calls: window.integrationCalls,
  }))).toEqual({ title: true, checkbox: true, plan: true, calls: [] });
  const titleBox = await toggle(page).boundingBox();
  const help = transforms(page).locator('.transformation-help');
  const helpBox = await help.boundingBox();
  const editBox = await page.locator('#presets-edit').boundingBox();
  expect(helpBox.x).toBeGreaterThanOrEqual(titleBox.x + titleBox.width - 1);
  expect(helpBox.x - titleBox.x - titleBox.width).toBeLessThan(12);
  expect(helpBox.x).toBeLessThan(editBox.x);
  // The empty header area is also a collapse target; Help/Edit stay separate.
  const blankX = (helpBox.x + helpBox.width + editBox.x) / 2;
  await page.mouse.click(blankX, helpBox.y + helpBox.height / 2);
  await expect(toggle(page)).toHaveAttribute('aria-expanded', 'false');
  await page.mouse.click(blankX, helpBox.y + helpBox.height / 2);
  await expect(toggle(page)).toHaveAttribute('aria-expanded', 'true');
  expect(await page.evaluate(() => window.integrationCalls)).toEqual([]);
  await help.click();
  await expect(toggle(page)).toHaveAttribute('aria-expanded', 'true');
  await page.keyboard.press('Escape');
  expect(await page.evaluate(() => window.integrationOriginal.plan === window.tagrex.previewPlan)).toBe(true);
  await page.locator('#presets-edit').click();
  await expect(page.locator('#pe-editor')).toBeVisible();
  await expect(toggle(page)).toHaveAttribute('aria-expanded', 'true');
  await page.keyboard.press('Escape');
  expect(await page.evaluate(() => window.integrationOriginal.plan === window.tagrex.previewPlan)).toBe(true);
});

test('collapse uses a short interruptible transition and reduced motion is immediate', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'no-preference' });
  await open(page);
  await expect(toggle(page)).toHaveAttribute('aria-expanded', 'true');
  const motion = await toggle(page).evaluate(async button => {
    const module = button.closest('[data-dock-module]');
    const before = module.getBoundingClientRect().height;
    button.click();
    await new Promise(resolve => requestAnimationFrame(resolve));
    const durations = module.getAnimations({ subtree: true }).map(animation => animation.effect.getTiming().duration).filter(value => typeof value === 'number' && value > 1);
    const samples = [];
    const end = performance.now() + 220;
    while (performance.now() < end) { samples.push(module.getBoundingClientRect().height); await new Promise(resolve => requestAnimationFrame(resolve)); }
    return { before, durations, samples, after: module.getBoundingClientRect().height };
  });
  expect(motion.durations.some(value => value >= 140 && value <= 180)).toBe(true);
  expect(motion.samples.some(value => value > motion.after + 1 && value < motion.before - 1)).toBe(true);
  await toggle(page).click(); await page.waitForTimeout(50);
  await toggle(page).click(); await page.waitForTimeout(30);
  await toggle(page).click();
  await expect(toggle(page)).toHaveAttribute('aria-expanded', 'true');
  await expect.poll(async () => Math.abs((await transforms(page).boundingBox()).height - motion.before)).toBeLessThan(2);
  await toggle(page).click();
  await expect.poll(async () => (await transforms(page).boundingBox()).height).toBeLessThan(46);
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await toggle(page).click();
  await expect(toggle(page)).toHaveAttribute('aria-expanded', 'true');
  const reduced = await toggle(page).evaluate(async button => {
    button.click();
    await new Promise(resolve => requestAnimationFrame(resolve));
    const module = button.closest('[data-dock-module]');
    return { height: module.getBoundingClientRect().height, active: module.getAnimations({ subtree: true }).filter(animation => animation.playState === 'running').length };
  });
  expect(reduced.height).toBeLessThan(46);
  expect(reduced.active).toBe(0);
});

test('editor group folding retains drafts and metadata while preferences follow new selections', async ({ page }) => {
  await open(page);
  await editor(page);
  await page.evaluate(() => {
    const track = window.tagrex.tracks[0];
    track.tags = { ...track.tags, 'custom:QA_RAW_TAG': 'Existing custom metadata' };
    window.tagrex.renderTracks();
  });
  const advanced = page.locator('.fe-group[data-group="advanced"] > button.fe-group-head');
  await advanced.click();
  const custom = page.locator('input[data-key="custom:QA_RAW_TAG"]');
  await expect(custom).toHaveValue('Existing custom metadata');
  await custom.fill('Unstaged custom draft');
  await custom.evaluate(node => window.integrationCustom = node);
  await advanced.click();
  await expect(advanced).toHaveAttribute('aria-expanded', 'false');
  await advanced.click();
  await expect(custom).toHaveValue('Unstaged custom draft');
  expect(await custom.evaluate(node => node === window.integrationCustom)).toBe(true);
  const title = page.locator('input[data-key="title"]').first();
  await title.fill('Subject A draft');
  await page.locator('#tracks-body tr[data-path]').nth(1).locator('.file').click();
  await expect(title).toHaveValue('Radio');
  await expect(title).not.toHaveClass(/dirty/);
  expect(await page.evaluate(() => window.tagrex.tracks[0].tags['custom:QA_RAW_TAG'])).toBe('Existing custom metadata');
});

test('folding restores the user-resized module weight and context movement still retains nodes', async ({ page }) => {
  await open(page);
  const boundary = page.getByRole('separator', { name: 'Resize Transformations module', exact: true });
  await boundary.focus(); await page.keyboard.press('ArrowDown'); await page.keyboard.press('ArrowDown');
  const expanded = await transforms(page).boundingBox();
  const weight = await boundary.getAttribute('aria-valuenow');
  await toggle(page).click();
  await expect.poll(async () => (await transforms(page).boundingBox()).height).toBeLessThan(46);
  await toggle(page).click();
  await expect.poll(async () => Math.abs((await transforms(page).boundingBox()).height - expanded.height)).toBeLessThan(2);
  await expect(boundary).toHaveAttribute('aria-valuenow', weight);
  await page.locator('#groups-menu input.group-tick').first().evaluate(node => window.integrationCheckbox = node);
  await toggle(page).click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Dock bottom', exact: true }).click();
  await expect(page.locator('[data-dock-host="bottom"] #dock-module-transformations')).toBeVisible();
  expect(await page.locator('#groups-menu input.group-tick').first().evaluate(node => node === window.integrationCheckbox)).toBe(true);
  const bottomHeight = (await page.locator('[data-dock-host="bottom"]').boundingBox()).height;
  await toggle(page).click();
  await expect(toggle(page)).toHaveAttribute('aria-expanded', 'false');
  await expect.poll(async () => (await page.locator('[data-dock-host="bottom"]').boundingBox()).height).toBeLessThan(46);
  await expect(page.getByRole('separator', { name: 'Resize bottom modules', exact: true })).toBeHidden();
  await toggle(page).click();
  await expect(toggle(page)).toHaveAttribute('aria-expanded', 'true');
  await expect.poll(async () => Math.abs((await page.locator('[data-dock-host="bottom"]').boundingBox()).height - bottomHeight)).toBeLessThan(2);
});

for (const theme of ['dark', 'light']) for (const size of [{ width: 375, height: 667 }, { width: 844, height: 390 }, { width: 1440, height: 900 }]) {
  test(`${theme} ${size.width}×${size.height} integrated surfaces stay within the viewport and expose their controls`, async ({ page }) => {
    await page.setViewportSize(size);
    await open(page, theme);
    const compact = page.getByRole('tablist', { name: 'Workspace module' });
    if (await compact.isVisible()) await page.getByRole('tab', { name: 'Transformations', exact: true }).click();
    await expect(toggle(page)).toBeInViewport({ ratio: 1 });
    await expect(transforms(page).locator('.transformation-help')).toBeInViewport({ ratio: 1 });
    await expect(page.locator('#presets-edit')).toBeInViewport({ ratio: 1 });
    await toggle(page).click(); await toggle(page).click();
    await editor(page);
    await page.locator('#editor-field-title').scrollIntoViewIfNeeded();
    await expect(page.locator('#editor-field-title')).toBeInViewport({ ratio: 1 });
    await expect(page.locator('#staged-edits')).toBeHidden();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    const panel = await page.locator('#dock-module-inspector').evaluate(node => ({ scroll: node.scrollWidth, client: node.clientWidth }));
    expect(panel.scroll).toBeLessThanOrEqual(panel.client);
    if (process.env.INTEGRATION_CAPTURE_EVIDENCE === '1') await screenshot(page, `${theme}-${size.width}x${size.height}.png`);
  });
}
