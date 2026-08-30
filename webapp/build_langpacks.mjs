import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.dirname(fileURLToPath(import.meta.url));
const LANGPACK_DIR = path.join(ROOT, "langpacks");

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

export function buildAll({ check = false } = {}) {
  const sources = fs.readdirSync(LANGPACK_DIR).filter(name => name.endsWith(".langpack")).sort();
  let totalSource = 0;
  let totalBinary = 0;
  const stale = [];
  for (const name of sources) {
    const sourcePath = path.join(LANGPACK_DIR, name);
    const outputPath = sourcePath.replace(/\.langpack$/, ".slp");
    const source = fs.readFileSync(sourcePath, "utf8");
    const binary = encodePack(parsePack(source));
    totalSource += Buffer.byteLength(source);
    totalBinary += binary.byteLength;
    if (check) {
      if (!fs.existsSync(outputPath) || !sameBytes(binary, fs.readFileSync(outputPath))) stale.push(path.basename(outputPath));
    } else {
      fs.writeFileSync(outputPath, binary);
    }
  }
  if (stale.length) throw new Error(`stale generated langpacks: ${stale.join(", ")}`);
  return { count: sources.length, sourceBytes: totalSource, binaryBytes: totalBinary };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const check = process.argv.includes("--check");
  const result = buildAll({ check });
  console.log(`langpacks ${check ? "checked" : "built"}: ${result.count} packs, ${result.sourceBytes} text bytes -> ${result.binaryBytes} SLP1 bytes`);
}
