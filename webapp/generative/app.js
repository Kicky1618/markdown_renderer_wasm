import { Streamdown, parseLlmDescriptor } from "./streamdown.js";
import { componentSpan, layoutSpec } from "./layout.js";
import { canvasSpec, parseCanvasScene } from "./canvas.js";
import { safeEvaluate } from "./expression.js";
import { tabFor, tabsSpec } from "./tabs.js";
import { formSpec } from "./form.js";
import { consumeHttpResponse } from "./stream.js";
import { buildInteractionPrompt, buildLlmRequest } from "./llm_request.js";
import { layoutGraph, parseGraph } from "./graph.js";

const DEMO = `# A dashboard that exists before the LLM finishes

This page is not generated after Markdown completes. **The interface appears while tokens are still arriving.**

The current temperature is **{{temperature}}°C**. The LLM controls both runtime state and layout without emitting JavaScript.

:::llm ui type=layout id=dashboard
title=Live generated dashboard
columns=2
gap=14
min=220
:::

:::llm ui type=metric id=throughput
label=Current throughput
value=2.4M
unit=chars/s
trend=incremental AST
:::

:::llm ui type=slider id=temp state=temperature min=0 max=100 value=42 step=1
label=Temperature
unit=°C
:::

:::llm ui type=progress id=thermal state=temperature min=0 max=100
label=Thermal load
unit=°C
:::

:::llm ui type=button id=boost span=2
label=Boost temperature +5°C
action=increment:temperature:5
:::

:::llm ui type=chart id=latency title="Streaming latency" span=2
values=58,51,47,40,36,31,28,24,21,19,17,16
unit=ms
:::

:::llm ui type=canvas id=scene title="A scene drawn token by token" span=2 width=640 height=220
text 28 34 STREAMING SCENE
line 30 180 610 52
circle 176 126 34
rect 330 86 120 72
line 450 122 555 160
circle 570 168 18
:::

:::llm ui type=graph id=pipeline title="The UI grows from the token stream" span=2 height=300
node llm LLM tokens
node parser Incremental parser
node delta Delta AST
node runtime UI runtime
node screen Interactive screen
edge llm parser stream
edge parser delta diff
edge delta runtime apply
edge runtime screen render
:::

:::llm ui type=derive state=fahrenheit expr="temperature * 9 / 5 + 32"
:::

## Generated views

The LLM can derive values, show conditional UI, and generate form controls without executable JavaScript.

:::llm ui type=tabs id=views state=view labels="Status,Controls" values="status,controls" value=status
:::

:::llm ui type=metric id=fahrenheit tab=status
label=Derived temperature
value={{fahrenheit}}
unit=°F
:::

:::llm ui type=metric id=warning tab=status when="temperature >= 60"
label=Thermal warning
value=HOT
trend=condition: temperature >= 60
:::

:::llm ui type=input id=operator tab=controls state=operator input=text value=Kicky1618
label=Operator
placeholder=Your name
:::

:::llm ui type=select id=mode tab=controls state=mode options="Fast,Safe,Exact" values="fast,safe,exact" value=fast
label=Execution mode
:::

## A generated form

:::llm ui type=form id=launch title="Launch configuration" submit="Commit safe state" action=set:submitted:1
:::

:::llm ui type=input id=project state=project input=text value=Streamdown
label=Project
placeholder=Project name
:::

:::llm ui type=select id=target state=target options="Browser,WASM,Native" values="browser,wasm,native" value=browser
label=Target
:::

:::llm ui type=metric id=committed when="submitted == 1"
label=Form state
value=COMMITTED
trend=submit action executed locally
:::

## Model round trip

Configure a POST proxy above, then this generated control can explicitly ask the same model to continue the application from the current local state.

:::llm ui type=button id=ask-model
title=Model continuation
label=Generate next view from current state
action=llm:Use the current state to append one compact recommendation card
:::

## The document keeps going

The layout ends automatically when ordinary Markdown resumes. The chart above grows before its closing fence arrives.

> The same syntax can come directly from an LLM response. No eval(), generated JavaScript, or HTML injection is required.
`;

const source = document.querySelector("#source");
const preview = document.querySelector("#preview");
const streamButton = document.querySelector("#stream");
const renderNowButton = document.querySelector("#render");
const resetButton = document.querySelector("#reset");
const speed = document.querySelector("#speed");
const speedLabel = document.querySelector("#speed-label");
const streamState = document.querySelector("#stream-state");
const deltaStatus = document.querySelector("#delta-status");
const blockCount = document.querySelector("#blocks");
const parseMs = document.querySelector("#parse-ms");
const renderMs = document.querySelector("#render-ms");
const inputRate = document.querySelector("#input-rate");
const firstUi = document.querySelector("#first-ui");
const remoteForm = document.querySelector("#remote-stream");
const streamUrl = document.querySelector("#stream-url");
const streamFormat = document.querySelector("#stream-format");
const requestProtocol = document.querySelector("#request-protocol");
const streamModel = document.querySelector("#stream-model");
const streamPrompt = document.querySelector("#stream-prompt");
const postOnly = document.querySelector("[data-post-only]");
const connectStreamButton = document.querySelector("#connect-stream");

let parser;
let blockElements = [];
let animation = 0;
let generation = 0;
let structuresComposed = false;
let remoteController = null;
let sessionStartedAt = 0;
let sessionChars = 0;
let firstUiAt = null;
let sessionUiBaseline = 0;
const state = new Map();

source.value = DEMO;
speed.addEventListener("input", () => {
  speedLabel.value = `${speed.value} chars/s`;
});

function syncRequestProtocol() {
  const isPost = requestProtocol.value !== "get";
  postOnly.hidden = !isPost;
  streamPrompt.required = isPost;
}
requestProtocol.addEventListener("change", syncRequestProtocol);
syncRequestProtocol();

function setRuntimeState(value) {
  streamState.textContent = value;
}

function cancelStreaming() {
  generation += 1;
  if (animation) cancelAnimationFrame(animation);
  animation = 0;
  streamButton.textContent = "▶ Stream";
}

function cancelRemoteStream() {
  if (remoteController) remoteController.abort();
  remoteController = null;
  connectStreamButton.textContent = "Connect";
}

function parseBody(body) {
  const values = Object.create(null);
  for (const rawLine of body.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith("#")) continue;
    const equals = line.indexOf("=");
    if (equals < 1) continue;
    const key = line.slice(0, equals).trim();
    let value = line.slice(equals + 1).trim();
    if ((value.startsWith('"') && value.endsWith('"')) || (value.startsWith("'") && value.endsWith("'"))) {
      value = value.slice(1, -1);
    }
    values[key] = value;
  }
  return values;
}

