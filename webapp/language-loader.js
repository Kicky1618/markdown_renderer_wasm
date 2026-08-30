const MAX_REQUESTED_PACKS = 128;

const aliases = new Map([
  ["rs", "rust"], ["rust", "rust"],
  ["js", "javascript"], ["javascript", "javascript"], ["node", "javascript"],
  ["mjs", "javascript"], ["cjs", "javascript"], ["jsx", "javascript"],
  ["ts", "javascript"], ["typescript", "javascript"], ["mts", "javascript"],
  ["cts", "javascript"], ["tsx", "javascript"],
  ["py", "python"], ["python", "python"], ["python3", "python"],
  ["sh", "shell"], ["bash", "shell"], ["shell", "shell"], ["zsh", "shell"],
  ["c", "cpp"], ["h", "cpp"], ["cpp", "cpp"], ["c++", "cpp"],
  ["cc", "cpp"], ["cxx", "cpp"], ["hpp", "cpp"],
  ["java", "java"], ["go", "go"], ["golang", "go"],
  ["json", "json"], ["jsonc", "json"], ["css", "css"], ["scss", "css"],
  ["sql", "sql"], ["postgres", "sql"], ["postgresql", "sql"],
  ["yaml", "yaml"], ["yml", "yaml"], ["toml", "toml"],
]);

const sections = [
  "aliases",
  "keywords",
  "builtin_types",
  "function_declarations",
  "type_declarations",
  "macro_declarations",
  "preprocessor_macro_operands",
  "preprocessor_headers",
  "bang_macro_declarations",
  "macro_identifiers",
  "macro_operand_identifiers",
  "header_macro_identifiers",
  "expression_prefixes",
];

const flagBits = new Map([
  ["case_insensitive_keywords", 0], ["slash_comments", 1], ["dash_comments", 2],
  ["hash_comments", 3], ["block_comments", 4], ["nested_block_comments", 5],
  ["preprocessor", 6], ["decorators", 7], ["dollar_identifiers", 8],
  ["javascript_lexing", 9], ["python_strings", 10], ["rust_syntax", 11],
  ["multiline_strings", 12], ["rust_attributes", 13], ["bang_macros", 14],
  ["uppercase_macros", 15], ["macro_metavariables", 16],
]);

const requested = new Set();
const registered = new Set();
const encoder = new TextEncoder();

function resolvePack(name) {
  const normalized = String(name).trim().toLowerCase();
  const mapped = aliases.get(normalized);
  if (mapped) return mapped;
  return normalized.length > 0 && normalized.length <= 48 && /^[a-z0-9_-]+$/.test(normalized)
    ? normalized
    : null;
}

function parsePack(source) {
  const lines = source.split(/\r?\n/);
  if (lines.shift() !== "STREAMDOWN_LANGPACK\t1") throw new Error("invalid langpack header");
  const values = Object.fromEntries(sections.map(key => [key, []]));
  let flags = 0;
  for (const line of lines) {
    if (!line || line.startsWith("#")) continue;
    const [key, ...items] = line.split("\t").filter(Boolean);
    if (key === "flags") {
      for (const flag of items) {
        const bit = flagBits.get(flag);
        if (bit === undefined) throw new Error(`unknown langpack flag: ${flag}`);
        flags |= 1 << bit;
      }
    } else if (Object.hasOwn(values, key)) {
      values[key] = items;
    } else {
      throw new Error(`unknown langpack field: ${key}`);
    }
  }
  if (!values.aliases.length) throw new Error("langpack is missing aliases");
  return { values, flags: flags >>> 0 };
}

function encodePack({ values, flags }) {
  const encoded = sections.map(key => values[key].map(word => encoder.encode(word)));
  let size = 8;
  for (const words of encoded) {
    if (words.length > 0xffff) throw new Error("too many langpack words");
    size += 2;
    for (const word of words) {
      if (word.length > 0xffff) throw new Error("langpack word too long");
      size += 2 + word.length;
    }
  }
  const output = new Uint8Array(size);
  output.set([0x53, 0x4c, 0x50, 0x31], 0); // SLP1
  const view = new DataView(output.buffer);
  view.setUint32(4, flags, true);
  let at = 8;
  for (const words of encoded) {
    view.setUint16(at, words.length, true);
    at += 2;
    for (const word of words) {
      view.setUint16(at, word.length, true);
      at += 2;
      output.set(word, at);
      at += word.length;
    }
  }
  return output;
}

function invalidateRenderer() {
  const canvas = document.getElementById("app");
  if (canvas instanceof HTMLCanvasElement) canvas.width = Math.min(0xffffffff, canvas.width + 1);
}

export async function loadLanguagePack(name) {
  const pack = resolvePack(name);
  if (!pack || requested.has(pack) || requested.size >= MAX_REQUESTED_PACKS) return false;
  requested.add(pack);
  try {
    const url = new URL(`./langpacks/${pack}.langpack`, import.meta.url);
    const response = await fetch(url, { cache: "force-cache" });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    const binary = encodePack(parsePack(await response.text()));
    const wasm = await import("./pkg/streamdown_web.js");
    const registeredNow = wasm.register_language_pack_binary(binary);
    if (registeredNow) {
      registered.add(pack);
      const root = document.documentElement;
      root.dataset.languagePackRegistered = pack;
      root.dataset.languagePackRegisteredCount = String(registered.size);
      root.dataset.languagePacks = [...registered].sort().join(",");
      invalidateRenderer();
    } else {
      document.documentElement.dataset.languagePackError = `${pack}:wasm-rejected`;
    }
    return registeredNow;
  } catch (error) {
    document.documentElement.dataset.languagePackError = `${pack}:${String(error)}`;
    console.warn(`language pack ${JSON.stringify(pack)} unavailable:`, error);
    return false;
  }
}

export const __test = { resolvePack, parsePack, encodePack };
