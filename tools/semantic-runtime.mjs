import { Streamdown } from "../js/streamdown.js";
import { buildSemanticGraph, graphDiagnostics } from "./semantic-graph.mjs";
import { IncrementalSemanticTimeline } from "./semantic-timeline-incremental.mjs";
import { SemanticScheduler } from "./semantic-scheduler.mjs";
import { SemanticChangeDetector } from "./semantic-detector.mjs";
import { SemanticRuntimeSummary } from "./semantic-runtime-summary.mjs";

const EMPTY_EVENTS = Object.freeze([]);

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
    semanticScan = "incremental",
  } = {}) {
    this.parser = parser;
    this.semanticTimeline = new IncrementalSemanticTimeline();
    this.timelineState = this.semanticTimeline;
    this.scheduler = new SemanticScheduler({ concurrency, runners, onTransition });
    this.onSemanticEvent = onSemanticEvent;
    if (semanticScan !== "incremental" && semanticScan !== "always") {
      throw new TypeError("semanticScan must be \"incremental\" or \"always\"");
    }
    this.semanticScan = semanticScan;
    this.semanticDetector = new SemanticChangeDetector();
    this.semanticScans = 0;
    this.observedAtByte = 0;
    this.chunkIndex = 0;
    this.semanticSummary = new SemanticRuntimeSummary(parser.document);
    this.graph = buildSemanticGraph(this.semanticSummary.current());
    this.scheduler.updateGraph(this.graph);
    this.disposed = false;
  }

  append(chunk) {
    this.#assertActive();
    if (typeof chunk !== "string") throw new TypeError("append() expects a string");
    return this.#appendText(chunk);
  }

  async consume(source, { finalize = true, snapshotOptions = undefined } = {}) {
    this.#assertActive();
    const decoder = new TextDecoder();
    let decodingBytes = false;
    const feed = (chunk) => {
      if (typeof chunk === "string") {
        if (decodingBytes) {
          const tail = decoder.decode();
          if (tail) this.#appendText(tail, 0);
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
      if (text) this.#appendText(text, bytes.byteLength);
      else this.observedAtByte += bytes.byteLength;
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
      if (tail) this.#appendText(tail, 0);
    }
    return finalize ? this.finish(snapshotOptions) : this.snapshot(snapshotOptions);
  }

  async finish(snapshotOptions = undefined) {
    this.#assertActive();
    const previousBlockCount = this.parser.blockCount;
    this.parser.finish();
    const summary = this.semanticSummary.refreshTail(this.parser.document, previousBlockCount);
    this.#observe(summary);
    await this.scheduler.idle();
    return this.snapshot(snapshotOptions);
  }

  async idle(snapshotOptions = undefined) {
    this.#assertActive();
    await this.scheduler.idle();
    return this.snapshot(snapshotOptions);
  }

  snapshot({
    document = true,
    graph = true,
    diagnostics = true,
    scheduler = true,
  } = {}) {
    this.#assertActive();
    const output = {
      blockCount: this.parser.blockCount,
      semanticScans: this.semanticScans,
    };
    const graphSnapshot = graph || diagnostics
      ? buildSemanticGraph(this.semanticSummary.current())
      : null;
    if (document) output.document = this.parser.snapshot();
    if (graph) output.graph = graphSnapshot;
    if (diagnostics) output.diagnostics = graphDiagnostics(graphSnapshot);
    if (scheduler) output.scheduler = this.scheduler.snapshot();
    return output;
  }

  dispose() {
    if (this.disposed) return;
    this.parser.dispose();
    this.disposed = true;
  }

  #appendText(chunk, knownUtf8Bytes = null) {
    const previousBlockCount = this.parser.blockCount;
    this.parser.appendInPlace(chunk);
    const semanticScan = this.semanticDetector.scan(chunk, knownUtf8Bytes);
    const detectorObserve = semanticScan < 0;
    this.observedAtByte += detectorObserve ? -semanticScan - 1 : semanticScan;
    const shouldObserve = this.semanticScan === "always" || detectorObserve;
    const events = shouldObserve
      ? this.#observe(this.semanticSummary.refreshTail(this.parser.document, previousBlockCount)).events
      : EMPTY_EVENTS;
    this.chunkIndex += 1;
    return events;
  }

  #observe(summary = this.semanticSummary.current()) {
    this.semanticScans += 1;
    const observation = this.semanticTimeline.observe(summary, this.observedAtByte, this.chunkIndex);
    for (const { node, dependencies } of observation.changedNodes) {
      this.scheduler.upsertNode(node, dependencies);
    }
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
