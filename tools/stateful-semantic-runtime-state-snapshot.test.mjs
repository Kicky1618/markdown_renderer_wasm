import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { StatefulSemanticRuntime } from "./stateful-semantic-runtime.mjs";

const wasmPath = process.argv[2] ?? "target/wasm32-unknown-unknown/release/streamdown.wasm";
const wasm = await readFile(wasmPath);
const LIGHT = { document: false, graph: false, diagnostics: false, scheduler: false };

const runtime = await StatefulSemanticRuntime.load(wasm);
try {
  runtime.stateStore.initialize({
    kind: "state",
    key: "state:s",
    attributes: {},
    value: '{"count":1,"nested":{"ok":true}}',
  });

  const full = runtime.snapshot(LIGHT);
  assert.deepEqual(full.state, {
    values: { "state:s": { count: 1, nested: { ok: true } } },
    revisions: { "state:s": 1 },
  });

  const omitted = runtime.snapshot({ ...LIGHT, state: false });
  assert.equal("state" in omitted, false);
  assert.equal(omitted.blockCount, 0);
  assert.equal(omitted.semanticScans, 0);

  const consumed = await runtime.consume("plain text", {
    snapshotOptions: { ...LIGHT, state: false },
  });
  assert.equal("state" in consumed, false);
} finally {
  runtime.dispose();
}

console.log("stateful semantic runtime selective state snapshot: ok");
