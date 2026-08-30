import { performance } from "node:perf_hooks";
import { readFile } from "node:fs/promises";
import { Streamdown } from "../js/streamdown.js";
import { SemanticRuntime } from "./semantic-runtime.mjs";

const wasmPath = process.argv[2] ?? "target/wasm32-unknown-unknown/release/streamdown.wasm";
const wasm = await readFile(wasmPath);
const sizes = (process.env.SIZES ?? "100,200,400,800,1600")
  .split(",")
  .map(Number)
  .filter((n) => Number.isSafeInteger(n) && n > 0);

function chunks(count) {
  const out = [];
  for (let i = 0; i < count; i += 1) {
    out.push(`:::llm tool id=t${i}\n`, `{"i":${i}}\n`, ":::\n");
  }
  return out;
}

async function measureBare(input) {
  const parser = await Streamdown.load(wasm);
  const start = performance.now();
  for (const chunk of input) parser.appendInPlace(chunk);
  parser.finish();
  const elapsed = performance.now() - start;
  parser.dispose();
  return elapsed;
}

async function measureRuntime(input) {
  const runtime = await SemanticRuntime.load(wasm, { semanticScan: "incremental" });
  const start = performance.now();
  for (const chunk of input) runtime.append(chunk);
  await runtime.finish();
  const elapsed = performance.now() - start;
  const scans = runtime.semanticScans;
  runtime.dispose();
  return { elapsed, scans };
}

for (const size of sizes) {
  const input = chunks(size);
  // Warm JIT with a small prefix before recording each size.
  await measureBare(input.slice(0, Math.min(input.length, 60)));
  await measureRuntime(input.slice(0, Math.min(input.length, 60)));
  const bare = await measureBare(input);
  const runtime = await measureRuntime(input);
  console.log(`${size}\tbare=${bare.toFixed(2)}ms\truntime=${runtime.elapsed.toFixed(2)}ms\tover=${(runtime.elapsed / bare).toFixed(1)}x\trt/block=${(runtime.elapsed / size).toFixed(3)}ms\tscans=${runtime.scans}`);
}
