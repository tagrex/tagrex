import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';
const root = resolve('.test-dist');
const errors = new WeakMap();
test.beforeEach(async ({ page }) => {
  const observed = [];
  errors.set(page, observed);
  page.on('pageerror', error => observed.push(error.message));
  await page.route('http://tagrex.test/**', async route => {
    const path = resolve(root, '.' + new URL(route.request().url()).pathname);
    if (!path.startsWith(root + sep)) return route.abort();
    try { await route.fulfill({ body: await readFile(path), contentType:path.endsWith('.js') ? 'text/javascript' : path.endsWith('.css') ? 'text/css' : path.endsWith('.woff2') ? 'font/woff2' : 'text/html' }); }
    catch { await route.fulfill({ status:404, body:'Missing fixture' }); }
  });
  await page.goto('http://tagrex.test/tests/ui/dock-fixture/index.html');
  await expect.poll(() => page.evaluate(() => !!window.dockTest?.controller)).toBe(true);
});
test.afterEach(async ({ page }) => { expect(errors.get(page)).toEqual([]); });

async function moduleMenu(page, name) {
  await page.getByRole('button', { name:`${name} module layout`, exact:true }).click({button:'right'});
}

test('menu docking preserves the original edited field and listeners; saved layout reloads and resets', async ({ page }) => {
  await page.getByRole('textbox', { name:'Persistent title' }).fill('Still editing this title');
  await page.locator('#dock-legacy').click();
  const layoutButton = page.getByRole('button', {name:'Files module layout', exact:true});
  await layoutButton.focus();
  await page.keyboard.press('Enter');
  await page.getByRole('menuitem', { name:'Dock bottom', exact:true }).focus();
  await page.keyboard.press('Enter');
  await expect(page.locator('[data-dock-host="bottom"] #dock-edit')).toHaveValue('Still editing this title');
  expect(await page.evaluate(() => window.dockTest.original === document.getElementById('dock-edit'))).toBe(true);
  await page.locator('#dock-legacy').click();
  await expect(page.locator('#dock-clicks')).toHaveText('2');
  await expect(page.locator('#dock-value')).toHaveText('Still editing this title');
  await page.reload();
  await expect(page.locator('[data-dock-host="bottom"] #dock-edit')).toBeVisible();
  await moduleMenu(page, 'Files');
  await page.getByRole('menuitem', { name:'Reset workspace layout', exact:true }).click();
  await expect(page.locator('[data-dock-host="left"] #dock-edit')).toBeVisible();
  expect(await page.evaluate(() => window.dockTest.original === document.getElementById('dock-edit'))).toBe(true);
});

test('plain headings offer context movement and reorder the existing sibling roots without handles', async ({ page }) => {
  await expect(page.locator('.dock-grip, .dock-menu-button, [draggable="true"]')).toHaveCount(0);
  await moduleMenu(page, 'Editor');
  await page.getByRole('menuitem', { name:'Dock left', exact:true }).click();
  await expect(page.locator('[data-dock-host="left"] [data-dock-module="editor"]')).toBeVisible();
  await moduleMenu(page, 'Online');
  await page.getByRole('menuitem', { name:'Dock left', exact:true }).click();
  await moduleMenu(page, 'Online');
  await page.getByRole('menuitem', { name:'Move earlier', exact:true }).click();
  const order = await page.locator('[data-dock-host="left"] > [data-dock-module]').evaluateAll(nodes => nodes.map(node => node.dataset.dockModule));
  expect(order).toEqual(['files', 'online', 'editor']);
});

test('splitters resize by pointer and keyboard and keep their saved proportions', async ({ page }) => {
  const splitter = page.getByRole('separator', { name:'Resize workspace columns', exact:true });
  const before = await page.locator('[data-dock-host="left"]').boundingBox();
  await splitter.focus();
  await page.keyboard.press('ArrowLeft');
  await expect(splitter).toHaveAttribute('aria-valuenow', '70');
  expect((await page.locator('[data-dock-host="left"]').boundingBox()).width).toBeLessThan(before.width);
  const rect = await splitter.boundingBox();
  await page.mouse.move(rect.x + rect.width / 2, rect.y + 40);
  await page.mouse.down();
  await page.mouse.move(rect.x - 100, rect.y + 40, {steps:8});
  await page.mouse.up();
  const saved = await splitter.getAttribute('aria-valuenow');
  expect(Number(saved)).toBeLessThan(70);
  await page.reload();
  await expect(page.getByRole('separator', {name:'Resize workspace columns', exact:true})).toHaveAttribute('aria-valuenow', saved);
});

