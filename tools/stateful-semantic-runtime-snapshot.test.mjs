import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { StatefulSemanticRuntime } from "./stateful-semantic-runtime.mjs";

const wasmPath = process.argv[2] ?? "target/wasm32-unknown-unknown/release/streamdown.wasm";
const wasm = await readFile(wasmPath);
const LIGHT = { document: false, graph: false, diagnostics: false, scheduler: false };

async function instrument(runtime, operation) {
  const calls = [];
  const original = runtime.runtime.snapshot.bind(runtime.runtime);
  runtime.runtime.snapshot = (options) => {
    calls.push(options);
    return original(options);
  };
  try {
    const result = await operation();
    return { calls, result };
  } finally {
    runtime.runtime.snapshot = original;
  }
}

{
  const runtime = await StatefulSemanticRuntime.load(wasm);
  try {
    runtime.append(":::llm state id=session\n");
    runtime.append('{"count":1}\n');
    runtime.append(":::\n");
    const { calls, result } = await instrument(runtime, () => runtime.finish());
    assert.equal(calls.length, 2);
    assert.deepEqual(calls[0], LIGHT, "internal finish snapshot must be metadata-only");
    assert.equal(calls[1], undefined, "outer snapshot keeps the default full compatibility result");
    assert.equal(result.state.values["state:session"].count, 1);
    assert.ok(Array.isArray(result.document));
    assert.ok(result.graph);
    assert.ok(result.scheduler);
  } finally {
    runtime.dispose();
  }
}

{
  const runtime = await StatefulSemanticRuntime.load(wasm);
  try {
    const requested = { document: false, graph: false, diagnostics: false, scheduler: true };
    const source = [
      ":::llm state id=session\n",
      '{"count":2}\n',
      ":::\n",
    ];
    const { calls, result } = await instrument(runtime, () => runtime.consume(source, { snapshotOptions: requested }));
    assert.equal(calls.length, 2);
    assert.deepEqual(calls[0], LIGHT, "internal consume snapshot must be metadata-only");
    assert.deepEqual(calls[1], requested, "outer snapshot must honor caller selection");
    assert.equal("document" in result, false);
    assert.equal("graph" in result, false);
    assert.ok(result.scheduler);
    assert.equal(result.state.values["state:session"].count, 2);
  } finally {
    runtime.dispose();
  }
}

{
  const runtime = await StatefulSemanticRuntime.load(wasm);
  try {
    const { calls } = await instrument(runtime, () => runtime.idle({
      document: false,
      graph: false,
      diagnostics: false,
      scheduler: false,
    }));
    assert.equal(calls.length, 2);
    assert.deepEqual(calls[0], LIGHT, "internal idle snapshot must be metadata-only");
    assert.deepEqual(calls[1], LIGHT);
  } finally {
    runtime.dispose();
  }
}

console.log("stateful semantic runtime selective snapshot: ok");
