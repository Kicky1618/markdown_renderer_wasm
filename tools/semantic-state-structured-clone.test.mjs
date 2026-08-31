import assert from "node:assert/strict";
import { SemanticStateStore } from "./semantic-state.mjs";

const callbackValues = [];
const store = new SemanticStateStore({
  onChange(change) {
    callbackValues.push(change);
  },
});

const initialized = store.initialize({
  kind: "state",
  key: "state:s",
  attributes: {},
  value: JSON.stringify({
    text: "x".repeat(4096),
    nested: { count: 1 },
    list: [1, 2, 3],
  }),
});

initialized.nested.count = 99;
initialized.list.push(4);
assert.equal(store.get("state:s").nested.count, 1);
assert.deepEqual(store.get("state:s").list, [1, 2, 3]);

callbackValues[0].value.nested.count = 88;
assert.equal(store.get("state:s").nested.count, 1);

const patched = store.patch({
  kind: "patch",
  key: "patch:p",
  attributes: { target: "state:s", format: "merge" },
  value: JSON.stringify({ nested: { count: 2 }, extra: true }),
});
patched.nested.count = 77;
assert.equal(store.get("state:s").nested.count, 2);

const patchChange = callbackValues.at(-1);
patchChange.value.nested.count = 66;
patchChange.patch.nested.count = 55;
assert.equal(store.get("state:s").nested.count, 2);

const snapshot = store.snapshot();
snapshot["state:s"].nested.count = 44;
assert.equal(store.get("state:s").nested.count, 2);

assert.equal(store.revision("state:s"), 2);
console.log("semantic state structured clone isolation: ok");
