import { performance } from "node:perf_hooks";
import { SemanticScheduler } from "./semantic-scheduler.mjs";
import { createStateRunners, SemanticStateStore } from "./semantic-state.mjs";

const keys = Number(process.env.KEYS ?? 4096);
const updates = Number(process.env.N ?? 500);
const repeats = Number(process.env.REPEATS ?? 5);
const initial = Object.fromEntries(Array.from({ length: keys }, (_, i) => [`k${i}`, i]));
const stateNode = { key: "state:s", kind: "state", id: "s", block: 0, closed: true, attributes: { id: "s" }, value: JSON.stringify(initial) };
const patchNodes = Array.from({ length: updates }, (_, i) => ({
  key: `patch:p${i}`, kind: "patch", id: `p${i}`, block: i + 1, closed: true,
  attributes: { id: `p${i}`, target: "state:s" }, value: JSON.stringify({ [`k${i % keys}`]: i + 100000 }),
}));

function median(a) { return [...a].sort((x,y)=>x-y)[Math.floor(a.length/2)]; }

async function run(lazyResults) {
  const store = new SemanticStateStore();
  const runners = createStateRunners(store, { lazyResults });
  const scheduler = new SemanticScheduler({ runners, concurrency: 1 });
  scheduler.upsertNode(stateNode, []);
  let previous = "state:s";
  for (const node of patchNodes) {
    scheduler.upsertNode(node, [previous]);
    previous = node.key;
  }
  const start = performance.now();
  scheduler.accept({ type: "ready", key: "state:s" });
  for (const node of patchNodes) scheduler.accept({ type: "ready", key: node.key });
  await scheduler.idle({ snapshot: false });
  const elapsed = performance.now() - start;
  if (store.revision("state:s") !== updates + 1) throw new Error("bad final revision");
  return elapsed;
}

await run(true); await run(false);
const eager=[]; const lazy=[];
for (let r=0;r<repeats;r++) { eager.push(await run(false)); lazy.push(await run(true)); }
const e=median(eager), l=median(lazy);
console.log(`keys=${keys} updates=${updates} repeats=${repeats}`);
console.log(`eager-result ${e.toFixed(2)} ms`);
console.log(`lazy-result  ${l.toFixed(2)} ms`);
console.log(`speedup ${(e/l).toFixed(2)}x`);
