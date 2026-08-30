#!/usr/bin/env node

import { readFile } from "node:fs/promises";
import { performance } from "node:perf_hooks";
import { resolve } from "node:path";
import { Streamdown } from "../js/streamdown.js";

function positiveInt(raw, name) {
  const value = Number(raw);
  if (!Number.isSafeInteger(value) || value <= 0) throw new Error(`${name} must be a positive integer`);
  return value;
}

let repeats = positiveInt(process.env.STREAMDOWN_BENCH_RUNS ?? "5", "STREAMDOWN_BENCH_RUNS");
let rounds = positiveInt(process.env.STREAMDOWN_WASM_BENCH_ROUNDS ?? "100000", "STREAMDOWN_WASM_BENCH_ROUNDS");
let warmup = positiveInt(process.env.STREAMDOWN_WASM_BENCH_WARMUP ?? "5000", "STREAMDOWN_WASM_BENCH_WARMUP");
let wasmPath = process.env.STREAMDOWN_WASM_BENCH_PATH ?? "target/wasm32-unknown-unknown/release/streamdown.wasm";

for (const arg of process.argv.slice(2)) {
  if (arg.startsWith("--runs=")) repeats = positiveInt(arg.slice(7), "--runs");
  else if (arg.startsWith("--rounds=")) rounds = positiveInt(arg.slice(9), "--rounds");
  else if (arg.startsWith("--warmup=")) warmup = positiveInt(arg.slice(9), "--warmup");
  else if (arg.startsWith("--wasm=")) wasmPath = arg.slice(7);
  else if (arg === "--help") {
    console.log("Usage: node scripts/wasm-bench-median.mjs [--runs=N] [--rounds=N] [--warmup=N] [--wasm=PATH]");
    process.exit(0);
  } else throw new Error(`unknown option: ${arg}`);
}

function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
}

async function benchScenario(wasm, scenario, method) {
  const parser = await Streamdown.load(wasm);
  try {
    const append = method === "appendInPlace" && typeof parser.appendInPlace === "function"
      ? (value) => parser.appendInPlace(value)
      : (value) => parser.append(value);
    if (scenario.setup) append(scenario.setup);
    for (let i = 0; i < Math.min(warmup, rounds); i++) append(scenario.chunk);
    parser.reset();
    if (scenario.setup) append(scenario.setup);

    const start = performance.now();
    for (let i = 0; i < rounds; i++) append(scenario.chunk);
    const elapsedMs = performance.now() - start;
    const bytes = rounds * Buffer.byteLength(scenario.chunk, "utf8");
    return {
      appendsPerSecond: rounds / (elapsedMs / 1000),
      mibPerSecond: bytes / 1_048_576 / (elapsedMs / 1000),
    };
  } finally {
    parser.dispose();
  }
}

const wasm = await readFile(resolve(wasmPath));
const scenarios = [
  { name: "plain-token", chunk: "token " },
  { name: "markdown-boundary", chunk: " **fast**\n\n" },
  { name: "markdown-link", chunk: "[x](url) " },
  { name: "unordered-list-line", setup: "- seed\n", chunk: "- item\n" },
  { name: "ordered-list-line", setup: "1. seed\n", chunk: "2. item\n" },
  { name: "table-row", setup: "a|b\n---|---\n", chunk: "x|y\n" },
  { name: "semantic-reference", chunk: "@[source:bench] " },
  { name: "semantic-citation", chunk: "[[cite:doc|Spec]] " },
  { name: "open-code", setup: "```text\n", chunk: "0123456789abcdef0123456789abcdef\n" },
  { name: "llm-semantic", setup: ":::llm tool name=bench id=q1\n", chunk: "{\"token\":\"0123456789abcdef\"}\n" },
];

const samples = new Map();
for (let run = 0; run < repeats; run++) {
  for (const scenario of scenarios) {
    for (const method of ["append", "appendInPlace"]) {
      const result = await benchScenario(wasm, scenario, method);
      const key = `${scenario.name}/${method === "appendInPlace" ? "inplace" : "append"}`;
      let entry = samples.get(key);
      if (!entry) {
        entry = { appends: [], mib: [] };
        samples.set(key, entry);
      }
      entry.appends.push(result.appendsPerSecond);
      entry.mib.push(result.mibPerSecond);
    }
  }
}

console.log(`wasm-bench median (${repeats} runs, ${rounds} appends/scenario)`);
for (const [name, values] of samples) {
  console.log(
    `${name.padEnd(27)} ${Math.round(median(values.appends)).toLocaleString("en-US").padStart(10)} append/s  ` +
    `${median(values.mib).toFixed(1).padStart(7)} MiB/s`,
  );
}
