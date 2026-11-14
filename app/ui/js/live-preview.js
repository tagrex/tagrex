// Raw pending inputs. Transformed output is never fed back into this layer.
import { edits, tracks, trackAt, setPendingImportCover } from './state.js';
let imported = new Map();
let cover = null;
let coverPaths = [];
let coverAssignments = [];
let coverOperations = new Map();
let coverPlanTemplate = null;
let revision = 0;
let importVersion = 0;
let importBusy = false;
let importHistory = [];
const invalid = new Set();
export function liveInputRevision() { return revision; }
export function importedValue(path, field) { return imported.get(path)?.get(field); }
export function liveInputChanged() {
  revision++;
  document.dispatchEvent(new CustomEvent('tagrex:live-input'));
}
export function recordLiveField(paths, field, value, valid = true) {
  for (const path of paths) {
    if (!trackAt(path)) continue;
    const base = imported.get(path)?.get(field) ?? trackAt(path)?.tags?.[field] ?? '';
    if (value === base && valid) {
      edits.get(path)?.delete(field);
      if (!edits.get(path)?.size) edits.delete(path);
    } else {
      if (!edits.has(path)) edits.set(path, new Map());
      edits.get(path).set(field, value);
    }
    const key = path + '\0' + field;
    if (valid) invalid.delete(key); else invalid.add(key);
  }
  liveInputChanged();
}
export function hasInvalidLiveFields(locked = []) {
  const locks = new Set(locked);
  for (const key of invalid) if (!locks.has(key.slice(key.indexOf('\0') + 1))) return true;
  return false;
}
// Any rejected proposed value blocks the whole live operation. No partial
// success is allowed to silently omit a bad tag, image or block snapshot.
export function hasInvalidPreview(plan) {
  const badImages = images => images?.some(image => image.invalid);
  return !!plan?.changes?.some(change => change.invalid ||
    change.tag_changes?.some(field => field.invalid) ||
    (change.cover_change && (change.cover_change.invalid || badImages(change.cover_change.old) || badImages(change.cover_change.new))) ||
    change.block_changes?.some(block => block.invalid || badImages(block.old?.covers) || badImages(block.new?.covers)));
}
export function liveInputs() {
  const combined = new Map([...imported].map(([path, fields]) => [path, new Map(fields)]));
  for (const [path, fields] of edits) {
    if (!combined.has(path)) combined.set(path, new Map());
    for (const [field, value] of fields) combined.get(path).set(field, value);
  }
  const values = [];
  for (const [path, fields] of combined) if (trackAt(path)) {
    for (const [field, value] of fields) values.push({ path, field, value });
  }
  return { edits: values, cover:null, coverPaths:[], basePlan:coverPlanTemplate && coverOperations.size ? {...coverPlanTemplate, changes:[...coverOperations.values()]} : null, covers:coverAssignments.map(item => ({cover:item.cover, paths:item.paths.filter(path => !!trackAt(path))})).filter(item => item.paths.length), paths:[...new Set([...combined.keys(), ...coverAssignments.flatMap(item => item.paths), ...coverOperations.keys()])].filter(path => !!trackAt(path)) };
}
export function hasLiveInputs() { return edits.size > 0 || imported.size > 0 || coverAssignments.length > 0 || coverOperations.size > 0; }
export function isLiveImportBusy() { return importBusy; }
export function adoptLiveCoverPlan(plan, paths) {
  coverPlanTemplate = {...plan, cover_assets:[]};
  for (const path of paths) coverOperations.delete(path);
  for (const change of plan.changes || []) if (change.cover_change) coverOperations.set(change.path,{...change,tag_changes:[]});
  liveInputChanged();
}
export function beginLiveImport() { importBusy = true; const version = ++importVersion; liveInputChanged(); return version; }
export function finishLiveImport(version) { if (version === importVersion && importBusy) { importBusy = false; liveInputChanged(); } }
export function isLiveImportCurrent(version) { return version === importVersion; }
export function replaceLiveImport(plan, image, paths, version) {
  if (!isLiveImportCurrent(version)) return false;
  importHistory.push({ imported:new Map(imported), cover, coverPaths, coverAssignments });
  if (importHistory.length > 20) importHistory.shift();
  imported = new Map(imported);
  const affected = new Set(paths);
  for (const path of affected) imported.delete(path);
  coverAssignments = coverAssignments.map(item => ({cover:item.cover, paths:item.paths.filter(path => !affected.has(path))})).filter(item => item.paths.length);
  if (image) coverAssignments.push({cover:image, paths:[...paths]});
  for (const change of plan.changes || []) {
    const fields = new Map((change.tag_changes || []).map(change => [change.field, change.new ?? '']));
    if (fields.size) imported.set(change.path, fields);
  }
  cover = image;
  coverPaths = image ? [...paths] : [];
  setPendingImportCover(null);
  liveInputChanged();
  return true;
}
export function undoLiveImport() {
  if (importBusy) { importVersion++; importBusy = false; liveInputChanged(); return true; }
  const previous = importHistory.pop();
  if (!previous) return false;
  importVersion++;
  ({ imported, cover, coverPaths, coverAssignments } = previous);
  liveInputChanged();
  return true;
}
export function clearLiveInputs({ notify = false } = {}) {
  edits.clear(); imported.clear(); invalid.clear(); cover = null; coverPaths = []; coverAssignments = []; coverOperations.clear(); coverPlanTemplate = null; importHistory = []; importBusy = false; importVersion++;
  setPendingImportCover(null);
  revision++;
  if (notify) document.dispatchEvent(new CustomEvent('tagrex:live-input'));
}
export function removeAppliedLiveInputs(paths) {
  for (const path of paths) {
    edits.delete(path); imported.delete(path); coverOperations.delete(path);
    for (const key of invalid) if (key.startsWith(path + '\0')) invalid.delete(key);
  }
  coverPaths = coverPaths.filter(path => !paths.has(path));
  if (!coverPaths.length) cover = null;
  coverAssignments = coverAssignments.map(item => ({cover:item.cover, paths:item.paths.filter(path => !paths.has(path))})).filter(item => item.paths.length);
  importHistory = [];
  revision++;
}
export function optimisticLivePlan() {
  const { edits: list } = liveInputs();
  const grouped = new Map();
  for (const edit of list) {
    const old = trackAt(edit.path)?.tags?.[edit.field] || null;
    const value = edit.value || null;
    if (old === value) continue;
    if (!grouped.has(edit.path)) grouped.set(edit.path, []);
    grouped.get(edit.path).push({field:edit.field, old, new:value, invalid:invalid.has(edit.path+'\0'+edit.field)});
  }
  const changes = new Map([...grouped].map(([path,tag_changes]) => [path,{path,tag_changes,rename_to:null,cover_change:null}]));
  for (const [path,change] of coverOperations) changes.set(path,{...change,tag_changes:changes.get(path)?.tag_changes || []});
  return {description:'Edit tags', changes:[...changes.values()], notes:[], locked_skipped:[]};
}
