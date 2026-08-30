import { performance } from "node:perf_hooks";
import { SemanticScheduler } from "./semantic-scheduler.mjs";

const sizes = (process.env.SIZES ?? "1000,5000,10000,20000,50000")
  .split(",")
  .map(Number)
  .filter((value) => Number.isSafeInteger(value) && value > 0);
const concurrency = Number(process.env.CONCURRENCY ?? 128);

function node(key) {
  const colon = key.indexOf(":");
  return {
    key,
    kind: key.slice(0, colon),
    id: key.slice(colon + 1),
    block: 0,
    closed: true,
    attributes: {},
  };
}

for (const size of sizes) {
  const scheduler = new SemanticScheduler({
    concurrency,
    runners: {
      tool: async () => 1,
      artifact: async () => 1,
    },
  });
  scheduler.upsertNode(node("tool:root"), []);
  for (let i = 0; i < size; i += 1) {
    const key = `artifact:a${i}`;
    scheduler.upsertNode(node(key), ["tool:root"]);
    scheduler.accept({ type: "ready", key });
  }

  const start = performance.now();
  scheduler.accept({ type: "ready", key: "tool:root" });
  await scheduler.idle();
  const elapsed = performance.now() - start;
  console.log(`${size}\t${elapsed.toFixed(2)}ms\t${(elapsed / size * 1000).toFixed(3)}us/job`);
}
