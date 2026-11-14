import {test,expect} from '@playwright/test';
import {readFile} from 'node:fs/promises';
import {resolve,sep} from 'node:path';
const root=resolve('.test-dist');
test.beforeEach(async({page})=>{
  page.__selectionErrors=[];page.on('pageerror',error=>page.__selectionErrors.push(error.message));
  await page.route('http://tagrex.test/**',async route=>{
    const path=resolve(root,'.'+new URL(route.request().url()).pathname);if(!path.startsWith(root+sep))return route.abort();
    try{await route.fulfill({body:await readFile(path),contentType:path.endsWith('.js')?'text/javascript':path.endsWith('.css')?'text/css':path.endsWith('.woff2')?'font/woff2':'text/html'});}catch{await route.fulfill({status:404,body:'Missing fixture'});}
  });
  await page.goto('http://tagrex.test/app/ui/index.html');await expect.poll(()=>page.evaluate(()=>typeof window.tagrex?.openLibrary)).toBe('function');
  await page.locator('#root-display').click();await page.locator('#root').fill('/music');await page.locator('#root').press('Enter');await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
});
test.afterEach(async({page})=>expect(page.__selectionErrors).toEqual([]));
const rows=page=>page.locator('#tracks-body tr[data-path]');
async function editor(page){await page.locator('.subtab[data-subtab="editor"]').click();}
async function preview(page,text){await page.locator('#fields-body [data-key="title"]').fill(text);await expect(page.locator('#diff-apply')).toBeEnabled();}