test('narrow module switching and wider restoration preserve field state and DOM identity', async ({ page }) => {
  await page.getByRole('textbox', {name:'Persistent title'}).fill('Preserved at every size');
  await page.setViewportSize({width:375,height:667});
  await expect(page.getByRole('tablist', {name:'Workspace module'})).toBeVisible();
  await page.getByRole('tab', {name:'Online',exact:true}).click();
  await expect(page.locator('#dock-module-online')).toBeVisible();
  await expect(page.locator('#dock-module-files')).toBeHidden();
  await page.getByRole('tab', {name:'Files',exact:true}).click();
  await page.getByRole('tab', {name:'Files',exact:true}).focus();
  await page.keyboard.press('ArrowLeft');
  await expect(page.locator('#dock-module-transforms')).toBeVisible();
  await page.keyboard.press('ArrowRight');
  await expect(page.getByRole('textbox', {name:'Persistent title'})).toHaveValue('Preserved at every size');
  expect(await page.evaluate(() => window.dockTest.original === document.getElementById('dock-edit'))).toBe(true);
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(375);
  await page.setViewportSize({width:1280,height:800});
  await expect(page.locator('#dock-module-files')).toBeVisible();
  await expect(page.locator('#dock-module-editor')).toBeVisible();
  await expect(page.locator('#dock-module-online')).toBeVisible();
});

test('the real inspector keeps a staged field and the same native nodes while docked', async ({ page }) => {
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(() => page.evaluate(() => !!window.tagrex)).toBe(true);
  await page.locator('#root-display').click();
  await page.locator('#root').fill('/music');
  await page.locator('#root').press('Enter');
  await page.locator('.subtab[data-subtab="editor"]').click();
  const title = page.locator('#fields-body input[data-key="title"]');
  await title.fill('An edit kept through docking');
  await page.evaluate(() => { window.dockAppOriginal = { title:document.querySelector('#fields-body input[data-key="title"]'), table:document.getElementById('tracks-body') }; });
  await expect(page.getByRole('button', {name:'Inspector module layout', exact:true})).toBeHidden();
  const editorTab = page.locator('.subtab[data-subtab="editor"]');
  await editorTab.focus();
  await page.keyboard.press('Shift+F10');
  await expect(page.getByRole('menuitem', {name:'Dock bottom',exact:true})).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(editorTab).toBeFocused();
  await page.locator('.mode-col .subtabs').click({button:'right'});
  await page.getByRole('menuitem', {name:'Dock bottom',exact:true}).click();
  await expect(page.locator('[data-dock-host="bottom"] #fields-body input[data-key="title"]')).toHaveValue('An edit kept through docking');
  await expect(title).toHaveClass(/dirty/);
  expect(await page.evaluate(() => window.dockAppOriginal.title === document.querySelector('#fields-body input[data-key="title"]') && window.dockAppOriginal.table === document.getElementById('tracks-body'))).toBe(true);
  await expect(page.locator('#tracks-body tr.selected')).toHaveCount(1);
});


test('the real Transformations boundary resizes its adjacent Inspector and persists pointer and keyboard sizes', async ({ page }) => {
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(() => page.evaluate(() => !!window.tagrex)).toBe(true);
  const top = page.locator('#dock-module-transformations');
  const bottom = page.locator('#dock-module-inspector');
  const boundary = page.getByRole('separator', {name:'Resize Transformations module',exact:true});
  const initialTop = await top.boundingBox();
  const initialBottom = await bottom.boundingBox();
  const handle = await boundary.boundingBox();
  expect(handle.height).toBe(4);
  expect(await boundary.evaluate(node => getComputedStyle(node).backgroundColor)).toBe('rgba(0, 0, 0, 0)');
  const seam = await boundary.evaluate(node => {
    const style = getComputedStyle(node, '::after');
    return { height: style.height, background: style.backgroundColor };
  });
  expect(seam.height).toBe('1px');
  expect(seam.background).not.toBe('rgba(0, 0, 0, 0)');
  await page.mouse.move(handle.x + 50, handle.y + 2);
  await page.mouse.down();
  await page.mouse.move(handle.x + 50, handle.y + 82, {steps:12});
  expect((await top.boundingBox()).height).toBeGreaterThan(initialTop.height + 50);
  await page.mouse.up();
  const resizedTop = await top.boundingBox();
  const resizedBottom = await bottom.boundingBox();
  expect(resizedTop.height + resizedBottom.height).toBeCloseTo(initialTop.height + initialBottom.height, 0);
  expect(resizedBottom.height).toBeLessThan(initialBottom.height - 50);
  await boundary.focus();
  await page.keyboard.press('ArrowDown');
  expect((await top.boundingBox()).height).toBeGreaterThan(resizedTop.height + 10);
  const saved = (await top.boundingBox()).height;
  await page.reload();
  await expect.poll(async () => (await page.locator('#dock-module-transformations').boundingBox()).height).toBeCloseTo(saved, 0);
});

