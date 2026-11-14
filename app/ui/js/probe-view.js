// A bounded read-out of backend-extracted tags. This renders results only;
// parsing and transformations stay with the existing backend/chain pipeline.
import { escapeHtml } from './dom.js';
import { t } from './i18n.js';
import { columnLabel } from './columns.js';

export function probeMessage(box, message) {
  box.innerHTML = `<p class="probe-miss">${escapeHtml(message)}</p>`;
}

export function renderProbeTracks(box, samples, { name = false, subjectHtml = escapeHtml } = {}) {
  const rows = samples.map(({ probe, fields, path }, index) => {
    const value = key => fields.find(([field]) => field === key)?.[1];
    const title = !probe.matched ? t(name ? 'fromname.probeNoMatch' : 'fromText.probeNoMatch') : value('title') || t('inspector.untitled');
    const artist = value('artist');
    const invalid = new Set(probe.invalid || []);
    const source = `<div class="probe-source"><span>${escapeHtml(t(name ? 'inspector.originalName' : 'inspector.sourceLine'))}</span><div class="probe-subject">${subjectHtml(probe.subject)}</div></div>`;
    if (!probe.matched || !fields.length) return `${source}<p class="probe-miss">${escapeHtml(t(name ? 'fromname.probeNoMatch' : 'fromText.probeNoMatch'))}</p>`;
    const details = fields.map(([field, text]) => `<div class="probe-row${invalid.has(field) ? ' probe-invalid' : ''}"><dt class="probe-field">${escapeHtml(columnLabel(field))}</dt><dd class="probe-value">${escapeHtml(text)}${invalid.has(field) ? `<span class="probe-warning">${escapeHtml(t('inspector.invalidValue'))}</span>` : ''}</dd></div>`).join('');
    const file = path ? `<div class="probe-target"><span>${escapeHtml(t('inspector.targetFile'))}</span><span>${escapeHtml(path.split(/[\\/]/).pop())}</span></div>` : '';
    const summary = `<span class="probe-track-number">${escapeHtml(value('track') || String(index + 1))}</span><span class="probe-track-identity"><strong>${escapeHtml(title)}</strong><span>${escapeHtml(artist || t('inspector.noArtist'))}</span></span><svg class="ico probe-track-caret" aria-hidden="true"><use href="#i-caret-down" /></svg>`;
    // Keep source visible for file names. Text sources/field diagnostics expand
    // on demand; several tracks still share one quiet inset surface.
    return `${name ? source : ''}<details class="probe-track"><summary>${summary}</summary><div class="probe-track-detail">${name ? '' : source}${file}<dl class="probe-fields">${details}</dl></div></details>`;
  });
  box.innerHTML = `<div class="probe-heading">${escapeHtml(t('inspector.tagsPreview'))}</div><div class="probe-tracks">${rows.join('')}</div>`;
}
