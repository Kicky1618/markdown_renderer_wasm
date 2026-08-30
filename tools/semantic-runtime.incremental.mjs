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

function splitPseudoRandom(text, seed) {
  const out = [];
  let offset = 0;
  let state = seed >>> 0;
  while (offset < text.length) {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    const width = 1 + (state % 11);
    out.push(text.slice(offset, offset + width));
    offset += width;
  }
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

// Ordinary Markdown, including CRLF split across append boundaries, must never
// rebuild the semantic graph. finish() performs the one mandatory final scan.
const ordinaryChunks = [];
for (let i = 0; i < 1000; i += 1) {
  ordinaryChunks.push(`paragraph ${i} with **markdown** and [link](https://example.com)\r`, "\n");
}
const ordinary = await run(ordinaryChunks, "incremental");
assert.equal(ordinary.events.length, 0);
assert.equal(ordinary.snapshot.semanticScans, 1, "ordinary Markdown should only scan once at finish()");
assert.equal(ordinary.snapshot.graph.nodes.length, 0);

// References still force a scan exactly when their closing bracket arrives.
const reference = await run(["plain @[artifact:x", "] tail\n"], "incremental");
assert.equal(reference.snapshot.semanticScans, 2, "reference close + finish should be the only scans");
assert.ok(reference.events.some((event) => event.type === "reference" && event.key === "artifact:x"));

// Stress the reference detector across many irregular chunk boundaries. This
// corpus mixes ordinary Markdown closers, malformed candidates, nested starts,
// and valid Unicode IDs. Always-scan is the executable reference behavior.
const referenceCorpus = [
  "ordinary [link](x) and [[cite:doc]] and @mention\n",
  "valid @[source:turn7search2] then @[artifact:日本語]\n",
  "malformed @[bad kind:id] @[kind:] @[kind:id|oops]\n",
  "nested @[bad @[source:ok] tail\n",
  "split targets @[tool:a-b_1] and @[ui:metric.42]\n",
].join("");
for (let seed = 1; seed <= 24; seed += 1) {
  const chunks = splitPseudoRandom(referenceCorpus, seed);
  const always = await run(chunks, "always");
  const incremental = await run(chunks, "incremental");
  assert.deepEqual(incremental.events, always.events, `reference fuzz event mismatch seed=${seed}`);
  assert.deepEqual(incremental.snapshot.graph, always.snapshot.graph, `reference fuzz graph mismatch seed=${seed}`);
}

console.log("semantic runtime incremental scan equivalence: ok");
