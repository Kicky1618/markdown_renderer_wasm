import assert from "node:assert/strict";
import { applyJsonMergePatch, SemanticStateStore } from "./semantic-state.mjs";

// Public RFC 7396 helper stays fully detached from both inputs.
const target = { keep: { deep: { x: 1 } }, array: [1, 2], count: 0 };
const patch = { count: 1, add: { z: 2 } };
const publicResult = applyJsonMergePatch(target, patch);
publicResult.keep.deep.x = 9;
publicResult.add.z = 8;
assert.equal(target.keep.deep.x, 1);
assert.equal(patch.add.z, 2);

let lastChange = null;
const store = new SemanticStateStore({
  onChange(change) {
    lastChange = change;
    change.value.keep.deep.x = 77;
    if (change.patch?.add) change.patch.add.z = 66;
  },
});
store.initialize({
  kind: "state",
  key: "state:s",
  attributes: {},
  value: JSON.stringify(target),
});
const result = store.patch({
  kind: "patch",
  key: "patch:p",
  attributes: { target: "state:s" },
  value: JSON.stringify(patch),
});

// The internal merge may share untouched canonical branches, but no public
// surface is allowed to expose those references.
result.keep.deep.x = 55;
result.add.z = 44;
assert.equal(store.get("state:s").keep.deep.x, 1);
assert.equal(store.get("state:s").add.z, 2);
assert.equal(lastChange.patch.add.z, 66);
assert.equal(store.get("state:s").add.z, 2, "callback patch mutation must remain detached");

console.log("semantic state copy-on-write merge: ok");
