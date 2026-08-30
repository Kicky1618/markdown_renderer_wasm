const KEY = /^[A-Za-z_][A-Za-z0-9_.-]{0,63}$/;
const SENSITIVE = /(?:password|passwd|secret|token|api[-_.]?key|credential|auth)/i;
const RESERVED = new Set([
  "type", "id", "label", "title", "tab", "span", "when", "unit",
  "action", "submit", "state", "input", "options", "values", "value",
]);

function primitive(value) {
  const text = String(value ?? "").trim();
  if (text === "true") return true;
  if (text === "false") return false;
  if (text === "null") return null;
  if (/^-?(?:\d+|\d*\.\d+)(?:[eE][+-]?\d+)?$/.test(text)) {
    const number = Number(text);
    if (Number.isFinite(number)) return number;
  }
  return text.slice(0, 512);
}

/** Normalize an LLM `type=state` descriptor into bounded primitive state updates. */
export function statePatch(config = {}, maxEntries = 32) {
  const limit = Math.max(0, Math.min(32, Number(maxEntries) || 32));
  const updates = [];
  for (const [rawKey, rawValue] of Object.entries(config)) {
    if (updates.length >= limit) break;
    const key = String(rawKey);
    if (RESERVED.has(key) || !KEY.test(key) || SENSITIVE.test(key)) continue;
    if (String(rawValue ?? "").trim() === "") continue;
    updates.push([key, primitive(rawValue)]);
  }
  return updates;
}

export function statePatchSignature(config = {}) {
  return JSON.stringify(statePatch(config));
}
