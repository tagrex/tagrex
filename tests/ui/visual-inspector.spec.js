import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';

// Inspector and dock geometry: type roles, grouping rhythm, control contours,
// narrow-dock compression, dock menus and the integrated filter rail.
// Behaviour lives in the existing inspector/docking/filter suites; this file
// asserts the shared visual roles only.
const root = resolve('.test-dist');
const errors = new WeakMap();
test.use({ viewport: { width: 1440, height: 900 }, reducedMotion: 'reduce' });

test.beforeEach(async ({ page }) => {
  const issues = [];
  errors.set(page, issues);
  page.on('pageerror', (error) => issues.push(error.message));
  await page.route('http://tagrex.test/**', async (route) => {
    const path = resolve(root, '.' + new URL(route.request().url()).pathname);
    if (!path.startsWith(root + sep)) return route.abort();
    try {
      await route.fulfill({
        body: await readFile(path),
        contentType: path.endsWith('.js') ? 'text/javascript'
          : path.endsWith('.css') ? 'text/css'
          : path.endsWith('.woff2') ? 'font/woff2' : 'text/html',
      });
    } catch { await route.fulfill({ status: 404, body: 'Missing fixture' }); }
  });
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(() => page.evaluate(() => typeof window.tagrex?.openLibrary)).toBe('function');
  await page.locator('#root-display').click();
  await page.locator('#root').fill('/music');
  await page.locator('#root').press('Enter');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
  await page.locator('#tracks-body tr[data-path]').first().click();
  await page.locator('#tracks-body tr[data-path]').last().click({ modifiers: ['Shift'] });
});

test.afterEach(async ({ page }) => { expect(errors.get(page)).toEqual([]); });

async function tab(page, name) {
  await page.locator(`.subtab[data-subtab="${name}"]`).click();
  await expect(page.locator(`#subtab-${name}`)).toBeVisible();
}

async function mode(page, name) {
  await page.locator(`.mode-tab[data-mode="${name}"]`).click();
  await expect(page.locator(`#panel-${name}`)).toBeVisible();
}

async function sizeInspector(page, width) {
  const area = await page.locator('.dock-middle').boundingBox();
  const separator = page.getByRole('separator', { name: 'Resize workspace columns', exact: true });
  const handle = await separator.boundingBox();
  await page.mouse.move(handle.x + handle.width / 2, handle.y + handle.height / 2);
  await page.mouse.down();
  await page.mouse.move(area.x + area.width - width - handle.width / 2, handle.y + handle.height / 2, { steps: 4 });
  await page.mouse.up();
  await expect.poll(async () => (await page.locator('.mode-col').boundingBox()).width).toBeLessThan(Math.max(width + 60, 448));
}

const box = (page, selector) => page.locator(selector).first().evaluate((node) => {
  const rect = node.getBoundingClientRect();
  return { x: rect.x, y: rect.y, width: rect.width, height: rect.height, bottom: rect.bottom, right: rect.right };
});

const style = (page, selector, keys) => page.locator(selector).first().evaluate(
  (node, properties) => Object.fromEntries(properties.map((key) => [key, getComputedStyle(node)[key]])),
  keys,
);

const number = (value) => Number.parseFloat(value);

