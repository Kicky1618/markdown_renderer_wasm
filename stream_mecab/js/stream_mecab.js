const encoder = new TextEncoder();
const decoder = new TextDecoder();

function readU32(view, offset) {
  return view.getUint32(offset, true);
}

function readI32(view, offset) {
  return view.getInt32(offset, true);
}

function readU64Number(view, offset) {
  const value = view.getBigUint64(offset, true);
  if (value > BigInt(Number.MAX_SAFE_INTEGER)) {
    throw new RangeError("stream-mecab token offset exceeds JavaScript's safe integer range");
  }
  return Number(value);
}

export function decodeDelta(bytes) {
  if (bytes.length < 12 || bytes[0] !== 0x53 || bytes[1] !== 0x4d || bytes[2] !== 0x54 || bytes[3] !== 0x31) {
    throw new Error("invalid SMT1 delta");
  }
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const retract = readU32(view, 4);
  const count = readU32(view, 8);
  let p = 12;
  const push = new Array(count);
  const readString = () => {
    if (p + 4 > bytes.length) throw new Error("truncated SMT1 string length");
    const length = readU32(view, p);
    p += 4;
    if (p + length > bytes.length) throw new Error("truncated SMT1 string");
    const value = decoder.decode(bytes.subarray(p, p + length));
    p += length;
    return value;
  };
  for (let i = 0; i < count; i++) {
    if (p + 24 > bytes.length) throw new Error("truncated SMT1 token");
    const start = readU64Number(view, p); p += 8;
    const end = readU64Number(view, p); p += 8;
    const tag = view.getUint16(p, true); p += 2;
    const origin = bytes[p++] === 0 ? "lexicon" : "unknown";
    p += 1;
    const wordCost = readI32(view, p); p += 4;
    push[i] = {
      start,
      end,
      tag,
      origin,
      wordCost,
      surface: readString(),
      lemma: readString(),
      reading: readString(),
    };
  }
  if (p !== bytes.length) throw new Error("trailing bytes in SMT1 delta");
  return { retract, push };
}

export function decodeSurfaceDelta(bytes) {
  if (bytes.length < 12 || bytes[0] !== 0x53 || bytes[1] !== 0x4d || bytes[2] !== 0x54 || bytes[3] !== 0x31) {
    throw new Error("invalid SMT1 delta");
  }
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const retract = readU32(view, 4);
  const count = readU32(view, 8);
  let p = 12;
  const push = new Array(count);
  const skipString = () => {
    if (p + 4 > bytes.length) throw new Error("truncated SMT1 string length");
    const length = readU32(view, p);
    p += 4;
    if (p + length > bytes.length) throw new Error("truncated SMT1 string");
    const start = p;
    p += length;
    return [start, length];
  };
  for (let i = 0; i < count; i++) {
    if (p + 24 > bytes.length) throw new Error("truncated SMT1 token");
    p += 24; // start/end/tag/origin/padding/wordCost
    const [surfaceStart, surfaceLength] = skipString();
    push[i] = decoder.decode(bytes.subarray(surfaceStart, surfaceStart + surfaceLength));
    skipString(); // lemma
    skipString(); // reading
  }
  if (p !== bytes.length) throw new Error("trailing bytes in SMT1 delta");
  return { retract, push };
}

export function applyDelta(tokens, delta) {
  tokens.length = Math.max(0, tokens.length - delta.retract);
  tokens.push(...delta.push);
  return tokens;
}

export class StreamMecab {
  constructor(exports) {
    this.exports = exports;
    this.handle = exports.sm_create();
    if (!this.handle) throw new Error("sm_create failed");
  }

  static async instantiate(bytesOrModule) {
    const result = await WebAssembly.instantiate(bytesOrModule, {});
    const instance = result instanceof WebAssembly.Instance ? result : result.instance;
    return new StreamMecab(instance.exports);
  }

  destroy() {
    if (this.handle) {
      this.exports.sm_destroy(this.handle);
      this.handle = 0;
    }
  }

  #error() {
    const length = this.exports.sm_error_len(this.handle);
    if (!length) return "stream-mecab wasm error";
    const ptr = this.exports.sm_error_ptr(this.handle);
    return decoder.decode(new Uint8Array(this.exports.memory.buffer, ptr, length));
  }

  #write(text) {
    const capacity = Math.max(1, text.length * 3);
    const ptr = this.exports.sm_input_reserve(this.handle, capacity);
    if (!ptr) throw new Error("sm_input_reserve failed");
    const target = new Uint8Array(this.exports.memory.buffer, ptr, capacity);
    const { read, written } = encoder.encodeInto(text, target);
    if (read !== text.length) throw new Error("UTF-8 staging buffer was too small");
    return written;
  }

  #writeBytes(bytes) {
    const source = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
    const capacity = Math.max(1, source.byteLength);
    const ptr = this.exports.sm_input_reserve(this.handle, capacity);
    if (!ptr) throw new Error("sm_input_reserve failed");
    new Uint8Array(this.exports.memory.buffer, ptr, source.byteLength).set(source);
    return source.byteLength;
  }

  #deltaBytes() {
    const ptr = this.exports.sm_delta_ptr(this.handle);
    const length = this.exports.sm_delta_len(this.handle);
    return new Uint8Array(this.exports.memory.buffer, ptr, length);
  }

  #delta() {
    return decodeDelta(this.#deltaBytes());
  }

  #surfaceDelta() {
    return decodeSurfaceDelta(this.#deltaBytes());
  }

  addTsv(tsv) {
    const length = this.#write(tsv);
    if (!this.exports.sm_add_tsv_input(this.handle, length)) throw new Error(this.#error());
    return this;
  }

  addTransitionTsv(tsv) {
    const length = this.#write(tsv);
    if (!this.exports.sm_add_transition_tsv_input(this.handle, length)) throw new Error(this.#error());
    return this;
  }

  loadCompiled(bytes) {
    const length = this.#writeBytes(bytes);
    if (!this.exports.sm_load_compiled_input(this.handle, length)) throw new Error(this.#error());
    return this;
  }

  setTransition(previous, next, cost) {
    if (!this.exports.sm_set_transition(this.handle, previous, next, cost)) throw new Error(this.#error());
    return this;
  }

  setMaxUnknownChars(chars) {
    if (!this.exports.sm_set_max_unknown_chars(this.handle, chars)) throw new Error(this.#error());
    return this;
  }

  start() {
    if (!this.exports.sm_start(this.handle)) throw new Error(this.#error());
    return this.#delta();
  }

  append(text) {
    const length = this.#write(text);
    if (!this.exports.sm_append_input(this.handle, length)) throw new Error(this.#error());
    return this.#delta();
  }

  appendSurfaces(text) {
    const length = this.#write(text);
    if (!this.exports.sm_append_input(this.handle, length)) throw new Error(this.#error());
    return this.#surfaceDelta();
  }

  finish() {
    if (!this.exports.sm_finish(this.handle)) throw new Error(this.#error());
    return this.#delta();
  }
}
