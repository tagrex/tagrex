// Artwork bytes travel once per plan; UI consumers retain the familiar inline
// cover shape. Copy only changed containers, preserving the pending UI draft.
function mapCovers(plan, mapCover) {
  if (!plan.changes.some(change => change.cover_change || change.block_changes?.length)) return plan;
  return {
    ...plan,
    changes: plan.changes.map(change => !change.cover_change && !change.block_changes?.length ? change : ({
      ...change,
      ...(change.cover_change ? { cover_change: {
        ...change.cover_change,
        ...(change.cover_change.old ? { old: change.cover_change.old.map(mapCover) } : {}),
        ...(change.cover_change.new ? { new: change.cover_change.new.map(mapCover) } : {}),
      } } : {}),
      ...(change.block_changes ? { block_changes: change.block_changes.map(block => ({
        ...block,
        ...(block.old?.covers ? { old: { ...block.old, covers: block.old.covers.map(mapCover) } } : {}),
        ...(block.new?.covers ? { new: { ...block.new, covers: block.new.covers.map(mapCover) } } : {}),
      })) } : {}),
    })),
  };
}

export function hydratePlan(plan) {
  if (!plan || !Array.isArray(plan.changes)) return plan;
  const assets = plan.cover_assets ?? [];
  if (!Array.isArray(assets)) throw new Error('Invalid artwork reference pool.');
  const { cover_assets, ...inlinePlan } = plan;
  const hydrated = mapCovers(inlinePlan, cover => {
    if (cover.data_ref == null) return cover;
    const index = cover.data_ref;
    if (!Number.isSafeInteger(index) || index < 0 || typeof assets[index] !== 'string'
      || (cover.data_base64 && cover.data_base64 !== assets[index])) {
      throw new Error('Invalid artwork reference. Preview the artwork again.');
    }
    const { data_ref, ...inline } = cover;
    return { ...inline, data_base64: assets[index] };
  });
  return hydrated;
}

export function compactPlan(plan) {
  const inline = hydratePlan(plan);
  if (!inline || !Array.isArray(inline.changes)) return inline;
  const assets = [], indices = new Map();
  const compact = mapCovers(inline, cover => {
    const data = cover.data_base64;
    if (typeof data !== 'string' || !data.length) return cover;
    let index = indices.get(data);
    if (index === undefined) {
      index = assets.length;
      indices.set(data, index);
      assets.push(data);
    }
    return { ...cover, data_base64: '', data_ref: index };
  });
  if (assets.length) compact.cover_assets = assets;
  return compact;
}

export function hydratePlanResponse(result) {
  if (Array.isArray(result?.changes)) return hydratePlan(result);
  return Array.isArray(result?.plan?.changes) ? { ...result, plan: hydratePlan(result.plan) } : result;
}

// Older backends ignore unknown JSON fields. Never send them an empty inline
// image plus a reference they cannot resolve. New backends advertise the pool
// on artwork previews with a nonempty pool. Tag-only previews omit the pool,
// so keep sending inline data until an artwork preview advertises support.
export function createPlanCodec() {
  let pooled = false;
  return {
    prepare(args) {
      return pooled && args?.plan ? { ...args, plan: compactPlan(args.plan) } : args;
    },
    accept(result) {
      const plan = Array.isArray(result?.changes) ? result : result?.plan;
      if (Array.isArray(plan?.cover_assets)) pooled = true;
      return hydratePlanResponse(result);
    },
  };
}
