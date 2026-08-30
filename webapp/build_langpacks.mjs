import fs from "node:fs";
import { createHash } from "node:crypto";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.dirname(fileURLToPath(import.meta.url));
const LANGPACK_DIR = path.join(ROOT, "langpacks");
const INDEX_NAME = "_index.slp";
const SAFE_ALIAS = /^[a-z0-9_+#-]+$/;

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
  ["semicolon_comments", 17], ["percent_comments", 18],
  ["apostrophe_comments", 19], ["bang_comments", 20],
  ["hyphen_identifiers", 21], ["question_identifiers", 22], ["bang_identifiers", 23],
  ["paren_star_comments", 24], ["brace_dash_comments", 25],
  ["triple_double_strings", 26], ["triple_single_strings", 27],
]);

const encoder = new TextEncoder();

export function parsePack(source) {
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

export function encodePack({ values, flags }) {
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

function sameBytes(a, b) {
  return a.length === b.length && a.every((value, index) => value === b[index]);
}

function normalizedAliases(profile, packName) {
  const aliases = profile.values.aliases.map(alias => alias.toLowerCase());
  if (!aliases.includes(packName)) throw new Error(`${packName}.langpack must include its canonical name as an alias`);
  for (const alias of aliases) {
    if (!alias || alias.length > 48 || !SAFE_ALIAS.test(alias)) throw new Error(`unsafe langpack alias: ${alias}`);
  }
  return aliases;
}

export function buildAll({ check = false } = {}) {
  const sources = fs.readdirSync(LANGPACK_DIR).filter(name => name.endsWith(".langpack")).sort();
  const outputs = new Map();
  const aliasToPack = new Map();
  const versionHash = createHash("sha256");
  let totalSource = 0;
  let totalBinary = 0;

  for (const name of sources) {
    const packName = name.replace(/\.langpack$/, "").toLowerCase();
    const source = fs.readFileSync(path.join(LANGPACK_DIR, name), "utf8");
    const profile = parsePack(source);
    const binary = encodePack(profile);
    const aliases = normalizedAliases(profile, packName);
    const outputName = `${packName}.slp`;
    if (outputs.has(outputName)) throw new Error(`duplicate canonical langpack output: ${packName}`);
    outputs.set(outputName, binary);
    // Version the emitted bytes, not the source text. Encoder/schema changes that
    // alter SLP1 therefore invalidate browser caches even when .langpack text is unchanged.
    versionHash.update(packName).update("\0").update(binary).update("\0");
    totalSource += Buffer.byteLength(source);
    totalBinary += binary.byteLength;
    for (const alias of aliases) {
      if (aliasToPack.has(alias)) throw new Error(`duplicate langpack alias: ${alias}`);
      aliasToPack.set(alias, packName);
    }
  }

  if (outputs.has(INDEX_NAME)) throw new Error(`${INDEX_NAME} is reserved`);
  const aliasCount = aliasToPack.size;
  const emittedBytes = [...outputs.values()].reduce((sum, binary) => sum + binary.byteLength, 0);
  const version = versionHash.digest("hex").slice(0, 16);
  const aliasMap = Object.fromEntries([...aliasToPack.entries()].sort(([left], [right]) => left.localeCompare(right)));
  const index = encoder.encode(JSON.stringify({ v: version, m: aliasMap }));
  outputs.set(INDEX_NAME, index);

  const stale = [];
  const existing = fs.readdirSync(LANGPACK_DIR).filter(name => name.endsWith(".slp"));
  for (const name of existing) {
    if (!outputs.has(name)) {
      if (check) stale.push(`unexpected:${name}`);
      else fs.rmSync(path.join(LANGPACK_DIR, name));
    }
  }
  for (const [name, binary] of outputs) {
    const outputPath = path.join(LANGPACK_DIR, name);
    if (check) {
      if (!fs.existsSync(outputPath) || !sameBytes(binary, fs.readFileSync(outputPath))) stale.push(name);
    } else {
      fs.writeFileSync(outputPath, binary);
    }
  }
  if (stale.length) throw new Error(`stale generated langpacks: ${stale.join(", ")}`);
  return {
    count: sources.length,
    aliases: aliasCount,
    sourceBytes: totalSource,
    binaryBytes: totalBinary,
    emittedBytes,
    indexBytes: index.byteLength,
    version,
  };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const check = process.argv.includes("--check");
  const result = buildAll({ check });
  console.log(`langpacks ${check ? "checked" : "built"}: ${result.count} packs / ${result.aliases} aliases, version=${result.version}, ${result.sourceBytes} text bytes -> ${result.binaryBytes} canonical SLP1 bytes (${result.emittedBytes} canonical-file bytes + ${result.indexBytes} index bytes)`);
}