for (const theme of ['dark', 'light']) {
  test(`compact metadata fills its columns and paired rows keep their spacing (${theme})`, async ({ page }, testInfo) => {
    await page.evaluate(value => { document.documentElement.dataset.theme = value; }, theme);
    await tab(page, 'editor');
    await page.locator('[data-group="standard"] .fe-group-head').click();
    for (const width of [360, 440, 605]) {
      await sizeInspector(page, width);
      const geometry = await page.locator('#fields-body').evaluate(root => {
        const b = node => node.getBoundingClientRect();
        return {
          compact: [...root.querySelectorAll('.fe-inline-fields .field-input')].map(input => ({
            key: input.dataset.key, width: b(input).width, available: b(input.closest('.fe-cell')).width,
            x: b(input).x, labelRight: b(input.closest('.fe-row').querySelector('.fe-label')).right,
          })),
          pairs: [...root.querySelectorAll('.fe-duo-unit')].map(unit => ({
            labelRight: b(unit.querySelector('label')).right, input: b(unit.querySelector('input')).x,
          })),
          gaps: [...root.querySelectorAll('.fe-grid')].flatMap(grid => [...grid.children].slice(1).map((row, i) => b(row).top - b(grid.children[i]).bottom)),
        };
      });
      for (const input of geometry.compact) {
        expect(Math.abs(input.width - input.available), `${width}px ${input.key} fills its column`).toBeLessThan(1);
        expect(input.x - input.labelRight, `${width}px ${input.key} label is beside its value`).toBeCloseTo(4, 0);
      }
      for (const pair of geometry.pairs) {
        expect(pair.input - pair.labelRight, `${width}px number-pair label is beside its value`).toBeCloseTo(4, 0);
      }
      for (const gap of geometry.gaps) expect(gap, `${width}px row spacing includes paired rows`).toBeCloseTo(8, 0);
      if (process.env.COMPACT_INPUT_CAPTURE === '1') {
        await page.locator('.editor-scroll').evaluate(node => { node.scrollTop = 0; });
        await page.locator('.mode-col').screenshot({ path: testInfo.outputPath(`compact-${theme}-${width}.png`) });
        await page.locator('[data-group="standard"] .fe-group-head').scrollIntoViewIfNeeded();
        await page.locator('.mode-col').screenshot({ path: testInfo.outputPath(`more-tags-${theme}-${width}.png`) });
      }
    }
  });
}

test('every mode panel and Tagger tab uses the shared heading, body and help roles', async ({ page }) => {
  const bodyInk = await page.evaluate(() => getComputedStyle(document.body).color);
  const headings = {
    online: '#subtab-online .inspector-heading h2',
    editor: '#subtab-editor .inspector-heading h2',
    filename: '#subtab-filename .inspector-heading h2',
    fromtext: '#subtab-fromtext .inspector-heading h2',
  };
  for (const [name, selector] of Object.entries(headings)) {
    await tab(page, name);
    const text = await style(page, selector, ['fontSize', 'fontWeight', 'color', 'textTransform']);
    expect(number(text.fontSize), `${name} heading size`).toBe(14);
    expect(Number(text.fontWeight), `${name} heading weight`).toBe(600);
    expect(text.color, `${name} heading ink`).toBe(bodyInk);
    expect(text.textTransform, `${name} heading case`).toBe('none');
  }
  // The same heading role in the four non-tagger modes.
  for (const name of ['renamer', 'generator', 'deduplicator', 'exporter']) {
    await mode(page, name);
    const selector = `#panel-${name} h2`;
    const text = await style(page, selector, ['fontSize', 'fontWeight', 'color']);
    expect(number(text.fontSize), `${name} heading size`).toBe(14);
    expect(Number(text.fontWeight), `${name} heading weight`).toBe(600);
    expect(text.color, `${name} heading ink`).toBe(bodyInk);
  }
  // Help and secondary prose sit one step down; no important label drops to 10px.
  await mode(page, 'renamer');
  expect(number((await style(page, '#panel-renamer .panel-help', ['fontSize'])).fontSize)).toBe(12);
  await mode(page, 'tagger');
  await tab(page, 'editor');
  for (const selector of ['#fields-body .fe-label', '#fields-body .fe-duo-label']) {
    expect(number((await style(page, selector, ['fontSize'])).fontSize), selector).toBe(12);
  }
  expect(number((await style(page, '#fields-body .field-input', ['fontSize'])).fontSize)).toBe(13);
  await tab(page, 'filename');
  await page.locator('#from-name-mask').fill('%track% - %artist% - %title%');
  await expect(page.locator('#from-name-probe .probe-track')).toHaveCount(3);
  const probeField = await style(page, '#from-name-probe .probe-field', ['fontSize', 'textTransform']);
  expect(number(probeField.fontSize)).toBe(12);
  expect(probeField.textTransform).toBe('none');
  expect(number((await style(page, '#from-name-probe .probe-source > span:first-child', ['fontSize'])).fontSize)).toBe(12);
  await tab(page, 'online');
  expect(number((await style(page, '.subtab', ['fontSize'])).fontSize)).toBe(13);
  expect(number((await style(page, '.subtab', ['height'])).height)).toBe(28);
});

