import assert from "node:assert/strict";
import fs from "node:fs/promises";
import { Streamdown, parseLlmDescriptor } from "./streamdown.js";
import { componentSpan, layoutSpec } from "./layout.js";
import { canvasSpec, parseCanvasScene } from "./canvas.js";
import { evaluateExpression, safeEvaluate } from "./expression.js";
import { tabFor, tabsSpec } from "./tabs.js";
import { formSpec } from "./form.js";
import { layoutGraph, parseGraph } from "./graph.js";
import { consumeHttpResponse, decodeNdjsonLine, decodeSseEvent, extractDeltaText } from "./stream.js";
import { buildInteractionPrompt, buildLlmRequest, GENERATIVE_UI_SYSTEM_PROMPT, snapshotUiComponents, snapshotUiState } from "./llm_request.js";
import { statePatch, statePatchSignature } from "./state_patch.js";
import { componentPatch, componentPatchSignature, mergeComponentPatches } from "./component_patch.js";
import { summarizeModelCommit } from "./commit_summary.js";

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



const graph = parseGraph(`node a Parser
node b Delta AST
edge a b diff
edge b c render
`);
assert.equal(graph.nodes.length, 3);
assert.equal(graph.edges.length, 2);
assert.equal(graph.nodes.find(node => node.id === "c").label, "c");
const laidOut = layoutGraph(graph, 640, 300);
assert.equal(laidOut.nodes.length, 3);
assert.ok(laidOut.nodes.every(node => Number.isFinite(node.x) && Number.isFinite(node.y)));
assert.ok(laidOut.nodes.find(node => node.id === "a").x < laidOut.nodes.find(node => node.id === "c").x);

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


assert.equal(extractDeltaText({ choices: [{ delta: { content: "hello" } }] }), "hello");
assert.equal(extractDeltaText({ type: "response.output_text.delta", delta: " world" }), " world");
assert.deepEqual(decodeSseEvent('data: {"choices":[{"delta":{"content":"# Hi"}}]}'), { text: "# Hi", done: false });
assert.equal(decodeSseEvent("data: [DONE]").done, true);
assert.deepEqual(decodeNdjsonLine('{"delta":{"text":" streamed"}}'), { text: " streamed", done: false });

const sseChunks = [
  'data: {"choices":[{"delta":{"content":"# H"}}]}\n\n',
  'data: {"choices":[{"delta":{"content":"i"}}]}\n\n',
  'data: [DONE]\n\n',
];
const sseResponse = new Response(new ReadableStream({
  start(controller) {
    const encoder = new TextEncoder();
    for (const chunk of sseChunks) controller.enqueue(encoder.encode(chunk));
    controller.close();
  },
}), { headers: { "content-type": "text/event-stream" } });
let sseText = "";
const sseResult = await consumeHttpResponse(sseResponse, { onText: text => { sseText += text; } });
assert.equal(sseText, "# Hi");
assert.equal(sseResult.format, "sse");
assert.equal(sseResult.chunks, 2);

const getRequest = buildLlmRequest({ protocol: "get" });
assert.equal(getRequest.method, "GET");
assert.equal(getRequest.body, undefined);
const chatRequest = buildLlmRequest({ protocol: "chat", prompt: "Build a dashboard", model: "demo-model" });
assert.equal(chatRequest.method, "POST");
const chatBody = JSON.parse(chatRequest.body);
assert.equal(chatBody.stream, true);
assert.equal(chatBody.model, "demo-model");
assert.equal(chatBody.messages.at(-1).content, "Build a dashboard");
assert.match(chatBody.messages[0].content, /streaming Markdown application/);
assert.match(GENERATIVE_UI_SYSTEM_PROMPT, /type=graph/);
const responsesRequest = buildLlmRequest({ protocol: "responses", prompt: "Explain parsers" });
assert.equal(JSON.parse(responsesRequest.body).input, "Explain parsers");
assert.throws(() => buildLlmRequest({ protocol: "chat", prompt: "   " }), /Prompt is required/);

const interactionState = new Map([
  ["temperature", 65],
  ["mode", "safe"],
  ["api_token", "do-not-send"],
  ["password", "also-do-not-send"],
  ["nested", { unsafe: true }],
]);
assert.deepEqual({ ...snapshotUiState(interactionState) }, { temperature: 65, mode: "safe" });
const componentSnapshot = snapshotUiComponents([
  { type: "slider", id: "temp", state: "temperature", label: "Temperature", action: "ignored" },
  { type: "input", id: "secret", state: "api_token", label: "Token", body: "ignored" },
  { type: "button", id: "refine", label: "Refine", when: "temperature >= 40" },
]);
assert.deepEqual(componentSnapshot.map(item => ({ ...item })), [
  { type: "slider", id: "temp", label: "Temperature", state: "temperature" },
  { type: "input", id: "secret", label: "Token" },
  { type: "button", id: "refine", label: "Refine", when: "temperature >= 40" },
]);
assert.ok(componentSnapshot.every(item => !("action" in item) && !("body" in item)));
const interactionPrompt = buildInteractionPrompt({
  instruction: "Refine the dashboard",
  state: interactionState,
  components: componentSnapshot,
});
assert.match(interactionPrompt, /Refine the dashboard/);
assert.match(interactionPrompt, /"temperature":65/);
assert.match(interactionPrompt, /"type":"slider"/);
assert.match(interactionPrompt, /"id":"temp"/);
assert.doesNotMatch(interactionPrompt, /do-not-send|also-do-not-send|ignored/);
assert.match(GENERATIVE_UI_SYSTEM_PROMPT, /action=llm:/);
assert.match(GENERATIVE_UI_SYSTEM_PROMPT, /type=patch target=/);

