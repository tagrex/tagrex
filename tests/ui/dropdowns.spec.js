import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';
const root = resolve('.test-dist');
test.beforeEach(async ({ page }) => {
  await page.route('http://tagrex.test/**', async route => {
    const path = resolve(root, '.' + new URL(route.request().url()).pathname);
    if (!path.startsWith(root + sep)) return route.abort();
    try { await route.fulfill({ body: await readFile(path), contentType: path.endsWith('.js') ? 'text/javascript' : path.endsWith('.css') ? 'text/css' : path.endsWith('.woff2') ? 'font/woff2' : 'text/html' }); }
    catch { await route.fulfill({ status: 404, body: 'Missing fixture' }); }
  });
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(() => page.evaluate(() => !!window.tagrex)).toBe(true);
});

test('provider dropdown uses themed animated options and preserves native controller values', async ({ page }) => {
  const trigger = page.locator('#online-source-control');
  await trigger.click();
  const menu = page.locator('.app-select-menu');
  await expect(menu).toBeVisible();
  expect(await menu.evaluate(node => getComputedStyle(node).animationName)).toBe('app-select-in');
  await page.getByRole('option', { name: 'MusicBrainz', exact: true }).click();
  await expect(page.locator('#online-source')).toHaveValue('musicbrainz');
  await expect(trigger).toContainText('MusicBrainz');
  await expect(menu).toHaveCount(0);
  await page.evaluate(() => { window.selectEvents = 0; const node = document.querySelector('#online-source'); node.addEventListener('change', () => window.selectEvents++); node.value = 'discogs'; });
  await expect(trigger).toContainText('Discogs');
  expect(await page.evaluate(() => window.selectEvents)).toBe(0);
});

test('Escape closes a settings dropdown before its parent dialog; options fit a narrow window', async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 667 });
  await page.locator('#settings-open').click();
  await page.locator('#settings-search').fill('Read Latin-1');
  await page.locator('#set-code-page-control').click();
  const menu = page.locator('.app-select-menu');
  await expect(menu).toBeVisible();
  const rect = await menu.boundingBox();
  expect(rect.x).toBeGreaterThanOrEqual(0);
  expect(rect.x + rect.width).toBeLessThanOrEqual(375);
  await page.keyboard.press('Escape');
  await expect(menu).toHaveCount(0);
  await expect(page.locator('#settings')).toBeVisible();
  await page.locator('#set-code-page-control').click();
  await page.getByRole('option', { name: 'KOI8-R (Russian)', exact: true }).click();
  await expect(page.locator('#set-code-page')).toHaveValue('koi8-r');
});

test('the source selector keeps its accessible name without a redundant visible label', async ({ page }) => {
  await expect(page.locator('#online-source')).toHaveAttribute('aria-label','Source');
  await page.locator('#online-source-control').click();
  await expect(page.locator('.app-select-menu')).toBeVisible();
  await page.getByRole('option', { name: 'MusicBrainz', exact: true }).click();
  await expect(page.locator('#online-source')).toHaveValue('musicbrainz');
  await expect(page.locator('#online-source-control')).toContainText('MusicBrainz');
  await expect(page.locator('.app-select-menu')).toHaveCount(0);
});

test('changing an open option list closes stale choices and selecting again uses the current option', async ({ page }) => {
  await page.locator('#online-source-control').click();
  await expect(page.getByRole('option', { name: 'MusicBrainz', exact: true })).toBeVisible();
  await page.evaluate(() => {
    window.optionChanges = 0;
    const select = document.getElementById('online-source');
    select.addEventListener('change', () => window.optionChanges++);
    select.prepend(new Option('Added provider', 'added'));
  });
  await expect(page.locator('.app-select-menu')).toHaveCount(0);
  expect(await page.evaluate(() => window.optionChanges)).toBe(0);
  await page.locator('#online-source-control').click();
  await page.getByRole('option', { name: 'MusicBrainz', exact: true }).click();
  await expect(page.locator('#online-source')).toHaveValue('musicbrainz');
  expect(await page.evaluate(() => window.optionChanges)).toBe(1);
});

