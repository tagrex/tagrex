import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';

const root=resolve('.test-dist');
const families={inter:'TagRex Inter',plex:'TagRex Sans',source:'TagRex Source Sans',mono:'TagRex Mono'};
test.use({viewport:{width:1440,height:1000},reducedMotion:'reduce'});
test.beforeEach(async({page})=>{
  const errors=[];page.on('pageerror',e=>errors.push(e.message));page.visualErrors=errors;
  await page.route('http://tagrex.test/**',async route=>{
    const path=resolve(root,'.'+new URL(route.request().url()).pathname);
    if(!path.startsWith(root+sep))return route.abort();
    try{await route.fulfill({body:await readFile(path),contentType:path.endsWith('.js')?'text/javascript':path.endsWith('.css')?'text/css':path.endsWith('.woff2')?'font/woff2':'text/html'});}
    catch{await route.fulfill({status:404,body:'Missing fixture'});}
  });
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(()=>page.evaluate(()=>typeof window.tagrex?.openLibrary)).toBe('function');
  expect(await page.evaluate(()=>!!window.__TAURI__)).toBe(false);
  await page.locator('#root-display').click();await page.locator('#root').fill('/music');await page.locator('#root').press('Enter');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
});
test.afterEach(async({page})=>expect(page.visualErrors).toEqual([]));

test('all four interface families load locally, persist live through Cancel/reload, and keep data preferences independent',async({page})=>{
  const requests=[];page.on('request',r=>{if(/\.woff2(?:\?|$)/.test(r.url()))requests.push(r.url());});
  await page.locator('#settings-open').click();
  await expect(page.locator('#set-interface-font option')).toHaveCount(4);
  for(const [key,family] of Object.entries(families)) {
    await page.locator('#set-interface-font-control').click();
    await page.getByRole('option',{name:{inter:'Inter',plex:'IBM Plex Sans',source:'Source Sans 3',mono:'JetBrains Mono'}[key],exact:true}).click();
    await expect(page.locator('html')).toHaveAttribute('data-ui-font',key);
    await page.evaluate(async name=>{await document.fonts.load(`500 13px "${name}"`);},family);
    expect(await page.evaluate(name=>[...document.fonts].some(f=>f.family.replaceAll('"','')===name&&f.status==='loaded'),family)).toBe(true);
    expect(await page.locator('.settings-title').evaluate(node=>getComputedStyle(node).fontFamily)).toContain(family);
    expect(await page.evaluate(()=>localStorage.getItem('tagrex.valueFont'))).toBe('sans');
  }
  await page.locator('#settings-cancel').click();
  await expect(page.locator('html')).toHaveAttribute('data-ui-font','mono');
  await page.reload();
  await expect.poll(()=>page.evaluate(()=>!!window.tagrex)).toBe(true);
  await expect(page.locator('html')).toHaveAttribute('data-ui-font','mono');
  expect(requests.every(url=>url.startsWith('http://tagrex.test/'))).toBe(true);
});

test('a saved condensed data face stays independent while masks retain literal Mono',async({page})=>{
  await page.locator('#settings-open').click();
  await page.locator('#settings-search').fill('Data font');
  await page.locator('#set-value-font [data-value-font="condensed"]').click();
  await page.locator('#settings-cancel').click();
  expect(await page.locator('#tracks-body td[data-field="title"]').first().evaluate(node=>getComputedStyle(node).fontFamily)).toContain('TagRex Condensed');
  await page.locator('#settings-open').click();
  await page.locator('#settings-search').fill('Interface font');
  await page.locator('#set-interface-font').evaluate(node=>{node.value='source';node.dispatchEvent(new Event('change',{bubbles:true}));});
  await page.locator('#settings-cancel').click();
  expect(await page.locator('#tracks-body td[data-field="title"]').first().evaluate(node=>getComputedStyle(node).fontFamily)).toContain('TagRex Condensed');
  await page.locator('.subtab[data-subtab="filename"]').click();
  expect(await page.locator('#from-name-mask').evaluate(node=>getComputedStyle(node).fontFamily)).toContain('TagRex Mono');
  await page.reload();
  await expect.poll(()=>page.evaluate(()=>!!window.tagrex)).toBe(true);
  expect(await page.evaluate(()=>[localStorage.getItem('tagrex.interfaceFont'),localStorage.getItem('tagrex.valueFont')])).toEqual(['source','condensed']);
});

for(const font of Object.keys(families)) {
  test(`${font} keeps all tool modes reachable in a narrow window`,async({page})=>{
    await page.setViewportSize({width:375,height:750});
    await page.locator('#settings-open').click();
    await page.locator('#set-interface-font').evaluate((node,value)=>{node.value=value;node.dispatchEvent(new Event('change',{bubbles:true}));},font);
    await page.locator('#settings-cancel').click();
    for(const mode of ['tagger','renamer','generator','deduplicator','exporter']) {
      await page.locator(`.mode-tab[data-mode="${mode}"]`).click();
      await expect(page.locator(`#panel-${mode}`)).toBeVisible();
      expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1)).toBe(true);
    }
    await page.locator('.mode-tab[data-mode="tagger"]').click();
    for(const tab of ['online','editor','filename','fromtext']) {
      await page.locator(`.subtab[data-subtab="${tab}"]`).click();
      expect(await page.locator('.mode-col').evaluate(node=>node.scrollWidth<=node.clientWidth+1)).toBe(true);
    }
  });
}

