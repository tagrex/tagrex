import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
import { createHash } from 'node:crypto';

const assets=new URL('../../app/ui/assets/',import.meta.url);
const report=JSON.parse(await readFile(new URL('FONT-VERIFICATION.json',assets),'utf8'));
const css=await readFile(new URL('../../app/ui/fonts.css',import.meta.url),'utf8');

test('every shipped font matches its independently verified bytes and weight outlines',async()=>{
  const shipped=(await readdir(assets)).filter(name=>name.endsWith('.woff2')).sort();
  assert.deepEqual(shipped,Object.keys(report.assets).sort());
  for(const [name,metadata] of Object.entries(report.assets)) {
    const bytes=await readFile(new URL(name,assets));
    assert.equal(bytes.subarray(0,4).toString(),'wOF2',name);
    assert.equal(bytes.length,metadata.bytes,name);
    assert.equal(createHash('sha256').update(bytes).digest('hex'),metadata.sha256,name);
    if(!name.includes('condensed')) {
      assert.equal(new Set(Object.values(metadata.weight_outline_sha256)).size,4,`${name}: distinct 400/500/600/700 outlines`);
      assert.equal(Object.keys(metadata.locale_source_letters).length,7,`${name}: seven locale checks`);
    }
  }
});

test('font-face declarations have one offline owner and contain every real style/weight range',async()=>{
  const rules=[...css.matchAll(/@font-face\s*\{([^}]+)\}/g)].map(match=>match[1]);
  assert.equal(rules.length,9);
  for(const family of ['TagRex Inter','TagRex Sans','TagRex Source Sans','TagRex Mono']) {
    const faces=rules.filter(rule=>rule.includes(`font-family: "${family}";`));
    assert.equal(faces.length,2,family);
    assert.ok(faces.some(rule=>/font-style: normal;/.test(rule)),family);
    assert.ok(faces.some(rule=>/font-style: italic;/.test(rule)),family);
    for(const rule of faces) assert.match(rule,/font-weight: 400 700;/);
  }
  assert.doesNotMatch(css,/https?:|local\(/);
  for(const rule of rules) assert.match(rule,/font-display: swap;/);
  assert.doesNotMatch(await readFile(new URL('../../app/ui/style.css',import.meta.url),'utf8'),/@font-face\s*\{/);
  for(const notice of ['OFL-inter.txt','OFL-ibmplexsans.txt','OFL-sourcesans3.txt','OFL-jetbrainsmono.txt']) {
    assert.match(await readFile(new URL(notice,assets),'utf8'),/SIL OPEN FONT LICENSE/);
  }
});
