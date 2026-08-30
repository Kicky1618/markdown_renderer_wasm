import { performance } from "node:perf_hooks";
import { readFile } from "node:fs/promises";
import { Streamdown } from "../js/streamdown.js";
import { SemanticRuntime } from "./semantic-runtime.mjs";

const wasmPath = process.argv[2] ?? "target/wasm32-unknown-unknown/release/streamdown.wasm";
const rounds = Number(process.env.N ?? 100000);
const repeats = Number(process.env.REPEATS ?? 5);
const warmup = Number(process.env.WARMUP ?? 5000);
const chunk = process.env.CHUNK ?? "token ";
const wasm = await readFile(wasmPath);

function median(values) {
  return [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)];
}

async function bench(name, create, append) {
  const samples = [];
  let scans = null;
  for (let r = 0; r < repeats; r += 1) {
    const subject = await create();
    for (let i = 0; i < warmup; i += 1) append(subject, chunk);
    const start = performance.now();
    for (let i = 0; i < rounds; i += 1) append(subject, chunk);
    const elapsed = performance.now() - start;
    samples.push(rounds * 1000 / elapsed);
    scans = subject.semanticScans ?? scans;
    subject.dispose();
  }
  const rate = median(samples);
  console.log(`${name.padEnd(30)} ${Math.round(rate).toString().padStart(9)} append/s${scans === null ? "" : `  semanticScans=${scans}`}`);
  return rate;
}

console.log(`chunk=${JSON.stringify(chunk)} rounds=${rounds} repeats=${repeats} warmup=${warmup}`);
const parserRate = await bench(
  "Streamdown.appendInPlace",
  () => Streamdown.load(wasm),
  (parser, value) => parser.appendInPlace(value),
);
const alwaysRate = await bench(
  "SemanticRuntime(always)",
  () => SemanticRuntime.load(wasm, { semanticScan: "always" }),
  (runtime, value) => runtime.append(value),
);
const incrementalRate = await bench(
  "SemanticRuntime(incremental)",
  () => SemanticRuntime.load(wasm, { semanticScan: "incremental" }),
  (runtime, value) => runtime.append(value),
);

console.log(`incremental / always: ${(incrementalRate / alwaysRate).toFixed(2)}x`);
console.log(`incremental / bare:   ${(incrementalRate / parserRate * 100).toFixed(1)}%`);