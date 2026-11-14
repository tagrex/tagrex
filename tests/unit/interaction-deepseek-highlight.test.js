import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { createRequire } from 'node:module';
import vm from 'node:vm';

// The autocomplete's highlight, against the production function bodies.
//
// The search runs on the lowercased spelling, and lowercasing is not
// length-preserving for every character — "İ" folds to "i" + U+0307. The old
// code sliced the original spelling with that folded index, which put the bold
// one character to the right ("İs<b>t</b>anbul" when "s" was typed). These
// assertions are about the text the row shows: the whole value, with the bold
// starting exactly where the match was found.
//
// The fast path is also held to byte equality with the old slice formula for
// every value whose fold keeps its length — which is all ASCII values and nearly
// every other one — so the fix cannot have moved an ordinary highlight.
const root = resolve(process.env.TAGREX_SUGGEST_SOURCE || '.');
const require = createRequire(resolve(root, 'package.json'));
const ts = require('typescript');

function extract(file, names) {
  const source = readFileSync(resolve(root, file), 'utf8');
  const ast = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.JS);
  return names.map((name) => {
    const nodes = ast.statements.filter((node) => ts.isFunctionDeclaration(node) && node.name?.text === name);
    assert.equal(nodes.length, 1, `expected exactly one ${name}() in ${file}`);
    return nodes[0].getText(ast).replace(/^export\s+/, '');
  }).join('\n');
}

const body = extract('app/ui/js/suggest.js', ['collectValues', 'matches', 'markMatch', 'originalSpan']);
const escapeSource = extract('app/ui/js/dom.js', ['escapeHtml']);
// The same escape the page uses, in this realm, for the equality oracle.
const escapeHtml = (value) => String(value).replace(/[&<>"']/g, (c) => ({
  '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
})[c]);

function harness(spellings) {
  const context = vm.createContext({
    tracks: spellings.map((artist, index) => ({ path: `t${index}`, tags: { artist } })),
    edits: new Map(),
    tag: (track, key) => track.tags[key] || '',
  });
  vm.runInContext('const MAX_ROWS = 8; let values = [];', context);
  vm.runInContext(body, context);
  vm.runInContext(escapeSource, context);
  return {
    open() { vm.runInContext('values = collectValues("artist")', context); },
    query(typed) { return vm.runInContext(`matches(${JSON.stringify(typed)})`, context); },
    highlight(entry, typed) {
      const copy = JSON.stringify({ value: entry.value, at: entry.at, files: entry.files, folded: entry.folded });
      return vm.runInContext(`markMatch(${copy}, ${JSON.stringify(typed)})`, context);
    },
  };
}

const strip = (html) => html.replace(/<[^>]*>/g, '');
const head = (html) => html.split('<b>')[0];
const bold = (html) => (html.match(/<b>(.*?)<\/b>/) || [])[1] ?? '';

function legacy(entry, typed) {
  const length = typed.trim().length;
  return `${escapeHtml(entry.value.slice(0, entry.at))}<b>${escapeHtml(entry.value.slice(entry.at, entry.at + length))}</b>${escapeHtml(entry.value.slice(entry.at + length))}`;
}

// One assertion set per rendered row: nothing lost, the bold begins where the
// search said, and the bold really is the typed text (or the character whose
// case fold contains it).
function check(session, entries, typed) {
  const needle = typed.trim().toLowerCase();
  if (!needle) {
    for (const entry of entries) assert.equal(session.highlight(entry, typed), escapeHtml(entry.value));
    return;
  }
  for (const entry of entries) {
    const html = session.highlight(entry, typed);
    const where = `${JSON.stringify(entry.value)} / ${JSON.stringify(typed)}`;
    assert.equal(strip(html), entry.value, `text preserved for ${where}`);
    assert.equal(head(html).toLowerCase().length, entry.at, `bold starts at the match for ${where}`);
    const marked = bold(html);
    assert.ok(marked.length > 0, `something is bold for ${where}`);
    assert.ok(marked.toLowerCase().startsWith(needle), `bold is the typed text for ${where}`);
    if (entry.folded.length === entry.value.length) {
      assert.equal(html, legacy(entry, typed), `ordinary highlight unchanged for ${where}`);
    }
  }
}

test('a fold that changes the string length no longer shifts the bold', () => {
  const session = harness(['İstanbul', 'İİx', 'Éclair', 'Straße', '🎵 Song', 'Ω omega', 'Ǆungla', 'ΣΟΦΟΣ', 'The Artist', 'artist 3']);
  session.open();
  for (const typed of ['', 's', 'an', 'tan', 'i', 'İ', 'x', 'é', 'ra', 'song', 'artist', '3', 'οφ', 'ung']) {
    check(session, session.query(typed), typed);
  }
  const row = (typed, value) => session.query(typed).find((entry) => entry.value === value);
  const istanbul = row('s', 'İstanbul');
  assert.equal(istanbul.at, 2, 'the match is found in the folded spelling');
  assert.equal(session.highlight(istanbul, 's'), 'İ<b>s</b>tanbul');
  assert.equal(session.highlight(row('an', 'İstanbul'), 'an'), 'İst<b>an</b>bul');
  assert.equal(session.highlight(row('x', 'İİx'), 'x'), 'İİ<b>x</b>', 'a match after two expansions lands on the last character');
  assert.equal(session.highlight(row('i', 'İstanbul'), 'i'), '<b>İ</b>stanbul', 'a match inside an expansion marks the character');
});

test('the fast path is byte-identical to the previous slice for equal-length folds', () => {
  const alphabet = [...'ab AB-0_çéüΩ🎵'];
  const spellings = [];
  for (let index = 0; index < 2000; index++) {
    let value = '';
    let seed = (index * 2654435761) % 4294967296;
    for (let length = 2 + (index % 6); length > 0; length--) {
      seed = (seed * 1103515245 + 12345) % 2147483648;
      value += alphabet[seed % alphabet.length];
    }
    spellings.push(value.trim() || `v${index}`);
  }
  const session = harness(spellings);
  session.open();
  let compared = 0;
  for (const typed of ['a', 'A', 'é', 'ü', 'Ω', '🎵', 'ab', '  a ', 'b-', 'z']) {
    for (const entry of session.query(typed)) {
      if (entry.folded.length === entry.value.length) compared++;
    }
    check(session, session.query(typed), typed);
  }
  assert.ok(compared > 10, `the equality gate saw real rows (${compared})`);
});


test('a length-changing typed query highlights its entire folded match', () => {
  const session = harness(['İstanbul', 'i\u0307stanbul']); session.open();
  for (const typed of ['İs', 'i\u0307s', '  İst  ']) {
    const length = typed.trim().endsWith('t') ? 3 : 2;
    for (const entry of session.query(typed)) {
      const end = entry.value.startsWith('İ') ? length : length + 1;
      assert.equal(session.highlight(entry, typed), `<b>${entry.value.slice(0,end)}</b>${entry.value.slice(end)}`);
    }
  }
});
