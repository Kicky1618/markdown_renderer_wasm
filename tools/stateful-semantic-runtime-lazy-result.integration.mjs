import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { StatefulSemanticRuntime } from "./stateful-semantic-runtime.mjs";

const wasmPath = process.argv[2] ?? "target/wasm32-unknown-unknown/release/streamdown.wasm";
const wasm = await readFile(wasmPath);
const LIGHT = { document: false, graph: false, diagnostics: false, scheduler: false, state: false };

{
  const runtime = await StatefulSemanticRuntime.load(wasm);
  try {
    const source = [
      ":::llm state id=s\n", '{"n":0,"keep":"x"}\n', ":::\n",
      ":::llm patch id=p1 target=state:s depends=state:s\n", '{"n":1}\n', ":::\n",
      ":::llm patch id=p2 target=state:s depends=patch:p1\n", '{"n":2}\n', ":::\n",
    ];
    const result = await runtime.consume(source, { snapshotOptions: LIGHT });
    assert.equal("state" in result, false);
    assert.equal("scheduler" in result, false);
    assert.deepEqual(runtime.stateStore.get("state:s"), { n: 2, keep: "x" });
    assert.equal(runtime.scheduler.resultSources.size, 3, "ordering-only dependencies must keep all full-state results lazy");
    assert.deepEqual(runtime.scheduler.getResult("patch:p1"), { n: 1, keep: "x" });
    assert.equal(runtime.scheduler.resultSources.size, 2);
  } finally {
    runtime.dispose();
  }
}

{
  let artifactResult;
  const runtime = await StatefulSemanticRuntime.load(wasm, {
    runners: {
      artifact: async (_node, { dependencyResults }) => {
        artifactResult = dependencyResults["patch:p1"];
        return { observed: artifactResult.n };
      },
    },
  });
  try {
    const source = [
      ":::llm state id=s\n", '{"n":0}\n', ":::\n",
      ":::llm patch id=p1 target=state:s depends=state:s\n", '{"n":7}\n', ":::\n",
      ":::llm artifact id=a depends=patch:p1\n", '{}\n', ":::\n",
    ];
    await runtime.consume(source, { snapshotOptions: LIGHT });
    assert.deepEqual(artifactResult, { n: 7 });
    assert.equal(runtime.scheduler.getResult("artifact:a").observed, 7);
  } finally {
    runtime.dispose();
  }
}

console.log("stateful semantic runtime lazy results: ok");
