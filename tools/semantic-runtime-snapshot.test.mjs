import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { SemanticRuntime } from "./semantic-runtime.mjs";

const wasmPath = process.argv[2] ?? "target/wasm32-unknown-unknown/release/streamdown.wasm";
const wasm = await readFile(wasmPath);
const markdown = [
  "# Snapshot\n\n",
  ":::llm tool id=search\n",
  '{"q":"semantic runtime"}\n',
  ":::\n",
].join("");

const runners = { tool: async () => ({ ok: true }) };
const lightweight = { document: false, graph: false, diagnostics: false, scheduler: false };

{
  const runtime = await SemanticRuntime.load(wasm, { runners });
  try {
    runtime.append(markdown);
    const full = runtime.snapshot();
    assert.ok(Array.isArray(full.document));
    assert.ok(Array.isArray(full.graph.nodes));
    assert.equal(typeof full.diagnostics.ok, "boolean");
    assert.equal(typeof full.scheduler, "object");

    const small = runtime.snapshot(lightweight);
    assert.deepEqual(Object.keys(small).sort(), ["blockCount", "semanticScans"]);
    assert.equal(small.blockCount, full.blockCount);
    assert.equal(small.semanticScans, full.semanticScans);

    const finished = await runtime.finish(lightweight);
    assert.deepEqual(Object.keys(finished).sort(), ["blockCount", "semanticScans"]);
  } finally {
    runtime.dispose();
  }
}

{
  const runtime = await SemanticRuntime.load(wasm, { runners });
  try {
    const result = await runtime.consume(markdown, { snapshotOptions: lightweight });
    assert.deepEqual(Object.keys(result).sort(), ["blockCount", "semanticScans"]);
    assert.equal(result.blockCount, 2);
  } finally {
    runtime.dispose();
  }
}

{
  const runtime = await SemanticRuntime.load(wasm, { runners });
  try {
    runtime.append(markdown);
    const graphOnly = runtime.snapshot({ document: false, scheduler: false });
    assert.equal("document" in graphOnly, false);
    assert.equal("scheduler" in graphOnly, false);
    assert.ok(graphOnly.graph.nodes.some((node) => node.key === "tool:search"));
    assert.equal(graphOnly.diagnostics.ok, true);
  } finally {
    runtime.dispose();
  }
}

console.log("semantic runtime selective snapshot: ok");