test('live column resize retains table rows, coalesces frames and writes saved layout once on release', async ({ page }) => {
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(() => page.evaluate(() => !!window.tagrex)).toBe(true);
  await page.evaluate(async () => {
    await window.tagrex.invoke('open_drop', {paths:Array.from({length:3000}, (_, index) => `track-${String(index).padStart(4,'0')}.mp3`)});
    document.getElementById('root').value = '/dropped';
    await window.tagrex.openLibrary();
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    window.resizePerf = {paints:0,writes:0};
    window.firstResizeRow = document.querySelector('#tracks-body tr[data-path]');
    const replace = Element.prototype.replaceChildren;
    Element.prototype.replaceChildren = function(...nodes) { if (this.id === 'tracks-body') window.resizePerf.paints++; return replace.apply(this,nodes); };
    const save = Storage.prototype.setItem;
    Storage.prototype.setItem = function(key,value) { if (key === 'tagrex.workspace.v1') window.resizePerf.writes++; return save.call(this,key,value); };
  });
  const boundary = page.getByRole('separator', {name:'Resize workspace columns',exact:true});
  const handle = await boundary.boundingBox();
  const started = Date.now();
  await page.mouse.move(handle.x + 2, handle.y + 100);
  await page.mouse.down();
  await page.mouse.move(handle.x - 150, handle.y + 100, {steps:24});
  const during = await page.evaluate(() => ({...window.resizePerf, firstRowConnected:window.firstResizeRow.isConnected}));
  expect(during).toEqual({paints:0,writes:0,firstRowConnected:true});
  await page.mouse.up();
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  const after = await page.evaluate(() => window.resizePerf);
  expect(after.writes).toBe(1);
  expect(after.paints).toBeLessThanOrEqual(1);
  await test.info().attach('resize-performance', {body:JSON.stringify({rows:3000,steps:24,during,after,wallMs:Date.now()-started}),contentType:'application/json'});
});

test('Transformations uses its full resized height for presets instead of leaving a capped list above a void', async ({ page }) => {
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(() => page.evaluate(() => !!window.tagrex)).toBe(true);
  // The browser provider has only two built-ins. Repeat their real row markup
  // to exercise the long shelf from a library with many saved presets.
  await page.locator('#groups-menu').evaluate(menu => {
    const row = menu.querySelector('.preset-check');
    for (let index = 0; index < 30; index++) menu.append(row.cloneNode(true));
  });
  const boundary = page.getByRole('separator', {name:'Resize Transformations module',exact:true});
  await boundary.focus();
  for (let index = 0; index < 6; index++) await page.keyboard.press('ArrowDown');
  const sizes = await page.locator('#transform-block').evaluate(block => {
    const bounds = block.getBoundingClientRect();
    const list = block.querySelector('#groups-menu').getBoundingClientRect();
    return {height:bounds.height, listHeight:list.height, footerGap:bounds.bottom-list.bottom};
  });
  expect(sizes.listHeight).toBeGreaterThan(sizes.height - 90);
  expect(sizes.footerGap).toBeLessThan(12);
  await expect(page.locator('#dock-module-transformations > .dock-module-chrome #presets-edit')).toBeVisible();
  const edit = await page.locator('#presets-edit').boundingBox();
  const help = await page.locator('.transformation-help').boundingBox();
  expect(Math.abs(edit.y + edit.height / 2 - help.y - help.height / 2)).toBeLessThan(2);
  await page.locator('#groups-menu').evaluate(menu => { menu.scrollTop = menu.scrollHeight; });
  await expect(page.locator('#groups-menu .preset-check').last()).toBeInViewport();
});


for (const [weight, expectedMinimum, expectedMaximum] of [[.32, 215, 400], [.42, 150, 210]]) {
  test(`saved Transformations weight ${weight} ${weight === .32 ? 'receives the draft-default correction' : 'keeps the user ratio'}`, async ({ page }) => {
    await page.addInitScript(value => {
      localStorage.setItem('tagrex.workspace.v1', JSON.stringify({version:1,left:72,top:138,bottom:190,active:'files',modules:{
        files:{area:'left',index:0,weight:1}, transformations:{area:'right',index:0,weight:value}, inspector:{area:'right',index:1,weight:1},
      }}));
    }, weight);
    await page.goto('http://tagrex.test/app/ui/index.html');
    await expect.poll(() => page.evaluate(() => !!window.tagrex)).toBe(true);
    const height = (await page.locator('#dock-module-transformations').boundingBox()).height;
    expect(height).toBeGreaterThan(expectedMinimum);
    expect(height).toBeLessThan(expectedMaximum);
  });
}

test('bottom splitter follows arrow direction and persists the resulting height', async ({ page }) => {
  await moduleMenu(page, 'Files');
  await page.getByRole('menuitem', { name: 'Dock bottom', exact: true }).click();
  const splitter = page.getByRole('separator', { name: 'Resize bottom modules', exact: true });
  await expect(splitter).toBeVisible();
  const before = await splitter.boundingBox();
  const initialHeight = (await page.locator('[data-dock-host="bottom"]').boundingBox()).height;
  await splitter.focus();
  await page.keyboard.press('ArrowDown');
  const afterDown = await splitter.boundingBox();
  expect(afterDown.y).toBeGreaterThan(before.y + 10);
  expect((await page.locator('[data-dock-host="bottom"]').boundingBox()).height).toBeLessThan(initialHeight - 10);
  await page.keyboard.press('ArrowUp');
  expect((await splitter.boundingBox()).y).toBeCloseTo(before.y, 0);
  await page.keyboard.press('ArrowDown');
  const saved = await splitter.getAttribute('aria-valuenow');
  await page.reload();
  await expect(page.getByRole('separator', { name: 'Resize bottom modules', exact: true })).toHaveAttribute('aria-valuenow', saved);
});
