import assert from "node:assert/strict";
import { SemanticScheduler } from "./semantic-scheduler.mjs";

function graph(nodes, edges, executionOrder = nodes.map((node) => node.key)) {
  return { nodes, edges, executionOrder, cycles: [], duplicates: [], malformed: [], unresolved: [] };
}

function node(key) {
  const colon = key.indexOf(":");
  return { key, kind: key.slice(0, colon), id: key.slice(colon + 1), block: 0, closed: true, attributes: {} };
}

{
  const calls = [];
  const g = graph(
    [node("tool:search"), node("artifact:summary"), node("ui:metric")],
    [
      { from: "artifact:summary", to: "tool:search", source: "depends" },
      { from: "ui:metric", to: "artifact:summary", source: "depends" },
    ],
    ["tool:search", "artifact:summary", "ui:metric"],
  );
  const scheduler = new SemanticScheduler({
    concurrency: 3,
    runners: {
      tool: async () => { calls.push("tool:search"); return { hits: 4 }; },
      artifact: async (_node, context) => {
        assert.deepEqual(context.dependencyResults["tool:search"], { hits: 4 });
        calls.push("artifact:summary");
        return { total: 4 };
      },
      ui: async (_node, context) => {
        assert.deepEqual(context.dependencyResults["artifact:summary"], { total: 4 });
        calls.push("ui:metric");
        return "rendered";
      },
    },
  });
  scheduler.updateGraph(g);
  // Deliberately arrive in reverse order. Dependency completion, not event
  // arrival order, controls execution.
  scheduler.accept({ type: "ready", key: "ui:metric" });
  scheduler.accept({ type: "ready", key: "artifact:summary" });
  scheduler.accept({ type: "ready", key: "tool:search" });
  const state = await scheduler.idle();
  assert.deepEqual(calls, ["tool:search", "artifact:summary", "ui:metric"]);
  assert.equal(state["tool:search"].status, "completed");
  assert.equal(state["artifact:summary"].status, "completed");
  assert.equal(state["ui:metric"].status, "completed");
}

{
  let running = 0;
  let maxRunning = 0;
  const releases = [];
  const scheduler = new SemanticScheduler({
    concurrency: 2,
    runners: {
      tool: () => new Promise((resolve) => {
        running += 1;
        maxRunning = Math.max(maxRunning, running);
        releases.push(() => { running -= 1; resolve(); });
      }),
    },
  });
  scheduler.updateGraph(graph(
    [node("tool:a"), node("tool:b"), node("tool:c")],
    [],
    ["tool:a", "tool:b", "tool:c"],
  ));
  for (const key of ["tool:a", "tool:b", "tool:c"]) scheduler.accept({ type: "ready", key });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(maxRunning, 2);
  assert.equal(scheduler.get("tool:c").status, "ready");
  releases.shift()();
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(scheduler.get("tool:c").status, "running");
  while (releases.length) releases.shift()();
  await scheduler.idle();
  assert.equal(scheduler.get("tool:c").status, "completed");
}

{
  const called = [];
  const scheduler = new SemanticScheduler({
    runners: {
      tool: async () => { throw new Error("search failed"); },
      artifact: async () => { called.push("artifact"); },
      ui: async () => { called.push("ui"); },
    },
  });
  scheduler.updateGraph(graph(
    [node("tool:search"), node("artifact:summary"), node("ui:metric")],
    [
      { from: "artifact:summary", to: "tool:search", source: "depends" },
      { from: "ui:metric", to: "artifact:summary", source: "depends" },
    ],
  ));
  for (const key of ["ui:metric", "artifact:summary", "tool:search"]) scheduler.accept({ type: "ready", key });
  await scheduler.idle();
  assert.equal(scheduler.get("tool:search").status, "failed");
  assert.equal(scheduler.get("artifact:summary").status, "blocked");
  assert.equal(scheduler.get("ui:metric").status, "blocked");
  assert.deepEqual(called, []);
}

{
  const scheduler = new SemanticScheduler({ runners: {} });
  scheduler.updateGraph(graph([node("tool:x"), node("artifact:y")], [
    { from: "artifact:y", to: "tool:x", source: "depends" },
  ]));
  scheduler.accept({ type: "ready", key: "artifact:y" });
  scheduler.accept({ type: "ready", key: "tool:x" });
  await scheduler.idle();
  assert.equal(scheduler.get("tool:x").status, "failed");
  assert.equal(scheduler.get("tool:x").error.name, "MissingRunnerError");
  assert.equal(scheduler.get("artifact:y").status, "blocked");
}

{
  const calls = [];
  const scheduler = new SemanticScheduler({
    runners: {
      tool: async () => { calls.push("tool:a"); return 1; },
      artifact: async (_node, context) => { calls.push("artifact:b"); return context.dependencyResults["tool:a"] + 1; },
      ui: async (_node, context) => { calls.push("ui:c"); return context.dependencyResults["artifact:b"] + 1; },
    },
  });
  scheduler.upsertNode(node("ui:c"), ["artifact:b"]);
  scheduler.upsertNode(node("artifact:b"), ["tool:a"]);
  scheduler.accept({ type: "ready", key: "ui:c" });
  scheduler.accept({ type: "ready", key: "artifact:b" });
  // The dependency may arrive after its already-ready dependent.
  scheduler.upsertNode(node("tool:a"), []);
  scheduler.accept({ type: "ready", key: "tool:a" });
  const state = await scheduler.idle();
  assert.deepEqual(calls, ["tool:a", "artifact:b", "ui:c"]);
  assert.equal(state["ui:c"].result, 3);
}

{
  const size = 2000;
  let completed = 0;
  const scheduler = new SemanticScheduler({
    concurrency: 64,
    runners: {
      tool: async () => 1,
      artifact: async () => { completed += 1; return 1; },
    },
  });
  scheduler.upsertNode(node("tool:root"), []);
  for (let i = 0; i < size; i += 1) {
    const key = `artifact:fan${i}`;
    scheduler.upsertNode(node(key), ["tool:root"]);
    scheduler.accept({ type: "ready", key });
  }
  scheduler.accept({ type: "ready", key: "tool:root" });
  await scheduler.idle();
  assert.equal(completed, size);
  assert.equal(scheduler.pending.length, 0);
  assert.equal(scheduler.pendingHead, 0);
}

console.log("semantic scheduler: ok");