function uiConfig(block) {
  const descriptor = parseLlmDescriptor(block.language);
  if (!descriptor || descriptor.kind !== "ui") return null;
  return Object.assign(Object.create(null), descriptor.attributes, parseBody(block.value));
}

function appendBoundText(parent, text) {
  const pattern = /\{\{([A-Za-z_][A-Za-z0-9_.-]*)\}\}/g;
  let offset = 0;
  for (const match of text.matchAll(pattern)) {
    parent.append(document.createTextNode(text.slice(offset, match.index)));
    const binding = document.createElement("span");
    binding.dataset.stateValue = match[1];
    binding.textContent = String(state.get(match[1]) ?? "");
    parent.append(binding);
    offset = match.index + match[0].length;
  }
  parent.append(document.createTextNode(text.slice(offset)));
}

function appendInline(parent, nodes) {
  for (const node of nodes ?? []) {
    if (node.type === "text") appendBoundText(parent, node.value);
    else if (node.type === "emphasis" || node.type === "strong") {
      const element = document.createElement(node.type === "strong" ? "strong" : "em");
      appendInline(element, node.children);
      parent.append(element);
    } else if (node.type === "code") {
      const element = document.createElement("code");
      element.className = "inline";
      element.textContent = node.value;
      parent.append(element);
    } else if (node.type === "math") {
      const element = document.createElement("code");
      element.className = "inline";
      element.textContent = `${node.display ? "$$" : "$"}${node.value}${node.display ? "$$" : "$"}`;
      parent.append(element);
    } else if (node.type === "link") {
      const element = document.createElement("a");
      appendInline(element, node.children);
      if (/^(https?:|mailto:)/i.test(node.destination)) {
        element.href = node.destination;
        element.rel = "noreferrer noopener";
        element.target = "_blank";
      }
      parent.append(element);
    } else if (node.type === "citation") {
      const element = document.createElement("span");
      element.className = "inline";
      element.textContent = node.label || node.source;
      parent.append(element);
    } else if (node.type === "softBreak") parent.append(" ");
    else if (node.type === "hardBreak") parent.append(document.createElement("br"));
  }
}

function number(value, fallback) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : fallback;
}

function initState(config) {
  const key = config.state || config.id;
  if (!key) return null;
  if (!state.has(key)) state.set(key, number(config.value, config.value ?? 0));
  return key;
}

function derivedRules() {
  return parser?.document?.flatMap(block => {
    const config = uiConfig(block);
    if (config?.type !== "derive" || !config.state || !config.expr) return [];
    return [{ state: config.state, expr: config.expr }];
  }).slice(0, 32) ?? [];
}

function recomputeDerivedState() {
  const rules = derivedRules();
  for (let pass = 0; pass <= rules.length; pass++) {
    let changed = false;
    for (const rule of rules) {
      const value = safeEvaluate(rule.expr, state, undefined);
      if (value === undefined || Object.is(state.get(rule.state), value)) continue;
      state.set(rule.state, value);
      changed = true;
    }
    if (!changed) break;
  }
}

function syncReactiveDom() {
  for (const element of document.querySelectorAll("[data-state-value]")) {
    const key = element.dataset.stateValue;
    element.textContent = `${state.get(key) ?? ""}${element.dataset.stateUnit || ""}`;
  }
  for (const input of document.querySelectorAll("[data-state-input]")) {
    const key = input.dataset.stateInput;
    if (document.activeElement !== input && state.has(key)) input.value = String(state.get(key));
  }
  for (const track of document.querySelectorAll("[data-state-progress]")) {
    const key = track.dataset.stateProgress;
    const min = number(track.dataset.progressMin, 0);
    const max = Math.max(min + 1, number(track.dataset.progressMax, 100));
    const ratio = Math.max(0, Math.min(1, (number(state.get(key), min) - min) / (max - min)));
    const fill = track.querySelector(".progress-fill");
    if (fill) fill.style.width = `${ratio * 100}%`;
  }
  for (const element of document.querySelectorAll("[data-when]")) {
    element.hidden = !Boolean(safeEvaluate(element.dataset.when, state, false));
  }
  for (const tabs of document.querySelectorAll("[data-tabs-state]")) {
    const key = tabs.dataset.tabsState;
    const active = String(state.get(key) ?? tabs.dataset.tabsDefault ?? "");
    for (const button of tabs.querySelectorAll("[data-tab-value]")) {
      const selected = button.dataset.tabValue === active;
      button.setAttribute("aria-selected", String(selected));
      button.tabIndex = selected ? 0 : -1;
    }
    for (const panel of tabs.querySelectorAll("[data-tab-panel]")) {
      panel.hidden = panel.dataset.tabPanel !== active;
    }
  }
}

function updateState(key, value) {
  if (!key) return;
  state.set(key, value);
  recomputeDerivedState();
  syncReactiveDom();
}

function makeUiShell(block, config) {
  const card = document.createElement("section");
  card.className = `ui-card${block.closed ? "" : " partial"}`;
  card.dataset.uiId = config.id || "";
  if (config.when) card.dataset.when = config.when;
  return card;
}

function renderMetric(block, config) {
  const card = makeUiShell(block, config);
  const title = document.createElement("h3");
  title.textContent = config.label || config.title || "Metric";
  const row = document.createElement("div");
  row.className = "metric-row";
  const value = document.createElement("span");
  value.className = "metric-value";
  appendBoundText(value, config.value || "—");
  const unit = document.createElement("span");
  unit.className = "metric-unit";
  unit.textContent = config.unit || "";
  row.append(value, unit);
  if (config.trend) {
    const trend = document.createElement("span");
    trend.className = "metric-trend";
    trend.textContent = config.trend;
    row.append(trend);
  }
  card.append(title, row);
  return card;
}

function renderSlider(block, config) {
  const card = makeUiShell(block, config);
  const key = initState(config);
  const title = document.createElement("h3");
  title.textContent = config.label || "Slider";
  const row = document.createElement("div");
  row.className = "slider-row";
  const input = document.createElement("input");
  input.type = "range";
  input.min = String(number(config.min, 0));
  input.max = String(number(config.max, 100));
  input.step = String(number(config.step, 1));
  input.value = String(state.get(key) ?? number(config.value, 0));
  input.dataset.stateInput = key || "";
  const output = document.createElement("output");
  output.className = "slider-value";
  output.dataset.stateValue = key || "";
  output.dataset.stateUnit = config.unit || "";
  output.textContent = `${input.value}${config.unit || ""}`;
  input.addEventListener("input", () => {
    const next = number(input.value, input.value);
    updateState(key, next);
  });
  row.append(input, output);
  card.append(title, row);
  return card;
}

