import assert from "node:assert/strict";
import { createLazySemanticResult } from "./semantic-lazy-result.mjs";
import { SemanticScheduler } from "./semantic-scheduler.mjs";
import { createStateRunners, SemanticStateStore } from "./semantic-state.mjs";

{
  let calls = 0;
  const lazy = createLazySemanticResult(() => { calls += 1; return { n: 1 }; });
  assert.equal(lazy.materialized, false);
  assert.deepEqual(lazy.materialize(), { n: 1 });
  assert.equal(lazy.materialized, true);
  lazy.materialize();
  assert.equal(calls, 1);
}

{
  // Default state-runner API remains eager and backward compatible.
  const eagerStore = new SemanticStateStore();
  const eager = createStateRunners(eagerStore).state({
    key: "state:eager", kind: "state", attributes: { id: "eager" }, value: '{"n":1}',
  });
  assert.deepEqual(eager, { n: 1 });

  // Lazy results capture the revision object, not whatever state exists later.
  const store = new SemanticStateStore();
  const runners = createStateRunners(store, { lazyResults: true });
  const initial = runners.state({
    key: "state:s", kind: "state", attributes: { id: "s" }, value: '{"n":1}',
  });
  assert.equal(initial.materialized, false);
  const patched = runners.patch({
    key: "patch:p", kind: "patch", attributes: { id: "p", target: "state:s" }, value: '{"n":2}',
  });
  assert.deepEqual(initial.materialize(), { n: 1 });
  assert.deepEqual(patched.materialize(), { n: 2 });
  assert.deepEqual(store.get("state:s"), { n: 2 });
}

{
  let materializations = 0;
  const calls = [];
  const scheduler = new SemanticScheduler({
    runners: {
      state: () => createLazySemanticResult(() => { materializations += 1; return { big: "x" }; }),
      patch: async (_node, context) => {
        // Ordering dependency exists, but the patch runner deliberately does
        // not consume the previous full-state result.
        calls.push(Object.keys(context.dependencyResults));
        return 2;
      },
    },
  });
  scheduler.upsertNode({ key: "state:s", kind: "state", id: "s", block: 0, closed: true, attributes: {} }, []);
  scheduler.upsertNode({ key: "patch:p", kind: "patch", id: "p", block: 1, closed: true, attributes: {} }, ["state:s"]);
  scheduler.accept({ type: "ready", key: "state:s" });
  scheduler.accept({ type: "ready", key: "patch:p" });
  await scheduler.idle({ snapshot: false });
  assert.deepEqual(calls, [["state:s"]]);
  assert.equal(materializations, 0, "enumerating dependency keys must not materialize results");
  assert.deepEqual(scheduler.getResult("state:s"), { big: "x" });
  assert.equal(materializations, 1);
  scheduler.getResult("state:s");
  assert.equal(materializations, 1, "materialized result must be cached");
}

{
  let materializations = 0;
  let observed;
  const scheduler = new SemanticScheduler({
    runners: { tool: () => createLazySemanticResult(() => { materializations += 1; return 7; }) },
    onTransition: (transition) => {
      if (transition.status === "completed") observed = transition;
    },
  });
  scheduler.upsertNode({ key: "tool:x", kind: "tool", id: "x", block: 0, closed: true, attributes: {} }, []);
  scheduler.accept({ type: "ready", key: "tool:x" });
  await scheduler.idle({ snapshot: false });
  assert.equal(materializations, 0, "completed callback that ignores result must stay lazy");
  assert.equal(observed.result, 7);
  assert.equal(materializations, 1);
}

console.log("semantic lazy results: ok");
