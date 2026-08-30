#!/usr/bin/env node

import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { stdin } from "node:process";
import { Streamdown } from "../js/streamdown.js";

function usage() {
  console.error(`Usage: node tools/streamdown-inspect.mjs [file] [options]

Options:
  --chunk=N       Feed N-byte chunks through TextDecoder (default: 32)
  --verify        Compare streamed AST with one-shot parsing and byte-by-byte parsing
  --deltas        Include per-chunk delta operation names
  --validate      Validate closed fences, unique semantic IDs, and JSON-looking payloads
  --wasm=PATH     WASM path (default: target/wasm32-unknown-unknown/release/streamdown.wasm)
  --help          Show this help

If file is omitted, Markdown is read from stdin.`);
}

function parseArgs(argv) {
  const options = {
    file: null,
    chunk: 32,
    verify: false,
    deltas: false,
    validate: false,
    wasm: "target/wasm32-unknown-unknown/release/streamdown.wasm",
  };

  for (const arg of argv) {
    if (arg === "--help") {
      usage();
      process.exit(0);
    }
    if (arg === "--verify") {
      options.verify = true;
      continue;
    }
    if (arg === "--deltas") {
      options.deltas = true;
      continue;
    }
    if (arg === "--validate") {
      options.validate = true;
      continue;
    }
    if (arg.startsWith("--chunk=")) {
      const value = Number(arg.slice("--chunk=".length));
      if (!Number.isSafeInteger(value) || value <= 0) {
        throw new Error("--chunk must be a positive integer");
      }
      options.chunk = value;
      continue;
    }
    if (arg.startsWith("--wasm=")) {
      options.wasm = arg.slice("--wasm=".length);
      continue;
    }
    if (arg.startsWith("--")) throw new Error(`unknown option: ${arg}`);
    if (options.file !== null) throw new Error("only one input file may be specified");
    options.file = arg;
  }

  return options;
}

async function readStdin() {
  const chunks = [];
  for await (const chunk of stdin) chunks.push(chunk);
  return Buffer.concat(chunks).toString("utf8");
}

function byteChunks(text, size) {
  const bytes = new TextEncoder().encode(text);
  const chunks = [];
  for (let offset = 0; offset < bytes.length; offset += size) {
    chunks.push(bytes.subarray(offset, Math.min(offset + size, bytes.length)));
  }
  return chunks;
}

async function createParser(wasmBytes) {
  return Streamdown.load(wasmBytes);
}

async function parseStream(wasmBytes, text, chunkSize, captureDeltas) {
  const parser = await createParser(wasmBytes);
  const deltaOps = [];
  try {
    await parser.consume(byteChunks(text, chunkSize), {
      onDelta(operations) {
        if (captureDeltas) deltaOps.push(operations.map((operation) => operation.op));
      },
    });

    const links = parser.getLinks();
    const semanticReferences = links
      .filter(({ destination }) => destination.startsWith("llm:") && !destination.startsWith("llm:cite:"))
      .map(({ block, text: label, destination }) => {
        const body = destination.slice("llm:".length);
        const colon = body.indexOf(":");
        return {
          block,
          kind: colon < 0 ? body : body.slice(0, colon),
          id: colon < 0 ? "" : body.slice(colon + 1),
          label,
        };
      });

    return {
      document: parser.snapshot(),
      summary: {
        blockCount: parser.blockCount,
        llmBlocks: parser.getLlmBlocks(),
        citations: parser.getCitations(),
        semanticReferences,
        plainText: parser.toPlainText(),
        deltaOps: captureDeltas ? deltaOps : undefined,
      },
    };
  } finally {
    parser.dispose();
  }
}

async function parseOneShot(wasmBytes, text) {
  const parser = await createParser(wasmBytes);
  try {
    parser.append(text);
    parser.finish();
    return parser.snapshot();
  } finally {
    parser.dispose();
  }
}

function validateSummary(summary) {
  const errors = [];
  const warnings = [];
  const ids = new Map();

  for (const block of summary.llmBlocks) {
    const id = block.attributes.id;
    const where = id ? `${block.kind}:${id}` : `${block.kind}@block${block.index}`;
    if (!block.closed) errors.push(`${where}: semantic fence is not closed`);

    if (id) {
      const key = `${block.kind}:${id}`;
      if (ids.has(key)) errors.push(`${where}: duplicate semantic id (first at block ${ids.get(key)})`);
      else ids.set(key, block.index);
    }

    const body = block.value.trim();
    const mime = block.attributes.mime ?? "";
    const claimsJson = mime === "application/json" || mime.endsWith("+json") || block.attributes.format === "json";
    const looksJson = body.startsWith("{") || body.startsWith("[");
    if ((claimsJson || looksJson) && body) {
      try {
        JSON.parse(body);
      } catch (error) {
        errors.push(`${where}: invalid JSON payload (${error.message})`);
      }
    }
    if (claimsJson && !body) warnings.push(`${where}: JSON payload is empty`);
  }

  for (const ref of summary.semanticReferences) {
    if (["tool", "artifact", "ui", "metric"].includes(ref.kind)) {
      const key = `${ref.kind}:${ref.id}`;
      if (!ids.has(key)) warnings.push(`${key}: semantic reference has no matching local block`);
    }
  }

  return { ok: errors.length === 0, errors, warnings };
}

function stableJson(value) {
  return JSON.stringify(value);
}

const options = parseArgs(process.argv.slice(2));
const text = options.file === null
  ? await readStdin()
  : await readFile(resolve(options.file), "utf8");
const wasmBytes = await readFile(resolve(options.wasm));
const streamed = await parseStream(wasmBytes, text, options.chunk, options.deltas);

let verification;
if (options.verify) {
  const oneShot = await parseOneShot(wasmBytes, text);
  const bytewise = await parseStream(wasmBytes, text, 1, false);
  const oneShotMatch = stableJson(streamed.document) === stableJson(oneShot);
  const bytewiseMatch = stableJson(streamed.document) === stableJson(bytewise.document);
  verification = { oneShotMatch, bytewiseMatch };
  if (!oneShotMatch || !bytewiseMatch) process.exitCode = 2;
}

const validation = options.validate ? validateSummary(streamed.summary) : undefined;
if (validation && !validation.ok && !process.exitCode) process.exitCode = 3;

console.log(JSON.stringify({
  input: {
    file: options.file,
    utf8Bytes: new TextEncoder().encode(text).length,
    chunkBytes: options.chunk,
  },
  verification,
  validation,
  ...streamed.summary,
  document: streamed.document,
}, null, 2));
