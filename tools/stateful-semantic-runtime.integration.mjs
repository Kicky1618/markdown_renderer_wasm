import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
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

function* chunks(text, width) {
  const bytes = new TextEncoder().encode(text);
  for (let offset = 0; offset < bytes.length; offset += width) {
    yield bytes.subarray(offset, Math.min(offset + width, bytes.length));
  }
}

for (const width of [1, 5, 23]) {
  const stateChanges = [];
  let artifactDependency = null;
  const runtime = await StatefulSemanticRuntime.load(wasm, {
    onStateChange(change) {
      stateChanges.push({ key: change.key, revision: change.revision, value: change.value });
    },
    runners: {
      artifact: async (node, { dependencyResults }) => {
        artifactDependency = dependencyResults["patch:ready"];
        return { config: JSON.parse(node.value), state: artifactDependency };
      },
    },
  });

  try {
    const result = await runtime.consume(chunks(markdown, width));
    assert.deepEqual(result.state.values, {
      "state:session": { count: 1, status: "ready" },
    });
    assert.deepEqual(result.state.revisions, { "state:session": 2 });
    assert.deepEqual(artifactDependency, { count: 1, status: "ready" });
    assert.deepEqual(result.scheduler["artifact:view"].result, {
      config: { name: "status-card" },
      state: { count: 1, status: "ready" },
    });
    assert.deepEqual(stateChanges.map(({ revision }) => revision), [1, 2]);

    const values = result.state.values;
    values["state:session"].count = 999;
    assert.equal(runtime.stateStore.get("state:session").count, 1, "stateful snapshot must not alias canonical state");
  } finally {
    runtime.dispose();
  }
}

await assert.rejects(
  StatefulSemanticRuntime.load(wasm, { stateStore: {}, onStateChange() {} }),
  /stateStore must be a SemanticStateStore|onStateChange cannot be used/,
);

console.log("stateful semantic runtime WASM integration: ok");