function renderProgress(block, config) {
  const card = makeUiShell(block, config);
  const key = initState(config);
  const title = document.createElement("h3");
  title.textContent = config.label || config.title || "Progress";
  const min = number(config.min, 0);
  const max = Math.max(min + 1, number(config.max, 100));
  const current = number(state.get(key), number(config.value, min));
  const row = document.createElement("div");
  row.className = "progress-row";
  const track = document.createElement("div");
  track.className = "progress-track";
  track.dataset.stateProgress = key || "";
  track.dataset.progressMin = String(min);
  track.dataset.progressMax = String(max);
  const fill = document.createElement("span");
  fill.className = "progress-fill";
  const ratio = Math.max(0, Math.min(1, (current - min) / (max - min)));
  fill.style.width = `${ratio * 100}%`;
  track.append(fill);
  const output = document.createElement("output");
  output.className = "progress-value";
  output.dataset.stateValue = key || "";
  output.dataset.stateUnit = config.unit || "";
  output.textContent = `${current}${config.unit || ""}`;
  row.append(track, output);
  card.append(title, row);
  return card;
}

function renderInput(block, config) {
  const card = makeUiShell(block, config);
  const key = initState(config);
  const title = document.createElement("label");
  title.className = "field-label";
  title.textContent = config.label || "Input";
  const input = document.createElement("input");
  const type = config.input === "number" ? "number" : "text";
  input.className = "generated-input";
  input.type = type;
  input.placeholder = String(config.placeholder || "").slice(0, 120);
  if (type === "number") {
    if (config.min !== undefined) input.min = String(number(config.min, 0));
    if (config.max !== undefined) input.max = String(number(config.max, 100));
    if (config.step !== undefined) input.step = String(number(config.step, 1));
  }
  input.value = String(state.get(key) ?? config.value ?? "");
  input.dataset.stateInput = key || "";
  input.addEventListener("input", () => {
    const value = type === "number" ? number(input.value, 0) : input.value.slice(0, 256);
    updateState(key, value);
  });
  card.append(title, input);
  return card;
}

function csv(value) {
  return String(value || "").split(",").map(item => item.trim()).filter(Boolean).slice(0, 32);
}

function renderSelect(block, config) {
  const card = makeUiShell(block, config);
  const key = initState(config);
  const labels = csv(config.options);
  const values = csv(config.values);
  const count = Math.min(32, Math.max(labels.length, values.length));
  const title = document.createElement("label");
  title.className = "field-label";
  title.textContent = config.label || "Select";
  const select = document.createElement("select");
  select.className = "generated-input";
  select.dataset.stateInput = key || "";
  for (let index = 0; index < count; index++) {
    const option = document.createElement("option");
    option.value = values[index] || labels[index] || String(index);
    option.textContent = labels[index] || values[index] || `Option ${index + 1}`;
    select.append(option);
  }
  const current = String(state.get(key) ?? config.value ?? select.options[0]?.value ?? "");
  if ([...select.options].some(option => option.value === current)) select.value = current;
  else if (select.options.length) updateState(key, select.options[0].value);
  select.addEventListener("change", () => updateState(key, select.value));
  card.append(title, select);
  return card;
}

function renderInvisibleMarker(kind) {
  const marker = document.createElement("span");
  marker.hidden = true;
  marker.dataset.uiMarker = kind;
  return marker;
}

function currentUiContext() {
  return (parser?.document || []).flatMap(block => {
    const config = uiConfig(block);
    if (!config?.type) return [];
    return [{
      type: config.type,
      id: config.id,
      label: config.label,
      title: config.title,
      state: config.state,
      tab: config.tab,
      when: config.when,
      unit: config.unit,
    }];
  });
}

async function runLlmInteraction(instruction) {
  if (remoteController) throw new Error("another remote stream is already active");
  if (requestProtocol.value === "get") throw new Error("action=llm requires a POST proxy protocol");

  let url;
  try {
    url = new URL(streamUrl.value.trim(), location.href);
    if (url.protocol !== "http:" && url.protocol !== "https:") throw new Error("HTTP(S) URL required");
  } catch (error) {
    throw new Error(`LLM action has no valid configured endpoint: ${error.message || error}`);
  }

  const requested = streamFormat.value;
  const accept = requested === "sse" ? "text/event-stream"
    : requested === "ndjson" ? "application/x-ndjson"
    : "text/plain, text/event-stream, application/x-ndjson;q=0.9, */*;q=0.5";
  const prompt = buildInteractionPrompt({
    instruction,
    state,
    components: currentUiContext(),
  });
  const request = buildLlmRequest({
    protocol: requestProtocol.value,
    prompt,
    model: streamModel.value,
  });

  cancelStreaming();
  beginInputSession(generatedUiCount());
  const controller = new AbortController();
  remoteController = controller;
  connectStreamButton.textContent = "■ Stop";
  setRuntimeState("LLM ACTION");
  deltaStatus.textContent = `Sending state to ${url.host}…`;
  let received = "";
  let prefix = "";

  try {
    const response = await fetch(url, {
      ...request,
      credentials: "omit",
      cache: "no-store",
      headers: { ...request.headers, Accept: accept },
      signal: controller.signal,
    });
    if (!response.ok) throw new Error(`stream request failed: HTTP ${response.status}`);
    prefix = parser.blockCount ? "\n\n" : "";
    if (prefix) appendChunk(prefix);
    setRuntimeState("LLM CONTINUATION");
    const result = await consumeHttpResponse(response, {
      format: requested,
      signal: controller.signal,
      onText(text) {
        received += text;
        appendChunk(text);
        preview.scrollTop = preview.scrollHeight;
      },
    });
    renderOperations(parser.finish());
    source.value += prefix + received;
    setRuntimeState("LIVE");
    deltaStatus.textContent = `LLM ${result.format.toUpperCase()} · ${result.chunks} chunks · ${result.chars} chars`;
    return result;
  } catch (error) {
    if (controller.signal.aborted || error?.name === "AbortError") {
      if (received) source.value += prefix + received;
      setRuntimeState("PAUSED");
      deltaStatus.textContent = "LLM continuation stopped";
      return null;
    }
    setRuntimeState("ERROR");
    deltaStatus.textContent = `LLM action failed: ${error.message || error}`;
    throw error;
  } finally {
    if (remoteController === controller) remoteController = null;
    connectStreamButton.textContent = "Connect";
  }
}

