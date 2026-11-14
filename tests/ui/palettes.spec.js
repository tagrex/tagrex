import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';

// Use the same isolated mock app as the existing workspace tests; no media or
// native runtime state is touched by these appearance-only checks.
const root = resolve('.test-dist');
test.beforeEach(async ({ page }) => {
  await page.route('http://tagrex.test/**', async route => {
    const path = resolve(root, '.' + new URL(route.request().url()).pathname);
    if (!path.startsWith(root + sep)) return route.abort();
    try {
      await route.fulfill({ body: await readFile(path), contentType: path.endsWith('.js') ? 'text/javascript' : path.endsWith('.css') ? 'text/css' : path.endsWith('.woff2') ? 'font/woff2' : 'text/html' });
    } catch { await route.fulfill({ status: 404, body: 'Missing fixture' }); }
  });
});
async function open(page) {
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(() => page.evaluate(() => typeof window.tagrex?.setMode)).toBe('function');
}
async function settings(page) {
  await page.locator('#settings-open').click();
  await page.locator('#settings-nav-list button').first().click();
}

const themes = [ ['navy','Navy'],['graphite','Graphite'],['cupertino','Cupertino'],['catppuccin','Catppuccin'],['nord','Nord'],['tokyonight','Tokyo Night'],['plum','Rosé Pine'],['gruvbox','Gruvbox'],['solarized','Solarized'],['forest','Everforest'] ];
async function choose(page,id,name) {
  await page.locator('#set-palette-control').click();
  await page.getByRole('option',{name,exact:true}).click();
  await expect(page.locator('html')).toHaveAttribute('data-palette',id);
}
test('one complete theme choice applies its accent and persists through Cancel, Save and reload',async({page})=>{
  await open(page);await settings(page);
  await expect(page.locator('#set-accent')).toHaveCount(0);await expect(page.locator('#set-palette-accent')).toHaveCount(0);
  await choose(page,'nord','Nord');
  expect(await page.evaluate(()=>localStorage.getItem('tagrex.themeAccent'))).toBe('preset');
  const accent=await page.evaluate(()=>document.documentElement.style.getPropertyValue('--accent'));
  await page.locator('#settings-cancel').click();await page.reload();await expect.poll(()=>page.evaluate(()=>!!window.tagrex)).toBe(true);
  await expect(page.locator('html')).toHaveAttribute('data-palette','nord');
  expect(await page.evaluate(()=>document.documentElement.style.getPropertyValue('--accent'))).toBe(accent);
  await settings(page);await choose(page,'catppuccin','Catppuccin');
  expect(await page.evaluate(()=>document.documentElement.style.getPropertyValue('--accent'))).not.toBe(accent);
  await page.locator('#settings-save').click();await page.reload();await expect(page.locator('html')).toHaveAttribute('data-palette','catppuccin');
});
for(const mode of ['light','dark']) test(`all ten theme presets retain shared UI hierarchy in ${mode}`,async({page},testInfo)=>{
  test.setTimeout(90000);
  await page.setViewportSize({width:1280,height:900});await open(page);
  await page.locator('#root-display').click();await page.locator('#root').fill('/music');await page.locator('#root').press('Enter');
  await expect(page.locator('#tracks-body tr[data-path]')).toHaveCount(3);
  for(const [id,name] of themes){
    await settings(page);await page.locator(`#set-theme [data-theme-mode="${mode}"]`).click();await choose(page,id,name);
    await expect(page.locator('#settings-save')).toBeInViewport();await page.locator('#settings-cancel').click();
    await page.locator('.subtab[data-subtab="editor"]').click();
    const geometry=await page.locator('#editor-field-title').evaluate(n=>({height:n.getBoundingClientRect().height,radius:getComputedStyle(n).borderRadius,ink:getComputedStyle(n).color,bg:getComputedStyle(n).backgroundColor}));
    expect(geometry.height).toBe(32);expect(geometry.radius).toBe('10px');expect(geometry.ink).not.toBe(geometry.bg);
    await page.locator('#editor-field-title').fill('Theme preview');await expect(page.locator('#diff-apply')).toBeEnabled();
    await page.locator('#diff-preview').click();await expect(page.locator('#tracks')).toHaveClass(/show-old/);
    await expect.poll(()=>page.evaluate(()=>getComputedStyle(document.getElementById('staged-tab')).backgroundColor===getComputedStyle(document.getElementById('root-field')).backgroundColor)).toBe(true);
    await page.screenshot({path:testInfo.outputPath(`${id}-${mode}.png`),animations:'disabled'});
    await page.locator('#diff-discard').click();await page.locator('#files-tab').click();
    for(const panel of ['renamer','generator','deduplicator','exporter','tagger']) { await page.locator(`.mode-tab[data-mode="${panel}"]`).click(); await expect(page.locator(`#panel-${panel}`)).toBeVisible(); }
    await page.locator('#presets-edit').click();await expect(page.locator('#pe-editor')).toBeVisible();await page.keyboard.press('Escape');
  }
});
test('theme dropdown is keyboard reachable in a narrow settings sheet',async({page})=>{
  await page.setViewportSize({width:560,height:700});await open(page);await settings(page);
  const picker=page.locator('#set-palette-control');await picker.focus();await picker.press('Enter');
  await expect(page.getByRole('option',{name:'Everforest',exact:true})).toBeVisible();
  await page.keyboard.press('End');await page.keyboard.press('Enter');
  await expect(page.locator('html')).toHaveAttribute('data-palette','forest');await expect(picker).toBeFocused();
});
