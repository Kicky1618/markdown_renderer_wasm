const MAX_CONCURRENT_PACKS = 16;
const MAX_PACK_BYTES = 16 * 1024;
const MAX_INDEX_BYTES = 16 * 1024;
const SAFE_LANGUAGE = /^[a-z0-9_+#-]+$/;
const SAFE_VERSION = /^[0-9a-f]{16}$/;
const INDEX_URL = new URL("./langpacks/_index.slp", import.meta.url);
const inFlight = new Map();
const failedAliases = new Set();
const loadedAliases = new Set();
const registered = new Set();
const decoder = new TextDecoder();
const packWaiters = [];
let activePackFetches = 0;
let aliasIndexPromise;

function normalizeLanguage(name) {
  const normalized = String(name).trim().toLowerCase();
  return normalized.length > 0 && normalized.length <= 48 && SAFE_LANGUAGE.test(normalized)
    ? normalized
    : null;
}

function readPackAliases(binary) {
  if (binary.length < 10 || binary[0] !== 0x53 || binary[1] !== 0x4c || binary[2] !== 0x50 || binary[3] !== 0x31) {
    throw new Error("invalid SLP1 header");
  }
  const view = new DataView(binary.buffer, binary.byteOffset, binary.byteLength);
  let at = 8; // magic + flags
  const count = view.getUint16(at, true);
  at += 2;
  if (count === 0) throw new Error("SLP1 pack has no aliases");
  const aliases = [];
  for (let i = 0; i < count; i += 1) {
    if (at + 2 > binary.length) throw new Error("truncated SLP1 alias length");
    const length = view.getUint16(at, true);
    at += 2;
    if (at + length > binary.length) throw new Error("truncated SLP1 alias");
    const alias = normalizeLanguage(decoder.decode(binary.subarray(at, at + length)));
    if (!alias) throw new Error("invalid SLP1 alias");
    aliases.push(alias);
    at += length;
  }
  return aliases;
}

async function responseBytes(response, maxBytes, label) {
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  const declaredLength = Number(response.headers.get("content-length"));
  if (Number.isFinite(declaredLength) && declaredLength > maxBytes) {
    throw new Error(`${label} too large: ${declaredLength} bytes`);
  }
  const buffer = await response.arrayBuffer();
  if (buffer.byteLength > maxBytes) throw new Error(`${label} too large: ${buffer.byteLength} bytes`);
  return new Uint8Array(buffer);
}

function loadAliasIndex() {
  if (!aliasIndexPromise) {
    aliasIndexPromise = (async () => {
      const response = await fetch(INDEX_URL, { cache: "no-cache" });
      const bytes = await responseBytes(response, MAX_INDEX_BYTES, "langpack index");
      const index = JSON.parse(decoder.decode(bytes));
      if (!index || !SAFE_VERSION.test(index.v) || !Array.isArray(index.a) || index.a.length === 0) {
        throw new Error("invalid langpack index");
      }
      const aliases = new Set();
      for (const value of index.a) {
        const alias = normalizeLanguage(value);
        if (!alias || alias !== value || aliases.has(alias)) throw new Error("invalid langpack index alias");
        aliases.add(alias);
      }
      return { aliases, version: index.v };
    })().catch(error => {
      aliasIndexPromise = undefined;
      throw error;
    });
  }
  return aliasIndexPromise;
}

function acquirePackSlot() {
  if (activePackFetches < MAX_CONCURRENT_PACKS) {
    activePackFetches += 1;
    return Promise.resolve();
  }
  return new Promise(resolve => packWaiters.push(resolve));
}

function releasePackSlot() {
  const next = packWaiters.shift();
  if (next) next();
  else activePackFetches -= 1;
}

function invalidateRenderer() {
  const canvas = document.getElementById("app");
  if (!(canvas instanceof HTMLCanvasElement)) return;
  // Renderer keyboard handlers already mark their scene dirty on a pause toggle.
  // Two synchronous toggles are an involution: no token can advance between them,
  // user-visible pause state is unchanged, and both GPU and Canvas2D redraw without
  // perturbing the backing-store size or resetting Canvas2D context state.
  for (let i = 0; i < 2; i += 1) {
    canvas.dispatchEvent(new KeyboardEvent("keydown", { key: "p", bubbles: true }));
  }
}

export async function loadLanguagePack(name) {
  const alias = normalizeLanguage(name);
  if (!alias || loadedAliases.has(alias) || failedAliases.has(alias)) return false;

  let knownAliases;
  try {
    knownAliases = await loadAliasIndex();
  } catch (error) {
    document.documentElement.dataset.languagePackError = `index:${String(error)}`;
    console.warn("language pack index unavailable:", error);
    return false;
  }
  // Unknown fence names never reach the network and are not cached locally: the
  // index membership check is cheap, while caching attacker-controlled misses
  // would make memory usage grow with document input.
  if (!knownAliases.aliases.has(alias)) return false;

  const existing = inFlight.get(alias);
  if (existing) return existing.then(() => false, () => false);

  const task = (async () => {
    await acquirePackSlot();
    try {
      if (loadedAliases.has(alias)) return false;
      const url = new URL(`./langpacks/${encodeURIComponent(alias)}.slp`, import.meta.url);
      url.searchParams.set("v", knownAliases.version);
      const response = await fetch(url, { cache: "force-cache" });
      const binary = await responseBytes(response, MAX_PACK_BYTES, "langpack");
      const packAliases = readPackAliases(binary);
      if (!packAliases.includes(alias)) throw new Error(`langpack alias mismatch: ${alias}`);
      const wasm = await import("./pkg/streamdown_web.js");
      if (packAliases.some(packAlias => loadedAliases.has(packAlias))) {
        for (const packAlias of packAliases) loadedAliases.add(packAlias);
        return false;
      }
      const registeredNow = wasm.register_language_pack_binary(binary);
      if (registeredNow) {
        for (const packAlias of packAliases) loadedAliases.add(packAlias);
        registered.add(alias);
        const root = document.documentElement;
        root.dataset.languagePackRegistered = alias;
        root.dataset.languagePackRegisteredCount = String(registered.size);
        root.dataset.languagePacks = [...registered].sort().join(",");
        invalidateRenderer();
      } else if (!packAliases.some(packAlias => loadedAliases.has(packAlias))) {
        failedAliases.add(alias);
        document.documentElement.dataset.languagePackError = `${alias}:wasm-rejected`;
      }
      return registeredNow;
    } catch (error) {
      failedAliases.add(alias);
      document.documentElement.dataset.languagePackError = `${alias}:${String(error)}`;
      console.warn(`language pack ${JSON.stringify(alias)} unavailable:`, error);
      return false;
    } finally {
      inFlight.delete(alias);
      releasePackSlot();
    }
  })();
  inFlight.set(alias, task);
  return task;
}

export const __test = { normalizeLanguage, readPackAliases };
