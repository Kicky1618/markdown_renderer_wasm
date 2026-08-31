import { performance } from "node:perf_hooks";
import { SemanticStateStore } from "./semantic-state.mjs";

const updates = Number(process.env.N ?? 500);
const repeats = Number(process.env.REPEATS ?? 5);
const payloadBytes = Number(process.env.STATE_BYTES ?? 65536);
const withCallback = process.env.CALLBACK !== "0";
const payload = "x".repeat(payloadBytes);

function median(values) {
  return [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)];
}

function stateNode() {
  return {
    kind: "state",
    key: "state:s",
    attributes: {},
    value: JSON.stringify({ payload, count: 0, nested: { a: 1, b: 2 } }),
  };
}

function patchNode(i) {
  return {
    kind: "patch",
    key: `patch:p${i}`,
    attributes: { target: "state:s" },
    value: JSON.stringify({ count: i }),
  };
}

const samples = [];
for (let repeat = 0; repeat < repeats; repeat += 1) {
  const store = new SemanticStateStore({ onChange: withCallback ? () => {} : null });
  store.initialize(stateNode());
  const start = performance.now();
  for (let i = 1; i <= updates; i += 1) store.patch(patchNode(i));
  samples.push(performance.now() - start);
  if (store.get("state:s").count !== updates) throw new Error("state benchmark result mismatch");
}

console.log(`updates=${updates} stateBytes=${payloadBytes} callback=${withCallback} median=${median(samples).toFixed(2)} ms`);
