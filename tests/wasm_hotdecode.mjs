import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { Streamdown } from "../js/streamdown.js";

const wasm = await readFile(new URL("../target/wasm32-unknown-unknown/release/streamdown.wasm", import.meta.url));

const parser = await Streamdown.load(wasm);
let ops = parser.append("plain ");
assert.equal(ops[0]?.op, "push");
ops = parser.append("token ");
assert.deepEqual(ops, [{ op: "appendText", block: 0, append: "token " }]);

parser.reset();
parser.append("Answer **bold** context: ");
ops = parser.append("token ");
assert.deepEqual(ops, [{ op: "appendInlineText", block: 0, append: "token " }]);
assert.equal(parser.toPlainText(), "Answer bold context: token ");

parser.reset();
parser.append("```text\n");
ops = parser.append("payload");
assert.deepEqual(ops, [{ op: "spliceCode", block: 0, truncateBytes: 0, append: "payload" }]);
assert.equal(parser.document[0].value, "payload");

ops = parser.reset();
assert.deepEqual(ops, [{ op: "truncate", from: 0 }]);
parser.dispose();

console.log("MDA1 hot delta decode: ok");