test('hiding a native select closes its menu and restores a usable trigger when shown', async ({ page }) => {
  const trigger = page.locator('#online-source-control');
  await trigger.click();
  await expect(page.locator('.app-select-menu')).toBeVisible();
  await page.evaluate(() => { document.getElementById('online-source').hidden = true; });
  await expect(trigger).toBeHidden();
  await expect(page.locator('.app-select-menu')).toHaveCount(0);
  await page.evaluate(() => { document.getElementById('online-source').hidden = false; });
  await expect(trigger).toBeVisible();
  await trigger.click();
  await page.getByRole('option', { name: 'Bandcamp', exact: true }).click();
  await expect(page.locator('#online-source')).toHaveValue('bandcamp');
});

test('a dynamic settings select follows its field when the window grows and retains explicit widths', async ({ page }) => {
  await page.setViewportSize({ width:375, height:844 });
  await page.locator('#settings-open').click();
  await page.evaluate(() => {
    const row = document.createElement('div');
    row.className = 'settings-row';
    row.id = 'responsive-select-row';
    const label = document.createElement('label');
    label.htmlFor = 'responsive-select';
    label.textContent = 'Responsive choice';
    const select = document.createElement('select');
    select.id = 'responsive-select';
    select.append(new Option('Short', 'short'), new Option('Longer choice', 'long'));
    row.append(label, select);
    document.querySelector('.settings-body').prepend(row);
  });
  const trigger = page.locator('#responsive-select-control');
  await expect(trigger).toBeVisible();
  const small = await trigger.boundingBox();
  await page.setViewportSize({ width:1280, height:844 });
  await expect.poll(async () => (await trigger.boundingBox()).width).toBeGreaterThan(small.width + 60);
  await trigger.click();
  await page.getByRole('option', { name:'Longer choice', exact:true }).click();
  await expect(page.locator('#responsive-select')).toHaveValue('long');
  await page.evaluate(() => {
    const select = document.createElement('select');
    select.id = 'fixed-width-select';
    select.style.width = '140px';
    select.append(new Option('Fixed width', 'fixed'));
    document.getElementById('responsive-select-row').after(select);
  });
  const fixed = page.locator('#fixed-width-select-control');
  await expect(fixed).toBeVisible();
  expect((await fixed.boundingBox()).width).toBeCloseTo(140, 0);
  await page.setViewportSize({ width:844, height:844 });
  expect((await fixed.boundingBox()).width).toBeCloseTo(140, 0);
});

for(const theme of ['light','dark']) {
  test(`field choices attach to the field regardless of click position (${theme})`,async({page})=>{
    await page.emulateMedia({reducedMotion:'reduce'});
    await page.evaluate(v=>document.documentElement.dataset.theme=v,theme);
    const trigger=page.locator('#online-source-control');
    const field=await trigger.boundingBox();
    for(const fraction of [.15,.85]){
      await trigger.click({position:{x:field.width*fraction,y:field.height/2}});
      const menu=page.locator('.app-select-menu');await expect(menu).toBeVisible();
      await expect.poll(async()=>Math.abs((await menu.boundingBox()).x-field.x)).toBeLessThan(2);
      const box=await menu.boundingBox();expect(Math.abs(box.y-field.y-field.height-4)).toBeLessThan(2);expect(box.width).toBeGreaterThanOrEqual(field.width-1);
      await page.keyboard.press('Escape');await expect(trigger).toBeFocused();
    }
    await trigger.press('ArrowDown');const menu=page.locator('.app-select-menu');await expect(menu).toBeVisible();expect(Math.abs((await menu.boundingBox()).x-field.x)).toBeLessThan(2);await page.keyboard.press('Escape');
  });
}

