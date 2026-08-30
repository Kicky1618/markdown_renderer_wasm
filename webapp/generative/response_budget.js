const DEFAULTS = Object.freeze({
  maxChars: 2 * 1024 * 1024,
  maxChunks: 8192,
  maxSemanticBlocks: 256,
});

const SEMANTIC = /:::llm\s+ui\b/gi;
const TAIL_CHARS = 32;

export class ResponseBudgetError extends Error {
  constructor(code, message, usage) {
    super(message);
    this.name = "ResponseBudgetError";
    this.code = code;
    this.usage = usage;
  }
}

function bounded(value, fallback, min, max) {
  const number = Number(value);
  if (!Number.isFinite(number)) return fallback;
  return Math.max(min, Math.min(max, Math.floor(number)));
}

/**
 * Incrementally limits one network/model response before text enters WASM/DOM.
 * Semantic fence counting is boundary-safe across arbitrary transport chunks.
 */
export class ResponseBudget {
  constructor(options = {}) {
    this.maxChars = bounded(options.maxChars, DEFAULTS.maxChars, 1024, 16 * 1024 * 1024);
    this.maxChunks = bounded(options.maxChunks, DEFAULTS.maxChunks, 1, 65536);
    this.maxSemanticBlocks = bounded(options.maxSemanticBlocks, DEFAULTS.maxSemanticBlocks, 1, 4096);
    this.chars = 0;
    this.chunks = 0;
    this.semanticBlocks = 0;
    this.tail = "";
  }

  snapshot() {
    return {
      chars: this.chars,
      chunks: this.chunks,
      semanticBlocks: this.semanticBlocks,
      maxChars: this.maxChars,
      maxChunks: this.maxChunks,
      maxSemanticBlocks: this.maxSemanticBlocks,
    };
  }

  push(value) {
    const text = String(value ?? "");
    if (!text) return this.snapshot();

    const nextChunks = this.chunks + 1;
    const nextChars = this.chars + text.length;
    if (nextChunks > this.maxChunks) {
      throw new ResponseBudgetError("chunks", `Model response exceeded ${this.maxChunks} streamed chunks`, this.snapshot());
    }
    if (nextChars > this.maxChars) {
      throw new ResponseBudgetError("chars", `Model response exceeded ${this.maxChars.toLocaleString("en-US")} characters`, this.snapshot());
    }

    const previousTailLength = this.tail.length;
    const scan = this.tail + text;
    let addedSemantic = 0;
    SEMANTIC.lastIndex = 0;
    for (const match of scan.matchAll(SEMANTIC)) {
      // Ignore matches wholly contained in the previous tail; they were counted
      // during the preceding push. A match crossing the boundary is new.
      if ((match.index ?? 0) + match[0].length > previousTailLength) addedSemantic += 1;
    }
    const nextSemantic = this.semanticBlocks + addedSemantic;
    if (nextSemantic > this.maxSemanticBlocks) {
      throw new ResponseBudgetError("semantic", `Model response exceeded ${this.maxSemanticBlocks} semantic UI blocks`, this.snapshot());
    }

    this.chunks = nextChunks;
    this.chars = nextChars;
    this.semanticBlocks = nextSemantic;
    this.tail = scan.slice(-TAIL_CHARS);
    return this.snapshot();
  }
}

export const RESPONSE_BUDGET_DEFAULTS = DEFAULTS;
