import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { SemanticRuntime } from "./semantic-runtime.mjs";
import { createStateRunners, SemanticStateStore } from "./semantic-state.mjs";

const wasmPath = process.argv[2] ?? "target/wasm32-unknown-unknown/release/streamdown.wasm";
const wasm = await readFile(wasmPath);
const markdown = await readFile("examples/llm_state.md", "utf8");

function byteChunks(text, width) {
  const bytes = new TextEncoder().encode(text);
  return (function* () {
    for (let offset = 0; offset < bytes.length; offset += width) {
      yield bytes.subarray(offset, Math.min(offset + width, bytes.length));
    }
  })();
}

for (const width of [1, 5, 17]) {
  const changes = [];
  const store = new SemanticStateStore({ onChange: (change) => changes.push(change) });
  const runtime = await SemanticRuntime.load(wasm, {
    concurrency: 4,
    runners: createStateRunners(store),
  });
  try {
    const result = await runtime.consume(byteChunks(markdown, width));
    assert.equal(result.scheduler["state:session"].status, "completed", `state status width=${width}`);
    assert.equal(result.scheduler["patch:step1"].status, "completed", `step1 status width=${width}`);
    assert.equal(result.scheduler["patch:step2"].status, "completed", `step2 status width=${width}`);
    assert.deepEqual(result.graph.executionOrder.filter((key) => key.startsWith("state:") || key.startsWith("patch:")), [
      "state:session",
      "patch:step1",
      "patch:step2",
    ]);
    assert.deepEqual(result.scheduler["state:session"].result, {
      count: 0,
      status: "warming",
      nested: { a: 1 },
    });
    assert.deepEqual(result.scheduler["patch:step1"].result, {
      count: 1,
      status: "ready",
      nested: { a: 1, b: 2 },
    });
    assert.deepEqual(result.scheduler["patch:step2"].result, {
      count: 1,
      status: "ready",
      nested: { b: 2 },
      extra: true,
    });
    assert.deepEqual(store.get("state:session"), result.scheduler["patch:step2"].result);
    assert.equal(store.revision("state:session"), 3);
    assert.deepEqual(changes.map(({ type, revision }) => [type, revision]), [
      ["initialize", 1],
      ["patch", 2],
      ["patch", 3],
    ]);
  } finally {
    runtime.dispose();
  }
}

const failingStore = new SemanticStateStore();
const failingRuntime = await SemanticRuntime.load(wasm, {
  runners: createStateRunners(failingStore),
});
try {
  const broken = [
    ":::llm state id=s\n{\"ok\":true}\n:::\n",
    ":::llm patch id=bad target=state:s depends=state:s\n{\n:::\n",
    ":::llm patch id=downstream target=state:s depends=patch:bad\n{\"never\":true}\n:::\n",
  ].join("");
  const result = await failingRuntime.consume(byteChunks(broken, 3));
  assert.equal(result.scheduler["state:s"].status, "completed");
  assert.equal(result.scheduler["patch:bad"].status, "failed");
  assert.match(result.scheduler["patch:bad"].error.message, /invalid JSON/);
  assert.equal(result.scheduler["patch:downstream"].status, "blocked");
  assert.deepEqual(failingStore.get("state:s"), { ok: true });
} finally {
  failingRuntime.dispose();
}

const conflictStore = new SemanticStateStore();
const conflictRuntime = await SemanticRuntime.load(wasm, {
  runners: createStateRunners(conflictStore),
});
try {
  const conflict = [
    ":::llm state id=s\n{\"count\":0}\n:::\n",
    ":::llm patch id=first target=state:s depends=state:s if_revision=1\n{\"count\":1}\n:::\n",
    ":::llm patch id=stale target=state:s depends=patch:first if_revision=1\n{\"count\":2}\n:::\n",
    ":::llm patch id=after target=state:s depends=patch:stale if_revision=2\n{\"never\":true}\n:::\n",
  ].join("");
  const result = await conflictRuntime.consume(byteChunks(conflict, 4));
  assert.equal(result.scheduler["state:s"].status, "completed");
  assert.equal(result.scheduler["patch:first"].status, "completed");
  assert.equal(result.scheduler["patch:stale"].status, "failed");
  assert.equal(result.scheduler["patch:stale"].error.name, "SemanticRevisionConflictError");
  assert.match(result.scheduler["patch:stale"].error.message, /expected 1, actual 2/);
  assert.equal(result.scheduler["patch:after"].status, "blocked");
  assert.deepEqual(conflictStore.get("state:s"), { count: 1 });
  assert.equal(conflictStore.revision("state:s"), 2);
} finally {
  conflictRuntime.dispose();
}

console.log("semantic state WASM integration: ok");
