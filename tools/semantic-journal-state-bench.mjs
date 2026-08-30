#!/usr/bin/env node

import { createSemanticJournalHooks, SemanticJournal } from "./semantic-journal.mjs";

const updates = Number(process.env.N ?? 1000);
const stateBytes = Number(process.env.STATE_BYTES ?? 65536);
if (!Number.isSafeInteger(updates) || updates <= 0) throw new RangeError("N must be a positive integer");
if (!Number.isSafeInteger(stateBytes) || stateBytes <= 0) throw new RangeError("STATE_BYTES must be a positive integer");

function buildJournal(stateEncoding) {
  const journal = new SemanticJournal();
  const hooks = createSemanticJournalHooks(journal, {
    scheduler: "none",
    stateEncoding,
  });
  const blob = "x".repeat(stateBytes);
  hooks.onStateChange({
    key: "state:session",
    revision: 1,
    type: "initialize",
    node: "state:session",
    value: { blob, counter: 0 },
  });
  for (let i = 1; i <= updates; i += 1) {
    hooks.onStateChange({
      key: "state:session",
      revision: i + 1,
      type: "patch",
      node: `patch:p${i}`,
      format: "merge",
      patch: { counter: i },
      value: { blob, counter: i },
    });
  }
  const verification = journal.verify();
  if (!verification.ok) throw new Error(verification.errors.join("; "));
  return journal;
}

const snapshot = buildJournal("snapshot");
const delta = buildJournal("delta");
const snapshotBytes = Buffer.byteLength(snapshot.toNDJSON());
const deltaBytes = Buffer.byteLength(delta.toNDJSON());
const replayed = delta.replayState();
if (replayed.values["state:session"].counter !== updates) throw new Error("delta replay mismatch");

console.log(`state payload:   ${stateBytes} bytes`);
console.log(`patches:         ${updates}`);
console.log(`snapshot journal:${snapshotBytes.toString().padStart(12)} bytes`);
console.log(`delta journal:   ${deltaBytes.toString().padStart(12)} bytes`);
console.log(`reduction:       ${(100 * (1 - deltaBytes / snapshotBytes)).toFixed(2)}%`);
console.log(`delta/snapshot:  ${(deltaBytes / snapshotBytes).toFixed(4)}x`);
