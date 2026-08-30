const MAX_CONCURRENT_PACKS = 16;
const MAX_FAILED_PACKS = 128;
const SAFE_LANGUAGE = /^[a-z0-9_+#-]+$/;
const inFlight = new Map();
const failedAliases = new Map();
const loadedAliases = new Set();
const registered = new Set();
const decoder = new TextDecoder();

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
  if (count === 0 || count > 0xffff) throw new Error("SLP1 pack has no aliases");
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

function rememberFailure(alias) {
  failedAliases.delete(alias);
  failedAliases.set(alias, true);
  if (failedAliases.size > MAX_FAILED_PACKS) {
    failedAliases.delete(failedAliases.keys().next().value);
  }
}

function invalidateRenderer() {
  const canvas = document.getElementById("app");
  if (canvas instanceof HTMLCanvasElement) canvas.width = Math.min(0xffffffff, canvas.width + 1);
}

export function loadLanguagePack(name) {
  const alias = normalizeLanguage(name);
  if (!alias || loadedAliases.has(alias) || failedAliases.has(alias)) return Promise.resolve(false);
  const existing = inFlight.get(alias);
  if (existing) return existing.then(() => false, () => false);
  if (inFlight.size >= MAX_CONCURRENT_PACKS) return Promise.resolve(false);

  const task = (async () => {
    try {
      const url = new URL(`./langpacks/${encodeURIComponent(alias)}.slp`, import.meta.url);
      const response = await fetch(url, { cache: "force-cache" });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const binary = new Uint8Array(await response.arrayBuffer());
      const packAliases = readPackAliases(binary);
      const wasm = await import("./pkg/streamdown_web.js");
      const registeredNow = wasm.register_language_pack_binary(binary);
      if (registeredNow) {
        for (const packAlias of packAliases) loadedAliases.add(packAlias);
        registered.add(alias);
        const root = document.documentElement;
        root.dataset.languagePackRegistered = alias;
        root.dataset.languagePackRegisteredCount = String(registered.size);
        root.dataset.languagePacks = [...registered].sort().join(",");
        invalidateRenderer();
      } else {
        rememberFailure(alias);
        document.documentElement.dataset.languagePackError = `${alias}:wasm-rejected`;
      }
      return registeredNow;
    } catch (error) {
      rememberFailure(alias);
      document.documentElement.dataset.languagePackError = `${alias}:${String(error)}`;
      console.warn(`language pack ${JSON.stringify(alias)} unavailable:`, error);
      return false;
    } finally {
      inFlight.delete(alias);
    }
  })();
  inFlight.set(alias, task);
  return task;
}

export const __test = { normalizeLanguage, readPackAliases, rememberFailure };
