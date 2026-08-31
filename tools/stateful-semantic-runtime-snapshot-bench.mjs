import { performance } from "node:perf_hooks";
import { readFile } from "node:fs/promises";
import { StatefulSemanticRuntime } from "./stateful-semantic-runtime.mjs";

const wasmPath = process.argv[2] ?? "target/wasm32-unknown-unknown/release/streamdown.wasm";
const wasm = await readFile(wasmPath);
const blocks = Number(process.env.N ?? 25000);
const repeats = Number(process.env.REPEATS ?? 7);
const LIGHT = Object.freeze({ document: false, graph: false, diagnostics: false, scheduler: false });

function median(values) {
  return [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)];
}

const runtime = await StatefulSemanticRuntime.load(wasm);
try {
  for (let i = 0; i < blocks; i += 1) {
    runtime.append(`:::llm tool id=t${i}\n`);
    runtime.append(`{"i":${i}}\n`);
    runtime.append(":::\n");
  }
  await runtime.runtime.idle(LIGHT);

  const oldStyle = [];
  const optimized = [];
  for (let i = 0; i < repeats; i += 1) {
    let start = performance.now();
    await runtime.runtime.idle();
    runtime.snapshot();
    oldStyle.push(performance.now() - start);

    start = performance.now();
    await runtime.idle();
    optimized.push(performance.now() - start);
  }

  const oldMedian = median(oldStyle);
  const optimizedMedian = median(optimized);
  console.log(`blocks=${blocks} repeats=${repeats}`);
  console.log(`old-double-full ${oldMedian.toFixed(2)} ms`);
  console.log(`optimized       ${optimizedMedian.toFixed(2)} ms`);
  console.log(`speedup         ${(oldMedian / optimizedMedian).toFixed(2)}x`);
} finally {
  runtime.dispose();
}