test('field groups keep one 24px separation and rows one 8px gap without doubled gaps', async ({ page }) => {
  await tab(page, 'editor');
  expect(number((await style(page, '#fields-body', ['rowGap'])).rowGap)).toBe(24);
  const groups = await page.locator('#fields-body > .fe-group').evaluateAll((nodes) => nodes.map((node) => {
    const grid = node.querySelector('.fe-grid');
    const head = node.querySelector('.fe-group-head');
    return {
      marginTop: getComputedStyle(node).marginTop,
      gap: getComputedStyle(node).rowGap,
      top: node.getBoundingClientRect().top,
      headBottom: head?.getBoundingClientRect().bottom ?? null,
      gridTop: grid.getBoundingClientRect().top,
      bottom: node.getBoundingClientRect().bottom,
      firstRow: grid.querySelector('.fe-row')?.getBoundingClientRect().top ?? null,
    };
  }));
  expect(groups.length).toBeGreaterThanOrEqual(2);
  // Group separation is the container's gap alone: no sibling margin adds to it.
  expect(groups.slice(1).every((group) => group.marginTop === '0px')).toBe(true);
  expect(groups[1].top - groups[0].bottom).toBeCloseTo(24, 0);
  // Head to first row is one 4px substep.
  expect(groups[0].headBottom).toBeNull();
  expect(groups[0].firstRow).toBeCloseTo(groups[0].gridTop, 0);
  // Two adjacent plain rows keep the 40px pitch (32px control + 8px gap).
  const rows = await page.locator('#fields-body .fe-grid > .fe-row').evaluateAll((nodes) => nodes
    .filter((node) => node.getBoundingClientRect().height < 34)
    .slice(0, 3)
    .map((node) => ({ top: node.getBoundingClientRect().top, height: node.getBoundingClientRect().height, marginTop: getComputedStyle(node).marginTop })));
  expect(rows.length).toBeGreaterThanOrEqual(2);
  expect(Math.abs(rows[0].height - 32)).toBeLessThan(0.6);
  expect(rows[1].top - rows[0].top).toBeCloseTo(40, 3);
  expect(number(rows[1].marginTop)).toBe(0);
  // Section separation is 32px and comes from the block margin, not from a gap.
  const scroll = await page.locator('#subtab-editor .editor-scroll').evaluate((node) => ({
    gap: getComputedStyle(node).rowGap,
    children: [...node.children].map((child) => ({ tag: child.className, top: child.getBoundingClientRect().top, bottom: child.getBoundingClientRect().bottom })),
  }));
  expect(number(scroll.gap)).toBe(0);
  const headingToFields = scroll.children[1].top - scroll.children[0].bottom;
  const fieldsToCover = scroll.children[2].top - scroll.children[1].bottom;
  expect(headingToFields).toBeCloseTo(24, 0);
  expect(fieldsToCover).toBeCloseTo(32, 0);
});

test('a staged field paints the same 10px contour as its control', async ({ page }) => {
  await tab(page, 'editor');
  const title = page.locator('#fields-body input[data-key="title"]');
  await title.fill('Contour check');
  const dirty = page.locator('#fields-body .fe-row').filter({ has: page.locator('input[data-key="title"]') });
  await expect(dirty).toHaveClass(/dirty/);
  await expect(dirty.locator('.fe-hint')).toHaveCSS('display', 'none');
  const metrics = await dirty.evaluate((row) => {
    const input = row.querySelector('.field-input');
    const marker = row.querySelector('.fe-marker');
    const rowBox = row.getBoundingClientRect();
    const inputBox = input.getBoundingClientRect();
    return {
      rowRadius: getComputedStyle(row).borderRadius,
      inputRadius: getComputedStyle(input).borderRadius,
      rowHeight: rowBox.height,
      inputHeight: inputBox.height,
      rowFill: getComputedStyle(row).backgroundColor,
      inputFill: getComputedStyle(input).backgroundColor,
      markerTop: marker ? marker.getBoundingClientRect().top - rowBox.top : null,
      markerRadius: marker ? getComputedStyle(marker).borderRadius : null,
    };
  });
  expect(metrics.rowRadius).toBe(metrics.inputRadius);
  expect(metrics.rowRadius).toBe('10px');
  expect(Math.abs(metrics.rowHeight - metrics.inputHeight)).toBeLessThan(0.6);
  expect(metrics.rowFill).not.toBe('rgba(0, 0, 0, 0)');
  // The 3px state rail is inset so it never pokes out of the row's curve.
  expect(metrics.markerTop).toBeGreaterThan(4);
  expect(number(metrics.markerRadius)).toBeLessThanOrEqual(2);
});

