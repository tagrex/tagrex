import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';

const root = resolve('.test-dist');
const errors = new WeakMap();
test.use({ viewport: { width: 1440, height: 1000 }, reducedMotion: 'reduce' });
test.beforeEach(async ({ page }) => {
  const issues = []; errors.set(page, issues); page.on('pageerror', error => issues.push(error.message));
  await page.route('http://tagrex.test/**', async route => {
    const path = resolve(root, '.' + new URL(route.request().url()).pathname);
    if (!path.startsWith(root + sep)) return route.abort();
    try {
      await route.fulfill({ body: await readFile(path), contentType: path.endsWith('.js') ? 'text/javascript' : path.endsWith('.css') ? 'text/css' : path.endsWith('.woff2') ? 'font/woff2' : 'text/html' });
    } catch { await route.fulfill({ status: 404, body: 'Missing fixture' }); }
  });
  await page.goto('http://tagrex.test/app/ui/index.html');
  await page.waitForFunction(() => !!window.tagrex);
  await page.locator('#root-display').click(); await page.locator('#root').fill('/music'); await page.locator('#root').press('Enter');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
});
test.afterEach(async ({ page }) => { expect(errors.get(page)).toEqual([]); });
async function selectAll(page) {
  await page.locator('#tracks-body tr[data-path]').first().click();
  await page.locator('#tracks-body tr[data-path]').last().click({ modifiers: ['Shift'] });
  await page.locator('.subtab[data-subtab="editor"]').click();
}

test('desktop metadata keeps fixed rows, left-aligned values and vertically centred labels', async ({ page }, testInfo) => {
  await selectAll(page);
  await expect(page.locator('[data-group="core"] .fe-group-head')).toHaveCount(0);
  for (const width of [426, 605, 320]) {
    await page.locator('.mode-col').evaluate((node, value) => { node.style.width = `${value}px`; node.style.flex = `0 0 ${value}px`; }, width);
    const geometry = await page.locator('#editor-group-core').evaluate(root => {
      const rect = selector => root.querySelector(selector).getBoundingClientRect();
      const fields = [...root.querySelectorAll('.field-input')].map(input => {
        const label = input.closest('.fe-duo-unit')?.querySelector('label') || input.closest('.fe-row').querySelector('.fe-label');
        const a = input.getBoundingClientRect(), b = label.getBoundingClientRect();
        return { key: input.dataset.key, align: getComputedStyle(input).textAlign, centred: Math.abs(a.y + a.height / 2 - b.y - b.height / 2), left: b.right <= a.left };
      });
      return { fields, track: rect('[data-key="track"]').y, disc: rect('[data-key="disc"]').y, year: rect('[data-key="year"]').y, compilation: rect('.fe-compilation-label').y, genre: rect('[data-key="genre"]').y, comment: rect('[data-key="comment"]').y, bpm: rect('[data-key="bpm"]').y, key: rect('[data-key="key"]').y };
    });
    expect(geometry.track).toBe(geometry.disc);
    expect(geometry.year).toBe(geometry.compilation);
    expect(geometry.genre).toBeGreaterThan(geometry.year);
    expect(geometry.bpm).toBeGreaterThan(geometry.comment);
    expect(geometry.bpm).toBe(geometry.key);
    for (const field of geometry.fields) {
      expect(field.align, field.key).toBe('start'); expect(field.left, field.key).toBe(true); expect(field.centred, field.key).toBeLessThan(1);
    }
    await page.locator('.editor-scroll').evaluate(node => { node.scrollTop = 0; node.scrollLeft = 0; });
    await page.locator('.mode-col').screenshot({ path: testInfo.outputPath(`editor-${width}.png`) });
  }
});