async function pack(page,count=200) {
  await page.evaluate(async count=>{await window.tagrex.invoke('open_drop',{paths:[...Array.from({length:count},(_,i)=>`A/track-${String(i).padStart(3,'0')}.mp3`),'B/other.mp3']});document.getElementById('root').value='/dropped';await window.tagrex.openLibrary();},count);
}
test('Shift press interrupted by blur completes the latest Editor subject',async({page})=>{
  await editor(page);const next=rows(page).nth(1);
  await next.evaluate(row=>{row.querySelector('td.file').dispatchEvent(new MouseEvent('mousedown',{button:0,detail:1,shiftKey:true,bubbles:true,cancelable:true,clientX:row.getBoundingClientRect().left+60,clientY:row.getBoundingClientRect().top+8}));window.dispatchEvent(new Event('blur'));});
  await expect(page.locator('#fields-body')).not.toHaveAttribute('inert','');await expect(page.locator('#fields-body')).not.toHaveAttribute('aria-busy','true');
  await expect(page.locator('#fields-body [data-key="title"]')).toHaveValue('Radio');
});
test('group selection includes collapsed offscreen members and mixed Ctrl-click only adds missing members',async({page})=>{
  await pack(page);const group=page.locator('tr.group-head').first(),label=group.locator('.group-label');
  await label.click();await expect(group).toHaveClass(/collapsed/);const other=await rows(page).first().getAttribute('data-path');await rows(page).first().locator('td.file').click();
  await page.evaluate(()=>{window.tagrex.selection.add('/dropped/A/track-000.mp3');window.tagrex.renderTracks();});
  await label.click({modifiers:['Control']});expect(await page.evaluate(()=>window.tagrex.selection.size)).toBe(201);expect(await page.evaluate(path=>window.tagrex.selection.has(path),other)).toBe(true);await expect(group).toHaveClass(/collapsed/);
  await label.click({modifiers:['Control']});expect(await page.evaluate(()=>[...window.tagrex.selection])).toEqual([other]);
  await label.click({button:'right'});expect(await page.evaluate(()=>window.tagrex.selection.size)).toBe(200);expect(await page.evaluate(path=>window.tagrex.selection.has(path),other)).toBe(false);
});
test('every click in a rapid group-header burst reverses the state without changing selection',async({page})=>{
  const before=await page.evaluate(()=>[...window.tagrex.selection]);
  await page.evaluate(()=>{window.groupClickStates=[];document.addEventListener('click',event=>{if(event.target.closest('tr.group-head'))window.groupClickStates.push(document.querySelector('tr.group-head').classList.contains('collapsed'));});});
  const group=page.locator('tr.group-head').first();await group.locator('.group-label').dblclick();
  expect(await page.evaluate(()=>window.groupClickStates)).toEqual([true,false]);
  await group.locator('.group-label').click({clickCount:3});await group.locator('.group-label').click({clickCount:4});
  expect(await page.evaluate(()=>window.groupClickStates)).toEqual([true,false,true,false,true,false,true,false,true]);
  await expect(group).toHaveClass(/collapsed/);expect(await page.evaluate(()=>[...window.tagrex.selection])).toEqual(before);
});
test('interrupting group motion repeatedly does not rebuild exit snapshots or queue row animations',async({page})=>{
  await pack(page,2000);await page.emulateMedia({reducedMotion:'no-preference'});
  const result=await page.evaluate(()=>{
    let clones=0,players=0;const clone=Element.prototype.cloneNode,animate=Element.prototype.animate;
    Element.prototype.cloneNode=function(...args){if(this.matches?.('#tracks-body tr[data-path]'))clones++;return clone.apply(this,args);};
    Element.prototype.animate=function(...args){players++;return animate.apply(this,args);};
    const clicks=[];
    for(let i=0;i<8;i++){
      const label=document.querySelector('tr.group-head .group-label');label.dispatchEvent(new MouseEvent('click',{button:0,detail:i+1,bubbles:true,cancelable:true}));
      clicks.push({collapsed:document.querySelector('tr.group-head').classList.contains('collapsed'),clones,players});
    }
    return{clicks,ghosts:document.querySelectorAll('.group-motion-exit').length,active:document.getAnimations().filter(animation=>animation.playState==='running' && animation.effect?.target?.closest?.('#tracks,.group-motion-exit')).length};
  });
  expect(result.clicks.map(click=>click.collapsed)).toEqual([true,false,true,false,true,false,true,false]);
  expect(result.clicks.at(-1).clones).toBe(result.clicks[0].clones);expect(result.clicks.at(-1).players-result.clicks[0].players).toBeLessThanOrEqual(7);
  expect(result.ghosts).toBe(0);expect(result.active).toBeLessThanOrEqual(1);
});
test('mouse Shift-click ranges tracks while Shift-arrows range cells',async({page})=>{
  const all=await page.evaluate(()=>window.tagrex.tracks.map(track=>track.path));await rows(page).first().locator('td[data-field="artist"]').click();await rows(page).last().locator('td[data-field="artist"]').click({modifiers:['Shift']});expect(await page.evaluate(()=>[...window.tagrex.selection])).toEqual(all);await expect(page.locator('td.sheet-range')).toHaveCount(1);
  await rows(page).first().locator('td[data-field="artist"]').click();await page.keyboard.press('Shift+ArrowRight');await page.keyboard.press('Shift+ArrowDown');await expect(page.locator('td.sheet-range')).toHaveCount(4);expect(await page.evaluate(()=>window.tagrex.selection.size)).toBe(1);
});
test('header chooser Escape closes only the menu, returns focus and preserves a pending preview',async({page})=>{
  await editor(page);await preview(page,'Keep this preview');const header=page.locator('th[data-sort="title"]');await header.click({button:'right'});await expect(page.locator('#columns-menu')).toBeVisible();await page.keyboard.press('Escape');await expect(page.locator('#columns-menu')).toBeHidden();await expect(header).toBeFocused();await expect(page.locator('#diff-apply')).toBeEnabled();
});
test('another context action replaces the column chooser and keyboard can reopen it',async({page})=>{
  const header=page.locator('th[data-sort="title"]');await header.click({button:'right'});await expect(page.locator('#columns-menu')).toBeVisible();await rows(page).first().locator('td.file').click({button:'right'});await expect(page.locator('#columns-menu')).toBeHidden();await expect(page.locator('#file-ctx')).toBeVisible();await page.keyboard.press('Escape');await header.focus();await page.keyboard.press('Shift+F10');await expect(page.locator('#columns-menu')).toBeVisible();
});
test('group motion is bounded to the viewport, cancellable and absent with reduced motion',async({page})=>{
  await pack(page,2000);await page.emulateMedia({reducedMotion:'no-preference'});
  await page.evaluate(()=>{window.groupAnimations=[];const original=Element.prototype.animate;Element.prototype.animate=function(frames,options){if(this.closest('#tracks')||this.classList.contains('group-motion-exit'))window.groupAnimations.push({duration:options.duration});return original.call(this,frames,options);};});
  const group=page.locator('tr.group-head').first();await group.locator('.group-label').click();await expect(group).toHaveClass(/collapsed/);expect(await page.evaluate(()=>window.groupAnimations.length)).toBeGreaterThan(0);expect(await page.evaluate(()=>window.groupAnimations.length)).toBeLessThanOrEqual(81);expect(await page.evaluate(()=>window.groupAnimations.every(animation=>animation.duration<=140))).toBe(true);expect(await page.locator('.group-motion-exit tr').count()).toBeLessThanOrEqual(80);
  await expect(page.locator('.group-motion-exit')).toHaveCount(0);await page.evaluate(()=>window.groupAnimations=[]);await group.locator('.group-label').click();expect(await page.evaluate(()=>window.groupAnimations.length)).toBeLessThanOrEqual(9);
  await group.locator('.group-label').click();await page.emulateMedia({reducedMotion:'reduce'});await page.evaluate(()=>window.groupAnimations=[]);await group.locator('.group-label').click();expect(await page.evaluate(()=>window.groupAnimations.length)).toBe(0);await expect(page.locator('.group-motion-exit')).toHaveCount(0);expect(await rows(page).count()).toBeLessThan(100);
});

