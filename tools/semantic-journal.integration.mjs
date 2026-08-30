import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createSemanticJournalHooks, SemanticJournal } from "./semantic-journal.mjs";
import { StatefulSemanticRuntime } from "./stateful-semantic-runtime.mjs";

const wasmPath = process.argv[2] ?? "target/wasm32-unknown-unknown/release/streamdown.wasm";
const wasm = await readFile(wasmPath);
const markdown = [
  ":::llm state id=session\n",
  '{"count":0,"status":"warming"}\n',
  ":::\n",
  ":::llm patch id=ready target=state:session depends=state:session if_revision=1\n",
  '{"count":1,"status":"ready"}\n',
  ":::\n",
  ":::llm artifact id=view depends=patch:ready\n",
  '{"name":"status-card"}\n',
  ":::\n",
].join("");

function* byteChunks(text, width) {
  const bytes = new TextEncoder().encode(text);
  for (let offset = 0; offset < bytes.length; offset += width) {
    yield bytes.subarray(offset, Math.min(offset + width, bytes.length));
  }
}

for (const width of [1, 7, 31]) {
  const journal = new SemanticJournal();
  const hooks = createSemanticJournalHooks(journal);
  const runtime = await StatefulSemanticRuntime.load(wasm, {
    onStateChange: hooks.onStateChange,
    onTransition: hooks.onTransition,
    runners: {
      artifact: async (node, { dependencyResults }) => ({
        config: JSON.parse(node.value),
        state: dependencyResults["patch:ready"],
      }),
    },
  });

  try {
    const result = await runtime.consume(byteChunks(markdown, width));
    assert.deepEqual(journal.verify(), { ok: true, errors: [] }, `journal verify width=${width}`);
    assert.deepEqual(journal.replayState(), result.state, `state replay width=${width}`);

    const restored = SemanticJournal.fromNDJSON(journal.toNDJSON());
    assert.deepEqual(restored.verify(), { ok: true, errors: [] }, `NDJSON verify width=${width}`);
    assert.deepEqual(restored.replayState(), result.state, `NDJSON replay width=${width}`);

    const schedulerEntries = restored.snapshot().filter((entry) => entry.type === "scheduler");
    assert.ok(schedulerEntries.some((entry) => entry.key === "state:session" && entry.status === "completed"));
    assert.ok(schedulerEntries.some((entry) => entry.key === "patch:ready" && entry.status === "completed"));
    assert.ok(schedulerEntries.some((entry) => entry.key === "artifact:view" && entry.status === "completed"));
  } finally {
    runtime.dispose();
  }
}

console.log("semantic journal WASM integration: ok");
