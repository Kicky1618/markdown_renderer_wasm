const utf8 = new TextEncoder();
const utf8Decoder = new TextDecoder();

function u32le(bytes, p) {
  return (bytes[p] | (bytes[p + 1] << 8) | (bytes[p + 2] << 16) | (bytes[p + 3] << 24)) >>> 0;
}

// Streaming normally emits exactly one tiny operation. Decode those directly
// without building the generic recursive decoder state on every LLM token.
function decodeHotDelta(bytes) {
  if (bytes.length < 9
      || bytes[0] !== 0x4d || bytes[1] !== 0x44 || bytes[2] !== 0x41 || bytes[3] !== 0x31
      || bytes[4] !== 1 || bytes[5] !== 0 || bytes[6] !== 0 || bytes[7] !== 0) {
    return null;
  }

  const tag = bytes[8];
  if (tag === 5 || tag === 6) {
    if (bytes.length < 17) return null;
    const length = u32le(bytes, 13);
    if (17 + length !== bytes.length) return null;
    return [{
      op: tag === 5 ? "appendText" : "appendInlineText",
      block: u32le(bytes, 9),
      append: utf8Decoder.decode(bytes.subarray(17)),
    }];
  }
  if (tag === 3) {
    if (bytes.length < 21) return null;
    const length = u32le(bytes, 17);
    if (21 + length !== bytes.length) return null;
    return [{
      op: "spliceCode",
      block: u32le(bytes, 9),
      truncateBytes: u32le(bytes, 13),
      append: utf8Decoder.decode(bytes.subarray(21)),
    }];
  }
  if (tag === 1 && bytes.length === 13) return [{ op: "truncate", from: u32le(bytes, 9) }];
  if (tag === 4 && bytes.length === 13) return [{ op: "sealCode", block: u32le(bytes, 9) }];
  return null;
}

function decodeHotParagraphPush(bytes) {
  if (bytes.length < 14
      || bytes[0] !== 0x4d || bytes[1] !== 0x44 || bytes[2] !== 0x41 || bytes[3] !== 0x31
      || bytes[4] !== 1 || bytes[5] !== 0 || bytes[6] !== 0 || bytes[7] !== 0
      || bytes[8] !== 2 || bytes[9] !== 1) {
    return null;
  }

  let p = 10;
  let valid = true;
  const readString = () => {
    if (p + 4 > bytes.length) { valid = false; return ""; }
    const length = u32le(bytes, p);
    p += 4;
    if (p + length > bytes.length) { valid = false; return ""; }
    const value = utf8Decoder.decode(bytes.subarray(p, p + length));
    p += length;
    return value;
  };
  const readInlines = () => {
    if (p + 4 > bytes.length) { valid = false; return []; }
    const count = u32le(bytes, p);
    p += 4;
    const nodes = new Array(count);
    for (let i = 0; i < count && valid; i++) {
      if (p >= bytes.length) { valid = false; break; }
      switch (bytes[p++]) {
        case 1: nodes[i] = { type: "text", value: readString() }; break;
        case 2: nodes[i] = { type: "emphasis", children: readInlines() }; break;
        case 3: nodes[i] = { type: "strong", children: readInlines() }; break;
        case 4: nodes[i] = { type: "code", value: readString() }; break;
        case 5: nodes[i] = { type: "link", children: readInlines(), destination: readString() }; break;
        case 6: nodes[i] = { type: "softBreak" }; break;
        case 7: nodes[i] = { type: "hardBreak" }; break;
        case 8: {
          if (p >= bytes.length) { valid = false; break; }
          nodes[i] = { type: "math", display: !!bytes[p++], value: readString() };
          break;
        }
        default: valid = false;
      }
    }
    return nodes;
  };

  const children = readInlines();
  if (!valid || p !== bytes.length) return null;
  return [{ op: "push", block: { type: "paragraph", children } }];
}

