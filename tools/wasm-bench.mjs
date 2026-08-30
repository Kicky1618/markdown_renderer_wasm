#!/usr/bin/env node

import { readFile } from "node:fs/promises";
import { performance } from "node:perf_hooks";
import { resolve } from "node:path";
import { Streamdown } from "../js/streamdown.js";

function parseArgs(argv) {
  const options = {
    wasm: "target/wasm32-unknown-unknown/release/streamdown.wasm",
    rounds: 100_000,
    warmup: 5_000,
    json: false,
  };
  for (const arg of argv) {
    if (arg === "--json") options.json = true;
    else if (arg.startsWith("--wasm=")) options.wasm = arg.slice(7);
    else if (arg.startsWith("--rounds=")) options.rounds = positiveInt(arg.slice(9), "--rounds");
    else if (arg.startsWith("--warmup=")) options.warmup = positiveInt(arg.slice(9), "--warmup");
    else if (arg === "--help") {
      console.log("Usage: node tools/wasm-bench.mjs [--rounds=N] [--warmup=N] [--wasm=PATH] [--json]");
      process.exit(0);
    } else throw new Error(`unknown option: ${arg}`);
  }
  return options;
}

function positiveInt(raw, name) {
  const value = Number(raw);
  if (!Number.isSafeInteger(value) || value <= 0) throw new Error(`${name} must be a positive integer`);
  return value;
}

async function makeParser(wasm) {
  return Streamdown.load(wasm);
}

async function benchScenario(wasm, { name, setup = "", chunk, rounds, warmup }) {
  const parser = await makeParser(wasm);
  try {
    if (setup) parser.append(setup);
    for (let i = 0; i < warmup; i++) parser.append(chunk);
    parser.reset();
    if (setup) parser.append(setup);

    const start = performance.now();
    for (let i = 0; i < rounds; i++) parser.append(chunk);
    const elapsedMs = performance.now() - start;
    const bytes = rounds * new TextEncoder().encode(chunk).length;
    return {
      name,
      rounds,
      bytes,
      elapsedMs,
      appendsPerSecond: rounds / (elapsedMs / 1000),
      mibPerSecond: bytes / 1_048_576 / (elapsedMs / 1000),
      finalBlocks: parser.blockCount,
    };
  } finally {
    parser.dispose();
  }
}

const options = parseArgs(process.argv.slice(2));
const wasm = await readFile(resolve(options.wasm));
const scenarios = [
  { name: "plain-token", chunk: "token " },
  { name: "markdown-boundary", chunk: " **fast**\n\n" },
  { name: "open-code", setup: "```text\n", chunk: "0123456789abcdef0123456789abcdef\n" },
  { name: "llm-semantic", setup: ":::llm tool name=bench id=q1\n", chunk: "{\"token\":\"0123456789abcdef\"}\n" },
];

const results = [];
for (const scenario of scenarios) {
  results.push(await benchScenario(wasm, {
    ...scenario,
    rounds: options.rounds,
    warmup: Math.min(options.warmup, options.rounds),
  }));
}

const report = {
  wasm: resolve(options.wasm),
  node: process.version,
  rounds: options.rounds,
  results,
};

if (options.json) {
  console.log(JSON.stringify(report, null, 2));
} else {
  for (const result of results) {
    console.log(
      `${result.name.padEnd(18)} ${result.appendsPerSecond.toFixed(0).padStart(10)} append/s  ` +
      `${result.mibPerSecond.toFixed(1).padStart(7)} MiB/s  ${result.elapsedMs.toFixed(2)} ms`,
    );
  }
}