const patchConfig = {
  type: "state",
  temperature: "58",
  exact: "true",
  note: "ready",
  nullable: "null",
  password: "do-not-apply",
  api_key: "do-not-apply-either",
  empty: "   ",
};
assert.deepEqual(statePatch(patchConfig), [
  ["temperature", 58],
  ["exact", true],
  ["note", "ready"],
  ["nullable", null],
]);
assert.equal(statePatchSignature(patchConfig), statePatchSignature({ ...patchConfig }));
assert.deepEqual(statePatch({ type: "state", temperature: "" }), []);

const visualPatch = componentPatch({
  type: "patch",
  target: "throughput",
  label: "Model throughput",
  value: "3.1M",
  unit: "chars/s",
  trend: "safe overlay",
  when: "temperature >= 50",
  action: "llm:must-not-change",
  state: "must-not-change",
  span: "4",
  tab: "controls",
});
assert.deepEqual({ target: visualPatch.target, values: { ...visualPatch.values } }, {
  target: "throughput",
  values: {
    label: "Model throughput",
    value: "3.1M",
    unit: "chars/s",
    trend: "safe overlay",
    when: "temperature >= 50",
  },
});
assert.equal(componentPatch({ type: "patch", target: "bad target!", value: "x" }), null);
const mergedPatches = mergeComponentPatches([
  componentPatch({ target: "throughput", label: "First", value: "1" }),
  componentPatch({ target: "throughput", value: "2", trend: "latest" }),
]);
assert.deepEqual({ ...mergedPatches.get("throughput") }, { label: "First", value: "2", trend: "latest" });
assert.equal(
  componentPatchSignature([componentPatch({ target: "x", value: "1" })]),
  componentPatchSignature([componentPatch({ target: "x", value: "1" })]),
);


const commitSummary = summarizeModelCommit({
  before: { source: "# Before", state: [["temperature", 42], ["api_token", "secret"]] },
  after: { source: "# Before\n\nmore", state: [["temperature", 58], ["api_token", "changed"], ["mode", "exact"]] },
  responseText: `:::llm ui type=state\ntemperature=58\n:::\n\n:::llm ui type=patch target=throughput value=3.1M\n:::\n\n:::llm ui type=metric id=new-card\nvalue=58\n:::\n`,
  format: "sse",
  chunks: 5,
  firstUiMs: 9.75,
});
assert.equal(commitSummary.sourceDelta, 6);
assert.deepEqual(commitSummary.stateKeys, ["temperature", "mode"]);
assert.equal(commitSummary.stateChangeCount, 2);
assert.deepEqual(commitSummary.patchTargets, ["throughput"]);
assert.equal(commitSummary.patchCount, 1);
assert.equal(commitSummary.semanticBlocks, 3);
assert.equal(commitSummary.newUiBlocks, 1);
assert.equal(commitSummary.format, "SSE");
assert.equal(commitSummary.chunks, 5);
assert.equal(commitSummary.firstUiMs, 9.75);


const securityHtml = await fs.readFile(new URL("./index.html", import.meta.url), "utf8");
const securityApp = await fs.readFile(new URL("./app.js", import.meta.url), "utf8");
assert.match(securityHtml, /script-src 'self' 'wasm-unsafe-eval'/);
assert.match(securityHtml, /object-src 'none'/);
assert.match(securityHtml, /frame-src 'none'/);
assert.match(securityHtml, /form-action 'none'/);
assert.match(securityHtml, /require-trusted-types-for 'script'/);
assert.match(securityHtml, /trusted-types 'none'/);
assert.doesNotMatch(securityApp, /\b(?:innerHTML|outerHTML|insertAdjacentHTML|document\.write)\b/);
assert.doesNotMatch(securityApp, /\bnew\s+Function\b|javascript:/i);
assert.equal([...securityApp.matchAll(/\beval\s*\(/g)].length, 1);
assert.match(securityApp, /No eval\(\)/);

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

const followOps = parser.append(`\n:::llm ui type=metric id=followup\nvalue=2\n:::\n`);
assert.ok(followOps.length > 0);
parser.finish();
assert.ok(parser.getLlmBlocks({ kind: "ui", closed: true }).some(block => block.attributes.id === "followup"));
parser.dispose();

console.log("webapp generative tests: ok");
