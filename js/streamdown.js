const utf8 = new TextEncoder();
const utf8Decoder = new TextDecoder();

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
  }

  append(chunk) {
    this.#assertActive();
    if (typeof chunk !== "string") throw new TypeError("append() expects a string");

    let ok;
    if (this.exports.md_input_reserve && this.exports.md_append_input && utf8.encodeInto) {
      // Three bytes per UTF-16 code unit is a safe upper bound for UTF-8. Grow
      // geometrically so normal token streams reserve WASM input memory once.
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
      let written = 0;
      if (needed) {
        const encoded = utf8.encodeInto(chunk, this.inputView);
        if (encoded.read !== chunk.length) throw new Error("WASM input buffer was too small");
        written = encoded.written;
      }
      ok = this.exports.md_append_input(this.handle, written);
    } else {
      // Compatibility path for older Streamdown WASM binaries.
      const input = utf8.encode(chunk);
      const ptr = this.exports.md_alloc(input.length);
      if (input.length) new Uint8Array(this.exports.memory.buffer, ptr, input.length).set(input);
      ok = this.exports.md_append(this.handle, ptr, input.length);
      this.exports.md_free(ptr);
    }

    if (!ok) throw new Error("WASM parser rejected the input");
    const ops = this.#readDelta();
    applyDelta(this.document, ops);
    return ops;
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
        const operations = this.append(text);
        onDelta?.(operations, this.document);
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
        const operations = this.append(text);
        onDelta?.(operations, this.document);
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
        const operations = this.append(tail);
        onDelta?.(operations, this.document);
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

  #assertActive() {
    if (!this.handle) throw new Error("parser has been disposed");
  }

  #readDelta() {
    const outPtr = this.exports.md_delta_ptr(this.handle);
    const outLen = this.exports.md_delta_len(this.handle);
    // Decode synchronously before the next parser call mutates the reusable
    // output buffer. No copy is needed because decodeDelta returns owned JS data.
    const bytes = new Uint8Array(this.exports.memory.buffer, outPtr, outLen);
    return decodeDelta(bytes);
  }
}
