import assert from "node:assert/strict";
import { createTimelineState, observeSemanticState } from "./semantic-timeline-core.mjs";
import { IncrementalSemanticTimeline } from "./semantic-timeline-incremental.mjs";

function block(index, kind, id, { closed = false, depends, value = "" } = {}) {
  return {
    index,
    kind,
    attributes: Object.assign(Object.create(null), { id }, depends ? { depends } : {}),
    value,
    closed,
  };
}

function ref(blockIndex, kind, id, label = `@[${kind}:${id}]`) {
  return { block: blockIndex, kind, id, label };
}

function cloneSummary(summary) {
  return {
    llmBlocks: summary.llmBlocks.map((entry) => ({
      ...entry,
      attributes: Object.assign(Object.create(null), entry.attributes),
    })),
    semanticReferences: summary.semanticReferences.map((entry) => ({ ...entry })),
  };
}

const sequences = [
  [
    { llmBlocks: [block(0, "tool", "search")], semanticReferences: [] },
    { llmBlocks: [block(0, "tool", "search", { closed: true, value: "{}" })], semanticReferences: [] },
    { llmBlocks: [block(0, "tool", "search", { closed: true, value: "{}" }), block(1, "artifact", "summary", { depends: "tool:search" })], semanticReferences: [] },
    { llmBlocks: [block(0, "tool", "search", { closed: true, value: "{}" }), block(1, "artifact", "summary", { closed: true, depends: "tool:search", value: "{}" })], semanticReferences: [ref(2, "artifact", "summary")] },
    { llmBlocks: [block(0, "tool", "search", { closed: true, value: "{}" }), block(1, "artifact", "summary", { closed: true, depends: "tool:search", value: "{}" }), block(3, "ui", "metric", { closed: true, depends: "artifact:summary" })], semanticReferences: [ref(2, "artifact", "summary"), ref(4, "ui", "metric")] },
  ],
  [
    { llmBlocks: [block(0, "artifact", "future", { closed: true, depends: "tool:later" })], semanticReferences: [] },
    { llmBlocks: [block(0, "artifact", "future", { closed: true, depends: "tool:later" }), block(1, "tool", "later", { closed: true })], semanticReferences: [] },
  ],
  [
    { llmBlocks: [block(0, "artifact", "bad", { closed: true, depends: "not-a-key" })], semanticReferences: [ref(1, "artifact", "bad")] },
  ],
];

for (const [sequenceIndex, sequence] of sequences.entries()) {
  const referenceState = createTimelineState();
  const incremental = new IncrementalSemanticTimeline();
  for (let step = 0; step < sequence.length; step += 1) {
    const summary = cloneSummary(sequence[step]);
    const byte = 100 + step * 13;
    const reference = observeSemanticState(summary, referenceState, byte, step);
    const actual = incremental.observe(summary, byte, step);
    assert.deepEqual(actual.events, reference.events, `event mismatch sequence=${sequenceIndex} step=${step}`);
  }
}

console.log("incremental semantic timeline equivalence: ok");
