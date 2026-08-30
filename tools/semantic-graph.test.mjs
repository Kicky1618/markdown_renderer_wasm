import assert from "node:assert/strict";
import { buildSemanticGraph, graphDiagnostics, graphToDot, parseDependencies } from "./semantic-graph.mjs";

assert.deepEqual(parseDependencies("tool:q1, artifact:a1"), [
  { raw: "tool:q1", parsed: { kind: "tool", id: "q1", key: "tool:q1" } },
  { raw: "artifact:a1", parsed: { kind: "artifact", id: "a1", key: "artifact:a1" } },
]);

const graph = buildSemanticGraph({
  llmBlocks: [
    { index: 0, kind: "tool", closed: true, attributes: { id: "q1" } },
    { index: 1, kind: "artifact", closed: true, attributes: { id: "a1", depends: "tool:q1" } },
    { index: 2, kind: "ui", closed: true, attributes: { id: "u1", depends: "artifact:a1" } },
  ],
  semanticReferences: [
    { block: 3, kind: "artifact", id: "a1", label: "@[artifact:a1]" },
  ],
});

assert.deepEqual(graph.edges, [
  { from: "artifact:a1", to: "tool:q1", source: "depends" },
  { from: "ui:u1", to: "artifact:a1", source: "depends" },
]);
assert.deepEqual(graph.cycles, []);
assert.deepEqual(graph.executionOrder, ["tool:q1", "artifact:a1", "ui:u1"]);
assert.equal(graphDiagnostics(graph).ok, true);
assert.match(graphToDot(graph), /"ui:u1" -> "artifact:a1"/);

const broken = buildSemanticGraph({
  llmBlocks: [
    { index: 0, kind: "artifact", closed: true, attributes: { id: "a", depends: "artifact:b" } },
    { index: 1, kind: "artifact", closed: true, attributes: { id: "b", depends: "artifact:a" } },
  ],
  semanticReferences: [
    { block: 2, kind: "ui", id: "missing", label: "@[ui:missing]" },
  ],
});
const diagnostics = graphDiagnostics(broken);
assert.equal(diagnostics.ok, false);
assert.equal(broken.cycles.length, 1);
assert.deepEqual(broken.executionOrder, []);
assert.ok(diagnostics.errors.some((message) => message.includes("cycle")));
assert.ok(diagnostics.warnings.some((message) => message.includes("ui:missing")));

console.log("semantic graph: ok");
