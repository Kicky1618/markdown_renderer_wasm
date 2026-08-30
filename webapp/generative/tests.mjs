import assert from "node:assert/strict";
import fs from "node:fs/promises";
import { Streamdown, parseLlmDescriptor } from "./streamdown.js";
import { componentSpan, layoutSpec } from "./layout.js";
import { canvasSpec, parseCanvasScene } from "./canvas.js";
import { evaluateExpression, safeEvaluate } from "./expression.js";
import { tabFor, tabsSpec } from "./tabs.js";
import { formSpec } from "./form.js";

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

const reactiveState = new Map([["temperature", 42], ["name", "gpu"]]);
assert.equal(evaluateExpression("temperature * 9 / 5 + 32", reactiveState), 107.6);
assert.equal(evaluateExpression("temperature >= 40 && temperature < 50", reactiveState), true);
assert.equal(evaluateExpression("name + '-runtime'", reactiveState), "gpu-runtime");
assert.equal(safeEvaluate("window.location", reactiveState, "blocked"), undefined);
assert.equal(safeEvaluate("temperature ** 2", reactiveState, "blocked"), "blocked");

const tabs = tabsSpec({ id: "views", state: "view", labels: "Status,Controls", values: "status,controls", value: "controls" });
assert.deepEqual(tabs.items, [{ label: "Status", value: "status" }, { label: "Controls", value: "controls" }]);
assert.equal(tabs.initial, "controls");
assert.equal(tabFor({ tab: "status" }, tabs), "status");
assert.equal(tabFor({ tab: "missing" }, tabs), "status");

assert.deepEqual(formSpec({ id: "launch", title: "Launch", submit: "Go", action: "set:submitted:1" }), {
  id: "launch",
  title: "Launch",
  submit: "Go",
  action: "set:submitted:1",
});


assert.deepEqual(canvasSpec({ width: "5000", height: "20", title: "Scene" }), {
  width: 1200,
  height: 120,
  title: "Scene",
});
const scene = parseCanvasScene(`width=640
line 0 1 2 3
circle 10 20 5
rect 1 2 3 4
text 8 9 hello streamed world
unknown 1 2 3
`);
assert.deepEqual(scene.map(command => command.type), ["line", "circle", "rect", "text"]);
assert.equal(scene[3].text, "hello streamed world");
assert.equal(parseCanvasScene(`line 1 2
circle 1`).length, 0);

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

console.log("webapp generative tests: ok");
