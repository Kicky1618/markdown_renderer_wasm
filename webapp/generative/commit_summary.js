const SENSITIVE_KEY = /(?:password|passwd|secret|token|api[-_.]?key|credential|auth)/i;

function stateMap(snapshot) {
  return new Map(Array.isArray(snapshot?.state) ? snapshot.state : []);
}

function cleanList(values, max = 12) {
  const seen = new Set();
  const result = [];
  for (const raw of values) {
    const value = String(raw || "").trim().slice(0, 80);
    if (!value || seen.has(value) || SENSITIVE_KEY.test(value)) continue;
    seen.add(value);
    result.push(value);
    if (result.length >= max) break;
  }
  return result;
}

function descriptorHeaders(markdown, max = 64) {
  const headers = [];
  const source = String(markdown || "");
  const fence = /^:::llm\s+ui\b([^\r\n]*)/gmi;
  for (const match of source.matchAll(fence)) {
    const attributes = Object.create(null);
    const tail = match[1] || "";
    const attr = /([A-Za-z_][A-Za-z0-9_.-]*)=(?:"([^"]*)"|'([^']*)'|([^\s]+))/g;
    for (const item of tail.matchAll(attr)) {
      attributes[item[1]] = item[2] ?? item[3] ?? item[4] ?? "";
    }
    headers.push(attributes);
    if (headers.length >= max) break;
  }
  return headers;
}

export function summarizeModelCommit({
  before,
  after,
  responseText = "",
  format = "",
  chunks = 0,
  firstUiMs = null,
} = {}) {
  const beforeState = stateMap(before);
  const afterState = stateMap(after);
  const stateKeys = new Set([...beforeState.keys(), ...afterState.keys()]);
  const changedState = [];
  for (const key of stateKeys) {
    if (SENSITIVE_KEY.test(String(key))) continue;
    if (!Object.is(beforeState.get(key), afterState.get(key))) changedState.push(String(key));
  }

  const headers = descriptorHeaders(responseText);
  const patchTargets = cleanList(headers
    .filter(item => item.type === "patch")
    .map(item => item.target));
  const semanticTypes = headers.map(item => item.type).filter(Boolean);
  const newUiBlocks = semanticTypes.filter(type => !["state", "patch", "derive"].includes(type)).length;
  const beforeChars = String(before?.source || "").length;
  const afterChars = String(after?.source || "").length;

  return {
    sourceDelta: afterChars - beforeChars,
    stateKeys: cleanList(changedState),
    stateChangeCount: changedState.length,
    patchTargets,
    patchCount: semanticTypes.filter(type => type === "patch").length,
    semanticBlocks: semanticTypes.length,
    newUiBlocks,
    format: String(format || "").toUpperCase().slice(0, 16),
    chunks: Math.max(0, Number(chunks) || 0),
    firstUiMs: Number.isFinite(Number(firstUiMs)) ? Number(firstUiMs) : null,
  };
}


export function summarizeStagedEffects(responseText = "") {
  const source = String(responseText || "").slice(0, 4 * 1024 * 1024);
  const headers = descriptorHeaders(source);
  const semanticTypes = headers.map(item => item.type).filter(Boolean);
  const patchTargets = cleanList(headers.filter(item => item.type === "patch").map(item => item.target));
  const stateKeys = [];
  const stateFence = /^:::llm\s+ui\b([^\r\n]*\btype=(?:"state"|'state'|state)[^\r\n]*)\r?\n([\s\S]*?)^:::\s*$/gmi;
  for (const match of source.matchAll(stateFence)) {
    for (const line of String(match[2] || "").split(/\r?\n/)) {
      const equals = line.indexOf("=");
      if (equals < 1) continue;
      const key = line.slice(0, equals).trim();
      if (!/^[A-Za-z_][A-Za-z0-9_.-]{0,63}$/.test(key) || SENSITIVE_KEY.test(key)) continue;
      stateKeys.push(key);
      if (stateKeys.length >= 32) break;
    }
  }
  const cleanState = cleanList(stateKeys);
  return {
    stateKeys: cleanState,
    stateCount: cleanState.length,
    patchTargets,
    patchCount: semanticTypes.filter(type => type === "patch").length,
    newUiBlocks: semanticTypes.filter(type => !["state", "patch", "derive"].includes(type)).length,
    semanticBlocks: semanticTypes.length,
  };
}
