import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';

const root = resolve('.test-dist');
const errors = new WeakMap();
test.beforeEach(async ({ page }) => {
  const pageErrors = [];
  errors.set(page, pageErrors);
  page.on('pageerror', error => pageErrors.push(error.message));
  await page.route('http://tagrex.test/**', async route => {
    const path = resolve(root, '.' + new URL(route.request().url()).pathname);
    if (!path.startsWith(root + sep)) return route.abort();
    try {
      const contentType = path.endsWith('.js') ? 'text/javascript' : path.endsWith('.css') ? 'text/css' : path.endsWith('.woff2') ? 'font/woff2' : 'text/html';
      await route.fulfill({ body: await readFile(path), contentType });
    } catch { await route.fulfill({ status: 404, body: 'Missing fixture' }); }
  });
  await page.goto('http://tagrex.test/app/ui/index.html');
  await expect.poll(() => page.evaluate(() => typeof window.tagrex?.setMode)).toBe('function');
});

test.afterEach(async ({ page }) => {
  expect(errors.get(page)).toEqual([]);
});

async function settings(page) {
  await page.locator('#settings-open').click();
  await expect(page.getByRole('dialog', { name: 'Settings', exact: true })).toBeVisible();
}

test('shadcn switches track silent native property writes and emit one change per user action', async ({ page }) => {
  await settings(page);
  const toggle = page.locator('#set-check-updates-control');
  await page.evaluate(() => {
    const input = document.getElementById('set-check-updates');
    window.controlEvents = [];
    window.controlClicks = 0;
    input.addEventListener('click', () => window.controlClicks++);
    input.addEventListener('input', () => window.controlEvents.push('input'));
    input.addEventListener('change', () => window.controlEvents.push('change'));
    input.checked = true;
  });
  await expect(toggle).toHaveAttribute('aria-checked', 'true');
  expect(await page.evaluate(() => window.controlEvents)).toEqual([]);
  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-checked', 'false');
  expect(await page.locator('#set-check-updates').isChecked()).toBe(false);
  expect(await page.evaluate(() => window.controlEvents)).toEqual(['input', 'change']);
  expect(await page.evaluate(() => window.controlClicks)).toBe(1);
  await page.locator('label[for="set-check-updates-control"]').click();
  await expect(toggle).toHaveAttribute('aria-checked', 'true');
  await expect(toggle).toBeFocused();
  await page.keyboard.press('Space');
  await expect(toggle).toHaveAttribute('aria-checked', 'false');
  await page.evaluate(() => { document.getElementById('set-check-updates').disabled = true; });
  await expect(toggle).toBeDisabled();
});

test('select-all tri-state and wrapping checkbox labels stay synchronized with native controllers', async ({ page }) => {
  await settings(page);
  await page.locator('#settings-save').click();
  await page.locator('#root-display').click();
  await page.locator('#root').fill('/music');
  await page.locator('#root').press('Enter');
  const header = page.locator('#select-all-control');
  await expect(header).toHaveAttribute('aria-checked', 'mixed');
  await page.locator('#tracks-body tr[data-path] td.file').first().click();
  await page.keyboard.press('Control+a');
  await expect(page.locator('#tracks-body tr.selected')).toHaveCount(3);
  await expect(header).toHaveAttribute('aria-checked', 'true');
  await page.locator('.mode-tab[data-mode="generator"]').click();
  await page.locator('#gen-job-number').click();
  const flag = page.locator('#autonum-total-control');
  await flag.locator('xpath=ancestor::label').click();
  await expect(flag).toHaveAttribute('aria-checked', 'true');
  expect(await page.locator('#autonum-total').isChecked()).toBe(true);
});

test('shadcn sliders preserve native value updates, focus, input events and live settings', async ({ page }) => {
  await settings(page);
  await page.locator('#settings-search').fill('Table font size');
  const slider = page.locator('#set-table-font-control [role="slider"]');
  await page.evaluate(() => {
    const input = document.getElementById('set-table-font');
    window.sliderEvents = [];
    input.addEventListener('input', () => window.sliderEvents.push('input'));
    input.addEventListener('change', () => window.sliderEvents.push('change'));
    input.value = '16';
    input.focus();
  });
  await expect(slider).toHaveAttribute('aria-valuenow', '16');
  await expect(slider).toBeFocused();
  expect(await page.evaluate(() => window.sliderEvents)).toEqual([]);
  await page.keyboard.press('ArrowRight');
  await expect(slider).toHaveAttribute('aria-valuenow', '17');
  await expect(page.locator('#set-table-font')).toHaveValue('17');
  await expect(page.locator('#set-table-font-val')).toHaveText('17px');
  expect(await page.evaluate(() => window.sliderEvents)).toContain('input');
  await page.evaluate(() => { document.getElementById('set-table-font').disabled = true; });
  await expect(slider).toHaveAttribute('aria-disabled', 'true');
});