async function executeAction(action) {
  const parts = String(action || "").split(":");
  const verb = parts.shift() || "";
  if (verb === "llm") {
    const instruction = parts.join(":").trim() || "Continue the current application using the latest state.";
    return runLlmInteraction(instruction);
  }
  const key = parts.shift();
  const raw = parts.join(":");
  if (!key) return;
  if (verb === "increment") updateState(key, number(state.get(key), 0) + number(raw, 1));
  else if (verb === "decrement") updateState(key, number(state.get(key), 0) - number(raw, 1));
  else if (verb === "set") updateState(key, number(raw, raw ?? ""));
}

async function runControlAction(control, action) {
  const original = control?.textContent || "";
  if (control) control.disabled = true;
  if (control && String(action).startsWith("llm:")) control.textContent = "Generating…";
  try {
    await executeAction(action);
  } catch (error) {
    console.error("Generated UI action failed", error);
  } finally {
    if (control) {
      control.disabled = false;
      control.textContent = original;
    }
  }
}

function renderButton(block, config) {
  const card = makeUiShell(block, config);
  const title = document.createElement("h3");
  title.textContent = config.title || "Action";
  const button = document.createElement("button");
  button.className = "generated-button";
  button.type = "button";
  button.textContent = config.label || "Run action";
  button.disabled = !config.action;
  button.addEventListener("click", () => { void runControlAction(button, config.action); });
  card.append(title, button);
  return card;
}

function drawChart(canvas, values) {
  const rect = canvas.getBoundingClientRect();
  if (!rect.width || !rect.height) return;
  const dpr = Math.min(devicePixelRatio || 1, 2);
  canvas.width = Math.round(rect.width * dpr);
  canvas.height = Math.round(rect.height * dpr);
  const ctx = canvas.getContext("2d");
  ctx.scale(dpr, dpr);
  const width = rect.width;
  const height = rect.height;
  const pad = 22;
  ctx.clearRect(0, 0, width, height);
  ctx.strokeStyle = "rgba(121, 151, 181, .13)";
  ctx.lineWidth = 1;
  for (let i = 0; i <= 4; i++) {
    const y = pad + (height - pad * 2) * i / 4;
    ctx.beginPath();
    ctx.moveTo(pad, y);
    ctx.lineTo(width - pad, y);
    ctx.stroke();
  }
  if (!values.length) return;
  const min = Math.min(...values);
  const max = Math.max(...values);
  const range = Math.max(max - min, 1);
  const x = index => values.length === 1 ? width / 2 : pad + index * (width - pad * 2) / (values.length - 1);
  const y = value => pad + (max - value) * (height - pad * 2) / range;
  const gradient = ctx.createLinearGradient(0, pad, 0, height - pad);
  gradient.addColorStop(0, "rgba(105, 223, 206, .28)");
  gradient.addColorStop(1, "rgba(105, 223, 206, 0)");
  ctx.beginPath();
  ctx.moveTo(x(0), height - pad);
  values.forEach((value, index) => ctx.lineTo(x(index), y(value)));
  ctx.lineTo(x(values.length - 1), height - pad);
  ctx.closePath();
  ctx.fillStyle = gradient;
  ctx.fill();
  ctx.beginPath();
  values.forEach((value, index) => index ? ctx.lineTo(x(index), y(value)) : ctx.moveTo(x(index), y(value)));
  ctx.lineWidth = 2.4;
  ctx.strokeStyle = "#69dfce";
  ctx.stroke();
  const last = values.length - 1;
  ctx.beginPath();
  ctx.arc(x(last), y(values[last]), 4, 0, Math.PI * 2);
  ctx.fillStyle = "#d7fff9";
  ctx.fill();
}

function renderChart(block, config) {
  const card = makeUiShell(block, config);
  const title = document.createElement("h3");
  title.textContent = config.title || config.label || "Chart";
  const values = String(config.values || "")
    .split(",")
    .map(value => Number(value.trim()))
    .filter(Number.isFinite);
  const wrap = document.createElement("div");
  wrap.className = "chart-wrap";
  const canvas = document.createElement("canvas");
  canvas.setAttribute("role", "img");
  canvas.setAttribute("aria-label", `${title.textContent}: ${values.join(", ")}`);
  wrap.append(canvas);
  const meta = document.createElement("div");
  meta.className = "chart-meta";
  const countLabel = document.createElement("span");
  countLabel.textContent = `${values.length} streamed points`;
  const lastLabel = document.createElement("span");
  lastLabel.textContent = values.length ? `${values.at(-1)}${config.unit || ""}` : "waiting for data";
  meta.append(countLabel, lastLabel);
  card.append(title, wrap, meta);
  requestAnimationFrame(() => drawChart(canvas, values));
  return card;
}

function drawCanvasScene(canvas, spec, commands) {
  const rect = canvas.getBoundingClientRect();
  if (!rect.width || !rect.height) return;
  const dpr = Math.min(devicePixelRatio || 1, 2);
  canvas.width = Math.max(1, Math.round(rect.width * dpr));
  canvas.height = Math.max(1, Math.round(rect.height * dpr));
  const ctx = canvas.getContext("2d");
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, rect.width, rect.height);
  const sx = rect.width / spec.width;
  const sy = rect.height / spec.height;
  ctx.save();
  ctx.scale(sx, sy);
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  for (const command of commands) {
    if (command.type === "line") {
      ctx.beginPath();
      ctx.moveTo(command.x1, command.y1);
      ctx.lineTo(command.x2, command.y2);
      ctx.strokeStyle = "#6edbcb";
      ctx.lineWidth = 2 / Math.max(sx, sy);
      ctx.stroke();
    } else if (command.type === "circle") {
      ctx.beginPath();
      ctx.arc(command.x, command.y, command.r, 0, Math.PI * 2);
      ctx.fillStyle = "rgba(110, 219, 203, .16)";
      ctx.fill();
      ctx.strokeStyle = "#72bdf4";
      ctx.lineWidth = 2 / Math.max(sx, sy);
      ctx.stroke();
    } else if (command.type === "rect") {
      ctx.fillStyle = "rgba(126, 151, 255, .13)";
      ctx.fillRect(command.x, command.y, command.w, command.h);
      ctx.strokeStyle = "#8299ef";
      ctx.lineWidth = 2 / Math.max(sx, sy);
      ctx.strokeRect(command.x, command.y, command.w, command.h);
    } else if (command.type === "text") {
      ctx.fillStyle = "#cbd8e6";
      ctx.font = `${14 / Math.max(sx, sy)}px ui-monospace, monospace`;
      ctx.fillText(command.text, command.x, command.y);
    }
  }
  ctx.restore();
}

