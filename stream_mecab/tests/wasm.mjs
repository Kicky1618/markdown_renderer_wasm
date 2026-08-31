import fs from "node:fs/promises";
import assert from "node:assert/strict";
import { StreamMecab, applyDelta } from "../js/stream_mecab.js";

const wasmPath = new URL("../target/wasm32-unknown-unknown/release/stream_mecab.wasm", import.meta.url);
const bytes = await fs.readFile(wasmPath);
const mecab = await StreamMecab.instantiate(bytes);
const tsv = [
  "私\t私\tワタシ\t9\t400",
  "は\tは\tハ\t10\t300",
  "東京\t東京\tトウキョウ\t9\t250",
  "大学\t大学\tダイガク\t9\t250",
  "東京大学\t東京大学\tトウキョウダイガク\t9\t100",
  "学生\t学生\tガクセイ\t9\t300",
  "です\tです\tデス\t11\t250",
].join("\n") + "\n";

mecab.addTsv(tsv).setMaxUnknownChars(4).start();
const tokens = [];
for (const chunk of ["私", "は東", "京", "大学", "の学", "生で", "す"]) {
  applyDelta(tokens, mecab.append(chunk));
}
applyDelta(tokens, mecab.finish());
assert.deepEqual(tokens.map((token) => token.surface), ["私", "は", "東京大学", "の", "学生", "です"]);
assert.equal(tokens[2].reading, "トウキョウダイガク");
assert.equal(tokens[2].origin, "lexicon");
assert.equal(tokens[3].origin, "unknown");

assert.throws(() => mecab.addTsv("猫\t猫\tネコ\t9\t10\n"), /after sm_start/);
mecab.destroy();

const surfaceOnly = await StreamMecab.instantiate(bytes);
surfaceOnly.addTsv(tsv).setMaxUnknownChars(4).start();
const surfaces = [];
for (const chunk of ["私", "は東", "京", "大学", "の学", "生で", "す"]) {
  applyDelta(surfaces, surfaceOnly.appendSurfaces(chunk));
}
applyDelta(surfaces, surfaceOnly.finish());
assert.deepEqual(surfaces, ["私", "は", "東京大学", "の", "学生", "です"]);
surfaceOnly.destroy();

const transitionModel = await StreamMecab.instantiate(bytes);
transitionModel
  .addTsv([
    "東京\t東京-A\t\t9\t0",
    "東京\t東京-B\t\t10\t0",
  ].join("\n") + "\n")
  .addTransitionTsv("0\t9\t100\n0\t10\t-100\n")
  .start();
const transitionTokens = [];
applyDelta(transitionTokens, transitionModel.append("東京"));
applyDelta(transitionTokens, transitionModel.finish());
assert.equal(transitionTokens.length, 1);
assert.equal(transitionTokens[0].tag, 10);
transitionModel.destroy();

console.log("stream-mecab raw WASM round trip: ok");