/** Decode the MDA1 binary format into lightweight JavaScript objects. */
export function decodeDelta(bytes) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let p = 0;
  const u8 = () => view.getUint8(p++);
  const u32 = () => { const n = view.getUint32(p, true); p += 4; return n; };
  const string = () => { const n = u32(); const s = utf8Decoder.decode(bytes.subarray(p, p + n)); p += n; return s; };
  const magic = String.fromCharCode(u8(), u8(), u8(), u8());
  if (magic !== "MDA1") throw new Error(`unsupported AST delta: ${magic}`);

  const inlines = () => Array.from({ length: u32() }, () => {
    switch (u8()) {
      case 1: return { type: "text", value: string() };
      case 2: return { type: "emphasis", children: inlines() };
      case 3: return { type: "strong", children: inlines() };
      case 4: return { type: "code", value: string() };
      case 5: return { type: "link", children: inlines(), destination: string() };
      case 6: return { type: "softBreak" };
      case 7: return { type: "hardBreak" };
      case 8: return { type: "math", display: !!u8(), value: string() };
      default: throw new Error("unknown inline node");
    }
  });
  const block = () => {
    switch (u8()) {
      case 1: return { type: "paragraph", children: inlines() };
      case 2: return { type: "heading", level: u8(), children: inlines() };
      case 3: return { type: "codeBlock", closed: !!u8(), language: string() || null, value: string() };
      case 4: return { type: "blockQuote", children: inlines() };
      case 5: return { type: "unorderedList", items: Array.from({ length: u32() }, inlines) };
      case 6: return { type: "orderedList", start: u32(), items: Array.from({ length: u32() }, inlines) };
      case 7: return { type: "thematicBreak" };
      case 8: {
        const readCells = () => Array.from({ length: u32() }, inlines);
        return { type: "table", headers: readCells(), rows: Array.from({ length: u32() }, readCells) };
      }
      default: throw new Error("unknown block node");
    }
  };
  const ops = Array.from({ length: u32() }, () => {
    switch (u8()) {
      case 1: return { op: "truncate", from: u32() };
      case 2: return { op: "push", block: block() };
      case 3: return { op: "spliceCode", block: u32(), truncateBytes: u32(), append: string() };
      case 4: return { op: "sealCode", block: u32() };
      case 5: return { op: "appendText", block: u32(), append: string() };
      case 6: return { op: "appendInlineText", block: u32(), append: string() };
      case 7: return { op: "spliceInlineTail", block: u32(), removeNodes: u32(), truncateBytes: u32(), append: inlines() };
      case 8: return { op: "appendListItem", block: u32(), item: inlines() };
      case 9: return { op: "spliceListItemTail", block: u32(), removeNodes: u32(), truncateBytes: u32(), append: inlines() };
      case 10: return { op: "spliceQuoteTail", block: u32(), removeNodes: u32(), truncateBytes: u32(), append: inlines() };
      case 11: return { op: "appendTableRow", block: u32(), row: Array.from({ length: u32() }, inlines) };
      case 12: return { op: "appendTableCell", block: u32(), cell: inlines() };
      case 13: return { op: "spliceTableCellTail", block: u32(), removeNodes: u32(), truncateBytes: u32(), append: inlines() };
      case 14: return { op: "spliceHeadingTail", block: u32(), removeNodes: u32(), truncateBytes: u32(), append: inlines() };
      default: throw new Error("unknown delta operation");
    }
  });
  if (p !== bytes.byteLength) throw new Error("trailing bytes in AST delta");
  return ops;
}

