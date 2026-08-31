const MAX_CONCURRENT_PACKS = 16;
const MAX_PACK_BYTES = 16 * 1024;
const MAX_INDEX_BYTES = 64 * 1024;
const SAFE_LANGUAGE = /^[a-z0-9_+#-]+$/;
const SAFE_VERSION = /^[0-9a-f]{16}$/;
const INDEX_URL = new URL("./langpacks/_index.slp", import.meta.url);
const inFlight = new Map();
const failedPacks = new Set();
const loadedPacks = new Set();
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

function readAliasIndex(bytes) {
  const index = JSON.parse(decoder.decode(bytes));
  if (!index || !SAFE_VERSION.test(index.v) || !Array.isArray(index.p) || index.p.length === 0) {
    throw new Error("invalid langpack index");
  }
  const packByAlias = new Map();
  for (const row of index.p) {
    if (!Array.isArray(row) || row.length === 0) throw new Error("invalid langpack index row");
    const pack = normalizeLanguage(row[0]);
    if (!pack || pack !== row[0] || packByAlias.has(pack)) throw new Error("invalid canonical langpack index entry");
    packByAlias.set(pack, pack);
    for (let i = 1; i < row.length; i += 1) {
      const alias = normalizeLanguage(row[i]);
      if (!alias || alias !== row[i] || packByAlias.has(alias)) throw new Error("invalid langpack index alias");
      packByAlias.set(alias, pack);
    }
  }
  return { packByAlias, version: index.v };
}

export function primeLanguageIndex(responsePromise) {
  if (!aliasIndexPromise) {
    aliasIndexPromise = Promise.resolve(responsePromise)
      .then(response => responseBytes(response, MAX_INDEX_BYTES, "langpack index"))
      .then(readAliasIndex)
      .catch(error => {
        aliasIndexPromise = undefined;
        throw error;
      });
  }
  return aliasIndexPromise;
}

function loadAliasIndex() {
  return aliasIndexPromise ?? primeLanguageIndex(fetch(INDEX_URL, { cache: "no-cache" }));
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
  if (!alias) return false;

  let index;
  try {
    index = await loadAliasIndex();
  } catch (error) {
    document.documentElement.dataset.languagePackError = `index:${String(error)}`;
    console.warn("language pack index unavailable:", error);
    return false;
  }

  const pack = index.packByAlias.get(alias);
  if (!pack || loadedPacks.has(pack) || failedPacks.has(pack)) return false;

  const existing = inFlight.get(pack);
  if (existing) return existing.then(() => false, () => false);

  const task = (async () => {
    await acquirePackSlot();
    try {
      if (loadedPacks.has(pack) || failedPacks.has(pack)) return false;
      const url = new URL(`./langpacks/${encodeURIComponent(pack)}.slp`, import.meta.url);
      url.searchParams.set("v", index.version);
      const response = await fetch(url, { cache: "force-cache" });
      const binary = await responseBytes(response, MAX_PACK_BYTES, "langpack");
      const packAliases = readPackAliases(binary);
      if (!packAliases.includes(pack) || !packAliases.includes(alias)) {
        throw new Error(`langpack alias mismatch: ${alias}->${pack}`);
      }
      const wasm = await import("./pkg/streamdown_web.js");
      const registeredNow = wasm.register_language_pack_binary(binary);
      if (registeredNow) {
        loadedPacks.add(pack);
        registered.add(pack);
        const root = document.documentElement;
        root.dataset.languagePackRegistered = pack;
        root.dataset.languagePackRegisteredCount = String(registered.size);
        root.dataset.languagePacks = [...registered].sort().join(",");
        invalidateRenderer();
      } else {
        failedPacks.add(pack);
        document.documentElement.dataset.languagePackError = `${pack}:wasm-rejected`;
      }
      return registeredNow;
    } catch (error) {
      failedPacks.add(pack);
      document.documentElement.dataset.languagePackError = `${pack}:${String(error)}`;
      console.warn(`language pack ${JSON.stringify(pack)} unavailable:`, error);
      return false;
    } finally {
      inFlight.delete(pack);
      releasePackSlot();
    }
  })();
  inFlight.set(pack, task);
  return task;
}

export const __test = { normalizeLanguage, readPackAliases, readAliasIndex };
