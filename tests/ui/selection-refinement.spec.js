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

test('primary pointer down immediately selects and highlights a row without waiting for release',async({page})=>{
  const cell=rows(page).nth(1).locator('td.file');const box=await cell.boundingBox();await page.mouse.move(box.x+80,box.y+box.height/2);await page.mouse.down();
  expect(await rows(page).nth(1).evaluate(node=>node.classList.contains('selected')&&window.tagrex.selection.has(node.dataset.path))).toBe(true);
  await page.mouse.up();
});
test('highlight is the only row selection indicator including live previews',async({page})=>{
  await editor(page);await preview(page,'No checkboxes');
  expect(await rows(page).locator('input[type="checkbox"]').count()).toBe(0);
  await expect(rows(page).first()).toHaveClass(/selected/);
});
test('comparison belongs to Staged and stays enabled across drafts',async({page})=>{
  await editor(page);await preview(page,'First preview');await page.locator('#diff-preview').click();await expect(page.locator('#tracks')).toHaveClass(/show-old/);
  await page.locator('#diff-discard').click();await preview(page,'Second preview');await expect(page.locator('#tracks')).toHaveClass(/show-old/);
  await page.locator('#files-tab').click();await expect(page.locator('#tracks')).not.toHaveClass(/show-old/);
});
test('right-clicking a standard column header opens the column chooser',async({page})=>{
  await page.locator('#tracks thead th[data-sort="title"]').click({button:'right'});await expect(page.locator('#columns-menu')).toBeVisible();
});
test('clicking a group label collapses the group without changing track selection',async({page})=>{
  const before=await page.evaluate(()=>[...window.tagrex.selection]);const group=page.locator('#tracks-body tr.group-head').first();await group.locator('.group-label').click();await expect(group).toHaveClass(/collapsed/);expect(await page.evaluate(()=>[...window.tagrex.selection])).toEqual(before);
});
test('selection handler baseline records synchronous click work with Editor visible',async({page})=>{
  await editor(page);
  const result=await page.evaluate(()=>{const times=[];for(let i=0;i<30;i++){const row=document.querySelectorAll('#tracks-body tr[data-path]')[i%3];const start=performance.now();row.querySelector('td.file').dispatchEvent(new MouseEvent('click',{bubbles:true}));times.push(performance.now()-start);}times.sort((a,b)=>a-b);return{median:times[15],p95:times[28],max:times[29]};});
  console.log('SELECTION_HANDLER_MS',JSON.stringify(result));
});