test('a complete theme uses the shared select control and retains keyboard focus',async({page})=>{
  await settings(page);const picker=page.locator('#set-palette-control');await picker.focus();await picker.press('Enter');
  await page.getByRole('option',{name:'Cupertino',exact:true}).click();
  await expect(page.locator('#set-palette')).toHaveValue('cupertino');await expect(picker).toBeFocused();
  await expect(page.getByRole('dialog',{name:'Settings',exact:true})).toBeVisible();
});

test('native form reset synchronizes the rendered checkbox and slider', async ({ page }) => {
  await settings(page);
  await page.evaluate(() => {
    const form = document.createElement('form');
    form.hidden = true;
    document.body.append(form);
    const checkbox = document.getElementById('set-check-updates');
    const slider = document.getElementById('set-table-font');
    form.append(checkbox, slider);
    checkbox.defaultChecked = true;
    checkbox.checked = false;
    slider.defaultValue = '15';
    slider.value = '18';
    form.reset();
  });
  await expect(page.locator('#set-check-updates-control')).toHaveAttribute('aria-checked', 'true');
  await page.locator('#settings-search').fill('Table font size');
  await expect(page.locator('#set-table-font-control [role="slider"]')).toHaveAttribute('aria-valuenow', '15');
});

test('row highlights preserve additive selection without checkbox mounts', async ({ page }) => {
  await page.locator('#root-display').click();await page.locator('#root').fill('/music');await page.locator('#root').press('Enter');
  const row=page.locator('#tracks-body tr[data-path]').nth(1);
  await expect(row.locator('input[type="checkbox"], [role="checkbox"]')).toHaveCount(0);
  await row.locator('td.file').click({modifiers:['Control']});
  await expect(page.locator('#tracks-body tr.selected')).toHaveCount(2);await expect(row).toHaveAttribute('aria-selected','true');
  await row.locator('td.file').click({modifiers:['Control']});
  await expect(page.locator('#tracks-body tr.selected')).toHaveCount(1);await expect(row).toHaveAttribute('aria-selected','false');
});

test('mounted shadcn editor controls preserve legacy values, content and single click handlers', async ({ page }) => {
  await page.locator('#root-display').click();
  await page.locator('#root').fill('/music');
  await page.locator('#root').press('Enter');
  await page.locator('.subtab[data-subtab="editor"]').click();
  const title = page.locator('#fields-body input[data-key="title"]');
  await expect(title).toHaveAttribute('data-slot', 'input');
  await expect(title).toHaveAttribute('data-component-element', 'input');
  await title.evaluate(input => {
    input.value = 'A programmatic title';
    // A same-turn reparent keeps the mounted component and its listeners alive.
    input.parentElement.append(input);
  });
  await expect(title).toHaveValue('A programmatic title');
  await title.fill('A newly staged title');
  await expect(title).toHaveClass(/dirty/);
  const lock = page.locator('#fields-body .fe-row:has(input[data-key="title"]) .fe-lock');
  await expect(lock).toHaveAttribute('data-slot', 'button');
  await expect(lock.locator('svg')).toHaveCount(1);
  await lock.evaluate(button => {
    window.legacyLockClicks = 0;
    button.addEventListener('click', () => window.legacyLockClicks++);
  });
  await lock.click();
  await expect(page.locator('#fields-body input[data-key="title"]')).toBeDisabled();
  expect(await page.evaluate(() => window.legacyLockClicks)).toBe(1);
});

test('provider actions mount real components while artwork tiles and native track events keep their behavior', async ({ page }) => {
  await page.evaluate(() => { document.getElementById('discogs-token').value = 'fixture'; });
  await page.locator('#discogs-query').fill('Helter Skelter');
  await page.locator('#discogs-search').click();
  const card = page.locator('.release-card').first();
  await expect(card).toBeVisible();
  await expect(card.locator('.release-head')).toHaveAttribute('data-component-element', 'button');
  await expect(card.locator('.release-import')).toHaveAttribute('data-slot', 'button');
  await expect(card.locator('.release-cover')).not.toHaveAttribute('data-component-element');
  await card.locator('.release-head').click();
  await expect(card).toHaveAttribute('aria-expanded', 'true');
  await expect(card.locator('[data-act="automatch"]')).toHaveAttribute('data-component-element', 'button');
  const checkbox = card.locator('tbody [role="checkbox"]').first();
  await expect(checkbox).toHaveAttribute('aria-checked', 'true');
  const tally = card.locator('.tk-tally');
  const before = await tally.textContent();
  await card.evaluate(node => {
    window.onlineTrackChanges = 0;
    node.addEventListener('change', event => {
      if (event.target.matches('tbody input[type=checkbox]')) window.onlineTrackChanges++;
    });
  });
  await checkbox.click();
  await expect(checkbox).toHaveAttribute('aria-checked', 'false');
  await expect(tally).not.toHaveText(before);
  expect(await page.evaluate(() => window.onlineTrackChanges)).toBe(1);
  await card.locator('.release-cover').click();
  await expect(page.getByRole('dialog', { name: 'Choose artwork', exact: true })).toBeVisible();
  await page.keyboard.press('Escape');
});