function renderCanvas(block, config) {
  const card = makeUiShell(block, config);
  const spec = canvasSpec(config);
  const commands = parseCanvasScene(block.value);
  const title = document.createElement("h3");
  title.textContent = spec.title;
  const wrap = document.createElement("div");
  wrap.className = "scene-wrap";
  wrap.style.aspectRatio = `${spec.width} / ${spec.height}`;
  const canvas = document.createElement("canvas");
  canvas.dataset.sceneCanvas = "true";
  canvas.setAttribute("role", "img");
  canvas.setAttribute("aria-label", `${spec.title}: ${commands.length} streamed drawing commands`);
  wrap.append(canvas);
  const meta = document.createElement("div");
  meta.className = "chart-meta";
  const countLabel = document.createElement("span");
  countLabel.textContent = `${commands.length} drawing commands`;
  const sizeLabel = document.createElement("span");
  sizeLabel.textContent = `${spec.width}×${spec.height}`;
  meta.append(countLabel, sizeLabel);
  card.append(title, wrap, meta);
  requestAnimationFrame(() => drawCanvasScene(canvas, spec, commands));
  return card;
}

function renderGraph(block, config) {
  const card = makeUiShell(block, config);
  const title = document.createElement("h3");
  title.textContent = config.title || config.label || "Graph";
  const height = Math.max(180, Math.min(520, number(config.height, 300)));
  const graph = parseGraph(block.value);
  const layout = layoutGraph(graph, 640, height);
  const svgNs = "http://www.w3.org/2000/svg";
  const wrap = document.createElement("div");
  wrap.className = "graph-wrap";
  const svg = document.createElementNS(svgNs, "svg");
  svg.setAttribute("viewBox", `0 0 ${layout.width} ${layout.height}`);
  svg.setAttribute("role", "img");
  svg.setAttribute("aria-label", `${title.textContent}: ${layout.nodes.length} nodes, ${layout.edges.length} edges`);
  const positions = new Map(layout.nodes.map(node => [node.id, node]));

  for (const edge of layout.edges) {
    const from = positions.get(edge.from);
    const to = positions.get(edge.to);
    if (!from || !to) continue;
    const line = document.createElementNS(svgNs, "line");
    line.setAttribute("x1", String(from.x));
    line.setAttribute("y1", String(from.y));
    line.setAttribute("x2", String(to.x));
    line.setAttribute("y2", String(to.y));
    line.setAttribute("class", "graph-edge");
    svg.append(line);
    if (edge.label) {
      const text = document.createElementNS(svgNs, "text");
      text.setAttribute("x", String((from.x + to.x) / 2));
      text.setAttribute("y", String((from.y + to.y) / 2 - 7));
      text.setAttribute("class", "graph-edge-label");
      text.textContent = edge.label;
      svg.append(text);
    }
  }

  for (const node of layout.nodes) {
    const group = document.createElementNS(svgNs, "g");
    group.setAttribute("class", "graph-node");
    const rect = document.createElementNS(svgNs, "rect");
    rect.setAttribute("x", String(node.x - 58));
    rect.setAttribute("y", String(node.y - 22));
    rect.setAttribute("width", "116");
    rect.setAttribute("height", "44");
    rect.setAttribute("rx", "12");
    const label = document.createElementNS(svgNs, "text");
    label.setAttribute("x", String(node.x));
    label.setAttribute("y", String(node.y + 4));
    label.textContent = node.label.length > 24 ? `${node.label.slice(0, 23)}…` : node.label;
    const full = document.createElementNS(svgNs, "title");
    full.textContent = node.label;
    group.append(rect, label, full);
    svg.append(group);
  }

  wrap.append(svg);
  const meta = document.createElement("div");
  meta.className = "chart-meta";
  const nodes = document.createElement("span");
  nodes.textContent = `${layout.nodes.length} nodes`;
  const edges = document.createElement("span");
  edges.textContent = `${layout.edges.length} edges`;
  meta.append(nodes, edges);
  card.append(title, wrap, meta);
  return card;
}

function renderLayoutMarker() {
  return renderInvisibleMarker("layout");
}

function renderUi(block, config) {
  switch (config.type) {
    case "layout": return renderLayoutMarker();
    case "tabs": return renderInvisibleMarker("tabs");
    case "form": return renderInvisibleMarker("form");
    case "derive": return renderInvisibleMarker("derive");
    case "metric": return renderMetric(block, config);
    case "slider": return renderSlider(block, config);
    case "progress": return renderProgress(block, config);
    case "input": return renderInput(block, config);
    case "select": return renderSelect(block, config);
    case "button": return renderButton(block, config);
    case "chart": return renderChart(block, config);
    case "canvas": return renderCanvas(block, config);
    case "graph": return renderGraph(block, config);
    default: {
      const card = makeUiShell(block, config);
      card.classList.add("ui-error");
      const title = document.createElement("h3");
      title.textContent = config.type ? `Unknown UI: ${config.type}` : "UI descriptor is still streaming";
      const detail = document.createElement("p");
      detail.className = "subtle";
      detail.textContent = block.value || "Waiting for component data…";
      card.append(title, detail);
      return card;
    }
  }
}

function createBlock(block) {
  if (block.type === "codeBlock") {
    const config = uiConfig(block);
    if (config) return renderUi(block, config);
    const pre = document.createElement("pre");
    pre.className = "md-block";
    const code = document.createElement("code");
    code.textContent = block.value;
    pre.append(code);
    return pre;
  }
  if (block.type === "heading") {
    const element = document.createElement(`h${Math.max(1, Math.min(6, block.level))}`);
    element.className = "md-block";
    appendInline(element, block.children);
    return element;
  }
  if (block.type === "paragraph" || block.type === "blockQuote") {
    const element = document.createElement(block.type === "blockQuote" ? "blockquote" : "p");
    element.className = "md-block";
    appendInline(element, block.children);
    return element;
  }
  if (block.type === "unorderedList" || block.type === "orderedList") {
    const element = document.createElement(block.type === "orderedList" ? "ol" : "ul");
    element.className = "md-block";
    if (block.type === "orderedList") element.start = block.start;
    for (const item of block.items) {
      const li = document.createElement("li");
      appendInline(li, item);
      element.append(li);
    }
    return element;
  }
  if (block.type === "thematicBreak") {
    const element = document.createElement("hr");
    element.className = "md-block";
    return element;
  }
  if (block.type === "table") {
    const table = document.createElement("table");
    table.className = "md-block";
    const head = table.createTHead().insertRow();
    for (const cell of block.headers) {
      const th = document.createElement("th");
      appendInline(th, cell);
      head.append(th);
    }
    const body = table.createTBody();
    for (const row of block.rows) {
      const tr = body.insertRow();
      for (const cell of row) {
        const td = tr.insertCell();
        appendInline(td, cell);
      }
    }
    return table;
  }
  const fallback = document.createElement("pre");
  fallback.className = "md-block";
  fallback.textContent = JSON.stringify(block, null, 2);
  return fallback;
}

