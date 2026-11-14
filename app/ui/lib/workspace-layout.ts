import { getContext, setContext } from 'svelte';

export const DOCK_AREAS = ['top', 'left', 'right', 'bottom'] as const;
export type DockArea = typeof DOCK_AREAS[number];
export type DockModuleInfo = { id: string; title: string; area: DockArea; index: number; weight: number; collapsed?: boolean };
export type DockSnapshot = { modules: DockModuleInfo[]; left: number; top: number; bottom: number; active: string; compact: boolean };
type Placement = { area: DockArea; index: number; weight: number; collapsed?: boolean };
type Module = { id: string; title: string; node: HTMLElement; defaults: Placement };
const CONTEXT = Symbol('tagrex-dock-workspace');
const clamp = (value: number, min: number, max: number) => Math.max(min, Math.min(max, value));
const areaValid = (area: unknown): area is DockArea => DOCK_AREAS.includes(area as DockArea);

/** A small DOM docking boundary. The controller owns placement only: the Svelte
 * module and every controller-owned child retain their original identity. */
export function createWorkspaceLayout(storageKey = 'tagrex.workspace.v1') {
  const modules = new Map<string, Module>();
  const placements = new Map<string, Placement>();
  const hosts = new Map<DockArea, HTMLElement>();
  const listeners = new Set<(snapshot: DockSnapshot) => void>();
  let left = 72;
  let top = 138;
  let bottom = 190;
  let active = '';
  let compact = false;
  let surface: HTMLElement | null = null;
  try {
    const saved = JSON.parse(localStorage.getItem(storageKey) || 'null');
    if (saved?.version === 1 && saved.modules && typeof saved.modules === 'object') {
      for (const [id, value] of Object.entries(saved.modules).slice(0, 64)) {
        const item = value as Partial<Placement>;
        if (id.length <= 80 && areaValid(item.area) && Number.isFinite(item.index) && Number.isFinite(item.weight)) {
          placements.set(id, { area: item.area, index: clamp(item.index!, 0, 63), weight: clamp(item.weight!, .1, 20), collapsed: item.collapsed === true });
        }
      }
      if (Number.isFinite(saved.left)) left = clamp(saved.left, 20, 82);
      if (Number.isFinite(saved.top)) top = clamp(saved.top, 70, 600);
      if (Number.isFinite(saved.bottom)) bottom = clamp(saved.bottom, 70, 600);
      if (typeof saved.active === 'string') active = saved.active;
      if (saved.transformSizingRevision !== 2) {
        const transforms = saved.modules.transformations;
        const inspector = saved.modules.inspector;
        const files = saved.modules.files;
        const oldDefault = transforms?.area === 'right' && transforms.index === 0
          && inspector?.area === 'right' && inspector.index === 1 && inspector.weight === 1
          && files?.area === 'left' && files.index === 0 && files.weight === 1
          && (transforms.weight === .24 || transforms.weight === .32);
        // Only the two shipped draft defaults are corrected. User-resized
        // ratios, moved modules, column width and all other saved state survive.
        if (oldDefault && placements.has('transformations')) {
          placements.get('transformations')!.weight = .6;
          saved.modules.transformations = { ...transforms, weight: .6 };
        }
        saved.transformSizingRevision = 2;
        localStorage.setItem(storageKey, JSON.stringify(saved));
      }

    }
  } catch { /* A missing, full or malformed local store must never hide work. */ }
  const ordered = (area: DockArea) => [...modules.values()].filter(module => placements.get(module.id)?.area === area)
    .sort((a, b) => placements.get(a.id)!.index - placements.get(b.id)!.index);
  function snapshot(): DockSnapshot {
    return { modules: DOCK_AREAS.flatMap(area => ordered(area).map((module, index) => ({ id: module.id, title: module.title, ...placements.get(module.id)!, index }))), left, top, bottom, active, compact };
  }
  function save() {
    try { localStorage.setItem(storageKey, JSON.stringify({ version: 1, transformSizingRevision: 2, modules: Object.fromEntries(placements), left, top, bottom, active })); } catch { /* Session-only layouts remain usable. */ }
  }
  function paint(persist = true) {
    if (!active) active = modules.keys().next().value || '';
    const focused = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    for (const area of DOCK_AREAS) {
      const host = hosts.get(area);
      if (!host) continue;
      const sequence = ordered(area);
      sequence.forEach((module, index) => {
        const existing = [...host.children].filter(node => node.hasAttribute('data-dock-module'))[index] || null;
        if (existing !== module.node) {
          // moveBefore preserves focus/iframe state where supported. The
          // fallback still moves the same native nodes, never clones them.
          const atomic = (host as HTMLElement & { moveBefore?: (node: Node, before: Node | null) => void }).moveBefore;
          if (atomic && module.node.isConnected) atomic.call(host, module.node, existing);
          else host.insertBefore(module.node, existing);
        }
        module.node.dataset.dockArea = area;
        module.node.dataset.dockActive = String(module.id === active);
        module.node.dataset.dockCollapsed = String(placements.get(module.id)?.collapsed === true);
        module.node.style.setProperty('--dock-weight', String(placements.get(module.id)!.weight));
      });
    }
    if (focused?.isConnected && document.activeElement !== focused) focused.focus({ preventScroll: true });
    if (persist) save();
    publish();
  }
  function publish() {
    const current = snapshot();
    for (const listener of listeners) listener(current);
  }
  function move(id: string, area: DockArea, index?: number) {
    if (!modules.has(id) || !areaValid(area)) return;
    const sequence = ordered(area).filter(module => module.id !== id);
    sequence.splice(clamp(index ?? sequence.length, 0, sequence.length), 0, modules.get(id)!);
    sequence.forEach((module, position) => placements.set(module.id, { ...placements.get(module.id)!, area, index: position }));
    active = id;
    paint();
  }
  return {
    snapshot,
    surface(node: HTMLElement) { surface = node; return () => { surface = null; }; },
    beginResize() { document.body.classList.add('resizing-dock'); },
    finishResize() {
      document.body.classList.remove('resizing-dock');
      save();
      publish();
      document.dispatchEvent(new CustomEvent('tagrex:dock-resize-end'));
    },
    ready() { if (!modules.has(active)) active = modules.keys().next().value || ''; paint(false); },
    subscribe(listener: (snapshot: DockSnapshot) => void) { listeners.add(listener); listener(snapshot()); return () => { listeners.delete(listener); }; },
    host(area: DockArea, node: HTMLElement) { hosts.set(area, node); paint(false); return () => { hosts.delete(area); }; },
    register(id: string, title: string, node: HTMLElement, defaultArea: DockArea = 'right', defaultIndex?: number, defaultWeight = 1) {
      if (modules.has(id)) throw new Error(`Duplicate dock module: ${id}`);
      const defaults = { area: defaultArea, index: defaultIndex ?? ordered(defaultArea).length, weight: clamp(defaultWeight, .1, 20), collapsed: false };
      modules.set(id, { id, title, node, defaults });
      if (!placements.has(id)) placements.set(id, { ...defaults });
      paint(false);
      return () => { modules.delete(id); paint(false); };
    },
    move,
    reorder(id: string, delta: number) {
      const placement = placements.get(id);
      if (placement) move(id, placement.area, Math.max(0, ordered(placement.area).findIndex(module => module.id === id) + delta));
    },
    reveal(id: string) {
      if (!modules.has(id)) return;
      active = id;
      const placement = placements.get(id);
      if (placement) placement.collapsed = false;
      paint();
    },
    toggleCollapsed(id: string) {
      const placement = placements.get(id);
      if (!placement) return;
      placement.collapsed = !placement.collapsed;
      // Keep the expanded weight intact. Folding only changes available space,
      // never the user's chosen ratio for reopening or moving the module.
      paint();
    },
    setCompact(value: boolean) { compact = value; paint(false); },
    resize(area: 'left' | 'top' | 'bottom', value: number, commit = true) {
      if (area === 'left') {
        left = clamp(value, 20, 82);
        surface?.style.setProperty('--dock-left', `${left}%`);
      } else if (area === 'top') {
        top = clamp(value, 70, 600);
        surface?.style.setProperty('--dock-top-size', `min(${top}px, 36%)`);
      } else {
        bottom = clamp(value, 70, 600);
        surface?.style.setProperty('--dock-bottom-size', `min(${bottom}px, 36%)`);
      }
      // Live resizing changes geometry only. No structural paint, Svelte-wide
      // notifications or synchronous storage is allowed on its frame path.
      if (commit) { save(); publish(); }
    },
    resizeModule(id: string, fraction: number, commit = true) {
      const placement = placements.get(id);
      if (!placement) return;
      const siblings = ordered(placement.area);
      const index = siblings.findIndex(module => module.id === id);
      const next = siblings[index + 1];
      if (!next) return;
      const following = placements.get(next.id)!;
      if (placement.collapsed || following.collapsed) return;
      const total = placement.weight + following.weight;
      const ratio = clamp(fraction, Math.max(.1 / total, 1 - 20 / total), Math.min(20 / total, 1 - .1 / total));
      // Only this adjacent pair changes; every other module keeps its share.
      placement.weight = total * ratio;
      following.weight = total * (1 - ratio);
      modules.get(id)!.node.style.setProperty('--dock-weight', String(placement.weight));
      next.node.style.setProperty('--dock-weight', String(following.weight));
      if (commit) { save(); publish(); }
    },
    reset() {
      placements.clear();
      for (const module of modules.values()) placements.set(module.id, { ...module.defaults });
      left = 72; top = 138; bottom = 190;
      active = modules.keys().next().value || '';
      paint();
    },
  };
}
export type WorkspaceLayout = ReturnType<typeof createWorkspaceLayout>;
export function provideWorkspaceLayout(layout: WorkspaceLayout) { setContext(CONTEXT, layout); }
export function useWorkspaceLayout() { return getContext<WorkspaceLayout>(CONTEXT); }
