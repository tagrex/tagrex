import { test, expect } from '@playwright/test';
import { readFile, mkdir } from 'node:fs/promises';
import { resolve, sep } from 'node:path';

const root = resolve('.test-dist');
test.use({ viewport:{width:1440,height:1000}, reducedMotion:'reduce' });
test.beforeEach(async ({page}) => {
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.route('http://tagrex.test/**', async route => {
    const path = resolve(root, '.' + new URL(route.request().url()).pathname);
    if (!path.startsWith(root + sep)) return route.abort();
    try {
      await route.fulfill({body:await readFile(path),contentType:path.endsWith('.js')?'text/javascript':path.endsWith('.css')?'text/css':path.endsWith('.woff2')?'font/woff2':'text/html'});
    } catch { await route.fulfill({status:404,body:'Missing fixture'}); }
  });
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(() => page.evaluate(() => typeof window.tagrex?.openLibrary)).toBe('function');
  await page.locator('#root-display').click();
  await page.locator('#root').fill('/music');
  await page.locator('#root').press('Enter');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
  page.testErrors = errors;
});
test.afterEach(async ({page}) => { expect(page.testErrors).toEqual([]); });

async function tab(page, name) { await page.locator(`.subtab[data-subtab="${name}"]`).click(); }
async function selectAll(page) {
  await page.locator('#tracks-body tr[data-path]').first().click();
  await page.locator('#tracks-body tr[data-path]').last().click({modifiers:['Shift']});
}
async function sizeInspector(page, width) {
  const area = await page.locator('.dock-middle').boundingBox();
  const separator = page.getByRole('separator',{name:'Resize workspace columns',exact:true});
  const handle = await separator.boundingBox();
  await page.mouse.move(handle.x+handle.width/2, handle.y+handle.height/2);
  await page.mouse.down();
  await page.mouse.move(area.x+area.width-width-handle.width/2, handle.y+handle.height/2, {steps:4});
  await page.mouse.up();
}

test('mixed and empty placeholders never become pending values; compact fields and more tags stay usable', async ({page}) => {
  await selectAll(page);
  await tab(page,'editor');
  const title = page.locator('#editor-field-title');
  await expect(title).toHaveValue('');
  await expect(title).toHaveAttribute('placeholder','Mixed values');
  const comment = page.locator('#editor-field-comment');
  await expect(comment).toHaveAttribute('placeholder','');
  await expect(page.locator('#diff-apply')).toBeDisabled();
  await expect(page.locator('[data-group="standard"] .fe-group-head')).toHaveAttribute('aria-expanded','false');
  await page.locator('[data-group="standard"] .fe-group-head').click();
  await expect(page.locator('#editor-field-isrc')).toBeVisible();
  await title.fill('New title');
  await expect(page.locator('#diff-apply')).toBeEnabled();
  const changes = await page.evaluate(() => window.tagrex.previewPlan.changes);
  expect(changes).toHaveLength(3);
  expect(changes.every(c => c.tag_changes.every(t => t.field==='title' && t.new==='New title'))).toBe(true);
  await page.locator('#diff-apply').click();
  await expect.poll(() => page.evaluate(() => window.tagrex.tracks.every(t => t.tags.title==='New title'))).toBe(true);
  await page.locator('#undo').click();
  await expect.poll(async () => page.evaluate(async () => (await window.tagrex.invoke('history',{})).length)).toBe(0);
});

test('locking preserves the input, draft and focused unlock button, and honors reduced motion', async ({page}) => {
  await selectAll(page); await tab(page,'editor');
  const title = page.locator('#editor-field-title');
  await title.fill('Keep raw draft');
  await expect(page.locator('#diff-apply')).toBeEnabled();
  await title.evaluate(node => { window.retainedInspectorInput=node; });
  const lock = page.locator('#editor-field-title').locator('xpath=ancestor::div[contains(@class,"fe-row")]').locator('.fe-lock');
  await lock.click();
  await expect(title).toBeDisabled();
  expect(await title.evaluate(node => node===window.retainedInspectorInput)).toBe(true);
  await expect(title).toHaveValue('Keep raw draft');
  await expect(lock).toHaveAttribute('aria-pressed','true');
  await expect(lock).toBeFocused();
  expect(await title.evaluate(node => getComputedStyle(node).transitionDuration)).toBe('0s');
  expect(Number(await title.evaluate(node => getComputedStyle(node).opacity))).toBeLessThan(.6);
  await lock.press('Enter');
  await expect(title).toBeEnabled();
  await expect(title).toHaveValue('Keep raw draft');
  expect(await title.evaluate(node => node===window.retainedInspectorInput)).toBe(true);
});