test('a narrow dock preserves the desktop editor structure in its own scrollport', async ({ page }) => {
  await sizeInspector(page, 212);
  // The compression is driven by the dock's own width, not the window.
  expect(await page.locator('.mode-col').evaluate((node) => getComputedStyle(node).containerType)).toBe('inline-size');
  for (const name of ['online', 'editor', 'filename', 'fromtext']) {
    await tab(page, name);
    if (name === 'filename') {
      await page.locator('#from-name-mask').fill('%track% - %artist% - %title%');
      await expect(page.locator('#from-name-probe .probe-track')).toHaveCount(3);
    }
    if (name === 'fromtext') {
      await page.locator('#from-text-lines').fill('01. A - B\n02. C - D');
      await expect(page.locator('#from-text-probe .probe-track')).toHaveCount(2);
    }
    const layout = await page.evaluate((active) => {
      const scope = document.querySelector(`#subtab-${active}`);
      const rows = [...scope.querySelectorAll('.fe-row')].map((row) => {
        const label = row.querySelector('.fe-label');
        const input = row.querySelector('.field-input');
        if (!label || !input || row.getBoundingClientRect().height === 0) return null;
        const labelBox = label.getBoundingClientRect();
        const inputBox = input.getBoundingClientRect();
        return {
          labelAbove: labelBox.bottom <= inputBox.top + 1,
          labelWidth: labelBox.width,
          inputWidth: inputBox.width,
          railWidth: row.querySelector(".fe-cell")?.getBoundingClientRect().width,
          overflow: row.scrollWidth - row.clientWidth,
        };
      }).filter(Boolean);
      const duo = [...scope.querySelectorAll('.fe-pair .field-input')].map((input) => input.getBoundingClientRect().width);
      return { rows, duo, column: { client: document.querySelector('.mode-col').clientWidth, scroll: document.querySelector('.mode-col').scrollWidth } };
    }, name);
    expect(layout.column.scroll).toBeLessThanOrEqual(layout.column.client + 1);
    for (const row of layout.rows) {
      expect(row.labelAbove, `${name}: label remains beside its field`).toBe(false);
      expect(row.labelWidth, `${name}: fixed label rail`).toBeGreaterThanOrEqual(32);
      expect(row.inputWidth, `${name}: value stays inside its rail`).toBeLessThanOrEqual(row.railWidth + 1);
      expect(row.inputWidth, `${name}: compact input stays usable`).toBeGreaterThanOrEqual(48);
      expect(row.overflow, `${name}: row overflow`).toBeLessThanOrEqual(1);
    }
    if (layout.duo.length) expect(Math.min(...layout.duo), `${name}: number pair input`).toBeGreaterThanOrEqual(40);
  }
  await tab(page, 'editor');
  const title = await box(page, '#fields-body input[data-key="title"]');
  expect(title.width).toBeGreaterThanOrEqual(140);
  // Field locks stay reachable and clickable at the narrow width.
  const lock = page.locator('#fields-body .fe-row').filter({ has: page.locator('input[data-key="title"]') }).locator('.fe-lock');
  const lockBox = await lock.boundingBox();
  expect(lockBox.width).toBeGreaterThanOrEqual(24);
  expect(lockBox.height).toBeGreaterThanOrEqual(24);
  await lock.focus();
  await page.keyboard.press('Enter');
  await expect(page.locator('#fields-body input[data-key="title"]')).toBeDisabled();
  await page.keyboard.press('Enter');
  await expect(page.locator('#fields-body input[data-key="title"]')).toBeEnabled();
  // The inspector's own help popover stays inside the window at this width.
  await tab(page, 'filename');
  const info = page.getByRole('button', { name: 'About reading file names', exact: true });
  await info.click();
  const help = page.locator('.inspector-help-content');
  await expect(help).toBeVisible();
  const helpBox = await help.boundingBox();
  expect(helpBox.x).toBeGreaterThanOrEqual(0);
  expect(helpBox.x + helpBox.width).toBeLessThanOrEqual(1440 + 1);
  expect(number((await style(page, '.inspector-help-content p', ['fontSize'])).fontSize)).toBe(12);
  await page.keyboard.press('Escape');
  await expect(info).toBeFocused();
});

