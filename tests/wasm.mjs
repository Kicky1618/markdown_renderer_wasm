import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { Streamdown } from "../js/streamdown.js";

const wasm = await readFile(new URL("../target/wasm32-unknown-unknown/release/streamdown.wasm", import.meta.url));
const parser = await Streamdown.load(wasm);

parser.append("# 高速\n\n```rust\n");
parser.append(`println!("日本");\n`);
const changes = parser.append("```\nDone **now**");

assert.equal(parser.document[0].type, "heading");
assert.deepEqual(parser.document[0].children, [{ type: "text", value: "高速" }]);
assert.deepEqual(parser.document[1], {
  type: "codeBlock",
  closed: true,
  language: "rust",
  value: "println!(\"日本\");\n",
});
assert.equal(parser.document[2].type, "paragraph");
assert.ok(changes.some((x) => x.op === "sealCode"));
parser.dispose();

const mathParser = await Streamdown.load(wasm);
mathParser.append("$$\nA_3=\\begin{pmatrix}\na_1 & x_1 \\\\\na_2 & \\frac{1}{2}\n\\end{pmatrix}\n$$");
assert.deepEqual(mathParser.document[0].children, [{
  type: "math",
  display: true,
  value: "\nA_3=\\begin{pmatrix}\na_1 & x_1 \\\\\na_2 & \\frac{1}{2}\n\\end{pmatrix}\n",
}]);
mathParser.dispose();

const tableParser = await Streamdown.load(wasm);
tableParser.append("| Name | Value |\n| --- | ---: |\n| alpha | 42 |\n");
assert.equal(tableParser.document[0].type, "table");
assert.equal(tableParser.document[0].headers.length, 2);
assert.equal(tableParser.document[0].rows[0].length, 2);
tableParser.dispose();

const apiParser = await Streamdown.load(wasm);
const batched = apiParser.appendMany([
  "# Answer\n\nSee [docs](https://example.com).\n\n```js\n",
  "console.log('ok');\n```",
]);
assert.ok(batched.length > 0);
const finishOps = apiParser.finish();
assert.ok(finishOps.some((operation) => operation.op === "sealCode"));
assert.equal(apiParser.blockCount, 3);
assert.equal(apiParser.isEmpty, false);
assert.deepEqual(apiParser.getLinks(), [{
  block: 1,
  text: "docs",
  destination: "https://example.com",
}]);
assert.deepEqual(apiParser.getCodeBlocks({ language: "js", closed: true }), [{
  index: 2,
  language: "js",
  value: "console.log('ok');\n",
  closed: true,
}]);
assert.match(apiParser.toPlainText(), /console\.log/);
const snapshot = apiParser.snapshot();
snapshot.length = 0;
assert.equal(apiParser.blockCount, 3);

const resetOps = apiParser.reset();
assert.deepEqual(resetOps, [{ op: "truncate", from: 0 }]);
assert.equal(apiParser.isEmpty, true);
const replacementOps = apiParser.setContent("Replacement");
assert.ok(replacementOps.some((operation) => operation.op === "push"));
assert.equal(apiParser.toPlainText(), "Replacement");

apiParser.reset();
apiParser.append("Fact [[cite:doc-42|spec]] and @[artifact:plot-1]");
assert.deepEqual(apiParser.getCitations(), [{ block: 0, source: "doc-42", label: "spec" }]);
assert.ok(apiParser.getLinks().some((link) => link.destination === "llm:artifact:plot-1"));

apiParser.reset();
apiParser.append("`");
const inlineSplice = apiParser.append("`");
assert.deepEqual(inlineSplice, [{
  op: "spliceInlineTail",
  block: 0,
  removeNodes: 0,
  truncateBytes: 1,
  append: [{ type: "code", value: "" }],
}]);
assert.deepEqual(apiParser.document[0].children, [{ type: "code", value: "" }]);

apiParser.reset();
apiParser.append("$");
apiParser.append("$");
apiParser.append("$");
const mathSplice = apiParser.append("$");
assert.deepEqual(mathSplice, [{
  op: "spliceInlineTail",
  block: 0,
  removeNodes: 0,
  truncateBytes: 3,
  append: [{ type: "math", display: true, value: "" }],
}]);
assert.deepEqual(apiParser.document[0].children, [{ type: "math", display: true, value: "" }]);

