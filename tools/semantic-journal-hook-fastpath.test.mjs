import assert from "node:assert/strict";
import { createSemanticJournalHooks, SemanticJournal } from "./semantic-journal.mjs";

{
  const journal = new SemanticJournal();
  const returned = journal.recordStateChange({
    key: "state:s",
    revision: 1,
    type: "initialize",
    node: "state:s",
    value: { nested: { n: 1 } },
  });
  returned.value.nested.n = 99;
  assert.equal(journal.entries[0].value.nested.n, 1, "default return must remain detached");

  const skipped = journal.recordStateChange({
    key: "state:t",
    revision: 1,
    type: "initialize",
    node: "state:t",
    value: { n: 2 },
  }, { returnEntry: false });
  assert.equal(skipped, undefined);
  assert.equal(journal.entries[1].value.n, 2);
  assert.throws(() => journal.recordStateChange({
    key: "state:u", revision: 1, value: {},
  }, { returnEntry: "no" }), /returnEntry/);
}

{
  const journal = new SemanticJournal();
  const returned = journal.recordSchedulerTransition({
    key: "tool:x",
    status: "completed",
    previousStatus: "running",
    sequence: 1,
    result: { nested: { n: 1 } },
  });
  returned.result.nested.n = 99;
  assert.equal(journal.entries[0].result.nested.n, 1, "default scheduler return must remain detached");

  const skipped = journal.recordSchedulerTransition({
    key: "tool:y",
    status: "completed",
    previousStatus: "running",
    sequence: 2,
    result: { n: 2 },
  }, { returnEntry: false });
  assert.equal(skipped, undefined);
  assert.equal(journal.entries[1].result.n, 2);
}

{
  const journal = new SemanticJournal();
  const hooks = createSemanticJournalHooks(journal, { scheduler: "terminal" });
  assert.equal(hooks.onStateChange({
    key: "state:hook",
    revision: 1,
    type: "initialize",
    node: "state:hook",
    value: { ok: true },
  }), undefined);
  assert.equal(hooks.onTransition({
    key: "tool:hook",
    status: "completed",
    previousStatus: "running",
    sequence: 1,
    result: { ok: true },
  }), undefined);
  assert.equal(journal.entries.length, 2);
}

console.log("semantic journal hook fast path: ok");
