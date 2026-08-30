const SENSITIVE = /(?:password|passwd|secret|token|api[-_.]?key|credential|auth)/i;

function cleanScalar(value) {
  if (value === null) return null;
  if (typeof value === "boolean") return value;
  if (typeof value === "number") return Number.isFinite(value) ? value : undefined;
  if (typeof value === "string") return value.slice(0, 160);
  if (value === undefined) return undefined;
  return String(value).slice(0, 160);
}

function changed(from, to) {
  return !Object.is(from, to);
}

/**
 * Build a bounded, display-only diff for staged semantic side effects.
 * Inputs are already policy-normalized state/component patches.
 */
export function buildReviewDiff({
  state = new Map(),
  components = new Map(),
  statePatches = [],
  componentPatches = [],
  maxChanges = 24,
} = {}) {
  const limit = Math.max(1, Math.min(64, Number(maxChanges) || 24));
  const stateMap = state instanceof Map ? state : new Map(Object.entries(state || {}));
  const componentMap = components instanceof Map ? components : new Map(Object.entries(components || {}));
  const stagedState = new Map(stateMap);
  const stagedComponents = new Map();
  for (const [id, config] of componentMap) stagedComponents.set(id, { ...(config || {}) });

  const stateChanges = [];
  const componentChanges = [];

  for (const patch of statePatches) {
    for (const [rawKey, rawValue] of patch || []) {
      if (stateChanges.length + componentChanges.length >= limit) break;
      const key = String(rawKey || "");
      if (!key || SENSITIVE.test(key)) continue;
      const from = cleanScalar(stagedState.get(key));
      const to = cleanScalar(rawValue);
      if (to === undefined || !changed(from, to)) continue;
      stateChanges.push({ key, from, to });
      stagedState.set(key, rawValue);
    }
  }

  for (const patch of componentPatches) {
    if (stateChanges.length + componentChanges.length >= limit) break;
    const target = String(patch?.target || "");
    if (!target || !patch?.values) continue;
    const config = { ...(stagedComponents.get(target) || {}) };
    for (const [field, rawValue] of Object.entries(patch.values)) {
      if (stateChanges.length + componentChanges.length >= limit) break;
      const from = cleanScalar(config[field]);
      const to = cleanScalar(rawValue);
      if (to === undefined || !changed(from, to)) continue;
      componentChanges.push({ target, field, from, to });
      config[field] = rawValue;
    }
    stagedComponents.set(target, config);
  }

  return {
    stateChanges,
    componentChanges,
    total: stateChanges.length + componentChanges.length,
    truncated: stateChanges.length + componentChanges.length >= limit,
  };
}

export function formatReviewValue(value) {
  if (value === undefined) return "∅";
  if (value === null) return "null";
  if (typeof value === "string") return value || "\"\"";
  return String(value);
}
