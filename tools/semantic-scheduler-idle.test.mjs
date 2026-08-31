import assert from "node:assert/strict";
import { SemanticScheduler } from "./semantic-scheduler.mjs";

{
  const scheduler = new SemanticScheduler();
  assert.deepEqual(await scheduler.idle(), {});
  assert.equal(await scheduler.idle({ snapshot: false }), undefined);
  await assert.rejects(async () => scheduler.idle({ snapshot: "no" }), /snapshot must be a boolean/);
}

{
  let release;
  const scheduler = new SemanticScheduler({
    runners: { tool: () => new Promise((resolve) => { release = resolve; }) },
  });
  scheduler.upsertNode({ key: "tool:x", kind: "tool", id: "x", block: 0, closed: true, attributes: {} }, []);
  scheduler.accept({ type: "ready", key: "tool:x" });
  await new Promise((resolve) => setImmediate(resolve));
  const light = scheduler.idle({ snapshot: false });
  const full = scheduler.idle();
  release(7);
  assert.equal(await light, undefined);
  const snapshot = await full;
  assert.equal(snapshot["tool:x"].status, "completed");
  assert.equal(snapshot["tool:x"].result, 7);
}

console.log("semantic scheduler selective idle snapshot: ok");
