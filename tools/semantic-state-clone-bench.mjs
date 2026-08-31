import { performance } from "node:perf_hooks";

const bytes = Number(process.env.STATE_BYTES ?? 65536);
const rounds = Number(process.env.N ?? 2000);
const repeats = Number(process.env.REPEATS ?? 7);
const value = {
  text: "x".repeat(Math.max(0, bytes - 128)),
  nested: { count: 1, ready: true },
  list: [1, 2, 3, 4],
};

function median(values) {
  return [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)];
}

function jsonClone(input) {
  return JSON.parse(JSON.stringify(input));
}

function bench(name, clone) {
  const samples = [];
  let sink = 0;
  for (let repeat = 0; repeat < repeats; repeat += 1) {
    const start = performance.now();
    for (let i = 0; i < rounds; i += 1) sink += clone(value).nested.count;
    samples.push(performance.now() - start);
  }
  const elapsed = median(samples);
  console.log(`${name.padEnd(18)} ${elapsed.toFixed(2)} ms  ${(elapsed * 1000 / rounds).toFixed(2)} us/clone`);
  return { elapsed, sink };
}

const json = bench("JSON clone", jsonClone);
if (typeof globalThis.structuredClone === "function") {
  const structured = bench("structuredClone", (input) => globalThis.structuredClone(input));
  console.log(`speedup ${(json.elapsed / structured.elapsed).toFixed(2)}x`);
}