/** Apply operations in-place. DOM/rendering layers can mirror the same ops. */
export function applyDelta(document, ops) {
  for (const change of ops) {
    if (change.op === "truncate") document.length = change.from;
    else if (change.op === "push") document.push(change.block);
    else if (change.op === "sealCode") document[change.block].closed = true;
    else if (change.op === "appendText") {
      const node = document[change.block];
      if (node.type !== "paragraph" || node.children.length !== 1 || node.children[0].type !== "text") {
        throw new Error("appendText target is not a plain paragraph");
      }
      node.children[0].value += change.append;
    }
    else if (change.op === "appendInlineText") {
      const node = document[change.block];
      if (node.type !== "paragraph") throw new Error("appendInlineText target is not a paragraph");
      const tail = node.children[node.children.length - 1];
      if (tail?.type === "text") tail.value += change.append;
      else node.children.push({ type: "text", value: change.append });
    }
    else if (change.op === "spliceInlineTail") {
      const node = document[change.block];
      if (node.type !== "paragraph") throw new Error("spliceInlineTail target is not a paragraph");
      if (change.truncateBytes) {
        const tail = node.children[node.children.length - 1];
        if (tail?.type !== "text") throw new Error("spliceInlineTail target has no trailing text");
        tail.value = removeUtf8Tail(tail.value, change.truncateBytes);
        if (!tail.value) node.children.pop();
      }
      if (change.removeNodes) {
        if (change.removeNodes > node.children.length) throw new Error("spliceInlineTail removes too many nodes");
        node.children.length -= change.removeNodes;
      }
      for (const incoming of change.append) {
        const tail = node.children[node.children.length - 1];
        if (incoming.type === "text" && tail?.type === "text") tail.value += incoming.value;
        else node.children.push(incoming);
      }
    }
    else if (change.op === "appendListItem") {
      const node = document[change.block];
      if (node.type !== "unorderedList" && node.type !== "orderedList") throw new Error("appendListItem target is not a list");
      node.items.push(change.item);
    }
    else if (change.op === "appendTableRow") {
      const node = document[change.block];
      if (node.type !== "table") throw new Error("appendTableRow target is not a table");
      node.rows.push(change.row);
    }
    else if (change.op === "appendTableCell") {
      const node = document[change.block];
      if (node.type !== "table" || !node.rows.length) throw new Error("appendTableCell target has no row");
      node.rows[node.rows.length - 1].push(change.cell);
    }
    else if (change.op === "spliceTableCellTail") {
      const node = document[change.block];
      if (node.type !== "table" || !node.rows.length || !node.rows[node.rows.length - 1].length) throw new Error("spliceTableCellTail target has no cell");
      const cell = node.rows[node.rows.length - 1][node.rows[node.rows.length - 1].length - 1];
      if (change.truncateBytes) {
        const tail = cell[cell.length - 1];
        if (tail?.type !== "text") throw new Error("spliceTableCellTail target has no trailing text");
        tail.value = removeUtf8Tail(tail.value, change.truncateBytes);
        if (!tail.value) cell.pop();
      }
      if (change.removeNodes) cell.length -= change.removeNodes;
      for (const incoming of change.append) {
        const tail = cell[cell.length - 1];
        if (incoming.type === "text" && tail?.type === "text") tail.value += incoming.value;
        else cell.push(incoming);
      }
    }
    else if (change.op === "spliceQuoteTail") {
      const node = document[change.block];
      if (node.type !== "blockQuote") throw new Error("spliceQuoteTail target is not a block quote");
      const children = node.children;
      if (change.truncateBytes) {
        const tail = children[children.length - 1];
        if (tail?.type !== "text") throw new Error("spliceQuoteTail target has no trailing text");
        tail.value = removeUtf8Tail(tail.value, change.truncateBytes);
        if (!tail.value) children.pop();
      }
      if (change.removeNodes) {
        if (change.removeNodes > children.length) throw new Error("spliceQuoteTail removes too many nodes");
        children.length -= change.removeNodes;
      }
      for (const incoming of change.append) {
        const tail = children[children.length - 1];
        if (incoming.type === "text" && tail?.type === "text") tail.value += incoming.value;
        else children.push(incoming);
      }
    }
    else if (change.op === "spliceHeadingTail") {
      const node = document[change.block];
      if (node.type !== "heading") throw new Error("spliceHeadingTail target is not a heading");
      const children = node.children;
      if (change.truncateBytes) {
        const tail = children[children.length - 1];
        if (tail?.type !== "text") throw new Error("spliceHeadingTail target has no trailing text");
        tail.value = removeUtf8Tail(tail.value, change.truncateBytes);
        if (!tail.value) children.pop();
      }
      if (change.removeNodes) {
        if (change.removeNodes > children.length) throw new Error("spliceHeadingTail removes too many nodes");
        children.length -= change.removeNodes;
      }
      for (const incoming of change.append) {
        const tail = children[children.length - 1];
        if (incoming.type === "text" && tail?.type === "text") tail.value += incoming.value;
        else children.push(incoming);
      }
    }
    else if (change.op === "spliceListItemTail") {
      const node = document[change.block];
      if (node.type !== "unorderedList" && node.type !== "orderedList") throw new Error("spliceListItemTail target is not a list");
      const item = node.items[node.items.length - 1];
      if (!item) throw new Error("spliceListItemTail target has no final item");
      if (change.truncateBytes) {
        const tail = item[item.length - 1];
        if (tail?.type !== "text") throw new Error("spliceListItemTail target has no trailing text");
        tail.value = removeUtf8Tail(tail.value, change.truncateBytes);
        if (!tail.value) item.pop();
      }
      if (change.removeNodes) {
        if (change.removeNodes > item.length) throw new Error("spliceListItemTail removes too many nodes");
        item.length -= change.removeNodes;
      }
      for (const incoming of change.append) {
        const tail = item[item.length - 1];
        if (incoming.type === "text" && tail?.type === "text") tail.value += incoming.value;
        else item.push(incoming);
      }
    }
    else if (change.op === "spliceCode") {
      const node = document[change.block];
      node.value = removeUtf8Tail(node.value, change.truncateBytes) + change.append;
    }
  }
  return document;
}

