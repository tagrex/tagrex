// Presentation only: descriptions never change or execute saved rules.
const replacements = [
  [String.raw`\s+`, ' ', 'spaces'],
  [String.raw`\s{2,}`, ' ', 'spaces'],
  [String.raw`^\s+|\s+$`, '', 'trimSpaces'],
  [String.raw`[^A-Za-z0-9._()-]+`, '_', 'unsupported'],
  [String.raw`_{2,}`, '_', 'underscores'],
  [String.raw`^_+|_+$`, '', 'trimUnderscores'],
  [String.raw`_+`, ' ', 'replaceUnderscores'],
  [String.raw`[-\x{2013}\x{2014}]+`, ' ', 'dashes'],
  [String.raw`\s*\(\d+\)$`, '', 'discogsNumber'],
  [String.raw`\*(\s*(?:$|[,;/&]|feat\b|featuring\b|ft\b|vs\b|presents\b|x\s))`, '$1', 'discogsStar'],
];

export function describeRule(rule, t) {
  let key = rule.kind;
  if (key === 'replace') {
    const known = rule.regex && !rule.whole_word && replacements.find(([from, to]) => rule.from === from && rule.to === to);
    key = known ? known[2] : rule.regex ? 'pattern' : 'replace';
  } else if (key === 'case') key = ['title', 'lower', 'upper', 'sentence'].includes(rule.style) ? rule.style : 'case';
  if (!['spaces', 'trimSpaces', 'unsupported', 'underscores', 'trimUnderscores', 'replaceUnderscores', 'dashes', 'discogsNumber', 'discogsStar', 'pattern', 'replace', 'title', 'lower', 'upper', 'sentence', 'case', 'diacritics', 'transliterate', 'untransliterate', 'key'].includes(key)) key = 'custom';
  return { title: t('presets.rule.' + key), help: t('presets.rule.' + key + '.hint') };
}

const examples = {
  'standard values': '  Blue   Moon  ',
  'discogs cleanup': 'Sunbeam (2)',
  'normalize english': 'the dark side of the moon',
  'general latin': 'Café Москва',
  'no dash': 'Blue---Moon',
  'no underscore': 'Blue___Moon',
  'lower case': 'BLUE MOON',
  'file extension': 'FLAC',
  'ftp format': 'Björk Jóga',
};

export function presetExample(group) {
  return (group?.builtin && examples[group.name.toLowerCase()]) || 'Blue Moon';
}

export function sampleRequest(group, target) {
  // The string-preview API accepts tags or filename. Run extension-only rules
  // on an extension string through that same engine, without changing storage.
  if (target === 'tags') return { group, target }; // Legacy generic sample callers.
  const fileTarget = target === 'fileext' || target === 'filename';
  return { target: fileTarget ? 'filename' : 'tags', group: { ...group, rules: group.rules.filter(rule => {
    const scope = rule.scope || group.scope || 'tags';
    return scope === target || (!fileTarget && scope === 'tags');
  }).map(rule => ({ ...rule, scope: fileTarget ? 'filename' : 'tags' })) } };
}

export function previewFields(group, tagFields = ['title']) {
  const fields = new Set();
  for (const rule of group.rules) {
    const scope = rule.scope || group.scope || 'tags';
    if (scope === 'tags') tagFields.forEach(field => fields.add(field));
    else fields.add(scope);
  }
  return fields.size ? [...fields] : ['title'];
}
