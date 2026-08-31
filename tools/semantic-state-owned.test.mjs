import assert from "node:assert/strict";
import { SemanticStateStore } from "./semantic-state.mjs";

let lastChange = null;
const store = new SemanticStateStore({
  onChange(change) {
    lastChange = change;
    change.value.nested.x = 99;
    if (change.patch?.nested) change.patch.nested.x = 88;
  },
});

const initialized = store.initialize({
  kind: "state",
  key: "state:s",
  attributes: {},
  value: '{"nested":{"x":1},"count":0}',
});
initialized.nested.x = 77;
assert.equal(store.get("state:s").nested.x, 1, "runner result must not alias canonical state");

const replaced = store.patch({
  kind: "patch",
  key: "patch:replace",
  attributes: { target: "state:s", format: "replace" },
  value: '{"nested":{"x":2},"count":1}',
});
replaced.nested.x = 66;
assert.equal(store.get("state:s").nested.x, 2, "replace result must remain detached");
assert.equal(lastChange.patch.nested.x, 88, "callback receives its own patch payload");
assert.equal(store.get("state:s").nested.x, 2, "callback patch mutation must not alias canonical replace state");

store.patch({
  kind: "patch",
  key: "patch:merge",
  attributes: { target: "state:s" },
  value: '{"nested":{"x":3},"extra":true}',
});
assert.deepEqual(store.get("state:s"), {
  nested: { x: 3 },
  count: 1,
  extra: true,
});

const snapshot = store.snapshot();
snapshot["state:s"].nested.x = 123;
assert.equal(store.get("state:s").nested.x, 3, "snapshot must remain detached");

console.log("semantic state owned canonical storage: ok");