test('text previews group tracks, preserve skipped-line mapping, escape metadata and only Apply writes', async ({page}) => {
  await selectAll(page); await tab(page,'fromtext');
  await page.locator('#from-text-mask').fill('%track%. %artist% - %title%');
  await page.locator('#from-text-lines').fill('Heading\n01. Artist A - First title\nSkip this\n02. Artist B - <img src=x onerror=alert(1)>\n03. Artist C - Third title\n04. Extra - Not applied');
  const samples = page.locator('#from-text-probe .probe-track');
  await expect(samples).toHaveCount(3);
  await expect(samples.first().locator('summary')).toContainText('First title');
  await expect(samples.nth(1).locator('summary')).toContainText('<img src=x onerror=alert(1)>');
  await expect(page.locator('#from-text-probe img')).toHaveCount(0);
  await expect(page.locator('#from-text-count')).toHaveText('3 matches · 1 extra track left out · 2 lines skipped');
  await samples.nth(1).locator('summary').click();
  await expect(samples.nth(1).locator('.probe-target')).toContainText('02 - wish mountain - radio.mp3');
  const original = await page.evaluate(() => window.tagrex.tracks.map(t=>t.tags.title));
  await page.locator('#from-text-preview').click();
  await expect(page.locator('#diff-apply')).toBeEnabled();
  expect(await page.evaluate(() => window.tagrex.tracks.map(t=>t.tags.title))).toEqual(original);
  await page.locator('#diff-apply').click();
  await expect.poll(() => page.evaluate(() => window.tagrex.tracks[1].tags.title)).toBe('<img src=x onerror=alert(1)>');
  await page.locator('#undo').click();
  // The browser mock consumes history; native journal restoration is outside this styling change.
  await expect.poll(() => page.evaluate(async () => (await window.tagrex.invoke('history',{})).length)).toBe(0);
});

test('name preview and pattern tools retain keyboard access; help closes back to its heading', async ({page}) => {
  await selectAll(page); await tab(page,'filename');
  await page.locator('#from-name-mask').fill('%track% - %artist% - %title%');
  await expect(page.locator('#from-name-probe .probe-track')).toHaveCount(3);
  await expect(page.locator('#from-name-probe .probe-source').first()).toContainText('File name');
  const info = page.getByRole('button',{name:'About reading file names',exact:true});
  await info.click();
  await expect(page.locator('.inspector-help-content')).toContainText('File names stay unchanged');
  await page.keyboard.press('Escape');
  await expect(info).toBeFocused();
  await page.locator('#from-name-builder-open').click();
  await expect(page.locator('#mask-builder')).toBeVisible();
});

test('search is inside the field, quick queries still open, and empty results offer recovery', async ({page}) => {
  await selectAll(page); await tab(page,'online');
  await expect(page.locator('.online-empty')).toContainText('Find the right release');
  await expect(page.locator('#query-box #discogs-search')).toHaveCount(1);
  await page.locator('#query-presets').click();
  await expect(page.locator('#query-preset-menu')).toBeVisible();
  await page.keyboard.press('Escape');
  await page.evaluate(() => {
    document.querySelector('#discogs-token').value='fixture';
    const select=document.querySelector('#search-format'); select.value='Cassette'; select.dispatchEvent(new Event('change',{bubbles:true}));
  });
  await page.locator('#discogs-query').fill('No cassette fixture');
  await page.locator('#discogs-search').click();
  await expect(page.locator('.online-empty')).toContainText('No releases found');
  await expect(page.locator('.online-empty')).toContainText('Try fewer words');
  expect(await page.locator('#discogs-empty').evaluate(node=>getComputedStyle(node).borderTopStyle)).toBe('none');
});

for (const theme of ['dark','light']) {
  test(`all four tabs fit a narrow inspector in ${theme}, with shared surfaces`, async ({page}) => {
    await selectAll(page); await sizeInspector(page,360);
    await page.evaluate(value => { document.documentElement.dataset.theme=value; document.documentElement.dataset.palette='navy'; },theme);
    for (const name of ['online','editor','filename','fromtext']) {
      await tab(page,name);
      if(name==='fromtext') {
        await page.locator('#from-text-lines').fill('01. Artist - First\n02. Artist - Second\n03. Artist - Third');
        await expect(page.locator('#from-text-probe .probe-track')).toHaveCount(3);
      }
      if(name==='filename') {
        await page.locator('#from-name-mask').fill('%track% - %artist% - %title%');
        await expect(page.locator('#from-name-probe .probe-track')).toHaveCount(3);
      }
      expect(await page.locator('.mode-col').evaluate(node=>node.scrollWidth <= node.clientWidth+1)).toBe(true);
      const controls=page.locator(`#subtab-${name} .field-tools`);
      if(await controls.count()) {
        const compound = await controls.first().locator('..').boundingBox(), tools=await controls.first().boundingBox();
        expect(tools.x+tools.width).toBeLessThanOrEqual(compound.x+compound.width+1);
      }
      if(process.env.INSPECTOR_CAPTURE==='1') {
        const directory=process.env.INSPECTOR_EVIDENCE;
        if(!directory) throw new Error('INSPECTOR_EVIDENCE is required');
        await mkdir(directory,{recursive:true});
        await page.locator('.mode-col').screenshot({path:resolve(directory,`${theme}-${name}.png`)});
      }
    }
  });
}
