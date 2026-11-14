import { el } from './dom.js';
import { t, tn } from './i18n.js';

// One compact summary of the actual pending plan. Preview lives beside Apply;
// clearing here only discards the draft, never tags or files on disk.
export function renderStaging(plan) {
  const count = new Set((plan?.changes || []).map(change => change.path)).size;
  el('staged-edits').hidden = count === 0;
  el('staged-count').textContent = t('staging.fileCount', { files: tn('unit.file', count) });
  el('staged-tab-count').textContent = count ? String(count) : '';
}
export function initStaging({ onClear, isBusy }) {
  el('diff-discard').addEventListener('click', () => { if (!isBusy()) onClear(); });
  renderStaging(null);
}