// Walks only the removed tail (normally the current line), not the whole code block.
function removeUtf8Tail(value, bytes) {
  let i = value.length;
  while (bytes > 0 && i > 0) {
    const low = value.charCodeAt(i - 1);
    let cp;
    if (low >= 0xdc00 && low <= 0xdfff && i >= 2) { cp = value.codePointAt(i - 2); i -= 2; }
    else { cp = low; i -= 1; }
    bytes -= cp <= 0x7f ? 1 : cp <= 0x7ff ? 2 : cp <= 0xffff ? 3 : 4;
  }
  if (bytes !== 0) throw new Error("splice does not end on a UTF-8 boundary");
  return value.slice(0, i);
}

function inlineText(nodes) {
  let text = "";
  for (const node of nodes) {
    if (node.type === "text" || node.type === "code" || node.type === "math") text += node.value;
    else if (node.type === "softBreak") text += " ";
    else if (node.type === "hardBreak") text += "\n";
    else if (node.children) text += inlineText(node.children);
  }
  return text;
}

function blockText(block) {
  if (block.type === "codeBlock") return block.value;
  if (block.type === "thematicBreak") return "---";
  if (block.type === "unorderedList") return block.items.map((item) => `• ${inlineText(item)}`).join("\n");
  if (block.type === "orderedList") return block.items
    .map((item, index) => `${block.start + index}. ${inlineText(item)}`)
    .join("\n");
  if (block.type === "table") return [block.headers, ...block.rows]
    .map((row) => row.map(inlineText).join("\t"))
    .join("\n");
  return inlineText(block.children ?? []);
}

function clone(value) {
  return typeof structuredClone === "function"
    ? structuredClone(value)
    : JSON.parse(JSON.stringify(value));
}

function splitLlmWords(input) {
  const words = [];
  let token = "";
  let quote = "";
  let escaped = false;
  const flush = () => {
    if (token) words.push(token);
    token = "";
  };
  for (const char of input) {
    if (escaped) {
      token += char;
      escaped = false;
      continue;
    }
    if (char === "\\") {
      escaped = true;
      continue;
    }
    if (quote) {
      if (char === quote) quote = "";
      else token += char;
      continue;
    }
    if (char === '"' || char === "'") {
      quote = char;
      continue;
    }
    if (/\s/.test(char)) flush();
    else token += char;
  }
  if (escaped) token += "\\";
  flush();
  return words;
}

/** Parse the normalized language field emitted for a `:::llm` semantic fence. */
export function parseLlmDescriptor(language) {
  if (language === "llm") return { kind: "generic", attributes: Object.create(null) };
  if (typeof language !== "string" || !language.startsWith("llm:")) return null;
  const words = splitLlmWords(language.slice(4));
  const kind = words.shift() || "generic";
  const attributes = Object.create(null);
  for (const word of words) {
    const equals = word.indexOf("=");
    if (equals <= 0) continue;
    attributes[word.slice(0, equals)] = word.slice(equals + 1);
  }
  return { kind, attributes };
}

