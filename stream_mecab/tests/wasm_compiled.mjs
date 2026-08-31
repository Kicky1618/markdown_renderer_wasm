import fs from "node:fs/promises";
import assert from "node:assert/strict";
import { StreamMecab, applyDelta } from "../js/stream_mecab.js";

const [wasm, dictionary] = await Promise.all([
  fs.readFile(new URL("../target/wasm32-unknown-unknown/release/stream_mecab.wasm", import.meta.url)),
  fs.readFile(new URL("../target/demo.smd1", import.meta.url)),
]);
const mecab = await StreamMecab.instantiate(wasm);
mecab.loadCompiled(dictionary).setMaxUnknownChars(4).start();
const tokens = [];
for (const chunk of ["私は", "東京", "大学", "の", "学生", "です"]) {
  applyDelta(tokens, mecab.append(chunk));
}
applyDelta(tokens, mecab.finish());
assert.deepEqual(tokens.map((token) => token.surface), ["私", "は", "東京大学", "の", "学生", "です"]);
mecab.destroy();

const mutableAfterLoad = await StreamMecab.instantiate(wasm);
mutableAfterLoad
  .loadCompiled(dictionary)
  .addTsv("猫\t猫\tネコ\t9\t10\n")
  .setMaxUnknownChars(4)
  .start();
const extended = [];
applyDelta(extended, mutableAfterLoad.append("猫"));
applyDelta(extended, mutableAfterLoad.finish());
assert.deepEqual(extended.map((token) => token.surface), ["猫"]);
assert.equal(extended[0].reading, "ネコ");
mutableAfterLoad.destroy();

console.log("stream-mecab SMD1 -> raw WASM: ok");