test('compilation is indeterminate for mixed files and edits each native field only on activation', async ({ page }) => {
  await page.evaluate(() => {
    const [a, b, c] = window.tagrex.tracks;
    a.compilation_field = 'custom:TCMP'; a.tags['custom:TCMP'] = '1';
    b.compilation_field = 'custom:COMPILATION'; b.tags['custom:COMPILATION'] = '0';
    c.compilation_field = 'custom:cpil';
  });
  await selectAll(page);
  const toggle = page.locator('#editor-compilation-control');
  await expect(toggle).toHaveAttribute('aria-checked', 'mixed');
  await expect(page.locator('#diff-apply')).toBeDisabled();
  const original = await page.evaluate(() => window.tagrex.tracks.map(track => ({ ...track.tags })));
  await toggle.click(); await expect(toggle).toHaveAttribute('aria-checked', 'true');
  await expect(page.locator('#diff-apply')).toBeEnabled();
  expect(await page.evaluate(() => window.tagrex.tracks.map(track => track.tags))).toEqual(original);
  const changes = await page.evaluate(() => window.tagrex.previewPlan.changes.flatMap(change => change.tag_changes));
  expect(changes.map(change => change.field).sort()).toEqual(['custom:COMPILATION', 'custom:cpil']);
  await page.locator('#diff-apply').click();
  await expect.poll(() => page.evaluate(() => window.tagrex.tracks.every(track => track.tags[track.compilation_field] === '1'))).toBe(true);
  expect(await page.evaluate(() => window.tagrex.tracks.map(track => track.tags.title))).toEqual(original.map(tags => tags.title));
});

test('compilation respects field locks and unavailable native flag metadata', async ({ page }) => {
  await selectAll(page);
  const toggle = page.locator('#editor-compilation-control');
  const lock = page.locator('.fe-compilation .fe-lock');
  await lock.click(); await expect(toggle).toBeDisabled(); await expect(page.locator('#diff-apply')).toBeDisabled();
  await lock.click(); await expect(toggle).toBeEnabled();
  await page.locator('#tracks-body tr[data-path]').first().click();
  await page.evaluate(() => { delete window.tagrex.tracks[1].compilation_field; });
  await page.locator('#tracks-body tr[data-path]').last().click({ modifiers: ['Shift'] });
  await expect(toggle).toBeDisabled();
});

test('no selection is blank, numeric controls reject text, and unaffected files keep normal ink', async ({ page }) => {
  await page.locator('#tracks-body tr[data-path]').first().click({modifiers:['Control']});
  expect(await page.evaluate(()=>window.tagrex.selection.size)).toBe(0);
  await page.locator('.subtab[data-subtab="editor"]').click();
  for (const input of await page.locator('#fields-body .field-input').all()) { await expect(input).toHaveValue(''); await expect(input).toHaveAttribute('placeholder',''); await expect(input).toBeDisabled(); }
  await expect(page.locator('#subtab-editor #fields-blocks')).toHaveCount(0); await expect(page.locator('#fields-clear')).toHaveCount(0);
  await page.locator('#tracks-body tr[data-path]').first().click();
  const year=page.locator('#editor-field-year');await year.fill('');await year.pressSequentially('2a0b0c1');await expect(year).toHaveValue('2001');
  const track=page.locator('#editor-field-track');await track.fill('A1');await expect(track).toHaveValue('');await track.fill('3');await expect(track).toHaveValue('3');
  const bpm=page.locator('#editor-field-bpm');await bpm.fill('128.5');await bpm.fill('loud');await expect(bpm).toHaveValue('128.5');
  await expect(page.locator('#fields-body .fe-hint:visible')).toHaveCount(0);
  const last=page.locator('#tracks-body tr[data-path]').last().locator('td').nth(1);await expect(last).toHaveCSS('opacity','1');
  const key=page.locator('#editor-field-key');expect(Math.abs((await bpm.boundingBox()).width-(await key.boundingBox()).width)).toBeLessThan(1);
});

test('Preview opens pending-only comparison, edits stay attached until commit, and clear hides the summary', async ({page},testInfo) => {
  await expect(page.locator('#staged-edits')).toBeHidden();
  await page.locator('.subtab[data-subtab="editor"]').click();
  await page.locator('#editor-field-title').fill('New title');
  await expect(page.locator('#diff-apply')).toBeEnabled();
  await expect(page.locator('#staged-count')).toHaveText('1 file staged');
  const selection=await page.evaluate(()=>[...window.tagrex.selection]);
  await page.locator('#diff-preview').click();
  await expect(page.locator('#staged-tab')).toHaveAttribute('aria-selected','true');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(1);
  await expect(page.locator('#tracks')).toHaveClass(/show-old/);
  const cell=page.locator('td.editable[data-field="title"]');
  await expect(cell).toContainText('New title');await expect(cell.locator('.cell-old')).toHaveText('Desert Rain');
  const artist=page.locator('td.editable[data-field="artist"]');await expect(artist).toHaveCSS('opacity','0.65');
  expect(await page.evaluate(()=>[...window.tagrex.selection])).toEqual(selection);
  await cell.dblclick();await expect(cell).toHaveAttribute('contenteditable','true');
  await cell.fill('Desert Rain');
  await expect(cell).toHaveAttribute('contenteditable','true'); // no disappearing row mid-edit
  await cell.press('Enter');
  await expect(page.locator('#staged-edits')).toBeHidden();
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(0);
  await expect(page.locator('#tracks-empty')).toBeVisible();
  await page.locator('#files-tab').click();await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
  await page.locator('#editor-field-artist').fill('New artist');await expect(page.locator('#diff-apply')).toBeEnabled();
  await page.locator('#diff-discard').click();await expect(page.locator('#staged-edits')).toBeHidden();
  expect(await page.evaluate(()=>window.tagrex.tracks[0].tags.artist)).toBe('The X Factor');
  await page.locator('.mode-col').screenshot({path:testInfo.outputPath('staged-cleared.png')});
});

