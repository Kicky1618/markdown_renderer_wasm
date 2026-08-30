import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { Streamdown } from "../js/streamdown.js";

const wasm = await readFile(new URL(
  "../target/wasm32-unknown-unknown/release/streamdown.wasm",
  import.meta.url,
));

const regular = await Streamdown.load(wasm);
const inPlace = await Streamdown.load(wasm);

const chunks = [
  "Prefix **bold** then ",
  "plain token ",
  "stream ",
  "[[cite:doc-42|spec]] and ",
  "@[source:turn7search2].\n\n",
  "::::llm artifact mime=text/plain\n",
  "alpha\n:::\nomega\n",
  "::::\n\n",
  "```text\n",
  "0123456789abcdef\n",
  "more code\n",
  "```\n\n",
  "Unicode 日本語 ✅ ",
  "\ud800",
];

for (const chunk of chunks) {
  regular.append(chunk);
  const returned = inPlace.appendInPlace(chunk);
  assert.equal(returned, inPlace.document);
  assert.deepEqual(inPlace.document, regular.document, `document diverged after ${JSON.stringify(chunk)}`);
}

regular.finish();
inPlace.finish();
assert.deepEqual(inPlace.document, regular.document);
regular.dispose();
inPlace.dispose();

const regularDelimiters = await Streamdown.load(wasm);
const inPlaceDelimiters = await Streamdown.load(wasm);
for (const chunk of ["`", "`", "$", "$", "$", "$", "*", "*", "*", "*", "_", "_", "_", "_"]) {
  regularDelimiters.append(chunk);
  inPlaceDelimiters.appendInPlace(chunk);
  assert.deepEqual(inPlaceDelimiters.document, regularDelimiters.document);
}
regularDelimiters.dispose();
inPlaceDelimiters.dispose();

const regularLists = await Streamdown.load(wasm);
const inPlaceLists = await Streamdown.load(wasm);
for (const chunk of ["- one\n", "-", " ", "two", "\n", "-", " ", "three"]) {
  regularLists.append(chunk);
  inPlaceLists.appendInPlace(chunk);
  assert.deepEqual(inPlaceLists.document, regularLists.document);
}
regularLists.dispose();
inPlaceLists.dispose();

const regularQuotes = await Streamdown.load(wasm);
const inPlaceQuotes = await Streamdown.load(wasm);
for (const chunk of ["> one\n", ">", " ", "two", "\n", ">", " ", "日本語"]) {
  regularQuotes.append(chunk);
  inPlaceQuotes.appendInPlace(chunk);
  assert.deepEqual(inPlaceQuotes.document, regularQuotes.document);
}
regularQuotes.dispose();
inPlaceQuotes.dispose();

const consumed = await Streamdown.load(wasm);
await consumed.consume(["Answer **now**: ", "token ", "token ", "日本語"], { finalize: true });
assert.equal(consumed.toPlainText(), "Answer now: token token 日本語");
consumed.dispose();

console.log("appendInPlace hot-path equivalence: ok");
