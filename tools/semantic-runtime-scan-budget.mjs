import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { SemanticRuntime } from "./semantic-runtime.mjs";

const wasmPath = process.argv[2] ?? "target/wasm32-unknown-unknown/release/streamdown.wasm";
const wasm = await readFile(wasmPath);

async function run(chunks) {
  const runtime = await SemanticRuntime.load(wasm, { semanticScan: "incremental" });
  try {
    for (const chunk of chunks) runtime.append(chunk);
    return await runtime.finish();
  } finally {
    runtime.dispose();
  }
}

const lf = Array.from({ length: 1000 }, (_, i) => `ordinary line ${i}\n`);
const lfResult = await run(lf);
assert.equal(lfResult.semanticScans, 1, "finish() is the only semantic scan for ordinary LF lines");
assert.equal(lfResult.graph.nodes.length, 0);

const crlf = Array.from({ length: 1000 }, (_, i) => [`ordinary line ${i}\r`, "\n"]).flat();
const crlfResult = await run(crlf);
assert.equal(crlfResult.semanticScans, 1, "finish() is the only semantic scan for split CRLF lines");
assert.equal(crlfResult.graph.nodes.length, 0);

const semantic = [
  "ordinary before\n",
  ":::llm tool id=probe\n",
  '{"ok":true}\n',
  ":::\n",
  "ordinary after\n",
];
const semanticResult = await run(semantic);
assert.equal(semanticResult.graph.nodes.length, 1);
assert.equal(semanticResult.graph.nodes[0].key, "tool:probe");
assert.ok(semanticResult.semanticScans >= 3, "semantic open/close plus finish must be observed");
assert.ok(semanticResult.semanticScans <= 4, "ordinary lines must not add semantic scans");

console.log("semantic runtime scan budget: ok");