test('staging stays pinned and Apply writes the complete pending draft', async ({page},testInfo) => {
  await selectAll(page);
  for(const [key,value] of [['title','New title'],['artist','New artist'],['album','New album'],['genre','Techno']]) await page.locator(`#editor-field-${key}`).fill(value);
  await expect(page.locator('#diff-apply')).toBeEnabled();
  await expect(page.locator('#staged-count')).toHaveText('3 files staged');
  const before=await page.locator('#staged-edits').boundingBox();
  await page.locator('.editor-scroll').evaluate(node=>node.scrollTop=node.scrollHeight);
  const after=await page.locator('#staged-edits').boundingBox();expect(Math.abs(before.y-after.y)).toBeLessThan(1);
  const pane=await page.locator('.mode-col').boundingBox();expect(Math.abs(after.y+after.height+12-pane.y-pane.height)).toBeLessThan(2);
  await page.locator('#diff-preview').click();await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
  await page.locator('.mode-col').screenshot({path:testInfo.outputPath('staged-pinned.png')});
  await page.locator('#diff-apply').click();
  await expect.poll(()=>page.evaluate(()=>window.tagrex.tracks.every(t=>t.tags.artist==='New artist'&&t.tags.genre==='Techno'))).toBe(true);
  await expect(page.locator('#staged-edits')).toBeHidden();await expect(page.locator('#diff-actionbar')).toBeHidden();
});

test('right-click preserves the selected scope for format tools', async ({page}) => {
  await selectAll(page);await page.locator('#editor-field-title').fill('New title');await expect(page.locator('#diff-apply')).toBeEnabled();
  await page.locator('#tracks-body tr[data-path]').nth(1).click({button:'right'});
  expect(await page.evaluate(()=>window.tagrex.selection.size)).toBe(3);await page.locator('#file-ctx [data-cmd="formats"]').click();await expect(page.locator('#tag-formats-modal')).toBeVisible();
  await page.keyboard.press('Escape');await expect(page.locator('#tag-formats-modal')).toBeHidden();expect(await page.evaluate(()=>window.tagrex.selection.size)).toBe(3);
});

test('collapsing a fractional-weight module fills the inspector and leaves the full cover reachable', async ({page},testInfo) => {
  await page.locator('.subtab[data-subtab="editor"]').click();
  const toggle=page.locator('#dock-module-transformations .dock-module-toggle');
  if(await toggle.getAttribute('aria-expanded')==='false') await toggle.click();
  await page.evaluate(()=>{
    document.getElementById('dock-module-inspector').style.setProperty('--dock-weight','.834152');
    document.getElementById('dock-module-transformations').style.setProperty('--dock-weight','.165848');
  });
  await toggle.click();
  await expect(page.locator('#dock-module-transformations')).toHaveAttribute('data-dock-collapsed','true');
  await expect.poll(()=>page.locator('#dock-module-inspector').evaluate(node=>Math.abs(node.getBoundingClientRect().bottom-node.parentElement.getBoundingClientRect().bottom))).toBeLessThan(1);
  const cover=page.locator('.cover-main-image');await expect(cover).toBeVisible();await cover.scrollIntoViewIfNeeded();
  const image=await cover.boundingBox(),scroll=await page.locator('.editor-scroll').boundingBox();
  expect(image.y).toBeGreaterThanOrEqual(scroll.y);expect(image.y+image.height).toBeLessThanOrEqual(scroll.y+scroll.height+1);
  await page.locator('.mode-col').screenshot({path:testInfo.outputPath('sidebar-cover-unclipped.png')});
});