test('dock menus use menu geometry and compact controls keep a 24px hit box', async ({ page }) => {
  await tab(page, 'editor');
  const geometry = await page.evaluate(() => {
    const size = (node) => { const rect = node.getBoundingClientRect(); return { width: rect.width, height: rect.height }; };
    const pick = (selector) => document.querySelector(selector);
    return {
      lock: size(pick('#fields-body .fe-lock')),
      subtab: size(pick('.subtab')),
      help: size(pick('#subtab-editor .inspector-help')),
    };
  });
  for (const [name, control] of Object.entries(geometry)) {
    if (!control) continue;
    expect(control.height, `${name} height`).toBeGreaterThanOrEqual(24);
  }
  expect(geometry.subtab.height).toBe(28);
  expect(geometry.lock.width).toBeGreaterThanOrEqual(24);
  // Splitters are the documented exception to the 24px hit tier: a 4px
  // allocation track with a 1px seam, and the keyboard separator as the
  // accessible resize path.
  const splitter = page.getByRole('separator', { name: 'Resize workspace columns', exact: true });
  const handle = await splitter.evaluate((node) => {
    const rect = node.getBoundingClientRect();
    const seam = getComputedStyle(node, '::after');
    return {
      width: rect.width,
      height: rect.height,
      background: getComputedStyle(node).backgroundColor,
      seamWidth: seam.width,
      seamBackground: seam.backgroundColor,
    };
  });
  expect(handle.width).toBe(4);
  expect(handle.height).toBeGreaterThan(4);
  expect(handle.background).toBe('rgba(0, 0, 0, 0)');
  expect(handle.seamWidth).toBe('1px');
  expect(handle.seamBackground).not.toBe('rgba(0, 0, 0, 0)');

  // The module layout menu: 16px shell, 8px rows, concentric 8px inset.
  await page.locator('.mode-col .subtabs').click({ button: 'right' });
  const menu = page.locator('.dock-layout-menu');
  await expect(menu).toBeVisible();
  const items = menu.getByRole('menuitem');
  const first = await items.first().boundingBox();
  const shell = await menu.boundingBox();
  const shellStyle = await style(page, '.dock-layout-menu', ['borderRadius', 'padding', 'borderTopWidth']);
  const itemStyle = await style(page, '.dock-layout-menu [data-slot="context-menu-item"]', ['borderRadius']);
  expect(shellStyle.borderRadius).toBe('16px');
  expect(shellStyle.padding).toBe('7px');
  expect(shellStyle.borderTopWidth).toBe('1px');
  expect(itemStyle.borderRadius).toBe('8px');
  expect(first.height).toBeGreaterThanOrEqual(28);
  expect(first.x - shell.x).toBeCloseTo(8, 0);
  // The last item is reachable and its contour stays inside the shell's inset.
  const last = items.last();
  await last.focus();
  await expect(last).toBeFocused();
  const lastBox = await last.boundingBox();
  expect(shell.y + shell.height - (lastBox.y + lastBox.height)).toBeCloseTo(8, 0);
  await page.keyboard.press('Escape');
  await expect(menu).toBeHidden();
});

