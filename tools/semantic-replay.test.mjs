import assert from "node:assert/strict";
import { SemanticJournal } from "./semantic-journal.mjs";
import { replaySemanticJournal, SemanticJournalVerificationError } from "./semantic-replay-core.mjs";

const journal = new SemanticJournal();
journal.recordStateChange({ key: "state:s", revision: 1, type: "initialize", node: "state:s", value: { n: 0 } });
journal.recordStateChange({ key: "state:s", revision: 2, type: "patch", node: "patch:p", value: { n: 1 } });

const replay = replaySemanticJournal(journal.toNDJSON(), { includeEntries: true });
assert.deepEqual(replay.verification, { ok: true, errors: [] });
assert.deepEqual(replay.state, {
  values: { "state:s": { n: 1 } },
  revisions: { "state:s": 2 },
});
assert.equal(replay.entries.length, 2);

const tampered = journal.snapshot();
tampered[1].revision = 4;
const tamperedText = new SemanticJournal(tampered).toNDJSON();
assert.throws(
  () => replaySemanticJournal(tamperedText),
  (error) => error instanceof SemanticJournalVerificationError && /expected revision 2/.test(error.message),
);
const forced = replaySemanticJournal(tamperedText, { verify: false });
assert.equal(forced.verification.ok, false);
assert.equal(forced.state.revisions["state:s"], 4);

assert.throws(() => replaySemanticJournal("{bad}\n"), /invalid semantic journal NDJSON/);

console.log("semantic replay core: ok");