test('Tag formats refreshes the selected file outside the Editor tab', async ({page}) => {
  await expect(page.locator('#subtab-online')).toBeVisible();
  await page.locator('#tracks-body tr[data-path]').first().locator('td.file').click({button:'right'});
  await page.locator('#file-ctx [data-cmd="formats"]').click();await expect(page.locator('#fields-blocks')).toContainText('ID3v2');
  await page.keyboard.press('Escape');
  await page.locator('#tracks-body tr[data-path]').last().locator('td.file').click({button:'right'});
  await page.locator('#file-ctx [data-cmd="formats"]').click();await expect(page.locator('#fields-blocks')).toContainText('APE');
});

test('an explicit spreadsheet range retains its menu inside a multi-file selection', async ({page}) => {
  const artist=page.locator('#tracks-body td[data-field="artist"]').first();await artist.click();
  await page.keyboard.press('Control+a');expect(await page.evaluate(()=>window.tagrex.selection.size)).toBe(3);
  await page.keyboard.press('Shift+ArrowDown');await expect(page.locator('td.sheet-range')).toHaveCount(2);
  await artist.click({button:'right'});
  await expect(page.getByRole('menuitem',{name:/^Copy/})).toBeVisible();await expect(page.getByRole('menuitem',{name:/Fill down/})).toBeVisible();
  await expect(page.locator('#file-ctx')).toBeHidden();
});

test('editing a manual tag preview preserves a narrowed Apply scope', async ({page}) => {
  await page.locator('#tracks-body td[data-field="artist"]').first().click();
  await page.evaluate(()=>{const data=new DataTransfer();data.setData('text/plain','New artist\nNew artist\nNew artist');document.activeElement.dispatchEvent(new ClipboardEvent('paste',{bubbles:true,cancelable:true,clipboardData:data}));});
  await expect(page.locator('#diff-apply')).toBeEnabled();await expect(page.locator('#ab-count')).toHaveText('3');
  await page.locator('#tracks-body tr[data-path]').first().locator('td.file').click();await expect(page.locator('#ab-count')).toHaveText('1');
  await page.locator('#diff-preview').click();
  const title=page.locator('#tracks-body td[data-field="title"]').first();await title.dblclick();await title.fill('Scoped title');await title.press('Enter');
  await expect(page.locator('#diff-apply')).toBeEnabled();await expect(page.locator('#ab-count')).toHaveText('1');
  await page.locator('#diff-apply').click();
  await expect.poll(()=>page.evaluate(()=>window.tagrex.tracks[0].tags.title)).toBe('Scoped title');
  expect(await page.evaluate(()=>window.tagrex.tracks.slice(1).every(track=>track.tags.artist!=='New artist'))).toBe(true);
});

test('compact browser tabs have distinct states and a rounded address bar separated from the table', async ({page}) => {
  for(const theme of ['light','dark'])for(const palette of ['graphite','navy','forest','plum']) {
    await page.evaluate(({theme,palette})=>{document.documentElement.dataset.theme=theme;document.documentElement.dataset.palette=palette;},{theme,palette});
    const metrics=await page.evaluate(()=>{const tab=document.getElementById('files-tab'),other=document.getElementById('staged-tab'),bar=document.getElementById('root-field'),table=document.getElementById('files-view');return {width:tab.offsetWidth,active:getComputedStyle(tab).backgroundColor,inactive:getComputedStyle(other).backgroundColor,radius:getComputedStyle(bar).borderRadius,gap:table.getBoundingClientRect().top-bar.getBoundingClientRect().bottom};});
    expect(metrics.width).toBe(116);expect(metrics.active).not.toBe(metrics.inactive);expect(metrics.radius).toBe('10px');expect(metrics.gap).toBeGreaterThanOrEqual(8);
  }
  await page.locator('#files-tab').focus();await page.keyboard.press('ArrowRight');await expect(page.locator('#staged-tab')).toHaveAttribute('aria-selected','true');await expect(page.locator('#staged-tab')).toBeFocused();
  await expect(page.locator('#tracks-empty')).toBeVisible();await page.keyboard.press('Home');await expect(page.locator('#files-tab')).toHaveAttribute('aria-selected','true');
});

test('opening a library from an empty Staged view returns to Files', async ({page}) => {
  await page.locator('#staged-tab').click();await expect(page.locator('#tracks-empty')).toBeVisible();
  await page.locator('#root-display').click();await page.locator('#root').fill('/another');await page.locator('#root').press('Enter');
  await expect(page.locator('#files-tab')).toHaveAttribute('aria-selected','true');await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);await expect(page.locator('#staged-edits')).toBeHidden();
});
