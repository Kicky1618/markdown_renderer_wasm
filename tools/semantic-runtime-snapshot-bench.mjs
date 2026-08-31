import { performance } from "node:perf_hooks";
import { readFile } from "node:fs/promises";
import { SemanticRuntime } from "./semantic-runtime.mjs";

const wasmPath = process.argv[2] ?? "target/wasm32-unknown-unknown/release/streamdown.wasm";
const wasm = await readFile(wasmPath);
const count = Number(process.env.N ?? 25000);

if (!Number.isSafeInteger(count) || count <= 0) throw new RangeError("N must be a positive integer");

const variants = [
  ["full", undefined],
  ["no-document", { document: false }],
  ["graph+diagnostics", { document: false, scheduler: false }],
  ["scheduler-only", { document: false, graph: false, diagnostics: false }],
  ["metadata-only", { document: false, graph: false, diagnostics: false, scheduler: false }],
];

async function buildRuntime() {
  const runtime = await SemanticRuntime.load(wasm, { semanticScan: "incremental" });
  for (let i = 0; i < count; i += 1) {
    runtime.append(`:::llm tool id=t${i}\n`);
    runtime.append(`{"i":${i}}\n`);
    runtime.append(":::\n");
  }
  return runtime;
}

for (const [name, options] of variants) {
  globalThis.gc?.();
  const runtime = await buildRuntime();
  globalThis.gc?.();
  const start = performance.now();
  const snapshot = runtime.snapshot(options);
  const elapsed = performance.now() - start;
  const bytes = Buffer.byteLength(JSON.stringify(snapshot));
  console.log(`${name.padEnd(18)} ${elapsed.toFixed(2).padStart(9)} ms  json=${(bytes / 1024 / 1024).toFixed(2)} MiB`);
  runtime.dispose();
}