function throwIfAborted(signal) {
  if (!signal?.aborted) return;
  if (signal.reason !== undefined) throw signal.reason;
  const error = new Error("The stream was aborted");
  error.name = "AbortError";
  throw error;
}

export class Streamdown {
  static async load(source) {
    const result = source instanceof Response
      ? await WebAssembly.instantiateStreaming(source, {})
      : await WebAssembly.instantiate(source, {});
    return new Streamdown(result.instance ?? result);
  }

  constructor(instance) {
    this.instance = instance;
    this.exports = instance.exports;
    this.handle = this.exports.md_create();
    this.document = [];
    this.inputPtr = 0;
    this.inputCapacity = 0;
    this.inputMemory = null;
    this.inputView = null;
    this.outputMemory = null;
    this.outputView = null;
  }

  append(chunk) {
    this.#assertActive();
    if (typeof chunk !== "string") throw new TypeError("append() expects a string");
    this.#appendWasm(chunk);
    const ops = this.#readDelta();
    applyDelta(this.document, ops);
    return ops;
  }

  /**
   * Append while updating `document` in-place without materializing hot-path
   * delta objects. Falls back to the normal decoder for structural changes.
   */
  appendInPlace(chunk) {
    this.#assertActive();
    if (typeof chunk !== "string") throw new TypeError("appendInPlace() expects a string");
    const written = this.#appendWasm(chunk);
    if (!this.#applyHotDeltaInPlace(chunk, written)) {
      applyDelta(this.document, this.#readDelta());
    }
    return this.document;
  }

  /** Append several text chunks while preserving streaming parser semantics. */
  appendMany(chunks) {
    this.#assertActive();
    if (chunks == null || typeof chunks[Symbol.iterator] !== "function") {
      throw new TypeError("appendMany() expects an iterable of strings");
    }
    const operations = [];
    for (const chunk of chunks) operations.push(...this.append(chunk));
    return operations;
  }

  /** Reset the parser for the next assistant message. */
  reset() {
    this.#assertActive();
    if (!this.exports.md_reset(this.handle)) throw new Error("WASM parser could not be reset");
    const operations = this.#readDelta();
    applyDelta(this.document, operations);
    return operations;
  }

  /** Replace the current assistant message with a complete or partial value. */
  setContent(markdown) {
    if (typeof markdown !== "string") throw new TypeError("setContent() expects a string");
    const operations = this.reset();
    operations.push(...this.append(markdown));
    return operations;
  }

  /** Finalize the last streamed block (notably a code fence at EOF). */
  finish() {
    this.#assertActive();
    if (!this.exports.md_finish(this.handle)) throw new Error("WASM parser could not be finalized");
    const operations = this.#readDelta();
    applyDelta(this.document, operations);
    return operations;
  }

  /**
   * Consume a fetch Response, ReadableStream, async iterable, or iterable.
   * Binary chunks are decoded safely even when a UTF-8 character crosses chunks.
   */
  async consume(source, { signal, onDelta, finalize = true } = {}) {
    this.#assertActive();
    throwIfAborted(signal);
    if (typeof Response !== "undefined" && source instanceof Response) {
      if (!source.ok) throw new Error(`response failed with HTTP ${source.status}`);
      if (!source.body) {
        const text = await source.text();
        throwIfAborted(signal);
        if (onDelta) {
          const operations = this.append(text);
          onDelta(operations, this.document);
        } else {
          this.appendInPlace(text);
        }
        if (finalize) {
          const finalOperations = this.finish();
          if (finalOperations.length) onDelta?.(finalOperations, this.document);
        }
        return this.document;
      }
      source = source.body;
    }

    const decoder = new TextDecoder();
    let decodingBytes = false;
    const consumeChunk = (chunk) => {
      throwIfAborted(signal);
      let text;
      if (typeof chunk === "string") {
        text = (decodingBytes ? decoder.decode() : "") + chunk;
        decodingBytes = false;
      } else {
        const bytes = chunk instanceof ArrayBuffer
          ? new Uint8Array(chunk)
          : ArrayBuffer.isView(chunk)
            ? new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength)
            : null;
        if (!bytes) throw new TypeError("stream chunks must be strings, ArrayBuffers, or typed arrays");
        text = decoder.decode(bytes, { stream: true });
        decodingBytes = true;
      }
      if (text) {
        if (onDelta) {
          const operations = this.append(text);
          onDelta(operations, this.document);
        } else {
          this.appendInPlace(text);
        }
      }
    };

    if (typeof source === "string" || source instanceof ArrayBuffer || ArrayBuffer.isView(source)) {
      consumeChunk(source);
    } else if (source?.getReader) {
      const reader = source.getReader();
      const cancel = () => { reader.cancel(signal?.reason).catch(() => {}); };
      signal?.addEventListener("abort", cancel, { once: true });
      try {
        while (true) {
          throwIfAborted(signal);
          const { done, value } = await reader.read();
          if (done) break;
          consumeChunk(value);
        }
      } finally {
        signal?.removeEventListener("abort", cancel);
        reader.releaseLock();
      }
    } else if (source?.[Symbol.asyncIterator]) {
      for await (const chunk of source) consumeChunk(chunk);
    } else if (source?.[Symbol.iterator]) {
      for (const chunk of source) consumeChunk(chunk);
    } else {
      throw new TypeError("consume() expects a Response, stream, or iterable");
    }

    throwIfAborted(signal);
    if (decodingBytes) {
      const tail = decoder.decode();
      if (tail) {
        if (onDelta) {
          const operations = this.append(tail);
          onDelta(operations, this.document);
        } else {
          this.appendInPlace(tail);
        }
      }
    }
    if (finalize) {
      const operations = this.finish();
      if (operations.length) onDelta?.(operations, this.document);
    }
    throwIfAborted(signal);
    return this.document;
  }

