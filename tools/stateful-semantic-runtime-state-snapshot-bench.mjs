import { performance } from "node:perf_hooks";
import { readFile } from "node:fs/promises";
import { StatefulSemanticRuntime } from "./stateful-semantic-runtime.mjs";

const wasmPath = process.argv[2] ?? "target/wasm32-unknown-unknown/release/streamdown.wasm";
const wasm = await readFile(wasmPath);
const states = Number(process.env.STATES ?? 500);
const bytes = Number(process.env.STATE_BYTES ?? 16384);
const repeats = Number(process.env.REPEATS ?? 7);
const LIGHT = { document: false, graph: false, diagnostics: false, scheduler: false };

function median(values) {
  return [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)];
}

const runtime = await StatefulSemanticRuntime.load(wasm);
try {
  const payload = "x".repeat(Math.max(0, bytes - 64));
  for (let i = 0; i < states; i += 1) {
    runtime.stateStore.initialize({
      kind: "state",
      key: `state:s${i}`,
      attributes: {},
      value: JSON.stringify({ i, payload }),
    });
  }

  const withState = [];
  const withoutState = [];
  for (let repeat = 0; repeat < repeats; repeat += 1) {
    let start = performance.now();
    runtime.snapshot(LIGHT);
    withState.push(performance.now() - start);

    start = performance.now();
    runtime.snapshot({ ...LIGHT, state: false });
    withoutState.push(performance.now() - start);
  }

  const full = median(withState);
  const omitted = median(withoutState);
  console.log(`states=${states} bytes/state≈${bytes} repeats=${repeats}`);
  console.log(`with-state    ${full.toFixed(2)} ms`);
  console.log(`state:false   ${omitted.toFixed(3)} ms`);
  console.log(`speedup       ${(full / omitted).toFixed(1)}x`);
} finally {
  runtime.dispose();
}