test('compact Apply rail appears without resizing the file table and does not reserve space when empty',async({page})=>{
  await page.setViewportSize({width:1024,height:800});
  await page.locator('.subtab[data-subtab="editor"]').click();
  const before=await page.locator('#files-view').boundingBox();
  const idle=await page.locator('.statusbar').boundingBox();
  expect(idle.height).toBeLessThanOrEqual(52);
  await page.locator('#editor-field-title').fill('Footer geometry fixture');
  await expect(page.locator('#diff-apply')).toBeEnabled();
  const after=await page.locator('#files-view').boundingBox();
  expect(Math.abs(before.height-after.height)).toBeLessThan(2);
  await expect(page.locator('#diff-apply')).toBeInViewport({ratio:1});
  await expect(page.locator('#diff-discard')).toBeInViewport({ratio:1});
});

for(const font of ['inter','mono']) {
  test(`${font} Track/Disc pairs keep readable content widths in normal and narrow docks`,async({page})=>{
    await page.locator('#settings-open').click();
    await page.locator('#set-interface-font').evaluate((node,value)=>{node.value=value;node.dispatchEvent(new Event('change',{bubbles:true}));},font);
    await page.locator('#settings-cancel').click();
    await page.locator('#tracks-body tr[data-path]').first().click();
    await page.locator('.subtab[data-subtab="editor"]').click();
    for(const narrow of [false,true]){
      if(narrow){const splitter=page.getByRole('separator',{name:'Resize workspace columns',exact:true});const handle=await splitter.boundingBox();const area=await page.locator('.dock-middle').boundingBox();await page.mouse.move(handle.x+2,handle.y+80);await page.mouse.down();await page.mouse.move(area.x+area.width-245,handle.y+80,{steps:4});await page.mouse.up();}
      const inputs=page.locator('#fields-body .fe-duo .field-input');
      await expect(inputs).toHaveCount(4);
      for(let i=0;i<4;i++){
        const input=inputs.nth(i);await input.fill(i%2?'999':'123');
        // Input clientWidth excludes padding in Chromium 158, unlike 155.
        // Measure the same rendered content box without subtracting it twice.
        expect(await input.evaluate(node=>{
          const s=getComputedStyle(node);
          return node.getBoundingClientRect().width
            -parseFloat(s.borderLeftWidth)-parseFloat(s.borderRightWidth)
            -parseFloat(s.paddingLeft)-parseFloat(s.paddingRight);
        })).toBeGreaterThanOrEqual(24);
        await expect(input).toHaveValue(i%2?'999':'123');
      }
    }
  });
}

test('mask builder and custom column syntax stay Mono with either proportional or condensed data',async({page})=>{
  for(const data of ['sans','condensed']){
    await page.locator('#settings-open').click();await page.locator('#settings-search').fill('Data font');await page.locator(`#set-value-font [data-value-font="${data}"]`).click();await page.locator('#settings-cancel').click();
    await page.locator('.mode-tab[data-mode="renamer"]').click();await page.locator('#mask-builder-open').click();
    await expect(page.locator('#mb-expr')).toBeVisible();expect(await page.locator('#mb-expr').evaluate(n=>getComputedStyle(n).fontFamily)).toContain('TagRex Mono');await page.locator('#mb-close').click();
    await page.locator('#tracks thead th[data-sort="title"]').click({button:'right'});await expect(page.locator('#columns-menu')).toBeVisible();await page.locator('#columns-menu .column-action').first().click();
    await expect(page.locator('#colmask-pattern')).toBeVisible();expect(await page.locator('#colmask-pattern').evaluate(n=>getComputedStyle(n).fontFamily)).toContain('TagRex Mono');await page.locator('#colmask-cancel').click();
  }
});

test('visible catalogue and counts honor the badge face independently from condensed data',async({page})=>{
  await page.locator('#settings-open').click();await page.locator('#settings-search').fill('Data font');await page.locator('#set-value-font [data-value-font="condensed"]').click();await page.locator('#settings-cancel').click();
  await page.locator('.subtab[data-subtab="online"]').click();await page.locator('#discogs-token').evaluate(n=>{n.value='fixture';});await page.locator('#discogs-query').fill('Helter Skelter');await page.locator('#discogs-search').click();
  await expect(page.locator('.release-card')).toHaveCount(5);const facts=page.locator('.release-card').first().locator('.rb-catno, .tk-count');
  await expect(page.locator('.release-card').first().locator('.tk-count')).toContainText('3 tracks');
  for(const [choice,family] of [['mono','TagRex Mono'],['sans','TagRex Inter']]){
    await page.locator('#settings-open').click();await page.locator('#settings-search').fill('Release badge font');await page.locator(`#set-badge-font [data-badge-font="${choice}"]`).click();await page.locator('#settings-cancel').click();
    for(const node of await facts.all())expect(await node.evaluate(n=>getComputedStyle(n).fontFamily)).toContain(family);
  }
  // The historical size token remains a supported CSS consumer, even though no
  // separate badge-size preference is currently exposed in Settings.
  for(const px of [11,16]){await page.evaluate(v=>document.documentElement.style.setProperty('--badge-font-size',v+'px'),px);for(const node of await facts.all())expect(await node.evaluate(n=>getComputedStyle(n).fontSize)).toBe(px+'px');}
});