function truncateBlocks(from) {
  for (let i = blockElements.length - 1; i >= from; i--) blockElements[i]?.remove();
  blockElements.length = from;
}

function structureType(block) {
  const type = uiConfig(block)?.type;
  return type === "layout" || type === "tabs" || type === "form" ? type : null;
}

function createTabsRoot(block, spec) {
  const root = document.createElement("section");
  root.className = `generated-tabs${block.closed ? "" : " partial"}`;
  root.dataset.tabsId = spec.id;
  root.dataset.tabsState = spec.state;
  root.dataset.tabsDefault = spec.initial;
  if (!state.has(spec.state)) state.set(spec.state, spec.initial);

  const header = document.createElement("header");
  header.className = "generated-tabs-header";
  const title = document.createElement("h3");
  title.textContent = spec.title;
  const nav = document.createElement("div");
  nav.className = "generated-tab-list";
  nav.setAttribute("role", "tablist");
  for (const item of spec.items) {
    const button = document.createElement("button");
    button.type = "button";
    button.setAttribute("role", "tab");
    button.dataset.tabValue = item.value;
    button.textContent = item.label;
    button.addEventListener("click", () => updateState(spec.state, item.value));
    nav.append(button);
  }
  header.append(title, nav);
  root.append(header);

  const panels = new Map();
  for (const item of spec.items) {
    const panel = document.createElement("div");
    panel.className = "generated-tab-panel";
    panel.dataset.tabPanel = item.value;
    panel.setAttribute("role", "tabpanel");
    root.append(panel);
    panels.set(item.value, panel);
  }
  return { root, panels };
}

function createFormRoot(block, spec) {
  const form = document.createElement("form");
  form.className = `generated-form${block.closed ? "" : " partial"}`;
  form.dataset.formId = spec.id;

  const header = document.createElement("header");
  header.className = "generated-form-header";
  const title = document.createElement("h3");
  title.textContent = spec.title;
  const badge = document.createElement("span");
  badge.textContent = "LOCAL STATE";
  header.append(title, badge);

  const fields = document.createElement("div");
  fields.className = "generated-form-fields";

  const footer = document.createElement("footer");
  footer.className = "generated-form-footer";
  const submit = document.createElement("button");
  submit.type = "submit";
  submit.className = "generated-button";
  submit.textContent = spec.submit;
  submit.disabled = !spec.action;
  footer.append(submit);

  form.addEventListener("submit", event => {
    event.preventDefault();
    void runControlAction(submit, spec.action);
  });
  form.append(header, fields, footer);
  return { form, fields };
}

function composeStructures() {
  const fragment = document.createDocumentFragment();
  let grid = null;
  let gridSpec = null;
  let tabs = null;
  let currentTabsSpec = null;
  let tabPanels = null;
  let form = null;
  let currentFormSpec = null;
  let formFields = null;

  const closeGrid = () => {
    if (!grid) return;
    fragment.append(grid);
    grid = null;
    gridSpec = null;
  };
  const closeTabs = () => {
    if (!tabs) return;
    fragment.append(tabs);
    tabs = null;
    currentTabsSpec = null;
    tabPanels = null;
  };
  const closeForm = () => {
    if (!form) return;
    fragment.append(form);
    form = null;
    currentFormSpec = null;
    formFields = null;
  };
  const closeStructures = () => {
    closeGrid();
    closeTabs();
    closeForm();
  };

  for (let index = 0; index < parser.document.length; index++) {
    const block = parser.document[index];
    const element = blockElements[index];
    const config = uiConfig(block);

    if (config?.type === "layout") {
      closeStructures();
      gridSpec = layoutSpec(config);
      grid = document.createElement("section");
      grid.className = `generated-layout${block.closed ? "" : " partial"}`;
      grid.dataset.layoutId = gridSpec.id;
      grid.dataset.layoutColumns = String(gridSpec.columns);
      grid.style.setProperty("--layout-columns", String(gridSpec.columns));
      grid.style.setProperty("--layout-gap", `${gridSpec.gap}px`);
      grid.style.setProperty("--layout-min", `${gridSpec.minWidth}px`);
      const heading = document.createElement("header");
      heading.className = "generated-layout-header";
      const title = document.createElement("h3");
      title.textContent = gridSpec.title;
      const badge = document.createElement("span");
      badge.textContent = `${gridSpec.columns} columns`;
      heading.append(title, badge);
      grid.append(heading);
      continue;
    }

    if (config?.type === "tabs") {
      closeStructures();
      currentTabsSpec = tabsSpec(config);
      const built = createTabsRoot(block, currentTabsSpec);
      tabs = built.root;
      tabPanels = built.panels;
      continue;
    }

    if (config?.type === "form") {
      closeStructures();
      currentFormSpec = formSpec(config);
      const built = createFormRoot(block, currentFormSpec);
      form = built.form;
      formFields = built.fields;
      continue;
    }

    if (form && config) {
      formFields.append(element);
      continue;
    }

    if (tabs && config) {
      const target = tabFor(config, currentTabsSpec);
      tabPanels.get(target)?.append(element);
      continue;
    }

    if (grid && config) {
      element.style.setProperty("--component-span", String(componentSpan(config, gridSpec.columns)));
      grid.append(element);
      continue;
    }

    closeStructures();
    if (element) {
      element.style?.removeProperty("--component-span");
      fragment.append(element);
    }
  }
  closeStructures();
  preview.replaceChildren(fragment);
  structuresComposed = hasStructure();
  recomputeDerivedState();
  syncReactiveDom();
}

function hasStructure() {
  return parser.document.some(block => structureType(block));
}

