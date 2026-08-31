import { performance } from "node:perf_hooks";
import { SemanticScheduler } from "./semantic-scheduler.mjs";

const n = Number(process.env.N ?? 25000);
const repeats = Number(process.env.REPEATS ?? 7);
const scheduler = new SemanticScheduler({ runners: { tool: () => 1 }, concurrency: 1024 });
for (let i = 0; i < n; i += 1) {
  const key = `tool:t${i}`;
  scheduler.upsertNode({ key, kind: "tool", id: `t${i}`, block: i, closed: true, attributes: {} }, []);
  scheduler.accept({ type: "ready", key });
}
await scheduler.idle({ snapshot: false });
function median(a) { return [...a].sort((x,y)=>x-y)[Math.floor(a.length/2)]; }
const full=[]; const light=[];
for (let r=0;r<repeats;r++) {
  let t=performance.now(); await scheduler.idle(); full.push(performance.now()-t);
  t=performance.now(); await scheduler.idle({ snapshot:false }); light.push(performance.now()-t);
}
console.log(`records=${n} repeats=${repeats}`);
console.log(`idle snapshot      ${median(full).toFixed(3)} ms`);
console.log(`idle no-snapshot   ${median(light).toFixed(3)} ms`);
console.log(`speedup ${(median(full)/median(light)).toFixed(1)}x`);