  /** Return a detached AST suitable for state stores or message caches. */
  snapshot() {
    this.#assertActive();
    return clone(this.document);
  }

  /** Extract readable text from the current Markdown AST. */
  toPlainText() {
    this.#assertActive();
    return this.document.map(blockText).join("\n\n");
  }

  getCodeBlocks({ language, closed } = {}) {
    this.#assertActive();
    return this.document.flatMap((block, index) => {
      if (block.type !== "codeBlock") return [];
      if (language !== undefined && block.language !== language) return [];
      if (closed !== undefined && block.closed !== closed) return [];
      return [{ index, language: block.language, value: block.value, closed: block.closed }];
    });
  }

  /** Return structured LLM semantic fences without reparsing their streamed bodies. */
  getLlmBlocks({ kind, closed } = {}) {
    this.#assertActive();
    return this.document.flatMap((block, index) => {
      if (block.type !== "codeBlock") return [];
      const descriptor = parseLlmDescriptor(block.language);
      if (!descriptor) return [];
      if (kind !== undefined && descriptor.kind !== kind) return [];
      if (closed !== undefined && block.closed !== closed) return [];
      return [{
        index,
        kind: descriptor.kind,
        attributes: descriptor.attributes,
        value: block.value,
        closed: block.closed,
      }];
    });
  }

  getCitations() {
    this.#assertActive();
    const citations = [];
    const visit = (nodes, block) => {
      for (const node of nodes) {
        if (node.type === "link" && node.destination.startsWith("llm:cite:")) {
          citations.push({
            block,
            source: node.destination.slice("llm:cite:".length),
            label: inlineText(node.children),
          });
        }
        if (node.children) visit(node.children, block);
      }
    };
    this.document.forEach((block, index) => {
      if (block.children) visit(block.children, index);
      if (block.items) block.items.forEach((item) => visit(item, index));
      if (block.type === "table") [block.headers, ...block.rows].flat().forEach((cell) => visit(cell, index));
    });
    return citations;
  }

  getLinks() {
    this.#assertActive();
    const links = [];
    const visit = (nodes, block) => {
      for (const node of nodes) {
        if (node.type === "link") links.push({ block, text: inlineText(node.children), destination: node.destination });
        if (node.children) visit(node.children, block);
      }
    };
    this.document.forEach((block, index) => {
      if (block.children) visit(block.children, index);
      if (block.items) block.items.forEach((item) => visit(item, index));
      if (block.type === "table") [block.headers, ...block.rows].flat().forEach((cell) => visit(cell, index));
    });
    return links;
  }

