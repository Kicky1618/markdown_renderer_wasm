const ID = /^[A-Za-z_][A-Za-z0-9_.-]{0,63}$/;
const ALLOWED = new Set([
  "label", "title", "value", "unit", "trend", "min", "max", "step",
  "options", "values", "placeholder", "when", "height", "width",
]);

function cleanValue(key, value) {
  const text = String(value ?? "").trim();
  if (!text) return undefined;
  const max = key === "values" || key === "options" ? 2048 : 512;
  return text.slice(0, max);
}

/**
 * Normalize a closed `type=patch` descriptor.
 * Structural / executable fields are deliberately not patchable.
 */
export function componentPatch(config = {}) {
  const target = String(config.target ?? "").trim();
  if (!ID.test(target)) return null;
  const values = Object.create(null);
  for (const key of ALLOWED) {
    if (!(key in config)) continue;
    const value = cleanValue(key, config[key]);
    if (value !== undefined) values[key] = value;
  }
  return Object.keys(values).length ? { target, values } : null;
}

export function mergeComponentPatches(patches = []) {
  const merged = new Map();
  for (const patch of patches) {
    if (!patch?.target || !patch?.values) continue;
    const previous = merged.get(patch.target) || Object.create(null);
    merged.set(patch.target, Object.assign(Object.create(null), previous, patch.values));
  }
  return merged;
}

export function componentPatchSignature(patches = []) {
  return JSON.stringify(patches.map(patch => patch ? [patch.target, patch.values] : null));
}
