#!/usr/bin/env node

import { performance } from "node:perf_hooks";
import { createSemanticJournalHooks, SemanticJournal } from "./semantic-journal.mjs";

const updates = Number(process.env.N ?? 50000);
const repeats = Number(process.env.REPEATS ?? 5);
if (!Number.isSafeInteger(updates) || updates <= 0) throw new RangeError("N must be a positive integer");
if (!Number.isSafeInteger(repeats) || repeats <= 0) throw new RangeError("REPEATS must be a positive integer");

function median(values) {
  return [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)];
}

function recordWorkload(count, scheduler) {
  const journal = new SemanticJournal();
  const hooks = createSemanticJournalHooks(journal, { scheduler });
  let schedulerSequence = 0;
  for (let i = 1; i <= count; i += 1) {
    const key = `patch:p${i}`;
    hooks.onTransition({ key, status: "ready", previousStatus: null, sequence: ++schedulerSequence });
    hooks.onTransition({ key, status: "queued", previousStatus: "ready", sequence: ++schedulerSequence });
    hooks.onTransition({ key, status: "running", previousStatus: "queued", sequence: ++schedulerSequence });
    hooks.onStateChange({
      key: "state:session",
      revision: i,
      type: i === 1 ? "initialize" : "patch",
      node: key,
      value: { count: i, status: i === count ? "done" : "streaming" },
      format: "merge",
    });
    hooks.onTransition({
      key,
      status: "completed",
      previousStatus: "running",
      sequence: ++schedulerSequence,
      result: { count: i, status: i === count ? "done" : "streaming" },
    });
  }
  return journal;
}

async function benchMode(scheduler) {
  const recordRates = [];
  const serializeRates = [];
  const sizes = [];
  const entryCounts = [];
  for (let repeat = 0; repeat < repeats; repeat += 1) {
    const recordStart = performance.now();
    const journal = recordWorkload(updates, scheduler);
    const recordElapsed = performance.now() - recordStart;
    recordRates.push(updates * 1000 / recordElapsed);

    const serializeStart = performance.now();
    const ndjson = journal.toNDJSON();
    const serializeElapsed = performance.now() - serializeStart;
    serializeRates.push(updates * 1000 / serializeElapsed);
    sizes.push(Buffer.byteLength(ndjson));
    entryCounts.push(journal.entries.length);

    const verified = journal.verify();
    if (!verified.ok) throw new Error(`journal benchmark generated invalid journal: ${verified.errors.join("; ")}`);
  }

  const recordRate = median(recordRates);
  const serializeRate = median(serializeRates);
  const bytes = median(sizes);
  const entries = median(entryCounts);
  return {
    scheduler,
    recordRate,
    serializeRate,
    bytes,
    entries,
    entriesPerUpdate: entries / updates,
    bytesPerUpdate: bytes / updates,
    bytesPerEntry: bytes / entries,
  };
}

console.log(`updates: ${updates}, repeats: ${repeats}`);
for (const scheduler of ["all", "terminal", "none"]) {
  const result = await benchMode(scheduler);
  console.log(
    `${scheduler.padEnd(8)} record=${Math.round(result.recordRate).toString().padStart(8)}/s` +
    ` serialize=${Math.round(result.serializeRate).toString().padStart(8)}/s` +
    ` entries/update=${result.entriesPerUpdate.toFixed(1)}` +
    ` bytes/update=${result.bytesPerUpdate.toFixed(1)}` +
    ` bytes/entry=${result.bytesPerEntry.toFixed(1)}`,
  );
}
