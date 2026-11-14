import { test,expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve,sep } from 'node:path';
const root=resolve('.test-dist');
test.use({viewport:{width:900,height:800}});
test.beforeEach(async({page})=>{
  page.__tableErrors=[];page.on('pageerror',error=>page.__tableErrors.push(error.message));
  await page.route('http://tagrex.test/**',async route=>{
    const path=resolve(root,'.'+new URL(route.request().url()).pathname);
    if(!path.startsWith(root+sep))return route.abort();
    try{await route.fulfill({body:await readFile(path),contentType:path.endsWith('.js')?'text/javascript':path.endsWith('.css')?'text/css':path.endsWith('.woff2')?'font/woff2':'text/html'});}
    catch{await route.fulfill({status:404,body:'Missing fixture'});}
  });
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(()=>page.evaluate(()=>typeof window.tagrex?.openLibrary)).toBe('function');
  await page.locator('#root-display').click();await page.locator('#root').fill('/music');await page.locator('#root').press('Enter');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
  expect(await page.locator('#files-view').evaluate(node=>node.scrollWidth-node.clientWidth)).toBeGreaterThan(200);
});
test.afterEach(async({page})=>expect(page.__tableErrors).toEqual([]));
const x=page=>page.locator('#files-view').evaluate(node=>node.scrollLeft);
async function wheel(page,options){return page.locator('#files-view').evaluate((node,options)=>{const event=new WheelEvent('wheel',{bubbles:true,cancelable:true,...options});node.dispatchEvent(event);return {prevented:event.defaultPrevented,x:node.scrollLeft};},options);}

test('Filename scrolls with metadata while the small artwork column stays pinned',async({page})=>{
  const file=page.locator('#tracks-body tr[data-path] td.file').first(),art=page.locator('#tracks-body tr[data-path] td.artwork-col').first();
  const before=await file.boundingBox(),artBefore=await art.boundingBox();
  await page.locator('#files-view').evaluate(node=>node.scrollLeft=150);
  expect((await file.boundingBox()).x).toBeCloseTo(before.x-150,0);
  expect((await art.boundingBox()).x).toBeCloseTo(artBefore.x,0);
  expect(await file.evaluate(node=>getComputedStyle(node).position)).not.toBe('sticky');
});

test('native pixel-wheel movement remains proportional and vertical wheels are untouched',async({page})=>{
  const ignored=await wheel(page,{deltaY:100,deltaMode:0});expect(ignored.prevented).toBe(false);expect(ignored.x).toBe(0);
  await page.locator('#files-view').hover({position:{x:150,y:100}});await page.mouse.wheel(100,0);
  await expect.poll(()=>x(page)).toBeCloseTo(100,0);
});

test('Shift-wheel starts immediately, settles quickly and reversals replace the target',async({page})=>{
  await page.emulateMedia({reducedMotion:'no-preference'});
  const first=await wheel(page,{deltaY:100,shiftKey:true});expect(first.prevented).toBe(true);expect(first.x).toBeGreaterThan(40);expect(first.x).toBeLessThan(100);
  await expect.poll(()=>x(page)).toBeCloseTo(100,0);
  await page.locator('#files-view').evaluate(node=>{
    node.dispatchEvent(new WheelEvent('wheel',{deltaY:100,shiftKey:true,bubbles:true,cancelable:true}));
    node.dispatchEvent(new WheelEvent('wheel',{deltaY:-100,shiftKey:true,bubbles:true,cancelable:true}));
  });
  await expect.poll(()=>x(page)).toBeCloseTo(100,0);
});

test('pointer interaction cancels the settling tail and reduced motion is immediate',async({page})=>{
  await page.emulateMedia({reducedMotion:'no-preference'});
  const stopped=await page.locator('#files-view').evaluate(node=>{
    node.dispatchEvent(new WheelEvent('wheel',{deltaY:100,shiftKey:true,bubbles:true,cancelable:true}));
    node.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true}));return node.scrollLeft;
  });
  await page.waitForTimeout(100);expect(await x(page)).toBeCloseTo(stopped,0);
  await page.emulateMedia({reducedMotion:'reduce'});
  const next=await wheel(page,{deltaY:80,shiftKey:true});expect(next.x-stopped).toBeCloseTo(80,0);
});

test('horizontal motion retains rows and group labels stay legible and inside the viewport',async({page})=>{
  await page.locator('#tracks-body').evaluate(node=>{
    window.tableScrollMutations=0;window.firstScrollRow=node.querySelector('tr[data-path]');
    const observer=new MutationObserver(records=>{window.tableScrollMutations+=records.filter(record=>record.type==='childList').length;});observer.observe(node,{childList:true,subtree:true});
  });
  await wheel(page,{deltaY:140,shiftKey:true});await expect.poll(()=>x(page)).toBeCloseTo(140,0);
  expect(await page.evaluate(()=>window.firstScrollRow===document.querySelector('#tracks-body tr[data-path]'))).toBe(true);
  expect(await page.evaluate(()=>window.tableScrollMutations)).toBe(0);
  const band=page.locator('#tracks-body .group-heading-content').first();await expect(band).toBeInViewport();
  expect(await page.locator('#tracks-body .group-label').first().evaluate(node=>getComputedStyle(node).fontWeight)).toBe('500');
  expect(await page.locator('#tracks-body .group-label').first().evaluate(node=>getComputedStyle(node).color)).toBe(await page.evaluate(()=>getComputedStyle(document.body).color));
});