function replaceBlock(index) {
  const block = parser.document[index];
  if (!block) return;
  const next = createBlock(block);
  const previous = blockElements[index];
  const parentLayout = previous?.closest?.(".generated-layout");
  if (parentLayout) {
    const config = uiConfig(block);
    const columns = number(parentLayout.dataset.layoutColumns, 1);
    next.style?.setProperty("--component-span", String(componentSpan(config || {}, columns)));
  }
  if (previous) previous.replaceWith(next);
  else if (!structuresComposed && !hasStructure()) preview.append(next);
  blockElements[index] = next;
}

function renderOperations(ops) {
  const start = performance.now();
  let structuralLayoutChange = false;
  for (const op of ops) {
    if (op.op === "truncate") {
      structuralLayoutChange ||= structuresComposed || hasStructure();
      truncateBlocks(op.from);
    } else if (op.op === "push") {
      const index = blockElements.length;
      const element = createBlock(parser.document[index]);
      blockElements.push(element);
      if (structuresComposed || hasStructure()) structuralLayoutChange = true;
      else preview.append(element);
    } else if (op.op === "spliceCode" || op.op === "sealCode" || op.op === "appendText" || op.op === "appendInlineText") {
      structuralLayoutChange ||= Boolean(structureType(parser.document[op.block]));
      replaceBlock(op.block);
    }
  }
  if (structuralLayoutChange || (hasStructure() && !structuresComposed)) composeStructures();
  else {
    recomputeDerivedState();
    syncReactiveDom();
  }
  renderMs.textContent = (performance.now() - start).toFixed(3);
  blockCount.textContent = String(parser.blockCount);
  deltaStatus.textContent = ops.length ? `${ops.map(op => op.op).join(" · ")}` : "No structural change";
}

function generatedUiCount() {
  return preview.querySelectorAll(".ui-card, .generated-layout, .generated-tabs, .generated-form").length;
}

function beginInputSession(uiBaseline = generatedUiCount()) {
  sessionStartedAt = performance.now();
  sessionChars = 0;
  firstUiAt = null;
  sessionUiBaseline = uiBaseline;
  inputRate.textContent = "0";
  firstUi.textContent = "—";
}

function resetRuntime() {
  state.clear();
  beginInputSession(0);
  const start = performance.now();
  const ops = parser.reset();
  parseMs.textContent = (performance.now() - start).toFixed(3);
  renderOperations(ops);
  preview.scrollTop = 0;
}

function appendChunk(chunk) {
  const start = performance.now();
  const ops = parser.append(chunk);
  parseMs.textContent = (performance.now() - start).toFixed(3);
  renderOperations(ops);
  sessionChars += chunk.length;
  const elapsedMs = Math.max(0.01, performance.now() - sessionStartedAt);
  inputRate.textContent = Math.round(sessionChars * 1000 / elapsedMs).toLocaleString("en-US");
  if (firstUiAt === null && generatedUiCount() > sessionUiBaseline) {
    firstUiAt = performance.now() - sessionStartedAt;
    firstUi.textContent = `${firstUiAt.toFixed(1)}ms`;
  }
}

function renderAll() {
  cancelRemoteStream();
  cancelStreaming();
  setRuntimeState("RENDERING");
  resetRuntime();
  appendChunk(source.value);
  const start = performance.now();
  const finalOps = parser.finish();
  parseMs.textContent = (performance.now() - start).toFixed(3);
  renderOperations(finalOps);
  setRuntimeState("LIVE");
}

function streamAll() {
  cancelRemoteStream();
  cancelStreaming();
  resetRuntime();
  const text = source.value;
  let cursor = 0;
  let credit = 0;
  let previous = performance.now();
  const myGeneration = ++generation;
  streamButton.textContent = "■ Stop";
  setRuntimeState("STREAMING");

  const frame = now => {
    if (myGeneration !== generation) return;
    const elapsed = Math.min((now - previous) / 1000, .1);
    previous = now;
    credit += elapsed * Number(speed.value);
    const count = Math.min(Math.floor(credit), 4096, text.length - cursor);
    if (count > 0) {
      appendChunk(text.slice(cursor, cursor + count));
      cursor += count;
      credit -= count;
      preview.scrollTop = preview.scrollHeight;
    }
    if (cursor < text.length) {
      animation = requestAnimationFrame(frame);
      return;
    }
    const start = performance.now();
    renderOperations(parser.finish());
    parseMs.textContent = (performance.now() - start).toFixed(3);
    animation = 0;
    streamButton.textContent = "▶ Stream";
    setRuntimeState("LIVE");
  };
  animation = requestAnimationFrame(frame);
}

streamButton.addEventListener("click", () => {
  if (animation) {
    cancelStreaming();
    setRuntimeState("PAUSED");
  } else streamAll();
});
renderNowButton.addEventListener("click", renderAll);
resetButton.addEventListener("click", () => {
  cancelRemoteStream();
  cancelStreaming();
  source.value = DEMO;
  renderAll();
});

remoteForm.addEventListener("submit", async event => {
  event.preventDefault();
  if (remoteController) {
    cancelRemoteStream();
    setRuntimeState("PAUSED");
    return;
  }

  let url;
  try {
    url = new URL(streamUrl.value.trim(), location.href);
    if (url.protocol !== "http:" && url.protocol !== "https:") throw new Error("HTTP(S) URL required");
  } catch (error) {
    setRuntimeState("BAD URL");
    deltaStatus.textContent = String(error.message || error);
    return;
  }

  cancelStreaming();
  resetRuntime();
  const controller = new AbortController();
  remoteController = controller;
  connectStreamButton.textContent = "■ Stop";
  setRuntimeState("CONNECTING");
  deltaStatus.textContent = `Connecting to ${url.host}…`;
  let received = "";

  try {
    const requested = streamFormat.value;
    const accept = requested === "sse" ? "text/event-stream"
      : requested === "ndjson" ? "application/x-ndjson"
      : "text/plain, text/event-stream, application/x-ndjson;q=0.9, */*;q=0.5";
    const request = buildLlmRequest({
      protocol: requestProtocol.value,
      prompt: streamPrompt.value,
      model: streamModel.value,
    });
    const response = await fetch(url, {
      ...request,
      credentials: "omit",
      cache: "no-store",
      headers: { ...request.headers, Accept: accept },
      signal: controller.signal,
    });
    setRuntimeState("REMOTE STREAM");
    const result = await consumeHttpResponse(response, {
      format: requested,
      signal: controller.signal,
      onText(text) {
        received += text;
        appendChunk(text);
        preview.scrollTop = preview.scrollHeight;
      },
    });
    renderOperations(parser.finish());
    source.value = received;
    setRuntimeState("LIVE");
    deltaStatus.textContent = `${result.format.toUpperCase()} · ${result.chunks} chunks · ${result.chars} chars`;
  } catch (error) {
    if (controller.signal.aborted || error?.name === "AbortError") {
      setRuntimeState("PAUSED");
      deltaStatus.textContent = "Remote stream stopped";
    } else {
      setRuntimeState("ERROR");
      deltaStatus.textContent = `Remote stream failed: ${error.message || error}`;
      console.error(error);
    }
  } finally {
    if (remoteController === controller) remoteController = null;
    connectStreamButton.textContent = "Connect";
  }
});

