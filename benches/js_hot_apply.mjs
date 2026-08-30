import { readFile } from "node:fs/promises";
import { Streamdown } from "../js/streamdown.js";

const wasm = await readFile(new URL(
  "../target/wasm32-unknown-unknown/release/streamdown.wasm",
  import.meta.url,
));
const iterations = Number(process.env.N ?? 200_000);
const rounds = Number(process.env.ROUNDS ?? 5);
const chunk = process.env.CHUNK ?? "token ";

const median = (values) => {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)];
};
const rate = (ms) => iterations / (ms / 1000);

async function run(method) {
  const parser = await Streamdown.load(wasm);
  parser[method]("prefix ");
  const started = performance.now();
  for (let i = 0; i < iterations; i++) parser[method](chunk);
  const elapsed = performance.now() - started;
  parser.dispose();
  return elapsed;
}

const appendTimes = [];
const inPlaceTimes = [];
for (let i = 0; i < rounds; i++) {
  appendTimes.push(await run("append"));
  inPlaceTimes.push(await run("appendInPlace"));
}

const append = median(appendTimes);
const inPlace = median(inPlaceTimes);
console.log(`median of ${rounds} rounds, N=${iterations}, chunk=${JSON.stringify(chunk)}`);
console.log(`append:        ${append.toFixed(3)} ms (${rate(append).toFixed(0)} append/s)`);
console.log(`appendInPlace: ${inPlace.toFixed(3)} ms (${rate(inPlace).toFixed(0)} append/s, ${(append / inPlace).toFixed(2)}x append)`);
