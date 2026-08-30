import { performance } from "node:perf_hooks";
import { readFile } from "node:fs/promises";
import { Streamdown } from "../js/streamdown.js";
import { SemanticRuntime } from "./semantic-runtime.mjs";

const wasmPath = process.argv[2] ?? "target/wasm32-unknown-unknown/release/streamdown.wasm";
const rounds = Number(process.env.N ?? 100000);
const repeats = Number(process.env.REPEATS ?? 5);
const wasm = await readFile(wasmPath);

function median(values) {
  return [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)];
}

async function bench(name, create, append) {
  const samples = [];
  let scans = null;
  for (let r = 0; r < repeats; r += 1) {
    const subject = await create();
    for (let i = 0; i < 5000; i += 1) append(subject, "token ");
    const start = performance.now();
    for (let i = 0; i < rounds; i += 1) append(subject, "token ");
    const elapsed = performance.now() - start;
    samples.push(rounds * 1000 / elapsed);
    scans = subject.semanticScans ?? scans;
    subject.dispose();
  }
  const rate = median(samples);
  console.log(`${name.padEnd(30)} ${Math.round(rate).toString().padStart(9)} append/s${scans === null ? "" : `  semanticScans=${scans}`}`);
  return rate;
}

const parserRate = await bench(
  "Streamdown.appendInPlace",
  () => Streamdown.load(wasm),
  (parser, chunk) => parser.appendInPlace(chunk),
);
const alwaysRate = await bench(
  "SemanticRuntime(always)",
  () => SemanticRuntime.load(wasm, { semanticScan: "always" }),
  (runtime, chunk) => runtime.append(chunk),
);
const incrementalRate = await bench(
  "SemanticRuntime(incremental)",
  () => SemanticRuntime.load(wasm, { semanticScan: "incremental" }),
  (runtime, chunk) => runtime.append(chunk),
);

console.log(`incremental / always: ${(incrementalRate / alwaysRate).toFixed(2)}x`);
console.log(`incremental / bare:   ${(incrementalRate / parserRate * 100).toFixed(1)}%`);
