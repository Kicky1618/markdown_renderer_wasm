import { Streamdown, parseLlmDescriptor } from "./streamdown.js";
import { componentSpan, layoutSpec } from "./layout.js";
import { canvasSpec, parseCanvasScene } from "./canvas.js";

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

let parser;
let blockElements = [];
let animation = 0;
let generation = 0;
let layoutComposed = false;
const state = new Map();

source.value = DEMO;
speed.addEventListener("input", () => {
  speedLabel.value = `${speed.value} chars/s`;
});

function setRuntimeState(value) {
  streamState.textContent = value;
}

function cancelStreaming() {
  generation += 1;
  if (animation) cancelAnimationFrame(animation);
  animation = 0;
  streamButton.textContent = "▶ Stream";
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
  if (!state.has(key)) updateState(key, number(config.value, config.value ?? 0));
  return key;
}

function updateState(key, value) {
  state.set(key, value);
  for (const element of document.querySelectorAll("[data-state-value]")) {
    if (element.dataset.stateValue === key) element.textContent = `${value}${element.dataset.stateUnit || ""}`;
  }
  for (const input of document.querySelectorAll("input[data-state-input]")) {
    if (input.dataset.stateInput === key && document.activeElement !== input) input.value = String(value);
  }
  for (const track of document.querySelectorAll("[data-state-progress]")) {
    if (track.dataset.stateProgress !== key) continue;
    const min = number(track.dataset.progressMin, 0);
    const max = Math.max(min + 1, number(track.dataset.progressMax, 100));
    const ratio = Math.max(0, Math.min(1, (number(value, min) - min) / (max - min)));
    const fill = track.querySelector(".progress-fill");
    if (fill) fill.style.width = `${ratio * 100}%`;
  }
}

function makeUiShell(block, config) {
  const card = document.createElement("section");
  card.className = `ui-card${block.closed ? "" : " partial"}`;
  card.dataset.uiId = config.id || "";
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

function executeAction(action) {
  const [verb, key, raw] = String(action || "").split(":");
  if (!key) return;
  if (verb === "increment") updateState(key, number(state.get(key), 0) + number(raw, 1));
  else if (verb === "decrement") updateState(key, number(state.get(key), 0) - number(raw, 1));
  else if (verb === "set") updateState(key, number(raw, raw ?? ""));
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
  button.addEventListener("click", () => executeAction(config.action));
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

function renderLayoutMarker() {
  const marker = document.createElement("span");
  marker.hidden = true;
  marker.dataset.layoutMarker = "true";
  return marker;
}

function renderUi(block, config) {
  switch (config.type) {
    case "layout": return renderLayoutMarker();
    case "metric": return renderMetric(block, config);
    case "slider": return renderSlider(block, config);
    case "progress": return renderProgress(block, config);
    case "button": return renderButton(block, config);
    case "chart": return renderChart(block, config);
    case "canvas": return renderCanvas(block, config);
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

function isLayoutBlock(block) {
  return uiConfig(block)?.type === "layout";
}

function composeLayouts() {
  const fragment = document.createDocumentFragment();
  let grid = null;
  let gridSpec = null;

  const closeGrid = () => {
    if (!grid) return;
    fragment.append(grid);
    grid = null;
    gridSpec = null;
  };

  for (let index = 0; index < parser.document.length; index++) {
    const block = parser.document[index];
    const element = blockElements[index];
    const config = uiConfig(block);

    if (config?.type === "layout") {
      closeGrid();
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

    if (grid && config) {
      element.style.setProperty("--component-span", String(componentSpan(config, gridSpec.columns)));
      grid.append(element);
      continue;
    }

    closeGrid();
    if (element) {
      element.style?.removeProperty("--component-span");
      fragment.append(element);
    }
  }
  closeGrid();
  preview.replaceChildren(fragment);
  layoutComposed = hasLayout();
  for (const [key, value] of state) updateState(key, value);
}

function hasLayout() {
  return parser.document.some(isLayoutBlock);
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
  else if (!layoutComposed && !hasLayout()) preview.append(next);
  blockElements[index] = next;
}

function renderOperations(ops) {
  const start = performance.now();
  let structuralLayoutChange = false;
  for (const op of ops) {
    if (op.op === "truncate") {
      structuralLayoutChange ||= layoutComposed || hasLayout();
      truncateBlocks(op.from);
    } else if (op.op === "push") {
      const index = blockElements.length;
      const element = createBlock(parser.document[index]);
      blockElements.push(element);
      if (layoutComposed || hasLayout()) structuralLayoutChange = true;
      else preview.append(element);
    } else if (op.op === "spliceCode" || op.op === "sealCode" || op.op === "appendText") {
      structuralLayoutChange ||= isLayoutBlock(parser.document[op.block]);
      replaceBlock(op.block);
    }
  }
  if (structuralLayoutChange || (hasLayout() && !layoutComposed)) composeLayouts();
  renderMs.textContent = (performance.now() - start).toFixed(3);
  blockCount.textContent = String(parser.blockCount);
  deltaStatus.textContent = ops.length ? `${ops.map(op => op.op).join(" · ")}` : "No structural change";
}

function resetRuntime() {
  state.clear();
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
}

function renderAll() {
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
  cancelStreaming();
  source.value = DEMO;
  renderAll();
});

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
} catch (error) {
  setRuntimeState("ERROR");
  preview.textContent = `Could not start Streamdown WASM: ${error}`;
  console.error(error);
}
