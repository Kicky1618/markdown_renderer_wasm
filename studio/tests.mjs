import assert from "node:assert/strict";
import fs from "node:fs/promises";
import { Streamdown, parseLlmDescriptor } from "./streamdown.js";
import { componentSpan, layoutSpec } from "./layout.js";

assert.deepEqual(layoutSpec({ columns: "3", gap: "18", min: "240", title: "Grid" }), {
  id: "",
  title: "Grid",
  columns: 3,
  gap: 18,
  minWidth: 240,
});
assert.equal(layoutSpec({ columns: "99", gap: "-5", min: "20" }).columns, 4);
assert.equal(layoutSpec({ columns: "99", gap: "-5", min: "20" }).gap, 0);
assert.equal(layoutSpec({ columns: "99", gap: "-5", min: "20" }).minWidth, 120);
assert.equal(componentSpan({ span: "3" }, 2), 2);
assert.equal(componentSpan({ span: "0" }, 4), 1);

const wasm = await fs.readFile(new URL("./streamdown.wasm", import.meta.url));
const instance = await WebAssembly.instantiate(wasm, {});
const parser = new Streamdown(instance.instance);
const source = `:::llm ui type=layout id=main\ncolumns=2\ngap=12\n:::\n\n:::llm ui type=metric id=one\nvalue=1\n:::\n\n:::llm ui type=chart id=two span=2\nvalues=1,2,3\n:::\n`;
for (const character of source) parser.append(character);
parser.finish();

const blocks = parser.getLlmBlocks({ kind: "ui", closed: true });
assert.equal(blocks.length, 3);
assert.equal(parseLlmDescriptor(parser.document[0].language).attributes.type, "layout");
assert.equal(parseLlmDescriptor(parser.document[1].language).attributes.type, "metric");
assert.equal(parseLlmDescriptor(parser.document[2].language).attributes.type, "chart");
assert.match(blocks[0].value, /columns=2/);
assert.match(blocks[2].value, /values=1,2,3/);
parser.dispose();

console.log("studio tests: ok");
