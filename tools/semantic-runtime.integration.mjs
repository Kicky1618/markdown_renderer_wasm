import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { SemanticRuntime } from "./semantic-runtime.mjs";

const wasmPath = process.argv[2] ?? "target/wasm32-unknown-unknown/release/streamdown.wasm";
const wasm = await readFile(wasmPath);
const markdown = await readFile("examples/llm_graph.md", "utf8");
const bytes = new TextEncoder().encode(markdown);
const calls = [];
const semanticEvents = [];
let toolStarted = false;
let releaseTool;
const toolGate = new Promise((resolve) => { releaseTool = resolve; });

const runtime = await SemanticRuntime.load(wasm, {
  concurrency: 3,
  onSemanticEvent(event) {
    semanticEvents.push(event);
    if (event.type === "open" && event.key === "artifact:summary") {
      assert.equal(toolStarted, true, "parser should continue while tool runner is pending");
      releaseTool();
    }
  },
  runners: {
    tool: async (node) => {
      const request = JSON.parse(node.value);
      assert.equal(request.query, "streaming markdown wasm");
      calls.push(node.key);
      toolStarted = true;
      await toolGate;
      return { hits: [{ title: "Streamdown" }] };
    },
    artifact: async (node, context) => {
      assert.equal(JSON.parse(node.value).fast, true);
      assert.equal(context.dependencyResults["tool:search"].hits[0].title, "Streamdown");
      calls.push(node.key);
      return { summary: "Streamdown is ready" };
    },
    ui: async (node, context) => {
      assert.equal(JSON.parse(node.value).label, "Fast path");
      assert.equal(context.dependencyResults["artifact:summary"].summary, "Streamdown is ready");
      calls.push(node.key);
      return { rendered: true };
    },
  },
});

async function* chunks() {
  for (let offset = 0; offset < bytes.length; offset += 5) {
    yield bytes.subarray(offset, Math.min(offset + 5, bytes.length));
    // Yield to runner microtasks so tool execution can overlap with later input.
    await Promise.resolve();
  }
}

try {
  const result = await runtime.consume(chunks());
  assert.deepEqual(calls, ["tool:search", "artifact:summary", "ui:metric"]);
  assert.deepEqual(result.graph.executionOrder, ["tool:search", "artifact:summary", "ui:metric"]);
  assert.equal(result.scheduler["tool:search"].status, "completed");
  assert.equal(result.scheduler["artifact:summary"].status, "completed");
  assert.equal(result.scheduler["ui:metric"].status, "completed");
  assert.ok(semanticEvents.some((event) => event.type === "ready" && event.key === "tool:search"));
  assert.ok(semanticEvents.some((event) => event.type === "reference" && event.key === "ui:metric"));
  console.log("semantic runtime WASM integration: ok");
} finally {
  runtime.dispose();
}

const byteEvents = [];
const byteRuntime = await SemanticRuntime.load(wasm, {
  onSemanticEvent: (event) => byteEvents.push(event),
});
const unicodeReference = "前 @[artifact:日本語] 後";
const unicodeBytes = new TextEncoder().encode(unicodeReference);
async function* bytewiseUnicode() {
  for (let i = 0; i < unicodeBytes.length; i += 1) yield unicodeBytes.subarray(i, i + 1);
}
try {
  await byteRuntime.consume(bytewiseUnicode());
  const referenceEvent = byteEvents.find((event) => event.type === "reference" && event.key === "artifact:日本語");
  assert.ok(referenceEvent, "bytewise Unicode stream should emit semantic reference");
  const expectedObservedBytes = new TextEncoder().encode("前 @[artifact:日本語]").length;
  assert.equal(referenceEvent.observedAtByte, expectedObservedBytes);
} finally {
  byteRuntime.dispose();
}

