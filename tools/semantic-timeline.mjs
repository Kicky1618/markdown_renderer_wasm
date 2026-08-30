#!/usr/bin/env node

import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { stdin } from "node:process";
import { Streamdown } from "../js/streamdown.js";
import { graphDiagnostics } from "./semantic-graph.mjs";
import {
  createTimelineState,
  observeSemanticState,
  semanticReferencesFromLinks,
} from "./semantic-timeline-core.mjs";

function usage() {
  console.error(`Usage: node tools/semantic-timeline.mjs [file] [options]

Options:
  --chunk=N       Feed N-byte chunks through TextDecoder (default: 8)
  --ndjson        Emit one event per line, then a final summary record
  --wasm=PATH     WASM path (default: target/wasm32-unknown-unknown/release/streamdown.wasm)
  --help          Show this help

The timeline reports when LLM semantic blocks are first visible (open), closed,
ready after all local dependencies are ready, and referenced from Markdown.`);
}

function parseArgs(argv) {
  const options = {
    file: null,
    chunk: 8,
    ndjson: false,
    wasm: "target/wasm32-unknown-unknown/release/streamdown.wasm",
  };
  for (const arg of argv) {
    if (arg === "--help") {
      usage();
      process.exit(0);
    }
    if (arg === "--ndjson") {
      options.ndjson = true;
      continue;
    }
    if (arg.startsWith("--chunk=")) {
      const value = Number(arg.slice("--chunk=".length));
      if (!Number.isSafeInteger(value) || value <= 0) throw new Error("--chunk must be a positive integer");
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

function summaryFromParser(parser) {
  return {
    llmBlocks: parser.getLlmBlocks(),
    semanticReferences: semanticReferencesFromLinks(parser.getLinks()),
  };
}

function appendText(parser, text) {
  if (!text) return;
  if (typeof parser.appendInPlace === "function") parser.appendInPlace(text);
  else parser.append(text);
}

const options = parseArgs(process.argv.slice(2));
const text = options.file === null ? await readStdin() : await readFile(resolve(options.file), "utf8");
const wasmBytes = await readFile(resolve(options.wasm));
const parser = await Streamdown.load(wasmBytes);
const bytes = new TextEncoder().encode(text);
const decoder = new TextDecoder();
const state = createTimelineState();
const events = [];
let graph;
let chunkIndex = 0;

const observe = (observedAtByte) => {
  const result = observeSemanticState(summaryFromParser(parser), state, observedAtByte, chunkIndex);
  graph = result.graph;
  for (const event of result.events) {
    events.push(event);
    if (options.ndjson) console.log(JSON.stringify({ type: "event", ...event }));
  }
  chunkIndex++;
};

try {
  for (let offset = 0; offset < bytes.length; offset += options.chunk) {
    const end = Math.min(offset + options.chunk, bytes.length);
    const decoded = decoder.decode(bytes.subarray(offset, end), { stream: true });
    if (decoded) {
      appendText(parser, decoded);
      observe(end);
    }
  }
  const tail = decoder.decode();
  if (tail) {
    appendText(parser, tail);
    observe(bytes.length);
  }
  parser.finish();
  observe(bytes.length);

  const diagnostics = graphDiagnostics(graph);
  const final = {
    input: {
      file: options.file,
      utf8Bytes: bytes.length,
      chunkBytes: options.chunk,
    },
    diagnostics,
    executionOrder: graph.executionOrder,
    events,
    finalGraph: graph,
  };

  if (options.ndjson) console.log(JSON.stringify({ type: "summary", ...final }));
  else console.log(JSON.stringify(final, null, 2));
  if (!diagnostics.ok) process.exitCode = 3;
} finally {
  parser.dispose();
}