async function runRemoteSmoke() {
  const requested = new URLSearchParams(location.search).get("remote_smoke");
  if (requested !== "1") return;
  const root = document.documentElement;
  root.dataset.remoteSmoke = "waiting";
  streamUrl.value = new URL("./fixtures/demo.sse", location.href).href;
  streamFormat.value = "sse";
  remoteForm.requestSubmit();
  for (let attempt = 0; attempt < 200; attempt++) {
    await new Promise(resolve => setTimeout(resolve, 20));
    if (streamState.textContent === "ERROR" || streamState.textContent === "BAD URL") break;
    if (!remoteController && streamState.textContent === "LIVE") {
      const card = preview.querySelector('[data-ui-id="remote"]');
      const value = card?.querySelector(".metric-value")?.textContent?.trim();
      root.dataset.remoteSmoke = value === "REMOTE" ? "pass" : "fail";
      return;
    }
  }
  root.dataset.remoteSmoke = "fail";
}

async function runInteractionSmoke() {
  if (new URLSearchParams(location.search).get("interaction_smoke") !== "1") return;
  const root = document.documentElement;
  root.dataset.interactionSmoke = "waiting";
  streamUrl.value = new URL("/v1/chat/completions", location.origin).href;
  requestProtocol.value = "chat";
  streamFormat.value = "sse";
  streamModel.value = "fixture-model";
  requestProtocol.dispatchEvent(new Event("change", { bubbles: true }));

  // These keys must never leave the browser through action=llm.
  updateState("password", "browser-only-password");
  updateState("api_token", "browser-only-token");

  const button = preview.querySelector('[data-ui-id="ask-model"] button');
  if (!button) {
    root.dataset.interactionSmoke = "fail";
    return;
  }
  button.click();
  for (let attempt = 0; attempt < 300; attempt++) {
    await new Promise(resolve => setTimeout(resolve, 20));
    if (streamState.textContent === "ERROR" || streamState.textContent === "BAD URL") break;
    if (!remoteController && streamState.textContent === "LIVE") {
      const card = preview.querySelector('[data-ui-id="interaction-result"]');
      const value = card?.querySelector(".metric-value")?.textContent?.trim();
      const unit = card?.querySelector(".metric-unit")?.textContent?.trim();
      const appended = source.value.includes("## Model continuation");
      root.dataset.interactionSmoke = value === "42" && unit === "°C" && appended ? "pass" : "fail";
      return;
    }
  }
  root.dataset.interactionSmoke = "fail";
}

async function runLlmPostSmoke() {
  if (new URLSearchParams(location.search).get("llm_smoke") !== "1") return;
  const root = document.documentElement;
  root.dataset.llmSmoke = "waiting";
  streamUrl.value = new URL("/v1/chat/completions", location.origin).href;
  requestProtocol.value = "chat";
  streamFormat.value = "sse";
  streamModel.value = "fixture-model";
  streamPrompt.value = "Build the POST smoke dashboard";
  requestProtocol.dispatchEvent(new Event("change", { bubbles: true }));
  remoteForm.requestSubmit();
  for (let attempt = 0; attempt < 250; attempt++) {
    await new Promise(resolve => setTimeout(resolve, 20));
    if (streamState.textContent === "ERROR" || streamState.textContent === "BAD URL") break;
    if (!remoteController && streamState.textContent === "LIVE") {
      const card = preview.querySelector('[data-ui-id="post-remote"]');
      const value = card?.querySelector(".metric-value")?.textContent?.trim();
      const unit = card?.querySelector(".metric-unit")?.textContent?.trim();
      root.dataset.llmSmoke = value === "POST" && unit === "SSE" ? "pass" : "fail";
      return;
    }
  }
  root.dataset.llmSmoke = "fail";
}

function runGenerativeSmoke() {
  if (new URLSearchParams(location.search).get("smoke") !== "1") return;
  const root = document.documentElement;
  root.dataset.generativeSmoke = "waiting";
  try {
    const derived = preview.querySelector('[data-ui-id="fahrenheit"] [data-state-value="fahrenheit"]');
    const initialDerived = derived?.textContent;
    const warning = preview.querySelector('[data-ui-id="warning"]');
    const controlsTab = preview.querySelector('[data-tab-value="controls"]');
    controlsTab?.click();
    const controlsPanel = preview.querySelector('[data-tab-panel="controls"]');

    const form = preview.querySelector('form.generated-form');
    form?.requestSubmit();
    const committed = preview.querySelector('[data-ui-id="committed"]');

    const slider = preview.querySelector('input[data-state-input="temperature"]');
    if (slider) {
      slider.value = "65";
      slider.dispatchEvent(new Event("input", { bubbles: true }));
    }

    const passed = initialDerived === "107.6"
      && controlsPanel?.hidden === false
      && committed?.hidden === false
      && warning?.hidden === false;
    root.dataset.generativeSmoke = passed ? "pass" : "fail";
  } catch (error) {
    root.dataset.generativeSmoke = "error";
    console.error("Generative UI smoke failed", error);
  }
}

window.addEventListener("resize", () => {
  for (const element of document.querySelectorAll(".ui-card canvas")) {
    const index = blockElements.findIndex(block => block.contains(element));
    const block = parser.document[index];
    const config = block && uiConfig(block);
    if (config?.type === "chart") {
      const values = String(config.values || "").split(",").map(Number).filter(Number.isFinite);
      drawChart(element, values);
    } else if (config?.type === "canvas") {
      drawCanvasScene(element, canvasSpec(config), parseCanvasScene(block.value));
    }
  }
});

try {
  parser = await Streamdown.load(await fetch("./streamdown.wasm"));
  setRuntimeState("READY");
  renderAll();
  runGenerativeSmoke();
  runRemoteSmoke();
  runLlmPostSmoke();
  runInteractionSmoke();
} catch (error) {
  setRuntimeState("ERROR");
  preview.textContent = `Could not start Streamdown WASM: ${error}`;
  console.error(error);
}
