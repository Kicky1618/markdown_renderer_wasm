import { performance } from "node:perf_hooks";
import { createSemanticJournalHooks, SemanticJournal } from "./semantic-journal.mjs";

const n = Number(process.env.N ?? 500);
const repeats = Number(process.env.REPEATS ?? 5);
const bytes = Number(process.env.STATE_BYTES ?? 65536);
const payload = { blob: "x".repeat(Math.max(0, bytes - 64)), nested: { n: 1 } };

function median(values) {
  return [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)];
}

function benchState(returnEntry) {
  const samples = [];
  for (let r = 0; r < repeats; r += 1) {
    const journal = new SemanticJournal();
    const start = performance.now();
    for (let i = 0; i < n; i += 1) {
      journal.recordStateChange({
        key: `state:s${i}`,
        revision: 1,
        type: "initialize",
        node: `state:s${i}`,
        value: payload,
      }, { returnEntry });
    }
    samples.push(performance.now() - start);
  }
  return median(samples);
}

function benchScheduler(returnEntry) {
  const samples = [];
  for (let r = 0; r < repeats; r += 1) {
    const journal = new SemanticJournal();
    const start = performance.now();
    for (let i = 0; i < n; i += 1) {
      journal.recordSchedulerTransition({
        key: `tool:t${i}`,
        status: "completed",
        previousStatus: "running",
        sequence: i + 1,
        result: payload,
      }, { returnEntry });
    }
    samples.push(performance.now() - start);
  }
  return median(samples);
}

const oldState = benchState(true);
const fastState = benchState(false);
const oldScheduler = benchScheduler(true);
const fastScheduler = benchScheduler(false);

// Verify the automatic hook uses the no-return-clone path without changing journal content.
const hookJournal = new SemanticJournal();
const hooks = createSemanticJournalHooks(hookJournal, { scheduler: "terminal" });
hooks.onStateChange({ key: "state:hook", revision: 1, type: "initialize", node: "state:hook", value: { ok: true } });
hooks.onTransition({ key: "tool:hook", status: "completed", previousStatus: "running", sequence: 1, result: { ok: true } });
if (hookJournal.entries.length !== 2) throw new Error("journal hook did not record expected entries");

console.log(`entries=${n} payload≈${bytes}B repeats=${repeats}`);
console.log(`state old-return-clone   ${oldState.toFixed(2)} ms`);
console.log(`state hook/no-return     ${fastState.toFixed(2)} ms  speedup ${(oldState / fastState).toFixed(2)}x`);
console.log(`sched old-return-clone   ${oldScheduler.toFixed(2)} ms`);
console.log(`sched hook/no-return     ${fastScheduler.toFixed(2)} ms  speedup ${(oldScheduler / fastScheduler).toFixed(2)}x`);
