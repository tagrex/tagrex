import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { DEFAULT_ACCENT, PALETTES, paletteAccent } from '../../app/ui/js/palettes.js';

const css = await readFile(new URL('../../app/ui/palettes.css', import.meta.url), 'utf8');
const surfaces = new Map([...css.matchAll(/^:root\[data-palette="(\w+)"\](\[data-theme="dark"\])? \{([^}]+)\}/gm)]
  .map(([, id, dark, declarations]) => [`${id}:${dark ? 'dark' : 'light'}`, Object.fromEntries([...declarations.matchAll(/--([\w-]+):\s*(#[\da-f]+);/g)].map(([, token, value]) => [token, value]))]));
const luminance = hex => {
  const channels = hex.slice(1).match(/../g).map(value => parseInt(value, 16) / 255)
    .map(value => value <= .04045 ? value / 12.92 : ((value + .055) / 1.055) ** 2.4);
  return channels.reduce((sum, channel, i) => sum + channel * [.2126, .7152, .0722][i], 0);
};
const contrast = (a, b) => (Math.max(luminance(a), luminance(b)) + .05) / (Math.min(luminance(a), luminance(b)) + .05);
let sequence = 0;
async function appearance(seed = {}, dark = true) {
  const savedGlobals = Object.fromEntries(['document', 'localStorage', 'window', 'getComputedStyle'].map(name => [name, Object.getOwnPropertyDescriptor(globalThis, name)]));
  const values = new Map(Object.entries(seed));
  const styles = new Map();
  const root = { dataset: {}, style: { setProperty: (name, value) => styles.set(name, value), removeProperty: name => styles.delete(name) } };
  const events = [];
  const media = { matches: dark, addEventListener(_name, callback) { this.change = callback; } };
  Object.assign(globalThis, {
    localStorage: { getItem: name => values.get(name) ?? null, setItem: (name, value) => values.set(name, String(value)), removeItem: name => values.delete(name) },
    document: { documentElement: root, dispatchEvent: event => events.push(event.type) },
    window: { matchMedia: () => media },
    getComputedStyle: () => ({ getPropertyValue: name => surfaces.get(`${root.dataset.palette}:${root.dataset.theme}`)?.[name.slice(2)] || '' }),
  });
  const prefs = await import(`../../app/ui/js/prefs.js?palette-test=${sequence++}`);
  return { prefs, root, values, styles, media, events, restore() {
    for (const [name, descriptor] of Object.entries(savedGlobals)) {
      if (descriptor) Object.defineProperty(globalThis, name, descriptor);
      else delete globalThis[name];
    }
  } };
}

test('all palette modes have readable normal and muted text on every surface', () => {
  assert.equal(surfaces.size, PALETTES.length * 2);
  for (const [name, tokens] of surfaces) {
    for (const text of ['text', 'muted']) for (const surface of ['bg', 'panel', 'row-hover', 'row-stripe']) {
      assert.ok(contrast(tokens[text], tokens[surface]) >= 4.5, `${name}: ${text} on ${surface} = ${contrast(tokens[text], tokens[surface]).toFixed(2)}`);
    }
    assert.ok(contrast(tokens.bg, tokens['row-stripe']) < 1.15, `${name}: stripes must remain quiet`);
  }
});

test('placeholder colours stay quieter than values and readable in every palette', () => {
  for (const [name, tokens] of surfaces) {
    for (const surface of ['bg','panel']) assert.ok(contrast(tokens['field-ghost'],tokens[surface])>=4.5, `${name}: placeholder on ${surface}`);
    assert.ok(contrast(tokens['field-ghost'],tokens.bg)<contrast(tokens.muted,tokens.bg),`${name}: placeholder recedes below helper text`);
  }

});

test('dark palettes retain the approved lifted surfaces and sampled Navy colours', () => {
  // Resolved pixels from the approved 751×733 reference image, rather than
  // inferred navy colours: shell, filled inputs/table, and alternating rows.
  const navy = surfaces.get('navy:dark');
  assert.equal(navy.panel, '#1a1f34');
  assert.equal(navy.bg, '#252a40');
  assert.equal(navy['row-stripe'], '#2b3046');
  for (const palette of PALETTES) {
    const tokens = surfaces.get(`${palette.id}:dark`);
    assert.ok(luminance(tokens.bg) > luminance(tokens.panel), `${palette.id}: controls/table lift above shell`);
    assert.ok(luminance(tokens['row-stripe']) > luminance(tokens.bg), `${palette.id}: stripe is the next quiet tone`);
  }
});

test('named theme core surfaces and primaries match the verified upstream references', async () => {
  const references=JSON.parse(await readFile(new URL('../../docs/design/theme-references.json',import.meta.url),'utf8'));
  for(const [id,modes] of Object.entries(references.anchors))for(const [mode,anchor] of Object.entries(modes)) {
    const tokens=surfaces.get(`${id}:${mode}`);
    assert.equal(tokens.bg,anchor.bg,`${id}:${mode} background`);assert.equal(tokens.panel,anchor.panel,`${id}:${mode} chrome`);
    assert.equal(paletteAccent(id,mode),anchor.primary,`${id}:${mode} primary`);
  }
});

test('fresh and legacy preferences migrate to complete Navy without erasing stored custom colours', async () => {
  for (const seed of [{}, {'tagrex.theme':'auto'}, {'tagrex.theme':'dark','tagrex.accent':'#123abc'}]) {
    const app=await appearance(seed);
    try {
      assert.equal(app.root.dataset.palette,'navy');assert.equal(app.prefs.accentColor(),DEFAULT_ACCENT);
      assert.equal(app.values.get('tagrex.themeAccent'),'preset');
      if(seed['tagrex.accent'])assert.equal(app.values.get('tagrex.accent'),seed['tagrex.accent']);
    } finally {app.restore();}
  }
});

test('surface choices preserve explicit accents and emit the shared appearance event', async () => {
  const app = await appearance({ 'tagrex.theme': 'dark', 'tagrex.accent': '#123abc' });
  try {
    app.prefs.saveAccent('#123abc');
    const count = app.events.length;
    app.prefs.applyPalette('forest');
    assert.equal(app.root.dataset.palette, 'forest');
    assert.equal(app.values.get('tagrex.palette'), 'forest');
    assert.equal(app.prefs.accentColor(), '#123abc');
    assert.equal(app.events.length, count + 1);
    assert.equal(app.events.at(-1), 'tagrex:appearance');
    app.prefs.saveAccent(paletteAccent('forest'));
    assert.equal(app.prefs.accentColor(), paletteAccent('forest'));
    app.prefs.applyPalette('plum');
    assert.equal(app.prefs.accentColor(), paletteAccent('forest'));
    app.prefs.saveAccent(null);
    assert.equal(app.prefs.accentColor(), null);
    assert.equal(app.styles.has('--accent'), false);
  } finally { app.restore(); }
});

test('saved palettes survive reload, invalid values fall back, and Auto keeps following the OS', async () => {
  const app = await appearance({ 'tagrex.palette': 'forest', 'tagrex.theme': 'auto', 'tagrex.accent': '#aabbcc' }, false);
  try {
    app.prefs.saveAccent('#aabbcc');
    assert.equal(app.root.dataset.palette, 'forest');
    assert.equal(app.root.dataset.theme, 'light');
    app.media.matches = true;
    app.media.change();
    assert.equal(app.root.dataset.theme, 'dark');
    assert.equal(app.root.dataset.palette, 'forest');
    app.prefs.applyTheme('light');
    app.media.change();
    assert.equal(app.root.dataset.theme, 'light');
    app.prefs.applyPalette('unknown');
    assert.equal(app.root.dataset.palette, 'navy');
    assert.equal(app.prefs.accentColor(), '#aabbcc');
  } finally { app.restore(); }
});

test('preset and extreme custom accents retain readable ink and button labels on every palette', async () => {
  const app = await appearance({ 'tagrex.theme': 'dark' });
  try {
    for (const palette of PALETTES) for (const mode of ['light', 'dark']) {
      app.prefs.applyTheme(mode);
      app.prefs.applyPalette(palette.id);
      const tokens = surfaces.get(`${palette.id}:${mode}`);
      for (const hex of [palette.accent, ...app.prefs.ACCENT_PRESETS.map(preset => preset.hex).filter(Boolean), '#ffffff', '#000000', '#888888', '#ffff00', '#123abc']) {
        app.prefs.saveAccent(hex);
        assert.ok(contrast(app.styles.get('--accent'), app.styles.get('--accent-text')) >= 4.5, `${palette.id}:${mode} ${hex}: button label`);
        for (const surface of ['bg', 'panel']) assert.ok(contrast(app.styles.get('--accent-ink'), tokens[surface]) >= 4.5, `${palette.id}:${mode} ${hex}: accent ink on ${surface} = ${contrast(app.styles.get('--accent-ink'), tokens[surface]).toFixed(2)}`);
      }
    }
  } finally { app.restore(); }
});

test('complete presets own coordinated accents in both modes while preserving the legacy custom preference',async()=>{
 const app=await appearance({'tagrex.theme':'auto','tagrex.accent':'#123abc'},false);
 try {
  assert.equal(PALETTES.length,10);
  for(const palette of PALETTES){
   app.prefs.applyThemePreset(palette.id);
   assert.equal(app.prefs.accentColor(),palette.lightAccent);
   assert.equal(app.values.get('tagrex.accent'),'#123abc');
   app.prefs.applyTheme('dark');assert.equal(app.prefs.accentColor(),palette.accent);
   app.prefs.applyTheme('light');assert.equal(app.prefs.accentColor(),palette.lightAccent);
  }
  app.prefs.applyTheme('auto');app.media.matches=true;app.media.change();
  assert.equal(app.prefs.accentColor(),PALETTES.at(-1).accent);
  assert.equal(app.values.get('tagrex.themeAccent'),'preset');
 }finally{app.restore();}
});

test('error and success text remain readable on their validation tints in every theme',()=>{
 const mix=(a,b,n)=>'#'+a.slice(1).match(/../g).map((v,i)=>Math.round(parseInt(v,16)*(1-n)+parseInt(b.slice(1+i*2,3+i*2),16)*n).toString(16).padStart(2,'0')).join('');
 for(const [name,tokens]of surfaces)for(const role of ['del','add'])for(const surface of ['bg','panel','row-hover','row-stripe']){
  assert.ok(contrast(tokens[role],tokens[surface])>=4.5,`${name} ${role} on ${surface}`);
  assert.ok(contrast(tokens[role],mix(tokens[surface],tokens[role],.12))>=4.5,`${name} ${role} on its tint`);
 }
});

test('a preset remains a complete live choice when storage writes fail',async()=>{
 const app=await appearance({'tagrex.palette':'navy','tagrex.theme':'dark'});
 try{
  localStorage.setItem=()=>{throw new Error('quota');};
  app.prefs.applyThemePreset('cupertino');
  assert.equal(app.root.dataset.palette,'cupertino');assert.equal(app.prefs.paletteMode(),'cupertino');
  assert.equal(app.prefs.accentColor(),paletteAccent('cupertino','dark'));
 }finally{app.restore();}
});
