import { mount } from 'svelte';
import SpreadsheetToolbar from '../components/SpreadsheetToolbar.svelte';
import { TAURI } from './invoke.js';
import { TABLE_DATA_COLUMN_OFFSET } from './state.js';

const MAX_CELLS = 10000;
const MAX_TEXT = 1024 * 1024;

// TSV with quoted tabs/newlines round-trips through spreadsheet applications.
// Reject malformed/ragged data before producing any backend request.
export function parseTsv(text) {
  if (!text) throw new Error('The clipboard is empty.');
  if (text.length > MAX_TEXT) throw new Error('Paste is limited to 1 MiB of text.');
  if (text.includes('\0')) throw new Error('The clipboard contains an unsupported null character. Copy plain text and try again.');
  const rows = [];
  let row = [], value = '', quoted = false, closed = false;
  const field = () => { row.push(value); value = ''; closed = false; };
  const line = () => {
    field(); rows.push(row); row = [];
    if (rows.length > MAX_CELLS) throw new Error('Paste is limited to 10,000 cells.');
  };
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quoted) {
      if (c === '"') {
        if (text[i + 1] === '"') { value += '"'; i++; }
        else { quoted = false; closed = true; }
      } else value += c;
    } else if (c === '\t') field();
    else if (c === '\n' || c === '\r') {
      if (c === '\r' && text[i + 1] === '\n') i++;
      line();
    } else if (c === '"' && value === '' && !closed) quoted = true;
    else {
      if (closed) throw new Error('The clipboard has text after a closing quote. Check the copied cells and try again.');
      value += c;
    }
    if (row.length > MAX_CELLS) throw new Error('Paste is limited to 10,000 cells.');
  }
  if (quoted) throw new Error('The clipboard has an unclosed quote. Check the copied cells and try again.');
  if (value !== '' || row.length || !/[\r\n]$/.test(text)) line();
  if (!rows.length) rows.push(['']);
  const width = rows[0].length;
  if (rows.some((r) => r.length !== width)) throw new Error('Paste needs a rectangular grid. Every row must have the same number of columns.');
  if (rows.length * width > MAX_CELLS) throw new Error('Paste is limited to 10,000 cells.');
  return rows;
}

