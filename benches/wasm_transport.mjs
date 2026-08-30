import { readFile } from "node:fs/promises";
import { Streamdown } from "../js/streamdown.js";

const wasm = await readFile(new URL("../target/wasm32-unknown-unknown/release/streamdown.wasm", import.meta.url));
const { instance } = await WebAssembly.instantiate(wasm, {});
const e = instance.exports;
const encoder = new TextEncoder();
const iterations = Number(process.env.N ?? 200_000);
const wrapperIterations = Number(process.env.WRAPPER_N ?? 100_000);
const rounds = Number(process.env.ROUNDS ?? 5);
const chunk = process.env.CHUNK ?? "token ";

const median = (values) => {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)];
};
const rate = (n, ms) => n / (ms / 1000);

function runLegacy() {
  const handle = e.md_create();
  const started = performance.now();
  for (let i = 0; i < iterations; i++) {
    const input = encoder.encode(chunk);
    const ptr = e.md_alloc(input.length);
    new Uint8Array(e.memory.buffer, ptr, input.length).set(input);
    if (!e.md_append(handle, ptr, input.length)) throw new Error("legacy append failed");
    e.md_free(ptr);
  }
  const elapsed = performance.now() - started;
  e.md_destroy(handle);
  return elapsed;
}

function runReusable() {
  const handle = e.md_create();
  const capacity = chunk.length * 3;
  const started = performance.now();
  for (let i = 0; i < iterations; i++) {
    const ptr = e.md_input_reserve(handle, capacity);
    const target = new Uint8Array(e.memory.buffer, ptr, capacity);
    const { read, written } = encoder.encodeInto(chunk, target);
    if (read !== chunk.length || !e.md_append_input(handle, written)) {
      throw new Error("reusable append failed");
    }
  }
  const elapsed = performance.now() - started;
  e.md_destroy(handle);
  return elapsed;
}

function runCachedReusable() {
  const handle = e.md_create();
  const capacity = Math.max(64, chunk.length * 3);
  const ptr = e.md_input_reserve(handle, capacity);
  let buffer = e.memory.buffer;
  let target = new Uint8Array(buffer, ptr, capacity);
  const started = performance.now();
  for (let i = 0; i < iterations; i++) {
    if (buffer !== e.memory.buffer) {
      buffer = e.memory.buffer;
      target = new Uint8Array(buffer, ptr, capacity);
    }
    const { read, written } = encoder.encodeInto(chunk, target);
    if (read !== chunk.length || !e.md_append_input(handle, written)) {
      throw new Error("cached reusable append failed");
    }
  }
  const elapsed = performance.now() - started;
  e.md_destroy(handle);
  return elapsed;
}

async function runWrapper() {
  const parser = await Streamdown.load(wasm);
  const started = performance.now();
  for (let i = 0; i < wrapperIterations; i++) parser.append(chunk);
  const elapsed = performance.now() - started;
  parser.dispose();
  return elapsed;
}

// Warm up V8/WASM before taking medians.
runCachedReusable();
const legacyTimes = [];
const reusableTimes = [];
const cachedTimes = [];
const wrapperTimes = [];
for (let i = 0; i < rounds; i++) {
  legacyTimes.push(runLegacy());
  reusableTimes.push(runReusable());
  cachedTimes.push(runCachedReusable());
  wrapperTimes.push(await runWrapper());
}

const legacy = median(legacyTimes);
const reusable = median(reusableTimes);
const cached = median(cachedTimes);
const wrapper = median(wrapperTimes);
console.log(`median of ${rounds} rounds, chunk=${JSON.stringify(chunk)}`);
console.log(`legacy alloc/copy:       ${legacy.toFixed(3)} ms (${rate(iterations, legacy).toFixed(0)} append/s)`);
console.log(`reusable reserve/view:   ${reusable.toFixed(3)} ms (${rate(iterations, reusable).toFixed(0)} append/s, ${(legacy / reusable).toFixed(2)}x legacy)`);
console.log(`cached reusable input:   ${cached.toFixed(3)} ms (${rate(iterations, cached).toFixed(0)} append/s, ${(legacy / cached).toFixed(2)}x legacy)`);
console.log(`public Streamdown.append ${wrapper.toFixed(3)} ms (${rate(wrapperIterations, wrapper).toFixed(0)} append/s)`);
