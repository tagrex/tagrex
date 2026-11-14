export const WORKSPACE_MODES = ['tagger', 'renamer', 'generator', 'deduplicator', 'exporter'] as const;
export const WORKSPACE_SUBTABS = ['online', 'editor', 'filename', 'fromtext'] as const;
export type WorkspaceMode = typeof WORKSPACE_MODES[number];
export type WorkspaceSubtab = typeof WORKSPACE_SUBTABS[number];
export type WorkspaceState = Readonly<{ mode: WorkspaceMode; subtab: WorkspaceSubtab }>;
type Controller = { setMode(mode: WorkspaceMode): void; setSubtab(subtab: WorkspaceSubtab): void };

let state: WorkspaceState = { mode: 'tagger', subtab: 'online' };
let controller: Controller | undefined;
const subscribers = new Set<(state: WorkspaceState) => void>();

function publish(next: WorkspaceState) {
  if (state.mode === next.mode && state.subtab === next.subtab) return false;
  state = next;
  subscribers.forEach(subscriber => subscriber(state));
  return true;
}

export function subscribeWorkspaceState(subscriber: (state: WorkspaceState) => void) {
  subscribers.add(subscriber);
  subscriber(state);
  return () => { subscribers.delete(subscriber); };
}

// Controller-driven restores publish without simulating UI events or invoking
// their own commands again. UI requests use the same snapshot, then call once.
export function publishWorkspaceMode(mode: WorkspaceMode) {
  publish({ ...state, mode });
}
export function publishWorkspaceSubtab(subtab: WorkspaceSubtab) {
  publish({ ...state, subtab });
}
export function setWorkspaceMode(value: string) {
  if (!WORKSPACE_MODES.includes(value as WorkspaceMode)) return;
  const mode = value as WorkspaceMode;
  if (publish({ ...state, mode })) controller?.setMode(mode);
}
export function setWorkspaceSubtab(value: string) {
  if (!WORKSPACE_SUBTABS.includes(value as WorkspaceSubtab)) return;
  const subtab = value as WorkspaceSubtab;
  if (publish({ ...state, subtab })) controller?.setSubtab(subtab);
}
export function connectWorkspaceController(next: Controller) {
  controller = next;
  return () => { if (controller === next) controller = undefined; };
}
