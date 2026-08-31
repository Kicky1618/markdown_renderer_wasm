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

function firstStringMismatch(expected, actual) {
  const a = String(expected);
  const b = String(actual);
  const limit = Math.min(a.length, b.length);
  for (let index = 0; index < limit; index++) {
    if (a[index] !== b[index]) return index;
  }
  return a.length === b.length ? null : limit;
}

function firstSemanticMismatch(expected, actual) {
  const limit = Math.min(expected.length, actual.length);
  for (let index = 0; index < limit; index++) {
    if (!sameJson(expected[index], actual[index])) return index;
  }
  return expected.length === actual.length ? null : limit;
}

function stateMismatchDetails(expected, actual, maxKeys = 16) {
  const a = new Map(expected);
  const b = new Map(actual);
  const keys = [...new Set([...a.keys(), ...b.keys()])].sort();
  const result = [];
  for (const key of keys) {
    const aHas = a.has(key);
    const bHas = b.has(key);
    if (aHas === bHas && Object.is(a.get(key), b.get(key))) continue;
    result.push({
      key,
      expected: aHas ? a.get(key) : undefined,
      actual: bHas ? b.get(key) : undefined,
      expectedPresent: aHas,
      actualPresent: bHas,
    });
    if (result.length >= maxKeys) break;
  }
  return result;
}

function semanticBlockSummary(block) {
  if (!block) return null;
  const attrs = Object.fromEntries(block.attributes || []);
  const type = String(attrs.type || block.kind || "semantic").slice(0, 48);
  const id = String(attrs.id || "").slice(0, 80);
  const target = String(attrs.target || "").slice(0, 80);
  const suffix = id ? `#${id}` : target ? ` -> ${target}` : "";
  return {
    type, id, target, closed: Boolean(block.closed),
    valueLength: String(block.value || "").length,
    label: `${type}${suffix}${block.closed ? "" : " (open)"}`,
  };
}

export function compareReplayDeterminism({ expectedSource = "", actualSource = "", expectedSemantic = [], actualSemantic = [], expectedState = [], actualState = [] } = {}) {
  const normalizedExpectedSemantic = normalizeSemanticBlocks(expectedSemantic);
  const normalizedActualSemantic = normalizeSemanticBlocks(actualSemantic);
  const normalizedExpectedState = normalizeDeterminismState(expectedState);
  const normalizedActualState = normalizeDeterminismState(actualState);
  const source = String(expectedSource) === String(actualSource);
  const semantic = sameJson(normalizedExpectedSemantic, normalizedActualSemantic);
  const state = sameJson(normalizedExpectedState, normalizedActualState);
  const semanticAt = semantic ? null : firstSemanticMismatch(normalizedExpectedSemantic, normalizedActualSemantic);
  const stateChanges = state ? [] : stateMismatchDetails(normalizedExpectedState, normalizedActualState);
  return {
    verified: source && semantic && state,
    source,
    semantic,
    state,
    mismatch: {
      sourceAt: source ? null : firstStringMismatch(expectedSource, actualSource),
      sourceLengths: source ? null : [String(expectedSource).length, String(actualSource).length],
      semanticAt,
      semanticExpected: semanticAt === null ? null : semanticBlockSummary(normalizedExpectedSemantic[semanticAt]),
      semanticActual: semanticAt === null ? null : semanticBlockSummary(normalizedActualSemantic[semanticAt]),
      stateKeys: stateChanges.map(change => change.key),
      stateChanges,
    },
  };
}