test('resizing the dock keeps the table rows and the active field draft intact', async ({ page }) => {
  await tab(page, 'editor');
  const title = page.locator('#fields-body input[data-key="title"]');
  await title.fill('Draft kept through a resize');
  await title.evaluate((node) => { window.__keptInspectorField = node; });
  await page.locator('#tracks-body tr[data-path]').first().evaluate((node) => {
    window.__keptRow = node;
    window.__layoutWrites = 0;
    const save = Storage.prototype.setItem;
    Storage.prototype.setItem = function (key, value) {
      if (key === 'tagrex.workspace.v1') window.__layoutWrites += 1;
      return save.call(this, key, value);
    };
  });
  const area = await page.locator('.dock-middle').boundingBox();
  const splitter = page.getByRole('separator', { name: 'Resize workspace columns', exact: true });
  const handle = await splitter.boundingBox();
  await page.mouse.move(handle.x + 2, handle.y + 80);
  await page.mouse.down();
  await page.mouse.move(handle.x - 120, handle.y + 80, { steps: 12 });
  const during = await page.evaluate(() => ({
    resizing: document.body.classList.contains('resizing-dock'),
    rowKept: Boolean(window.__keptRow?.isConnected),
    writes: window.__layoutWrites,
  }));
  expect(during.resizing).toBe(true);
  expect(during.rowKept).toBe(true);
  expect(during.writes).toBe(0);
  expect(await title.evaluate((node) => node === window.__keptInspectorField)).toBe(true);
  await page.mouse.up();
  await expect(title).toHaveValue('Draft kept through a resize');
  await expect(page.locator('#diff-apply')).toBeEnabled();
});

test('the filter rail is one integrated 28px control row with a floating builder', async ({ page }) => {
  const head = await style(page, '#filter-panel', ['backgroundColor', 'borderTopWidth', 'padding']);
  expect(head.backgroundColor).toBe('rgba(0, 0, 0, 0)');
  expect(head.borderTopWidth).toBe('0px');
  expect(head.padding).toBe('0px 8px 4px');
  expect(number((await style(page, '#filter-panel-toggle', ['height'])).height)).toBe(28);
  await page.locator('#filter-panel-toggle').click();
  await expect(page.locator('#filter')).toBeVisible();
  const field = await style(page, '.filter-ctl', ['height', 'minHeight', 'borderRadius']);
  expect(number(field.height)).toBe(28);
  expect(number(field.minHeight)).toBe(28);
  expect(field.borderRadius).toBe('10px');
  const input = await style(page, '#filter', ['height', 'borderRadius']);
  expect(number(input.borderRadius)).toBe(10);
  const flag = await box(page, '#filter-regex');
  expect(flag.width).toBeGreaterThanOrEqual(24);
  expect(flag.height).toBeGreaterThanOrEqual(24);
  // An invalid pattern changes ink, never geometry.
  await page.locator('#filter-regex').click();
  await page.locator('#filter').fill('[');
  await expect(page.locator('#filter')).toHaveAttribute('aria-invalid', 'true');
  expect(number((await style(page, '.filter-ctl', ['height'])).height)).toBe(28);
  await page.locator('#filter').fill('');
  // The condition builder floats clear of the dock and stays fully visible.
  const add = page.locator('#filter-add');
  await add.click();
  const builder = page.locator('.filter-builder');
  await expect(builder).toBeVisible();
  const builderBox = await builder.boundingBox();
  expect(builderBox.x).toBeGreaterThanOrEqual(0);
  expect(builderBox.y).toBeGreaterThanOrEqual(0);
  expect(builderBox.x + builderBox.width).toBeLessThanOrEqual(1440 + 1);
  expect(builderBox.y + builderBox.height).toBeLessThanOrEqual(900 + 1);
  const control = await style(page, '.filter-builder input', ['borderRadius', 'minHeight']);
  expect(control.borderRadius).toBe('10px');
  expect(number(control.minHeight)).toBe(32);
  await page.keyboard.press('Escape');
  await expect(builder).toBeHidden();
  await expect(add).toBeFocused();
});
