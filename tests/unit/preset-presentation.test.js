import { test } from 'node:test';
import assert from 'node:assert/strict';
import { describeRule, presetExample, sampleRequest } from '../../app/ui/js/preset-presentation.js';

test('specialised descriptions require an exact matching replacement', () => {
  const rule = { kind:'replace', regex:true, from:String.raw`[^A-Za-z0-9._()-]+`, to:'_' };
  const t = key => key;
  assert.equal(describeRule(rule, t).title, 'presets.rule.unsupported');
  assert.equal(describeRule({ ...rule, to:' ' }, t).title, 'presets.rule.pattern');
  assert.equal(describeRule({ ...rule, regex:false }, t).title, 'presets.rule.replace');
  assert.equal(describeRule({ ...rule, whole_word:true }, t).title, 'presets.rule.pattern');
  assert.equal(describeRule({ kind:'unknown' }, t).title, 'presets.rule.custom');
});

test('extension preview adapts a copy and preserves the stored scope and disabled flag', () => {
  const group = { name:'Mixed', scope:'tags', rules:[{kind:'case', scope:'fileext', style:'lower', enabled:false}, {kind:'case',scope:'title',style:'upper'}] };
  const before = structuredClone(group);
  const request = sampleRequest(group, 'fileext');
  assert.equal(request.target, 'filename');
  assert.deepEqual(request.group.rules, [{kind:'case',scope:'filename',style:'lower',enabled:false}]);
  assert.deepEqual(group, before);
  assert.equal(sampleRequest(group,'tags').group, group);
});

test('preset examples demonstrate built-ins without treating a renamed personal preset as built-in', () => {
  assert.equal(presetExample({ name:'Standard values',builtin:true }), '  Blue   Moon  ');
  assert.equal(presetExample({ name:'FTP format',builtin:true }), 'Björk Jóga');
  assert.equal(presetExample({ name:'File extension',builtin:true }), 'FLAC');
  assert.equal(presetExample({ name:'FTP format',builtin:false }), 'Blue Moon');
});

test('field previews retain global rules and the chosen field in original order', async () => {
  const { previewFields } = await import('../../app/ui/js/preset-presentation.js');
  const group = {scope:'tags',rules:[{kind:'case',scope:'artist',style:'upper'},{kind:'replace',scope:'tags',from:'x',to:'y'},{kind:'case',scope:'title',style:'lower'},{kind:'case',scope:'filename',style:'upper'}]};
  const before = structuredClone(group);
  assert.deepEqual(sampleRequest(group,'artist').group.rules.map(r=>r.kind), ['case','replace']);
  assert.deepEqual(sampleRequest(group,'title').group.rules.map(r=>r.kind), ['replace','case']);
  assert.equal(sampleRequest(group,'filename').group.rules.length,1);
  assert.deepEqual(previewFields(group,['title','artist']),['artist','title','filename']);
  assert.deepEqual(group,before);
});
