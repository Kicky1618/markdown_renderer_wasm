import { performance } from "node:perf_hooks";
import { readFile } from "node:fs/promises";
import { SemanticRuntime } from "./semantic-runtime.mjs";

const wasmPath = process.argv[2] ?? "target/wasm32-unknown-unknown/release/streamdown.wasm";
const rounds = Number(process.env.N ?? 100000);
const repeats = Number(process.env.REPEATS ?? 5);
const warmup = Number(process.env.WARMUP ?? 5000);
const wasm = await readFile(wasmPath);

function median(values) {
  return [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)];
}

async function bench(mode) {
  const rates = [];
  let semanticScans = 0;
  for (let repeat = 0; repeat < repeats; repeat += 1) {
    const runtime = await SemanticRuntime.load(wasm, { semanticScan: mode });
    try {
      runtime.append(":::llm tool id=stream\n");
      for (let i = 0; i < warmup; i += 1) runtime.append("payload\n");
      const start = performance.now();
      for (let i = 0; i < rounds; i += 1) runtime.append("payload\n");
      const elapsed = performance.now() - start;
      rates.push(rounds * 1000 / elapsed);
      semanticScans = runtime.semanticScans;
    } finally {
      runtime.dispose();
    }
  }
  return { mode, appendPerSecond: median(rates), semanticScans };
}

const always = await bench("always");
const incremental = await bench("incremental");

for (const result of [always, incremental]) {
  console.log(`${result.mode.padEnd(12)} ${Math.round(result.appendPerSecond).toString().padStart(9)} append/s  semanticScans=${result.semanticScans}`);
}
console.log(`speedup ${(incremental.appendPerSecond / always.appendPerSecond).toFixed(2)}x`);
