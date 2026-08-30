import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { Streamdown } from "../js/streamdown.js";
import { buildSemanticGraph } from "./semantic-graph.mjs";
import { createTimelineState, observeSemanticState, semanticReferencesFromLinks } from "./semantic-timeline-core.mjs";
import { SemanticScheduler } from "./semantic-scheduler.mjs";

const wasmPath = process.argv[2] ?? "target/wasm32-unknown-unknown/release/streamdown.wasm";
const wasm = await readFile(wasmPath);
const markdown = await readFile("examples/llm_graph.md", "utf8");
const parser = await Streamdown.load(wasm);
const timelineState = createTimelineState();
const calls = [];
const scheduler = new SemanticScheduler({
  concurrency: 4,
  runners: {
    tool: async (node) => { calls.push(node.key); return { hits: 3 }; },
    artifact: async (node, context) => {
      assert.deepEqual(context.dependencyResults["tool:search"], { hits: 3 });
      calls.push(node.key);
      return { title: "summary" };
    },
    ui: async (node, context) => {
      assert.deepEqual(context.dependencyResults["artifact:summary"], { title: "summary" });
      calls.push(node.key);
      return { rendered: true };
    },
  },
});

const bytes = new TextEncoder().encode(markdown);
const decoder = new TextDecoder();
let offset = 0;
let chunkIndex = 0;

function summary() {
  const links = parser.getLinks();
  return {
    llmBlocks: parser.getLlmBlocks(),
    semanticReferences: semanticReferencesFromLinks(links),
  };
}

try {
  while (offset < bytes.length) {
    const end = Math.min(offset + 5, bytes.length);
    const text = decoder.decode(bytes.subarray(offset, end), { stream: end < bytes.length });
    if (text) parser.appendInPlace(text);
    const observed = observeSemanticState(summary(), timelineState, end, chunkIndex);
    scheduler.updateGraph(observed.graph);
    for (const event of observed.events) scheduler.accept(event);
    // Give completed roots a chance to unlock later streamed dependents.
    await scheduler.idle();
    offset = end;
    chunkIndex += 1;
  }
  parser.finish();
  const finalSummary = summary();
  const finalGraph = buildSemanticGraph(finalSummary);
  scheduler.updateGraph(finalGraph);
  await scheduler.idle();

  assert.deepEqual(calls, ["tool:search", "artifact:summary", "ui:metric"]);
  assert.equal(scheduler.get("tool:search").status, "completed");
  assert.equal(scheduler.get("artifact:summary").status, "completed");
  assert.equal(scheduler.get("ui:metric").status, "completed");
  console.log("semantic scheduler WASM integration: ok");
} finally {
  parser.dispose();
}
