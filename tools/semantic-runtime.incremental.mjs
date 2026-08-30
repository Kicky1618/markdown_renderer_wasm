import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { SemanticRuntime } from "./semantic-runtime.mjs";

const wasmPath = process.argv[2] ?? "target/wasm32-unknown-unknown/release/streamdown.wasm";
const wasm = await readFile(wasmPath);

const markdown = [
  "prefix token token\n",
  "::::llm tool id=search name=\"x\"\n",
  '{"q":"日本語"}\n',
  ":::\n",
  "::::\n",
  "plain @[artifact:summary] tail\n",
  ":::llm artifact id=summary depends=tool:search\n",
  '{"ok":true}\n',
  ":::\n",
  `:::llm artifact name="${"x".repeat(700)}" id=late\n`,
  '{"late":true}\n',
  ":::\n",
].join("");

function makeRunners() {
  return {
    tool: async () => ({ ok: true }),
    artifact: async () => ({ ok: true }),
  };
}

async function run(chunks, semanticScan) {
  const events = [];
  const runtime = await SemanticRuntime.load(wasm, {
    semanticScan,
    runners: makeRunners(),
    onSemanticEvent: (event) => events.push(event),
  });
  try {
    for (const chunk of chunks) runtime.append(chunk);
    const snapshot = await runtime.finish();
    return { events, snapshot };
  } finally {
    runtime.dispose();
  }
}

function splitText(text, width) {
  const out = [];
  for (let i = 0; i < text.length; i += width) out.push(text.slice(i, i + width));
  return out;
}

for (const width of [1, 2, 3, 5, 7, 16, 31]) {
  const chunks = splitText(markdown, width);
  const always = await run(chunks, "always");
  const incremental = await run(chunks, "incremental");
  assert.deepEqual(incremental.events, always.events, `semantic event mismatch at width=${width}`);
  assert.deepEqual(incremental.snapshot.graph, always.snapshot.graph, `graph mismatch at width=${width}`);
  assert.deepEqual(incremental.snapshot.scheduler, always.snapshot.scheduler, `scheduler mismatch at width=${width}`);
  assert.ok(incremental.snapshot.semanticScans <= always.snapshot.semanticScans);
}

console.log("semantic runtime incremental scan equivalence: ok");