test('Escape from a marquee restores the previous Shift-range anchor as well as selection',async({page})=>{
  await rows(page).last().locator('td.file').click();const first=await rows(page).first().locator('td.file').boundingBox(),second=await rows(page).nth(1).locator('td.file').boundingBox();await page.mouse.move(first.x+60,first.y+first.height/2);await page.mouse.down();await page.mouse.move(second.x+90,second.y+second.height/2,{steps:5});await page.keyboard.press('Escape');await page.mouse.up();await rows(page).nth(1).locator('td.file').click({modifiers:['Shift']});const all=await page.evaluate(()=>window.tagrex.tracks.map(track=>track.path));expect(new Set(await page.evaluate(()=>[...window.tagrex.selection]))).toEqual(new Set(all.slice(1)));
});

for(const width of [375,900,1800])test(`column chooser stays compact and inside a ${width}px window`,async({page})=>{
  await page.setViewportSize({width,height:800});const header=page.locator('th[data-sort="file"]');await header.click({button:'right'});const menu=page.locator('#columns-menu');await expect(menu).toBeVisible();const box=await menu.boundingBox();expect(box.width).toBeLessThanOrEqual(280);expect(box.x).toBeGreaterThanOrEqual(0);expect(box.x+box.width).toBeLessThanOrEqual(width+1);expect(box.y+box.height).toBeLessThanOrEqual(793);expect(box.height).toBeGreaterThan(500);await page.keyboard.press('Escape');
});
test('group selection cannot send F2 to a previously focused cell in another group',async({page})=>{
  await pack(page,2);await rows(page).last().locator('td[data-field="artist"]').click();const group=page.locator('tr.group-head').first();await group.locator('.group-label').click({button:'right'});await group.focus();await page.keyboard.press('F2');await expect(page.locator('[contenteditable="true"]')).toHaveCount(0);expect(await page.evaluate(()=>[...window.tagrex.selection].every(path=>path.startsWith('/dropped/A/')))).toBe(true);
});

