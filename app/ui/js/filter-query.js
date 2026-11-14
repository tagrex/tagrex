// Small query data only. Library DTOs and compiled predicates stay outside Svelte.
export const FILTER_FIELDS = [
  { key: 'artist', label: 'Artist' }, { key: 'album', label: 'Album' },
  { key: 'title', label: 'Title' }, { key: 'file', label: 'Filename' },
  { key: 'cover', label: 'Artwork' },
];
export const FILTER_OPERATORS = [
  { key: 'contains', label: 'contains' }, { key: 'equals', label: 'is' },
  { key: 'starts', label: 'starts with' }, { key: 'empty', label: 'is empty' },
  { key: 'present', label: 'is not empty' },
];
const statuses = ['embedded', 'missing', 'unknown'];
const aliases = new Map(statuses.map(state => [state, state]));
aliases.set('no cover', 'missing');

export function coverState(track) {
  return track.unreadable || typeof track.has_cover !== 'boolean' ? 'unknown' : track.has_cover ? 'embedded' : 'missing';
}

export function normalizeConditions(value = []) {
  if (!Array.isArray(value) || value.length > 24) throw new Error('Use at most 24 filter conditions.');
  const ids = new Set();
  return value.map(item => {
    if (!item || typeof item.id !== 'string' || !item.id || item.id.length > 100 || ids.has(item.id) ||
        !FILTER_FIELDS.some(field => field.key === item.field) ||
        !FILTER_OPERATORS.some(op => op.key === item.operator) ||
        typeof item.value !== 'string' || item.value.length > 512 ||
        (item.field === 'cover' && (item.operator !== 'equals' || !statuses.includes(item.value))) ||
        (!['empty', 'present'].includes(item.operator) && !item.value.trim())) {
      throw new Error('Complete each filter condition before applying it.');
    }
    ids.add(item.id);
    return { id: item.id, field: item.field, operator: item.operator, value: item.value };
  });
}

export function conditionLabel(item) {
  return `${FILTER_FIELDS.find(field => field.key === item.field)?.label || item.field} ${
    FILTER_OPERATORS.find(op => op.key === item.operator)?.label || item.operator}${
    ['empty', 'present'].includes(item.operator) ? '' : ` ${item.value}`}`;
}

// Context supplies display values and the existing cached free-text index.
// Compile once per query; even artwork label/regex matching runs only three times.
export function compileQuery(model, context) {
  try {
    if (model.version !== 1 || typeof model.text !== 'string' || model.text.length > 2048) throw new Error('Filter text must be at most 2048 characters.');
    const conditions = normalizeConditions(model.conditions);
    if (!['all', 'any'].includes(model.mode)) throw new Error('Unknown filter combination.');
    const fold = value => model.caseSensitive ? String(value) : String(value).toLowerCase();
    const predicates = conditions.map(item => {
      const needle = fold(item.value);
      return track => {
        const value = fold(item.field === 'cover' ? coverState(track) : context.fieldValue(track, item.field));
        switch (item.operator) {
          case 'empty': return !value.trim();
          case 'present': return !!value.trim();
          case 'equals': return value === needle;
          case 'starts': return value.startsWith(needle);
          default: return value.includes(needle);
        }
      };
    });
    let raw = model.text;
    let scope = null;
    const prefix = raw.match(/^([A-Za-z0-9_]+):(.*)$/);
    if (prefix && (prefix[1].toLowerCase() === 'cover' || context.fields.includes(prefix[1].toLowerCase()))) {
      scope = prefix[1].toLowerCase(); raw = prefix[2];
    }
    let re = null;
    if (model.regex && raw) {
      try { re = new RegExp(raw, model.caseSensitive ? '' : 'i'); }
      catch { throw new Error('Invalid regular expression. Showing the last valid results.'); }
    }
    const needle = fold(raw);
    let coverMatches = null;
    if (scope === 'cover' && raw) {
      const alias = !model.regex && aliases.get(needle);
      coverMatches = new Set(statuses.filter(state => {
        if (alias) return state === alias;
        const label = context.coverLabel(state);
        return re ? re.test(state) || re.test(label) : fold(label).includes(needle);
      }));
    }
    return { model: { ...model, conditions }, error: '', match(track) {
      if (predicates.length && !(model.mode === 'any' ? predicates.some(test => test(track)) : predicates.every(test => test(track)))) return false;
      if (!raw) return true;
      if (coverMatches) return coverMatches.has(coverState(track));
      const hay = scope ? context.fieldValue(track, scope) : context.searchText(track, !model.caseSensitive && !re);
      return re ? re.test(hay) : (scope ? fold(hay) : hay).includes(needle);
    } };
  } catch (error) {
    return { model, error: String(error.message || error), match: null };
  }
}
