const SENSITIVE = /(?:password|passwd|secret|token|api[-_.]?key|credential|auth)/i;

function primitive(value) {
  return typeof value === "string" || typeof value === "boolean" || value === null || (typeof value === "number" && Number.isFinite(value));
}

export function normalizeDeterminismState(entries = [], maxEntries = 128) {
  const result = [];
  for (const entry of entries) {
    if (!Array.isArray(entry) || entry.length < 2) continue;
    const key = String(entry[0] ?? "").slice(0, 80);
    if (!key || SENSITIVE.test(key) || !primitive(entry[1])) continue;
    result.push([key, entry[1]]);
    if (result.length >= maxEntries) break;
  }
  result.sort((a, b) => a[0].localeCompare(b[0]));
  return result;
}

export function normalizeSemanticBlocks(blocks = [], maxBlocks = 512) {
  const result = [];
  for (const block of blocks) {
    if (!block || typeof block !== "object") continue;
    const attributes = Object.entries(block.attributes || {})
      .map(([key, value]) => [String(key), String(value ?? "")])
      .sort((a, b) => a[0].localeCompare(b[0]));
    result.push({
      kind: String(block.kind || "").slice(0, 48),
      attributes,
      value: String(block.value || ""),
      closed: Boolean(block.closed),
    });
    if (result.length >= maxBlocks) break;
  }
  return result;
}

export function expectedReplaySource(replay) {
  const body = (replay?.recording?.chunks || []).map(chunk => String(chunk?.text || "")).join("");
  if (replay?.kind === "append") return String(replay?.before?.source || "") + String(replay?.prefix || "") + body;
  return body;
}

function sameJson(a, b) {
  return JSON.stringify(a) === JSON.stringify(b);
}

export function compareReplayDeterminism({ expectedSource = "", actualSource = "", expectedSemantic = [], actualSemantic = [], expectedState = [], actualState = [] } = {}) {
  const source = String(expectedSource) === String(actualSource);
  const semantic = sameJson(normalizeSemanticBlocks(expectedSemantic), normalizeSemanticBlocks(actualSemantic));
  const state = sameJson(normalizeDeterminismState(expectedState), normalizeDeterminismState(actualState));
  return { verified: source && semantic && state, source, semantic, state };
}
