#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { resolve } from "node:path";
import process from "node:process";

const runs = Number.parseInt(process.env.STREAM_BENCH_RUNS || "5", 10);
if (!Number.isInteger(runs) || runs < 1 || runs > 31 || runs % 2 === 0) {
  console.error("stream bench median: STREAM_BENCH_RUNS must be an odd integer in 1..31");
  process.exit(2);
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: process.cwd(),
    encoding: "utf8",
    ...options,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    if (result.stdout) process.stdout.write(result.stdout);
    if (result.stderr) process.stderr.write(result.stderr);
    process.exit(result.status ?? 1);
  }
  return result;
}

run("cargo", ["build", "--release", "--bin", "stream-bench"], { stdio: "inherit" });

const binary = resolve("target", "release", process.platform === "win32" ? "stream-bench.exe" : "stream-bench");
if (!existsSync(binary)) {
  console.error(`stream bench median: benchmark binary not found: ${binary}`);
  process.exit(1);
}

const samples = new Map();
const order = [];
for (let attempt = 0; attempt < runs; attempt += 1) {
  const result = run(binary, [], { stdio: ["ignore", "pipe", "inherit"] });
  for (const rawLine of result.stdout.trim().split(/\r?\n/)) {
    let match = rawLine.match(/^(.+): .*\(([0-9.]+) appends\/s, ([0-9.]+) MiB\/s\)$/);
    let sample;
    if (match) {
      sample = { appendsPerSecond: Number(match[2]), mibPerSecond: Number(match[3]) };
    } else {
      match = rawLine.match(/^(.+): .*\(([0-9.]+) MiB\/s\)$/);
      if (!match) {
        console.error(`stream bench median: unrecognized output: ${rawLine}`);
        process.exit(1);
      }
      sample = { mibPerSecond: Number(match[2]) };
    }
    const label = match[1];
    if (!samples.has(label)) {
      samples.set(label, []);
      order.push(label);
    }
    samples.get(label).push(sample);
  }
}

function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)];
}

console.log(`stream-bench median (${runs} runs)`);
for (const label of order) {
  const group = samples.get(label);
  if (group.length !== runs) {
    console.error(`stream bench median: ${label} produced ${group.length}/${runs} samples`);
    process.exit(1);
  }
  const mib = median(group.map(sample => sample.mibPerSecond));
  const appends = group[0].appendsPerSecond === undefined
    ? null
    : median(group.map(sample => sample.appendsPerSecond));
  if (appends === null) {
    console.log(`${label.padEnd(30)} ${mib.toFixed(1)} MiB/s`);
  } else {
    console.log(`${label.padEnd(30)} ${Math.round(appends).toLocaleString("en-US")} append/s  ${mib.toFixed(1)} MiB/s`);
  }
}