export function encodeTsv(rows) {
  return rows.map((row) => row.map((value) => {
    const text = String(value ?? '');
    return /[\t\r\n"]/.test(text) ? `"${text.replaceAll('"', '""')}"` : text;
  }).join('\t')).join('\r\n');
}

/**
 * A path/column selection model beside the existing virtual renderer. getRows()
 * is the displayed, expanded model order, never a query of the rendered window.
 * It publishes a new read-only array when rebuilt; its path sequence, rather
 * than the array allocation, determines whether a rectangle is still valid.
 * The owner supplies validation and one batch preview, not per-cell writes.
 */
export function initSpreadsheet(options) {
  const { body, wrap, table } = options;
  const host = document.createElement('div');
  host.id = 'spreadsheet-tools';
  wrap.before(host);
  const toolbar = mount(SpreadsheetToolbar, { target: host, props: { onaction: action } });
  let active = null;
  let anchor = null;
  let lastRows = null;
  let lastColumns = [];
  let busy = false;
  let gridOwnsFocus = false;
  let scrollRefresh = false;
  let lastSummary = '';
  let lastToolbar = null;
  let selectionHeader = null;
  let selectionWidth = -1;
  let artworkHeader = null;
  let artworkWidth = -1;
  const paintedCells = new WeakMap();
  table.setAttribute('role', 'grid');
  table.setAttribute('aria-label', 'Audio files');
  table.setAttribute('aria-multiselectable', 'true');
  table.tabIndex = -1;

  const columns = () => options.getColumns();
  const rows = () => options.getRows();
  let indexedRows = null;
  let rowIndices = new Map();
  function rowIndex(path) {
    if (options.getRowIndex) return options.getRowIndex(path) ?? -1;
    const paths = rows();
    if (paths !== indexedRows) {
      indexedRows = paths;
      rowIndices = new Map(paths.map((value, index) => [value, index]));
    }
    return rowIndices.get(path) ?? -1;
  }
  const same = (a, b) => a && b && a.path === b.path && a.key === b.key;
  const editing = () => body.querySelector('[contenteditable="true"]');
  const sameSequence = (a, b) => a === b || (!!a && !!b && a.length === b.length && a.every((value, index) => value === b[index]));
  const attribute = (node, name, value) => {
    if (node && node.getAttribute(name) !== String(value)) node.setAttribute(name, String(value));
  };
  const classState = (node, name, on) => {
    if (node.classList.contains(name) !== on) node.classList.toggle(name, on);
  };
  function setSelectionWidth(width) {
    if (width === selectionWidth) return;
    selectionWidth = width;
    table.style.setProperty('--sheet-key-offset', `${width}px`);
  }
  function setArtworkWidth(width) {
    if (width === artworkWidth) return;
    artworkWidth = width;
    table.style.setProperty('--sheet-artwork-offset', `${width}px`);
  }
  const headerResize = new ResizeObserver((entries) => {
    for (const entry of entries) if (entry.target === selectionHeader || entry.target === artworkHeader) {
      const box = entry.borderBoxSize;
      const width = box?.[0]?.inlineSize ?? box?.inlineSize ?? entry.target.getBoundingClientRect().width;
      if (entry.target === selectionHeader) setSelectionWidth(width);
      else setArtworkWidth(width);
    }
  });
  function observeSelectionHeader() {
    const header = table.tHead?.querySelector('th.sel');
    if (header !== selectionHeader) {
      if (selectionHeader) headerResize.unobserve(selectionHeader);
      selectionHeader = header;
      if (header) headerResize.observe(header);
      else setSelectionWidth(0);
    }
    // A table filling spare pane width may distribute fractional CSS pixels.
    // File follows the actual artwork band, without synchronous layout reads
    // during row refreshes or wheel scrolling.
    const artwork = table.tHead?.querySelector('th.artwork-col');
    if (artwork !== artworkHeader) {
      if (artworkHeader) headerResize.unobserve(artworkHeader);
      artworkHeader = artwork;
      if (artwork) headerResize.observe(artwork);
      else setArtworkWidth(0);
    }
  }
  // Fit/resize can rebuild headers without repainting track rows.
  const headerChanges = new MutationObserver(observeSelectionHeader);
  if (table.tHead?.rows[0]) headerChanges.observe(table.tHead.rows[0], { childList: true });
  // The owner's scroll listener cuts the virtual window before our ordinary
  // listeners run. Capture marks just that dispatch, so scroll-only paints can
  // reuse a selection summary; selection/model changes still recompute it.
  wrap.addEventListener('scroll', () => {
    scrollRefresh = true;
    queueMicrotask(() => { scrollRefresh = false; });
  }, { capture: true, passive: true });
  function pointOf(target) {
    const cell = target.closest?.('td');
    const tr = cell?.closest('tr[data-path]');
    if (!tr || tr.classList.contains('unreadable') || cell.cellIndex < TABLE_DATA_COLUMN_OFFSET) return null;
    const key = columns()[cell.cellIndex - TABLE_DATA_COLUMN_OFFSET];
    return key ? { path: tr.dataset.path, key } : null;
  }
  function cellOf(point) {
    if (!point) return null;
    const tr = options.getRow(point.path);
    const index = columns().indexOf(point.key);
    return index < 0 ? null : tr?.cells[index + TABLE_DATA_COLUMN_OFFSET] || null;
  }
  function range() {
    if (!active || !anchor) return null;
    const paths = rows(), keys = columns();
    const a = rowIndex(anchor.path), b = rowIndex(active.path);
    const x = keys.indexOf(anchor.key), y = keys.indexOf(active.key);
    if (Math.min(a, b, x, y) < 0) return null;
    const top = Math.min(a, b), left = Math.min(x, y);
    return { top, left, paths: paths.slice(top, Math.max(a, b) + 1), keys: keys.slice(left, Math.max(x, y) + 1) };
  }
  function focus() {
    if (!active || options.isReviewing()) return;
    options.reveal(active.path);
    const cell = cellOf(active);
    if (!cell || cell.isContentEditable) return;
    cell.focus({ preventScroll: true });
    // Horizontal reveal without scrollIntoView scrolling the whole workbench.
    const rect = cell.getBoundingClientRect(), bounds = wrap.getBoundingClientRect();
    const keyCell = cell.closest('tr').querySelector('.file');
    const left = active.key === 'file' ? bounds.left : Math.max(bounds.left, keyCell?.getBoundingClientRect().right || bounds.left);
    if (rect.left < left) wrap.scrollLeft -= left - rect.left;
    else if (rect.right > bounds.right) wrap.scrollLeft += rect.right - bounds.right;
  }
  function choose(point, extend = false, giveFocus = true) {
    if (!point || options.isReviewing()) return;
    if (!extend || !anchor) anchor = { ...point };
    active = { ...point };
    options.setActiveRow(point.path);
    refresh();
    if (giveFocus) focus();
  }
  function clear(giveFocus = true, paint = true) {
    const path = active?.path;
    active = anchor = null;
    if (paint) refresh();
    if (path && giveFocus) options.focusRow(path);
  }
  function refresh() {
    const paths = rows(), keys = columns();
    // The owner replaces the row array when rebuilding its view, even when its
    // path order is unchanged. Compare that order once per replacement; stable
    // arrays take the identity fast path during scrolling/selection painting.
    const changed = !sameSequence(paths, lastRows) || !sameSequence(keys, lastColumns);
    if (changed) {
      // A changed filter/group/sort must not silently widen a rectangle.
      if (active && (!paths.includes(active.path) || !keys.includes(active.key))) active = null;
      anchor = active ? { ...active } : null;
    }
    lastRows = paths;
    // Column controllers may mutate their small key list in place.
    lastColumns = keys.slice();
    const hidden = wrap.hidden || paths.length === 0;
    if (host.hidden !== hidden) host.hidden = hidden;
    attribute(table, 'aria-rowcount', options.rowCount() + 1);
    attribute(table, 'aria-colcount', keys.length + TABLE_DATA_COLUMN_OFFSET);
    attribute(table.tHead?.rows[0], 'aria-rowindex', 1);
    const r = range();
    const selected = new Set(r?.paths || []), selectedKeys = new Set(r?.keys || []);
    const reviewing = options.isReviewing();
    const reasons = new Map(keys.map((key) => [key, options.readOnly(key)]));
    for (const tr of body.querySelectorAll('tr[data-path]')) {
      if (tr.classList.contains('unreadable')) continue;
      if (active && !reviewing && tr.tabIndex !== -1) tr.tabIndex = -1;
      attribute(tr, 'aria-rowindex', options.rowNumber(tr.dataset.path));
      for (let i = TABLE_DATA_COLUMN_OFFSET; i < tr.cells.length; i++) {
        const cell = tr.cells[i], key = keys[i - TABLE_DATA_COLUMN_OFFSET];
        if (!key) continue;
        const point = { path: tr.dataset.path, key };
        const included = !reviewing && selected.has(point.path) && selectedKeys.has(key);
        let painted = paintedCells.get(cell);
        if (!painted) {
          cell.classList.add('sheet-cell');
          attribute(cell, 'role', 'gridcell');
          painted = {};
          paintedCells.set(cell, painted);
        }
        if (painted.column !== i + 1) {
          attribute(cell, 'aria-colindex', i + 1);
          painted.column = i + 1;
        }
        const readonly = reviewing || !!reasons.get(key);
        if (painted.readonly !== readonly) {
          attribute(cell, 'aria-readonly', readonly);
          painted.readonly = readonly;
        }
        if (painted.included !== included) {
          classState(cell, 'sheet-range', included);
          attribute(cell, 'aria-selected', included);
          painted.included = included;
        }
        const states = {
          active: !reviewing && !!same(point, active),
          top: included && point.path === r.paths[0], bottom: included && point.path === r.paths.at(-1),
          left: included && key === r.keys[0], right: included && key === r.keys.at(-1),
        };
        for (const [state, on] of Object.entries(states)) {
          if (painted[state] !== on) {
            classState(cell, state === 'active' ? 'sheet-active' : `sheet-range-${state}`);
            painted[state] = on;
          }
        }
        const tabIndex = states.active ? 0 : -1;
        if (cell.tabIndex !== tabIndex) cell.tabIndex = tabIndex;
      }
    }
    observeSelectionHeader();
    const count = r ? r.paths.length * r.keys.length : 0;
    const readonlyReason = r?.keys.map((key) => reasons.get(key)).find(Boolean) || '';
    const editable = !!r && !readonlyReason;
    if (!r && !reviewing && (!scrollRefresh || changed || !lastSummary)) lastSummary = options.selectionSummary();
    const snapshot = {
      label: reviewing ? 'Review the staged changes, then Apply.' : r ? `${options.label(active.key)} · ${count === 1 ? '1 cell' : `${r.paths.length} × ${r.keys.length} cells`}` : lastSummary,
      detail: reviewing ? 'Only highlighted files will be applied.' : readonlyReason || (r ? (count === 1 ? 'F2 edits · Shift + arrows select' : 'Copy, paste, or fill down') : 'Click a cell to start'),
      active: !!r, editable, canFill: editable && r.paths.length > 1,
      busy, reviewing,
    };
    if (!lastToolbar || Object.keys(snapshot).some((key) => lastToolbar[key] !== snapshot[key])) {
      toolbar.update(snapshot);
      lastToolbar = snapshot;
    }
    if (readonlyReason) {
      attribute(host, 'data-readonly-reason', readonlyReason);
      attribute(host, 'title', readonlyReason);
    } else if (host.hasAttribute('data-readonly-reason')) {
      host.removeAttribute('data-readonly-reason'); host.removeAttribute('title');
    }
    const currentCell = cellOf(active);
    const gridTabIndex = active && !reviewing && !currentCell ? 0 : -1;
    if (table.tabIndex !== gridTabIndex) table.tabIndex = gridTabIndex;
    // A virtual row can disappear during a wheel scroll. Retain keyboard focus
    // on the persistent grid, without revealing the old row or fighting the
    // wheel. Only an explicit navigation/edit action calls focus()/reveal().
    if (gridOwnsFocus && active && !reviewing && document.activeElement === document.body) {
      (currentCell || table).focus({ preventScroll: true });
    }
  }
  function move(rowDelta, colDelta, extend = false, tab = false) {
    if (!active) return false;
    const paths = rows(), keys = columns();
    let row = rowIndex(active.path), col = keys.indexOf(active.key);
    if (tab) {
      // Tab visits writable fields; arrow navigation still reaches read-only cells.
      const direction = colDelta < 0 ? -1 : 1;
      for (let steps = 0; steps < paths.length * keys.length; steps++) {
        col += direction;
        if (col >= keys.length) { col = 0; row++; }
        if (col < 0) { col = keys.length - 1; row--; }
        if (row < 0 || row >= paths.length) return false;
        if (!options.readOnly(keys[col])) break;
      }
    } else {
      row = Math.max(0, Math.min(paths.length - 1, row + rowDelta));
      col = Math.max(0, Math.min(keys.length - 1, col + colDelta));
    }
    choose({ path: paths[row], key: keys[col] }, extend);
    return true;
  }
  function beginEdit() {
    if (!active || options.isReviewing()) return;
    const reason = options.readOnly(active.key);
    if (reason) { options.error(reason); return; }
    anchor = { ...active };
    focus();
    const cell = cellOf(active);
    if (cell) options.beginEdit(cell);
    refresh();
  }
  function copyText() {
    const r = range();
    if (!r) throw new Error('Select a cell or range first.');
    if (r.paths.length * r.keys.length > MAX_CELLS) throw new Error('Copy is limited to 10,000 cells.');
    const text = encodeTsv(r.paths.map((path) => r.keys.map((key) => options.value(path, key))));
    if (text.length > MAX_TEXT) throw new Error('Copy is limited to 1 MiB of text.');
    return text;
  }
  async function writeClipboard(text) {
    // Copy events write directly. Toolbar copy uses the same native webview
    // mechanism as text fields, without requesting a new Tauri permission.
    const field = document.createElement('textarea');
    field.value = text;
    field.style.cssText = 'position:fixed;left:-10000px;top:0;opacity:0';
    document.body.append(field);
    field.select();
    let copied = false;
    try { copied = document.execCommand('copy'); } finally { field.remove(); }
    if (!copied) {
      if (!navigator.clipboard?.writeText) throw new Error('Clipboard access is unavailable. Select cells and press Ctrl/Cmd+C.');
      await navigator.clipboard.writeText(text);
    }
  }
  async function readClipboard() {
    if (TAURI) {
      const result = await TAURI.invoke('plugin:clipboard-manager|read_text');
      return typeof result === 'string' ? result : result?.plainText?.text || '';
    }
    if (!navigator.clipboard?.readText) throw new Error('Clipboard access is unavailable. Select a cell and press Ctrl/Cmd+V.');
    return navigator.clipboard.readText();
  }
  async function stage(matrix, kind) {
    if (busy) return;
    const r = range();
    if (!r) throw new Error('Select a cell or range first.');
    const problem = options.batchProblem();
    if (problem) throw new Error(problem);
    const paths = rows(), keys = columns();
    const multiple = r.paths.length * r.keys.length > 1;
    const repeat = matrix.length === 1 && matrix[0].length === 1 && multiple;
    const height = repeat ? r.paths.length : matrix.length;
    const width = repeat ? r.keys.length : matrix[0].length;
    if (height * width > MAX_CELLS) throw new Error('Batch edits are limited to 10,000 cells.');
    if (multiple && !repeat && (height !== r.paths.length || width !== r.keys.length)) throw new Error('Clipboard dimensions must match the selected range. A single value fills the whole range.');
    if (r.top + height > paths.length || r.left + width > keys.length) throw new Error('Paste extends beyond the visible rows or columns. Nothing was changed.');
    const edits = [], errors = [];
    for (let y = 0; y < height; y++) {
      for (let x = 0; x < width; x++) {
        const path = paths[r.top + y], key = keys[r.left + x];
        const value = String(repeat ? matrix[0][0] : matrix[y][x]);
        const readOnly = options.readOnly(key);
        const valid = options.validate(key, value, path);
        if (readOnly || !valid.ok) {
          if (errors.length < 3) errors.push(`${options.label(key)}, grid row ${options.rowNumber(path)} (${options.rowLabel(path)}): ${readOnly || valid.hint || 'Invalid value'}`);
        } else edits.push({ path, field: key, value });
      }
    }
    if (errors.length) throw new Error(`Nothing was staged. ${errors.join(' · ')}`);
    busy = true;
    refresh();
    try {
      await options.stage(edits, kind);
    } finally {
      busy = false;
      refresh();
    }
  }
  async function action(command) {
    try {
      if (command === 'focus') { focus(); return; }
      if (command === 'clear') { clear(); return; }
      if (command === 'edit') { beginEdit(); return; }
      if (command === 'copy') { await writeClipboard(copyText()); focus(); return; }
      if (command === 'paste') { await stage(parseTsv(await readClipboard()), 'Paste cells'); return; }
      if (command === 'fill') {
        const r = range();
        if (!r || r.paths.length < 2) throw new Error('Select at least two rows to fill down.');
        const first = r.keys.map((key) => options.value(r.paths[0], key));
        await stage(r.paths.map(() => first), 'Fill down');
      }
    } catch (error) { options.error(String(error.message || error)); }
  }
  body.addEventListener('click', (event) => {
    if (options.isReviewing() || event.target.isContentEditable) return;
    const point = pointOf(event.target);
    // Mouse modifiers belong to track selection. Cell rectangles use Shift+arrows.
    if (point && !event.ctrlKey && !event.metaKey && !event.shiftKey) choose(point, false);
  });
  body.addEventListener('contextmenu', (event) => {
    const point = pointOf(event.target);
    if (!point || point.key === 'file' || event.target.isContentEditable || options.isReviewing()) return;
    event.preventDefault();
    event.stopPropagation();
    const r = range();
    if (!r || !r.paths.includes(point.path) || !r.keys.includes(point.key)) choose(point);
    options.hideMenus();
    toolbar.openContext(event.clientX, event.clientY);
  });
  table.addEventListener('keydown', (event) => {
    if (options.isReviewing() || event.target.closest('tr.group-head')) return;
    if (event.target.isContentEditable) {
      if (event.key !== 'Tab') return; // suggestions/Enter/Escape retain their existing contract
      event.stopImmediatePropagation();
      options.finishEdit(event.target);
      if (move(0, event.shiftKey ? -1 : 1, false, true)) event.preventDefault();
      else focus(); // at the boundary, allow native Tab to leave the grid
      return;
    }
    if (event.target.closest('input,textarea,select,button')) return;
    const targetPoint = pointOf(event.target);
    if (targetPoint && !same(targetPoint, active)) choose(targetPoint, false, false);
    const rowPath = event.target.closest('tr[data-path]')?.dataset.path;
    if (!targetPoint && active && rowPath && rowPath !== active.path) active = anchor = null;
    if (!active) {
      const path = event.target.closest('tr[data-path]')?.dataset.path;
      if (!path || !['F2', 'Enter', 'ArrowLeft', 'ArrowRight'].includes(event.key)) return;
      choose({ path, key: columns().find((key) => !options.readOnly(key)) || columns()[0] }, false, false);
    }
    const modified = event.ctrlKey || event.metaKey;
    if (modified && event.key.toLowerCase() === 'd') {
      event.preventDefault(); event.stopImmediatePropagation(); void action('fill'); return;
    }
    if (modified) return; // existing row selection / app commands keep their keys
    const delta = { ArrowUp: [-1, 0], ArrowDown: [1, 0], ArrowLeft: [0, -1], ArrowRight: [0, 1] }[event.key];
    if (delta) {
      event.preventDefault(); event.stopImmediatePropagation(); move(...delta, event.shiftKey); return;
    }
    if (event.key === 'Tab') {
      if (move(0, event.shiftKey ? -1 : 1, false, true)) {
        event.preventDefault(); event.stopImmediatePropagation();
      }
    } else if (event.key === 'F2' || event.key === 'Enter') {
      event.preventDefault(); event.stopImmediatePropagation(); beginEdit();
    } else if (event.key === 'Escape') {
      event.preventDefault(); event.stopImmediatePropagation(); clear();
    } else if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
      event.preventDefault(); event.stopImmediatePropagation();
      const bounds = cellOf(active)?.getBoundingClientRect();
      if (bounds) toolbar.openContext(bounds.left, bounds.bottom);
    }
  }, true);
  table.addEventListener('copy', (event) => {
    if (!active || event.target.isContentEditable || event.target.closest('input,textarea,select,button') || options.isReviewing()) return;
    try { const text = copyText(); event.clipboardData.setData('text/plain', text); event.preventDefault(); }
    catch (error) { event.preventDefault(); options.error(error.message); }
  });
  table.addEventListener('paste', (event) => {
    if (!active || event.target.isContentEditable || event.target.closest('input,textarea,select,button') || options.isReviewing()) return;
    event.preventDefault();
    try { void stage(parseTsv(event.clipboardData.getData('text/plain')), 'Paste cells').catch((error) => options.error(error.message)); }
    catch (error) { options.error(error.message); }
  });
  // A native WebKit key event need not produce a DOM paste event. Read through
  // the already-permitted clipboard plugin there; browser tests use paste events.
  if (TAURI) table.addEventListener('keydown', (event) => {
    if (active && !event.target.isContentEditable && !event.target.closest('input,textarea,select,button') && !options.isReviewing() && (event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'v') {
      event.preventDefault(); void action('paste');
    }
  });
  // Focus ownership is separate from selection. Never reset the anchor on
  // focusin: pointer focus arrives before Shift/right-click range gestures.
  table.addEventListener('focusin', (event) => {
    gridOwnsFocus = event.target === table || !!pointOf(event.target) || event.target.matches('tr[data-path]');
  });
  table.addEventListener('focusout', (event) => {
    if (event.relatedTarget && event.relatedTarget !== document.body && !table.contains(event.relatedTarget)) gridOwnsFocus = false;
  });
  document.addEventListener('pointerdown', (event) => {
    if (!table.contains(event.target)) gridOwnsFocus = false;
  }, true);
  function beforePaint() {
    if (body.contains(document.activeElement) && !editing()) gridOwnsFocus = true;
  }
  window.addEventListener('pagehide', () => { headerResize.disconnect(); headerChanges.disconnect(); }, { once: true });
  new MutationObserver(refresh).observe(wrap, { attributes: true, attributeFilter: ['hidden'] });
  new MutationObserver(refresh).observe(document.body, { attributes: true, attributeFilter: ['class'] });
  refresh();
  return {
    refresh, beforePaint, focusActive: focus,
    hasRangeAt(target) {
      if (options.isReviewing() || !active || !anchor || (active.path === anchor.path && active.key === anchor.key)) return false;
      const point = pointOf(target), selected = point && range();
      return !!selected && selected.paths.includes(point.path) && selected.keys.includes(point.key);
    },
    clearRange: ({refresh = true} = {}) => clear(false, refresh),
  };
}
