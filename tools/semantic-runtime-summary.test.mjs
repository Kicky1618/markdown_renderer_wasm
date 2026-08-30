import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { Streamdown } from "../js/streamdown.js";
import { semanticReferencesFromLinks } from "./semantic-timeline-core.mjs";
import { SemanticRuntimeSummary } from "./semantic-runtime-summary.mjs";

const wasmPath = process.argv[2] ?? "target/wasm32-unknown-unknown/release/streamdown.wasm";
const wasm = await readFile(wasmPath);

const markdown = [
  "plain @[artifact:first] and [ordinary](https://example.com)\n\n",
  ":::llm tool id=search name=\"web search\"\n",
  '{"query":"日本語 stream"}\n',
  ":::\n\n",
  "- list @[tool:search]\n",
  "- second item\n\n",
  "| ref | value |\n",
  "| --- | --- |\n",
  "| @[artifact:first] | **ok** |\n\n",
  "> quote @[ui:panel]\n\n",
  ":::llm artifact id=first depends=tool:search\n",
  '{"answer":true}\n',
  ":::\n\n",
  ":::llm ui id=panel depends=artifact:first\n",
  '{"type":"metric"}\n',
  ":::\n",
].join("");

function fullSummary(parser) {
  return {
    llmBlocks: parser.getLlmBlocks(),
    semanticReferences: semanticReferencesFromLinks(parser.getLinks()),
  };
}

function chunks(text, width) {
  const out = [];
  for (let i = 0; i < text.length; i += width) out.push(text.slice(i, i + width));
  return out;
}

for (const width of [1, 2, 5, 17, 64]) {
  const parser = await Streamdown.load(wasm);
  const cache = new SemanticRuntimeSummary(parser.document);
  try {
    for (const chunk of chunks(markdown, width)) {
      const previousBlockCount = parser.blockCount;
      parser.appendInPlace(chunk);
      const cached = cache.refreshTail(parser.document, previousBlockCount);
      assert.deepEqual(cached, fullSummary(parser), `summary mismatch at width=${width} after ${JSON.stringify(chunk)}`);
    }
    const previousBlockCount = parser.blockCount;
    parser.finish();
    assert.deepEqual(cache.refreshTail(parser.document, previousBlockCount), fullSummary(parser), `finish mismatch at width=${width}`);
  } finally {
    parser.dispose();
  }
}

console.log("semantic runtime summary cache: ok");
