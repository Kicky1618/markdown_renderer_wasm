import { Streamdown } from "../js/streamdown.js";
import { buildSemanticGraph, graphDiagnostics } from "./semantic-graph.mjs";
import { createTimelineState, observeSemanticState, semanticReferencesFromLinks } from "./semantic-timeline-core.mjs";
import { SemanticScheduler } from "./semantic-scheduler.mjs";

function utf8Length(text) {
  return new TextEncoder().encode(text).length;
}

function parserSummary(parser) {
  return {
    llmBlocks: parser.getLlmBlocks(),
    semanticReferences: semanticReferencesFromLinks(parser.getLinks()),
  };
}

/**
 * Streaming semantic runtime layered on top of Streamdown.
 *
 * Parsing never waits for runners. Once a semantic block closes and its
 * syntactic dependencies are ready, the scheduler may execute it concurrently
 * with later Markdown still arriving. Runtime dependencies still require
 * successful completion before downstream execution starts.
 */
export class SemanticRuntime {
  static async load(wasmSource, options = {}) {
    const parser = await Streamdown.load(wasmSource);
    return new SemanticRuntime(parser, options);
  }

  constructor(parser, {
    concurrency = 4,
    runners = {},
    onTransition = null,
    onSemanticEvent = null,
  } = {}) {
    this.parser = parser;
    this.timelineState = createTimelineState();
    this.scheduler = new SemanticScheduler({ concurrency, runners, onTransition });
    this.onSemanticEvent = onSemanticEvent;
    this.observedAtByte = 0;
    this.chunkIndex = 0;
    this.graph = buildSemanticGraph(parserSummary(parser));
    this.scheduler.updateGraph(this.graph);
    this.disposed = false;
  }

  append(chunk) {
    this.#assertActive();
    if (typeof chunk !== "string") throw new TypeError("append() expects a string");
    this.parser.appendInPlace(chunk);
    this.observedAtByte += utf8Length(chunk);
    const observation = this.#observe();
    this.chunkIndex += 1;
    return observation.events;
  }

  async consume(source, { finalize = true } = {}) {
    this.#assertActive();
    const decoder = new TextDecoder();
    let decodingBytes = false;
    const feed = (chunk) => {
      if (typeof chunk === "string") {
        if (decodingBytes) {
          const tail = decoder.decode();
          if (tail) this.append(tail);
          decodingBytes = false;
        }
        if (chunk) this.append(chunk);
        return;
      }
      const bytes = chunk instanceof ArrayBuffer
        ? new Uint8Array(chunk)
        : ArrayBuffer.isView(chunk)
          ? new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength)
          : null;
      if (!bytes) throw new TypeError("consume() chunks must be strings, ArrayBuffers, or typed arrays");
      const text = decoder.decode(bytes, { stream: true });
      decodingBytes = true;
      if (text) this.append(text);
    };

    if (typeof source === "string" || source instanceof ArrayBuffer || ArrayBuffer.isView(source)) {
      feed(source);
    } else if (source?.[Symbol.asyncIterator]) {
      for await (const chunk of source) feed(chunk);
    } else if (source?.[Symbol.iterator]) {
      for (const chunk of source) feed(chunk);
    } else {
      throw new TypeError("consume() expects a string, bytes, or iterable");
    }

    if (decodingBytes) {
      const tail = decoder.decode();
      if (tail) this.append(tail);
    }
    return finalize ? this.finish() : this.snapshot();
  }

  async finish() {
    this.#assertActive();
    this.parser.finish();
    this.#observe();
    await this.scheduler.idle();
    return this.snapshot();
  }

  async idle() {
    this.#assertActive();
    await this.scheduler.idle();
    return this.snapshot();
  }

  snapshot() {
    this.#assertActive();
    const summary = parserSummary(this.parser);
    const graph = buildSemanticGraph(summary);
    return {
      document: this.parser.snapshot(),
      graph,
      diagnostics: graphDiagnostics(graph),
      scheduler: this.scheduler.snapshot(),
      blockCount: this.parser.blockCount,
    };
  }

  dispose() {
    if (this.disposed) return;
    this.parser.dispose();
    this.disposed = true;
  }

  #observe() {
    const summary = parserSummary(this.parser);
    const observation = observeSemanticState(summary, this.timelineState, this.observedAtByte, this.chunkIndex);
    this.graph = observation.graph;
    this.scheduler.updateGraph(this.graph);
    for (const event of observation.events) {
      this.onSemanticEvent?.(event);
      this.scheduler.accept(event);
    }
    return observation;
  }

  #assertActive() {
    if (this.disposed) throw new Error("semantic runtime has been disposed");
  }
}
