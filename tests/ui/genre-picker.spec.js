import { test, expect } from '@playwright/test';
import { readFile, mkdir } from 'node:fs/promises';
import { resolve, sep } from 'node:path';
const root = resolve('.test-dist');
test.use({ viewport: { width:1440, height:900 }, reducedMotion:'reduce' });
const input = page => page.locator('#fields-body input[data-key="genre"]');
const toggle = page => page.locator('.genre-toggle');

test.beforeEach(async ({page}) => {
  const errors=[]; page.on('pageerror', error=>errors.push(error.message)); page.__genreErrors=errors;
  await page.route('http://tagrex.test/**',async route=>{
    const path=resolve(root,'.'+new URL(route.request().url()).pathname);
    if(!path.startsWith(root+sep)) return route.abort();
    try { await route.fulfill({body:await readFile(path), contentType:path.endsWith('.js')?'text/javascript':path.endsWith('.css')?'text/css':path.endsWith('.woff2')?'font/woff2':'text/html'}); }
    catch { await route.fulfill({status:404,body:'Missing fixture'}); }
  });
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(()=>page.evaluate(()=>typeof window.tagrex?.openLibrary)).toBe('function');
  await page.locator('#root-display').click(); await page.locator('#root').fill('/music'); await page.locator('#root').press('Enter');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
  await page.locator('.subtab[data-subtab="editor"]').click();
  await expect(input(page)).toHaveAttribute('role','combobox');
});
test.afterEach(async({page})=>expect(page.__genreErrors).toEqual([]));

test('genre dropdown keeps the existing input, known options and custom typing without writing files',async({page})=>{
  await input(page).evaluate(node=>{window.genreOriginal=node;});
  await toggle(page).click();
  await expect(page.getByRole('listbox',{name:'Genre',exact:true})).toBeVisible();
  await expect(input(page)).toBeFocused();
  await expect(page.getByRole('option',{name:'Happy Hardcore',exact:true})).toBeVisible();
  if (process.env.ITERATION_CAPTURE_EVIDENCE === '1') {
    await mkdir('docs/ui-iteration',{recursive:true});
    await page.screenshot({path:'docs/ui-iteration/editor-genre-options.png'});
  }
  await page.getByRole('option',{name:'Happy Hardcore',exact:true}).click();
  await expect(input(page)).toHaveValue('Happy Hardcore');
  await expect(input(page)).toBeFocused();
  expect(await input(page).evaluate(node=>node===window.genreOriginal)).toBe(true);
  await input(page).fill('My entirely custom genre');
  await expect(input(page)).toHaveValue('My entirely custom genre');
  expect(await page.evaluate(()=>window.tagrex.tracks[0].tags.genre||'')).not.toBe('My entirely custom genre');
  await expect.poll(()=>page.evaluate(()=>window.tagrex.previewPlan?.changes.some(change=>change.tag_changes.some(field=>field.field==='genre'&&field.new==='My entirely custom genre')))).toBe(true);
});

test('keyboard suggestions only accept an explicitly highlighted option and keep existing semicolon values',async({page})=>{
  await input(page).fill('Rock; Ho');
  await expect(page.getByRole('option',{name:'House',exact:true})).toBeVisible();
  await expect(page.getByRole('option',{name:'Ho',exact:true})).toHaveCount(0);
  await input(page).press('Enter');
  await expect(input(page)).toHaveValue('Rock; Ho');
  await input(page).press('ArrowDown');
  await input(page).press('Enter');
  await expect(input(page)).toHaveValue('Rock; House');
  await expect(input(page)).toHaveAttribute('aria-expanded','false');
});

test('choosing a token in the middle preserves later genres and literal slashes',async({page})=>{
  await input(page).fill('Pop/Rock; Ho; Soul');
  await input(page).evaluate(node=>{node.setSelectionRange(12,12);node.dispatchEvent(new MouseEvent('click',{bubbles:true}));});
  await expect(page.getByRole('option',{name:'House',exact:true})).toBeVisible();
  await page.getByRole('option',{name:'House',exact:true}).click();
  await expect(input(page)).toHaveValue('Pop/Rock; House; Soul');
});

test('library-specific genres are offered once opened, then filtered without rescanning the library',async({page})=>{
  await page.evaluate(()=>window.tagrex.tracks[1].tags.genre='Only In My Library; Pop/Rock');
  await toggle(page).click();
  await expect(page.getByRole('option',{name:'Only In My Library',exact:true})).toBeVisible();
  await input(page).fill('Only');
  await page.getByRole('option',{name:'Only In My Library',exact:true}).click();
  await expect(input(page)).toHaveValue('Only In My Library');
});

test('Escape closes suggestions while preserving draft and editor, and selection change destroys the old popup',async({page})=>{
  await input(page).fill('Tech');
  await expect(input(page)).toHaveAttribute('aria-expanded','true');
  await input(page).press('Escape');
  await expect(input(page)).toHaveValue('Tech');
  await expect(input(page)).toHaveAttribute('aria-expanded','false');
  await expect(page.locator('#subtab-editor')).toBeVisible();
  await toggle(page).click();
  await page.locator('#tracks-body tr[data-path] .file').nth(1).click();
  await expect(page.getByRole('listbox',{name:'Genre',exact:true})).toHaveCount(0);
  await expect(input(page)).not.toHaveValue('Tech');
});

test('genre suggestions and field fit a narrow dock and locked fields cannot open it',async({page})=>{
  await page.setViewportSize({width:375,height:750});
  await page.getByRole('tab',{name:'Inspector',exact:true}).click();
  await input(page).scrollIntoViewIfNeeded();
  await toggle(page).click();
  const menu=page.locator('.genre-options'); await expect(menu).toBeVisible();
  const box=await menu.boundingBox(); expect(box.x).toBeGreaterThanOrEqual(0); expect(box.x+box.width).toBeLessThanOrEqual(376);
  await input(page).press('Escape');
  await page.locator('.fe-row').filter({has:page.locator('input[data-key="genre"]')}).locator('.fe-lock').click();
  await expect(input(page)).toBeDisabled(); await expect(toggle(page)).toBeDisabled();
});
