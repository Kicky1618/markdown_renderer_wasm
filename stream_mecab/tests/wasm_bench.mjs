import fs from "node:fs/promises";
import { performance } from "node:perf_hooks";
import { StreamMecab } from "../js/stream_mecab.js";

const bytes = await fs.readFile(new URL("../target/wasm32-unknown-unknown/release/stream_mecab.wasm", import.meta.url));
const mecab = await StreamMecab.instantiate(bytes);
const tsv = [
  "今日\t今日\tキョウ\t9\t180",
  "は\tは\tハ\t10\t100",
  "東京\t東京\tトウキョウ\t9\t180",
  "大学\t大学\tダイガク\t9\t180",
  "東京大学\t東京大学\tトウキョウダイガク\t9\t80",
  "の\tの\tノ\t10\t100",
  "学生\t学生\tガクセイ\t9\t180",
  "です\tです\tデス\t11\t120",
].join("\n") + "\n";
mecab.addTsv(tsv).setMaxUnknownChars(4).start();
const chunks = ["今日", "は", "東京", "大学", "の", "学生", "です", "。"];
for (let i = 0; i < 1000; i++) for (const chunk of chunks) mecab.append(chunk);
const rounds = 20_000;
const start = performance.now();
let pushed = 0;
for (let i = 0; i < rounds; i++) {
  for (const chunk of chunks) pushed += mecab.append(chunk).push.length;
}
const ms = performance.now() - start;
const appends = rounds * chunks.length;
console.log(`wasm+js: ${appends} appends in ${ms.toFixed(3)} ms (${(appends / ms / 1000).toFixed(3)} M append/s), pushed=${pushed}`);
mecab.destroy();