test('settings choice menu stays attached inside a centred dialog',async({page})=>{
  await page.emulateMedia({reducedMotion:'reduce'});
  await page.locator('#settings-open').click();const trigger=page.locator('#set-interface-font-control');await trigger.click({position:{x:16,y:16}});
  const field=await trigger.boundingBox(),menu=page.locator('.app-select-menu');await expect(menu).toBeVisible();const box=await menu.boundingBox();expect(Math.abs(box.x-field.x)).toBeLessThan(2);expect(Math.abs(box.y-field.y-field.height-4)).toBeLessThan(2);
  await page.getByRole('option',{name:'IBM Plex Sans',exact:true}).click();await expect(trigger).toContainText('IBM Plex Sans');await expect(trigger).toBeFocused();
});

test('reference popups follow a scrolling owner, keep their own list open, and close when clipped',async({page})=>{
  await page.emulateMedia({reducedMotion:'reduce'});
  await page.evaluate(()=>{
    const host=document.createElement('div');host.id='scrolling-popup-owner';host.style.cssText='position:fixed;left:100px;top:100px;width:300px;height:250px;overflow:auto;z-index:350;background:var(--panel)';
    const inner=document.createElement('div');inner.style.height='1000px';const button=document.createElement('button');button.id='scrolling-reference';button.className='ph-btn';button.dataset.phTarget='mask';button.textContent='Scrolling reference';button.style.marginTop='100px';inner.append(button);host.append(inner);document.body.append(host);
  });
  const trigger=page.locator('#scrolling-reference');await trigger.click();const menu=page.locator('#ph-menu');await expect(menu).toBeVisible();
  await page.locator('#ph-menu .ph-body').evaluate(node=>{node.scrollTop=60;});await expect(menu).toBeVisible();
  const before=await menu.boundingBox();await page.locator('#scrolling-popup-owner').evaluate(node=>{node.scrollTop=20;});
  await expect.poll(async()=>Math.abs((await menu.boundingBox()).y-before.y+20)).toBeLessThan(2);
  await page.locator('#scrolling-popup-owner').evaluate(node=>{node.scrollTop=450;});await expect(menu).toBeHidden();
});

test('release artwork actions anchor to their button and support keyboard return without saving',async({page})=>{
  await page.emulateMedia({reducedMotion:'reduce'});
  const errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.locator('#discogs-token').evaluate(n=>{n.value='fixture';});await page.locator('#discogs-query').fill('Helter Skelter');await page.locator('#discogs-search').click();
  const card=page.locator('.release-card').first();await expect(card).toBeVisible();await card.locator('.release-head').click();
  const trigger=card.locator('[data-act="save-menu"]');await expect(trigger).toBeVisible();await trigger.click();const menu=card.locator('.tk-save-menu');await expect(menu).toBeVisible();
  const owner=await trigger.boundingBox(),box=await menu.boundingBox();const viewport=page.viewportSize();expect(Math.abs(box.x-Math.max(8,Math.min(owner.x,viewport.width-box.width-8)))).toBeLessThan(2);expect(Math.min(Math.abs(box.y-owner.y-owner.height-4),Math.abs(box.y+box.height-owner.y+4))).toBeLessThan(2);
  const items=menu.locator('button');await expect(items.first()).toHaveCSS('justify-content','flex-start');await expect(items.first()).toHaveCSS('text-align','start');await expect(items.first()).toHaveCSS('min-height','32px');await expect(items.first()).toBeFocused();await page.keyboard.press('ArrowDown');await expect(items.nth(1)).toBeFocused();await page.keyboard.press('Home');await expect(items.first()).toBeFocused();
  await page.keyboard.press('Escape');await expect(menu).toBeHidden();await expect(trigger).toBeFocused();await expect(trigger).toHaveAttribute('aria-expanded','false');expect(errors).toEqual([]);
});

test('Escape from an input right-click menu restores its field and keeps the draft',async({page})=>{
  const field=page.locator('#discogs-query');await field.fill('Keep this query');await field.click({button:'right'});await expect(page.locator('#edit-ctx')).toBeVisible();await page.keyboard.press('Escape');await expect(page.locator('#edit-ctx')).toBeHidden();await expect(field).toBeFocused();await expect(field).toHaveValue('Keep this query');
});
