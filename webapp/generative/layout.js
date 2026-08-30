function finiteNumber(value, fallback) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : fallback;
}

function clamp(value, min, max) {
  return Math.min(max, Math.max(min, value));
}

/** Normalize untrusted LLM layout attributes into bounded CSS-safe values. */
export function layoutSpec(config = {}) {
  const columns = Math.round(clamp(finiteNumber(config.columns, 2), 1, 4));
  const gap = clamp(finiteNumber(config.gap, 14), 0, 40);
  const minWidth = clamp(finiteNumber(config.min, 180), 120, 480);
  return {
    id: String(config.id || ""),
    title: String(config.title || config.label || "Generated layout"),
    columns,
    gap,
    minWidth,
  };
}

/** A generated component can span cells, but never escape the current grid. */
export function componentSpan(config = {}, columns = 1) {
  return Math.round(clamp(finiteNumber(config.span, 1), 1, Math.max(1, columns)));
}