  get blockCount() { return this.document.length; }
  get isEmpty() { return this.document.length === 0; }
  get isDisposed() { return !this.handle; }

  dispose() {
    if (this.handle) this.exports.md_destroy(this.handle);
    this.handle = 0;
    this.document.length = 0;
  }

  #appendWasm(chunk) {
    let ok;
    let written;
    if (this.exports.md_input_reserve && this.exports.md_append_input && utf8.encodeInto) {
      const needed = chunk.length * 3;
      if (needed > this.inputCapacity) {
        const capacity = Math.max(64, needed, this.inputCapacity * 2);
        this.inputPtr = this.exports.md_input_reserve(this.handle, capacity);
        this.inputCapacity = capacity;
        this.inputMemory = null;
        this.inputView = null;
      }
      const memory = this.exports.memory.buffer;
      if (this.inputCapacity && this.inputMemory !== memory) {
        this.inputMemory = memory;
        this.inputView = new Uint8Array(memory, this.inputPtr, this.inputCapacity);
      }
      written = 0;
      if (needed) {
        const encoded = utf8.encodeInto(chunk, this.inputView);
        if (encoded.read !== chunk.length) throw new Error("WASM input buffer was too small");
        written = encoded.written;
      }
      ok = this.exports.md_append_input(this.handle, written);
    } else {
      const input = utf8.encode(chunk);
      const ptr = this.exports.md_alloc(input.length);
      if (input.length) new Uint8Array(this.exports.memory.buffer, ptr, input.length).set(input);
      ok = this.exports.md_append(this.handle, ptr, input.length);
      this.exports.md_free(ptr);
      written = input.length;
    }
    if (!ok) throw new Error("WASM parser rejected the input");
    return written;
  }

  #applyHotDeltaInPlace(chunk, written) {
    const outPtr = this.exports.md_delta_ptr(this.handle);
    const outLen = this.exports.md_delta_len(this.handle);
    if (outLen < 9) return false;

    const memory = this.exports.memory.buffer;
    if (this.outputMemory !== memory) {
      this.outputMemory = memory;
      this.outputView = new DataView(memory);
    }
    const view = this.outputView;
    if (view.getUint32(outPtr, true) !== 0x3141444d || view.getUint32(outPtr + 4, true) !== 1) {
      return false;
    }

    const tag = view.getUint8(outPtr + 8);
    // Reuse the original JS string only for ASCII. For non-ASCII, the Rust
    // input may contain U+FFFD normalization for malformed UTF-16, so fall back
    // to the canonical MDA1 decoder.
    if (written !== chunk.length) return false;

    if (tag === 5 || tag === 6) {
      if (outLen < 17 || view.getUint32(outPtr + 13, true) !== written || outLen !== 17 + written) {
        return false;
      }
      const block = view.getUint32(outPtr + 9, true);
      const node = this.document[block];
      if (node?.type !== "paragraph") return false;
      if (tag === 5) {
        if (node.children.length !== 1 || node.children[0].type !== "text") return false;
        node.children[0].value += chunk;
      } else {
        const tail = node.children[node.children.length - 1];
        if (tail?.type === "text") tail.value += chunk;
        else node.children.push({ type: "text", value: chunk });
      }
      return true;
    }

    if (tag === 3) {
      if (outLen < 21
          || view.getUint32(outPtr + 13, true) !== 0
          || view.getUint32(outPtr + 17, true) !== written
          || outLen !== 21 + written) {
        return false;
      }
      const block = view.getUint32(outPtr + 9, true);
      const node = this.document[block];
      if (node?.type !== "codeBlock") return false;
      node.value += chunk;
      return true;
    }
    return false;
  }

  #assertActive() {
    if (!this.handle) throw new Error("parser has been disposed");
  }

  #readDelta() {
    const outPtr = this.exports.md_delta_ptr(this.handle);
    const outLen = this.exports.md_delta_len(this.handle);
    // Decode synchronously before the next parser call mutates the reusable
    // output buffer. No copy is needed because decodeDelta returns owned JS data.
    const bytes = new Uint8Array(this.exports.memory.buffer, outPtr, outLen);
    return decodeHotDelta(bytes) ?? decodeHotParagraphPush(bytes) ?? decodeDelta(bytes);
  }
}
