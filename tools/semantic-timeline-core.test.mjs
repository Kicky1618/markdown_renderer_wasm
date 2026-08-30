import assert from "node:assert/strict";
import { createTimelineState, observeSemanticState } from "./semantic-timeline-core.mjs";

const block = (index, kind, id, closed, depends) => ({
  index,
  kind,
  attributes: Object.fromEntries([
    ["id", id],
    ...(depends ? [["depends", depends]] : []),
  ]),
  value: "{}\n",
  closed,
});

const state = createTimelineState();
let result = observeSemanticState({
  llmBlocks: [block(0, "tool", "search", false)],
  semanticReferences: [],
}, state, 20, 0);
assert.deepEqual(result.events.map((event) => [event.type, event.key]), [
  ["open", "tool:search"],
]);

result = observeSemanticState({
  llmBlocks: [block(0, "tool", "search", true)],
  semanticReferences: [],
}, state, 40, 1);
assert.deepEqual(result.events.map((event) => [event.type, event.key]), [
  ["close", "tool:search"],
  ["ready", "tool:search"],
]);

result = observeSemanticState({
  llmBlocks: [
    block(0, "tool", "search", true),
    block(1, "artifact", "summary", true, "tool:search"),
    block(2, "ui", "metric", true, "artifact:summary"),
  ],
  semanticReferences: [
    { block: 3, kind: "artifact", id: "summary", label: "@[artifact:summary]" },
  ],
}, state, 100, 2);
assert.deepEqual(result.events.map((event) => [event.type, event.key]), [
  ["open", "artifact:summary"],
  ["close", "artifact:summary"],
  ["open", "ui:metric"],
  ["close", "ui:metric"],
  ["reference", "artifact:summary"],
  ["ready", "artifact:summary"],
  ["ready", "ui:metric"],
]);

result = observeSemanticState({
  llmBlocks: [
    block(0, "tool", "search", true),
    block(1, "artifact", "summary", true, "tool:search"),
    block(2, "ui", "metric", true, "artifact:summary"),
  ],
  semanticReferences: [
    { block: 3, kind: "artifact", id: "summary", label: "@[artifact:summary]" },
  ],
}, state, 120, 3);
assert.deepEqual(result.events, []);

const waiting = createTimelineState();
result = observeSemanticState({
  llmBlocks: [block(0, "artifact", "late", true, "tool:missing")],
  semanticReferences: [],
}, waiting, 10, 0);
assert.deepEqual(result.events.map((event) => event.type), ["open", "close"]);
assert.equal(waiting.readyNodes.has("artifact:late"), false);

const malformed = createTimelineState();
result = observeSemanticState({
  llmBlocks: [block(0, "artifact", "bad", true, "not-a-dependency")],
  semanticReferences: [],
}, malformed, 12, 0);
assert.deepEqual(result.events.map((event) => event.type), ["open", "close"]);
assert.equal(malformed.readyNodes.has("artifact:bad"), false);

console.log("semantic timeline core: ok");