apiParser.reset();
apiParser.append("prefix *");
apiParser.append("*");
apiParser.append("*");
const strongSplice = apiParser.append("*");
assert.deepEqual(strongSplice, [{
  op: "spliceInlineTail",
  block: 0,
  removeNodes: 1,
  truncateBytes: 1,
  append: [{ type: "strong", children: [] }],
}]);
assert.deepEqual(apiParser.document[0].children, [
  { type: "text", value: "prefix " },
  { type: "strong", children: [] },
]);

apiParser.reset();
apiParser.append("*");
apiParser.append("*");
apiParser.append("*");
const emphasisSplice = apiParser.append("*");
assert.deepEqual(emphasisSplice, [{
  op: "spliceInlineTail",
  block: 0,
  removeNodes: 1,
  truncateBytes: 1,
  append: [{ type: "strong", children: [] }],
}]);
assert.deepEqual(apiParser.document[0].children, [{ type: "strong", children: [] }]);

apiParser.reset();
apiParser.append("Answer with **important** context: ");
const inlineTailOps = apiParser.append("token ");
assert.deepEqual(inlineTailOps, [{ op: "appendInlineText", block: 0, append: "token " }]);
assert.equal(apiParser.toPlainText(), "Answer with important context: token ");

apiParser.reset();
apiParser.append("- one\n");
const listItem = apiParser.append("-");
assert.deepEqual(listItem, [{ op: "appendListItem", block: 0, item: [] }]);
apiParser.append(" ");
const listTail = apiParser.append("two");
assert.deepEqual(listTail, [{
  op: "spliceListItemTail",
  block: 0,
  removeNodes: 0,
  truncateBytes: 0,
  append: [{ type: "text", value: "two" }],
}]);
assert.deepEqual(apiParser.document[0], {
  type: "unorderedList",
  items: [
    [{ type: "text", value: "one" }],
    [{ type: "text", value: "two" }],
  ],
});

apiParser.reset();
apiParser.append("> one\n");
apiParser.append(">");
apiParser.append(" ");
const quoteTail = apiParser.append("two");
assert.deepEqual(quoteTail, [{
  op: "spliceQuoteTail",
  block: 0,
  removeNodes: 0,
  truncateBytes: 0,
  append: [
    { type: "softBreak" },
    { type: "text", value: "two" },
  ],
}]);
assert.deepEqual(apiParser.document[0], {
  type: "blockQuote",
  children: [
    { type: "text", value: "one" },
    { type: "softBreak" },
    { type: "text", value: "two" },
  ],
});

apiParser.reset();
apiParser.append(':::llm tool name="web search" id=q1\n');
apiParser.append('{"query":"rust wasm"}');
apiParser.append('\n:::\n');
const llmBlocks = apiParser.getLlmBlocks({ kind: "tool", closed: true });
assert.equal(llmBlocks.length, 1);
assert.equal(llmBlocks[0].attributes.name, "web search");
assert.equal(llmBlocks[0].attributes.id, "q1");
assert.equal(llmBlocks[0].value, '{"query":"rust wasm"}\n');

apiParser.reset();
const encoded = new TextEncoder().encode("逐次 **応答**");
const deltas = [];
await apiParser.consume([
  encoded.subarray(0, 2),
  encoded.subarray(2, 5),
  encoded.subarray(5),
], { onDelta: (operations) => deltas.push(operations) });
assert.equal(apiParser.toPlainText(), "逐次 応答");
assert.equal(deltas.length, 2);

const controller = new AbortController();
controller.abort(new Error("cancelled"));
await assert.rejects(apiParser.consume(["ignored"], { signal: controller.signal }), /cancelled/);
const pendingController = new AbortController();
const pendingConsume = apiParser.consume(new ReadableStream({ start() {} }), {
  signal: pendingController.signal,
});
pendingController.abort(new Error("pending cancelled"));
await assert.rejects(pendingConsume, /pending cancelled/);
apiParser.dispose();
assert.equal(apiParser.isDisposed, true);
assert.throws(() => apiParser.append("late"), /disposed/);

console.log("WASM + MDA1 JavaScript round trip: ok");
