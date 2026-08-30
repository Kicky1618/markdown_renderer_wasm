import assert from "node:assert/strict";
import { createSemanticJournalHooks, SemanticJournal } from "./semantic-journal.mjs";

const journal = new SemanticJournal();
const observed = [];
const hooks = createSemanticJournalHooks(journal, {
  onTransition: (entry) => observed.push(["scheduler", entry.key, entry.status]),
  onStateChange: (entry) => observed.push(["state", entry.key, entry.revision]),
});

hooks.onTransition({ key: "state:s", status: "ready", previousStatus: null, sequence: 1 });
hooks.onTransition({ key: "state:s", status: "running", previousStatus: "ready", sequence: 2 });
hooks.onStateChange({ key: "state:s", revision: 1, type: "initialize", node: "state:s", value: { n: 0 } });
hooks.onTransition({ key: "state:s", status: "completed", previousStatus: "running", sequence: 3, result: { n: 0 } });
hooks.onTransition({ key: "patch:p", status: "ready", previousStatus: null, sequence: 4 });
hooks.onTransition({ key: "patch:p", status: "running", previousStatus: "ready", sequence: 5 });
hooks.onStateChange({ key: "state:s", revision: 2, type: "patch", node: "patch:p", format: "merge", value: { n: 1 } });
hooks.onTransition({ key: "patch:p", status: "completed", previousStatus: "running", sequence: 6, result: { n: 1 } });

assert.deepEqual(observed.slice(0, 3), [
  ["scheduler", "state:s", "ready"],
  ["scheduler", "state:s", "running"],
  ["state", "state:s", 1],
]);
assert.deepEqual(journal.verify(), { ok: true, errors: [] });
assert.deepEqual(journal.replayState(), {
  values: { "state:s": { n: 1 } },
  revisions: { "state:s": 2 },
});

const ndjson = journal.toNDJSON();
const restored = SemanticJournal.fromNDJSON(ndjson);
assert.deepEqual(restored.snapshot(), journal.snapshot());
assert.deepEqual(restored.replayState(), journal.replayState());

const tamperedEntries = restored.snapshot();
tamperedEntries.find((entry) => entry.type === "state" && entry.revision === 2).revision = 4;
const tampered = new SemanticJournal(tamperedEntries);
assert.equal(tampered.verify().ok, false);
assert.throws(() => tampered.replayState(), /verification failed/);

const mismatchedEntries = restored.snapshot();
const completed = mismatchedEntries.find((entry) => entry.type === "scheduler" && entry.key === "patch:p" && entry.status === "completed");
completed.result = { n: 999 };
assert.match(new SemanticJournal(mismatchedEntries).verify().errors.join("\n"), /does not match recorded state change/);

const nonJson = new SemanticJournal();
nonJson.recordSchedulerTransition({ key: "tool:x", status: "completed", sequence: 1, result: 1n });
assert.equal(nonJson.snapshot()[0].resultOmitted, true);
assert.doesNotThrow(() => nonJson.toNDJSON());

assert.throws(() => SemanticJournal.fromNDJSON('{"seq":1}\n{bad}\n'), /line 2/);

const terminalJournal = new SemanticJournal();
const terminalObserved = [];
const terminalHooks = createSemanticJournalHooks(terminalJournal, {
  scheduler: "terminal",
  onTransition: (entry) => terminalObserved.push(entry.status),
});
for (const [sequence, status] of ["ready", "queued", "running", "completed"].entries()) {
  terminalHooks.onTransition({ key: "tool:t", status, sequence: sequence + 1 });
}
assert.deepEqual(terminalObserved, ["ready", "queued", "running", "completed"]);
assert.deepEqual(terminalJournal.snapshot().map((entry) => entry.status), ["completed"]);

const stateOnlyJournal = new SemanticJournal();
const stateOnlyHooks = createSemanticJournalHooks(stateOnlyJournal, { scheduler: "none" });
stateOnlyHooks.onTransition({ key: "state:compact", status: "completed", sequence: 1, result: { n: 1 } });
stateOnlyHooks.onStateChange({ key: "state:compact", revision: 1, type: "initialize", node: "state:compact", value: { n: 1 } });
assert.deepEqual(stateOnlyJournal.snapshot().map((entry) => entry.type), ["state"]);
assert.deepEqual(stateOnlyJournal.replayState(), { values: { "state:compact": { n: 1 } }, revisions: { "state:compact": 1 } });
assert.throws(() => createSemanticJournalHooks(new SemanticJournal(), { scheduler: "verbose" }), /journal mode/);

const deltaJournal = new SemanticJournal();
const deltaHooks = createSemanticJournalHooks(deltaJournal, { scheduler: "terminal", stateEncoding: "delta" });
deltaHooks.onStateChange({
  key: "state:delta",
  revision: 1,
  type: "initialize",
  node: "state:delta",
  value: { big: "x".repeat(1024), nested: { a: 1 }, count: 0 },
});
deltaHooks.onTransition({
  key: "state:delta",
  status: "completed",
  sequence: 1,
  result: { big: "x".repeat(1024), nested: { a: 1 }, count: 0 },
});
deltaHooks.onStateChange({
  key: "state:delta",
  revision: 2,
  type: "patch",
  node: "patch:delta",
  format: "merge",
  patch: { count: 1, nested: { a: null, b: 2 } },
  value: { big: "x".repeat(1024), nested: { b: 2 }, count: 1 },
});
deltaHooks.onTransition({
  key: "patch:delta",
  status: "completed",
  sequence: 2,
  result: { big: "x".repeat(1024), nested: { b: 2 }, count: 1 },
});
const deltaStateEntries = deltaJournal.snapshot().filter((entry) => entry.type === "state");
assert.equal(deltaStateEntries[0].encoding, "snapshot");
assert.equal(deltaStateEntries[1].encoding, "patch");
assert.equal(Object.prototype.hasOwnProperty.call(deltaStateEntries[1], "value"), false);
assert.deepEqual(deltaStateEntries[1].patch, { count: 1, nested: { a: null, b: 2 } });
assert.deepEqual(deltaJournal.verify(), { ok: true, errors: [] });
assert.deepEqual(deltaJournal.replayState(), {
  values: { "state:delta": { big: "x".repeat(1024), nested: { b: 2 }, count: 1 } },
  revisions: { "state:delta": 2 },
});
const tamperedDelta = new SemanticJournal(deltaJournal.snapshot());
tamperedDelta.entries.find((entry) => entry.encoding === "patch").patch.count = 9;
assert.match(tamperedDelta.verify().errors.join("\n"), /does not match recorded state change/);
assert.throws(() => createSemanticJournalHooks(new SemanticJournal(), { stateEncoding: "binary" }), /state journal encoding/);

console.log("semantic journal: ok");
