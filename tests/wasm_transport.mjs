import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { Streamdown } from "../js/streamdown.js";

const wasm = await readFile(new URL("../target/wasm32-unknown-unknown/release/streamdown.wasm", import.meta.url));
const { instance } = await WebAssembly.instantiate(wasm, {});
const e = instance.exports;

assert.equal(typeof e.md_input_reserve, "function");
assert.equal(typeof e.md_append_input, "function");

const handle = e.md_create();
assert.notEqual(handle, 0);

const encoder = new TextEncoder();
const writeReusable = (text) => {
  const capacity = text.length * 3;
  const ptr = e.md_input_reserve(handle, capacity);
  let written = 0;
  if (capacity) {
    const view = new Uint8Array(e.memory.buffer, ptr, capacity);
    const result = encoder.encodeInto(text, view);
    assert.equal(result.read, text.length);
    written = result.written;
  }
  return e.md_append_input(handle, written);
};

assert.equal(writeReusable("逐次 "), 1);
assert.equal(writeReusable("**高速**\n"), 1);
assert.equal(e.md_append_input(handle, 1_000_000), 0, "cannot read beyond reserved input");

// Invalid UTF-8 must fail without corrupting the parser handle.
const invalidPtr = e.md_input_reserve(handle, 1);
new Uint8Array(e.memory.buffer, invalidPtr, 1)[0] = 0xff;
assert.equal(e.md_append_input(handle, 1), 0);
assert.equal(writeReusable("ok"), 1);

// The legacy ABI remains valid for older wrappers.
const legacy = encoder.encode(" legacy");
const legacyPtr = e.md_alloc(legacy.length);
new Uint8Array(e.memory.buffer, legacyPtr, legacy.length).set(legacy);
assert.equal(e.md_append(handle, legacyPtr, legacy.length), 1);
e.md_free(legacyPtr);
e.md_destroy(handle);

// Public JS wrapper should select the reusable transport and preserve Unicode.
const parser = await Streamdown.load(wasm);
parser.append("# Transport\n\n");
for (const part of ["ASCII ", "日本語 ", "🙂 ", "[[cite:doc-1|仕様]]"]) parser.append(part);
assert.equal(parser.toPlainText(), "Transport\n\nASCII 日本語 🙂 仕様");
assert.deepEqual(parser.getCitations(), [{ block: 1, source: "doc-1", label: "仕様" }]);
parser.dispose();

console.log("WASM reusable input transport: ok");
