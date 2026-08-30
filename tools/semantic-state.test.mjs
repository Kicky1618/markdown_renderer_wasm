import assert from "node:assert/strict";
import { applyJsonMergePatch, createStateRunners, SemanticStateStore, validateSemanticStateProtocol } from "./semantic-state.mjs";
import { buildSemanticGraph } from "./semantic-graph.mjs";

assert.deepEqual(
  applyJsonMergePatch(
    { count: 0, nested: { keep: true, remove: 1 }, list: [1, 2] },
    { count: 1, nested: { remove: null, add: "x" }, list: [3] },
  ),
  { count: 1, nested: { keep: true, add: "x" }, list: [3] },
);
assert.deepEqual(applyJsonMergePatch({ old: true }, [1, 2]), [1, 2]);
assert.equal(applyJsonMergePatch({ old: true }, 7), 7);

const changes = [];
const store = new SemanticStateStore({ onChange: (change) => changes.push(change) });
const runners = createStateRunners(store);

const initial = runners.state({
  key: "state:session",
  kind: "state",
  attributes: { id: "session" },
  value: '{"count":0,"status":"warming","nested":{"a":1}}',
});
assert.deepEqual(initial, { count: 0, status: "warming", nested: { a: 1 } });
assert.equal(store.revision("state:session"), 1);

initial.count = 999;
assert.equal(store.get("state:session").count, 0, "runner result must not alias canonical state");

const patched = runners.patch({
  key: "patch:step1",
  kind: "patch",
  attributes: { id: "step1", target: "state:session" },
  value: '{"count":1,"status":"ready","nested":{"b":2}}',
});
assert.deepEqual(patched, { count: 1, status: "ready", nested: { a: 1, b: 2 } });
assert.equal(store.revision("state:session"), 2);

const cleaned = runners.patch({
  key: "patch:step2",
  kind: "patch",
  attributes: { id: "step2", target: "state:session", format: "merge" },
  value: '{"nested":{"a":null},"extra":true}',
});
assert.deepEqual(cleaned, { count: 1, status: "ready", nested: { b: 2 }, extra: true });
assert.equal(store.revision("state:session"), 3);
assert.deepEqual(store.snapshot(), { "state:session": cleaned });
assert.deepEqual(changes.map(({ type, revision }) => [type, revision]), [
  ["initialize", 1],
  ["patch", 2],
  ["patch", 3],
]);

assert.throws(() => runners.state({
  key: "state:session",
  kind: "state",
  attributes: { id: "session" },
  value: "{}",
}), /already initialized/);
assert.throws(() => runners.patch({
  key: "patch:missing",
  kind: "patch",
  attributes: { target: "state:nope" },
  value: "{}",
}), /not initialized/);
assert.throws(() => runners.patch({
  key: "patch:bad-target",
  kind: "patch",
  attributes: { target: "artifact:nope" },
  value: "{}",
}), /target must be state/);
assert.throws(() => runners.patch({
  key: "patch:bad-json",
  kind: "patch",
  attributes: { target: "state:session" },
  value: "{",
}), /invalid JSON/);
assert.throws(() => applyJsonMergePatch({}, JSON.parse('{"__proto__":{"polluted":true}}')), /forbidden JSON key/);
assert.equal({}.polluted, undefined);


function summaryOf(blocks) {
  return { llmBlocks: blocks, semanticReferences: [] };
}

const validSummary = summaryOf([
  { index: 0, kind: "state", attributes: { id: "s" }, value: '{"count":0}', closed: true },
  { index: 1, kind: "patch", attributes: { id: "p1", target: "state:s", depends: "state:s" }, value: '{"count":1}', closed: true },
  { index: 2, kind: "patch", attributes: { id: "p2", target: "state:s", depends: "patch:p1" }, value: '{"ready":true}', closed: true },
]);
const validGraph = buildSemanticGraph(validSummary);
assert.deepEqual(validateSemanticStateProtocol(validSummary, validGraph), { ok: true, errors: [], warnings: [] });

const missingPatchSummary = summaryOf([
  { index: 0, kind: "state", attributes: { id: "s" }, value: "{}", closed: true },
  { index: 1, kind: "patch", attributes: { id: "p", target: "state:s", depends: "patch:nope" }, value: "{}", closed: true },
]);
assert.ok(buildSemanticGraph(missingPatchSummary).unresolved.some((edge) => edge.to === "patch:nope"));

const parallelSummary = summaryOf([
  { index: 0, kind: "state", attributes: { id: "s" }, value: '{}', closed: true },
  { index: 1, kind: "patch", attributes: { id: "a", target: "state:s", depends: "state:s" }, value: '{"a":1}', closed: true },
  { index: 2, kind: "patch", attributes: { id: "b", target: "state:s", depends: "state:s" }, value: '{"b":1}', closed: true },
]);
const parallelValidation = validateSemanticStateProtocol(parallelSummary, buildSemanticGraph(parallelSummary));
assert.equal(parallelValidation.ok, true);
assert.match(parallelValidation.warnings.join("\n"), /concurrent execution may reorder updates/);

const invalidSummary = summaryOf([
  { index: 0, kind: "state", attributes: { id: "s" }, value: '{"__proto__":{"x":1}}', closed: true },
  { index: 1, kind: "patch", attributes: { id: "missing", target: "state:nope", depends: "state:s", format: "mystery" }, value: '{}', closed: true },
  { index: 2, kind: "patch", attributes: { id: "unordered", target: "state:s" }, value: '{}', closed: true },
]);
const invalidValidation = validateSemanticStateProtocol(invalidSummary, buildSemanticGraph(invalidSummary));
assert.equal(invalidValidation.ok, false);
assert.match(invalidValidation.errors.join("\n"), /forbidden JSON key/);
assert.match(invalidValidation.errors.join("\n"), /no local state initializer/);
assert.match(invalidValidation.errors.join("\n"), /unsupported patch format/);
assert.match(invalidValidation.errors.join("\n"), /no dependency path reaches state:s/);

console.log("semantic state: ok");