test('column chooser uses available height and scrolls its quiet list inside a fixed header and button footer',async({page})=>{
  await page.setViewportSize({width:1280,height:500});
  await page.locator('th[data-sort="file"]').click({button:'right'});
  const menu=page.locator('#columns-menu'),surface=menu.locator('.controller-popover-content'),list=menu.locator('.column-options'),fit=menu.locator('.column-fit-action'),footer=menu.locator('.col-menu-actions');
  await expect(menu).toBeVisible();
  await expect.poll(()=>surface.evaluate(node=>node.getAnimations().filter(animation=>animation.playState==='running').length)).toBe(0);
  const box=await surface.boundingBox(),listBox=await list.boundingBox(),fitBox=await fit.boundingBox(),footerBox=await footer.boundingBox();
  expect(box.height).toBeGreaterThanOrEqual(280);expect(box.y+box.height).toBeLessThanOrEqual(493);
  expect(fitBox.y+fitBox.height).toBeLessThan(listBox.y);expect(listBox.y+listBox.height).toBeLessThanOrEqual(footerBox.y);
  expect(listBox.x+listBox.width).toBeLessThan(box.x+box.width);
  await expect(footer.getByRole('button',{name:'Add',exact:true})).toBeVisible();await expect(footer.getByRole('button',{name:'Reset',exact:true})).toBeVisible();
  expect(await list.evaluate(node=>node.scrollHeight>node.clientHeight)).toBe(true);
  expect(await list.evaluate(node=>getComputedStyle(node).scrollbarColor)).toBe('rgba(0, 0, 0, 0) rgba(0, 0, 0, 0)');
  await list.evaluate(node=>{node.scrollTop=200;});await expect(list).toHaveClass(/is-scrolling/);
  expect((await footer.boundingBox()).y).toBeCloseTo(footerBox.y,0);expect((await fit.boundingBox()).y).toBeCloseTo(fitBox.y,0);
  await expect(list).not.toHaveClass(/is-scrolling/);
  const hidden=list.locator('.col-menu-row.is-hidden').first(),key=await hidden.getAttribute('data-key');
  const hiddenColor=await hidden.evaluate(node=>getComputedStyle(node).color),visibleColor=await list.locator('.col-menu-row:not(.is-hidden)').first().evaluate(node=>getComputedStyle(node).color);
  expect(hiddenColor).not.toBe(visibleColor);
  await hidden.locator('.col-menu-label').click();await expect(list.locator(`.col-menu-row[data-key="${key}"]`)).not.toHaveClass(/is-hidden/);
  await expect.poll(()=>menu.evaluate(node=>node.hidden)).toBe(false);
  await page.setViewportSize({width:600,height:390});
  await expect.poll(async()=>{const rect=await surface.boundingBox();return rect ? rect.y+rect.height : Infinity;}).toBeLessThanOrEqual(383);
  await expect(footer.getByRole('button',{name:'Reset',exact:true})).toBeVisible();
  const toggled=list.locator(`.col-menu-row[data-key="${key}"]`).getByRole('checkbox');
  await toggled.focus();await page.keyboard.press('Space');
  await expect(list.locator(`.col-menu-row[data-key="${key}"]`).getByRole('checkbox')).toBeFocused();
  await expect.poll(()=>menu.evaluate(node=>node.hidden)).toBe(false);
});

async function manualPlan(page,count=1201) {
  await page.evaluate(async count=>{
    const paths=Array.from({length:count},(_,i)=>`${i<Math.ceil(count/2)?'A':'B'}/track-${String(i).padStart(4,'0')}.mp3`);
    await window.tagrex.invoke('open_drop',{paths});document.getElementById('root').value='/dropped';await window.tagrex.openLibrary();
  },count);
  await rows(page).first().locator('td[data-field="artist"]').click();
  await page.evaluate(count=>{const data=new DataTransfer();data.setData('text/plain',Array(count).fill('New artist').join('\n'));document.activeElement.dispatchEvent(new ClipboardEvent('paste',{bubbles:true,cancelable:true,clipboardData:data}));},count);
  await expect(page.locator('#diff-apply')).toBeEnabled();
}
test('manual-review group selection keeps the whole group when a Shift range extends it',async({page})=>{
  await manualPlan(page,6);const groups=page.locator('tr.group-head');
  await groups.nth(1).locator('.group-label').click({button:'right'});await expect(page.locator('tr[data-path].selected')).toHaveCount(3);
  await rows(page).first().locator('td.file').click({modifiers:['Shift']});await expect(page.locator('tr[data-path].selected')).toHaveCount(6);
});
test('switching Staged comparison recalibrates the entire virtual scroll extent',async({page})=>{
  await manualPlan(page);const before=await page.locator('#files-view').evaluate(node=>node.scrollHeight);
  await page.locator('#staged-tab').click();const after=await page.locator('#files-view').evaluate(node=>node.scrollHeight);expect(after).toBeGreaterThan(before*1.5);
  await page.locator('#files-tab').click();expect(await page.locator('#files-view').evaluate(node=>node.scrollHeight)).toBeLessThan(after*.75);
});
test('a plain empty-space click preserves the highlighted manual Apply scope',async({page})=>{
  await manualPlan(page,6);const box=await page.locator('#files-view').boundingBox();await page.mouse.click(box.x+60,box.y+box.height-30);
  await expect(page.locator('tr[data-path].selected')).toHaveCount(6);await expect(page.locator('#diff-apply')).toBeEnabled();
  const last=await rows(page).last().locator('td.file').boundingBox();await page.mouse.move(box.x+60,box.y+box.height-30);await page.mouse.down();await page.mouse.move(last.x+60,last.y+last.height/2,{steps:5});await page.mouse.up();
  await expect(page.locator('tr[data-path].selected')).toHaveCount(1);await expect(page.locator('#diff-apply')).toBeEnabled();
});
