const utf8Encoder = new TextEncoder();

function compactLinePrefix(line) {
  const trimmed = line.trimStart();
  if (trimmed === "") return "";

  let colons = 0;
  while (colons < trimmed.length && trimmed.charCodeAt(colons) === 58) colons += 1;
  const tail = trimmed.slice(colons);
  if (colons >= 3 && tail.trim() === "") return ":::";
  if (colons > 3) return `:::${tail}`;
  return trimmed;
}

function classifyLinePrefix(line) {
  if (line === "") return { retain: true, confirmed: false };

  let colons = 0;
  while (colons < line.length && line.charCodeAt(colons) === 58) colons += 1;
  if (colons === 0) return { retain: false, confirmed: false };
  if (colons < 3) {
    return colons === line.length
      ? { retain: true, confirmed: false }
      : { retain: false, confirmed: false };
  }

  const tail = line.slice(colons);
  if (tail === "" || tail === "l" || tail === "ll") {
    return { retain: true, confirmed: false };
  }
  if (tail === "llm" || /^llm\s/.test(tail)) {
    return { retain: false, confirmed: true };
  }
  return { retain: false, confirmed: false };
}

function asciiTrimWhitespace(code) {
  return code <= 0x20;
}

/**
 * Cheap conservative detector for chunks that can change Streamdown's
 * semantic layer. `scan()` packs the observation flag and exact UTF-8 byte
 * length into one number, avoiding a per-token result allocation on the
 * SemanticRuntime hot path. Non-negative means no observation; a negative
 * value encodes `-(utf8Bytes + 1)` and means the semantic layer must observe.
 *
 * Outside a semantic fence we retain only a bounded `:{3,}llm` line prefix.
 * Once a semantic fence opens, its payload uses a constant-size closing-line
 * state machine instead of slicing/trimming every payload line. Inline
 * `@[kind:id]` references are ignored inside fenced payloads, matching the
 * Markdown parser, and are immediate observation triggers outside fences.
 */
export class SemanticChangeDetector {
  constructor() {
    this.linePrefix = "";
    this.headerLine = false;
    this.pendingCR = false;
    this.insideSemanticFence = false;
    this.fenceLineState = 0;
    this.fenceColonCount = 0;
  }

  scan(chunk) {
    if (typeof chunk !== "string") throw new TypeError("semantic detector expects a string");

    // Dominant LLM token path: once an ordinary line has been proven inert,
    // only a newline/CR or closing `]` can make the semantic layer relevant.
    // Scan once for those bytes and UTF-8 width, then return without touching
    // any of the line/fence state below.
    if (!this.insideSemanticFence && !this.pendingCR && !this.headerLine && this.linePrefix === null) {
      let asciiFast = true;
      let complex = false;
      for (let i = 0; i < chunk.length; i += 1) {
        const code = chunk.charCodeAt(i);
        if (code > 0x7f) asciiFast = false;
        if (code === 10 || code === 13 || code === 93) {
          complex = true;
          break;
        }
      }
      if (!complex) return asciiFast ? chunk.length : utf8Encoder.encode(chunk).length;
    }

    let observe = false;
    let ascii = true;
    let segmentStart = 0;

    if (this.pendingCR) {
      if (chunk.charCodeAt(0) === 10) {
        observe = this.#finishLine() || observe;
        segmentStart = 1;
      } else if (this.insideSemanticFence) {
        this.#advanceFenceCode(13);
      } else {
        this.#advanceNormal("\r");
      }
      this.pendingCR = false;
    }

    for (let i = segmentStart; i < chunk.length; i += 1) {
      const code = chunk.charCodeAt(i);
      if (code > 0x7f) ascii = false;

      if (this.insideSemanticFence) {
        if (code === 10) {
          observe = this.#finishFenceLine() || observe;
          segmentStart = i + 1;
        } else if (code === 13 && i + 1 === chunk.length) {
          this.pendingCR = true;
          segmentStart = i + 1;
        } else if (this.fenceLineState !== 3) {
          this.#advanceFenceCode(code);
        }
        continue;
      }

      if (code === 93) observe = true; // `]` may complete @[kind:id].
      if (code !== 10) continue;

      let end = i;
      if (end > segmentStart && chunk.charCodeAt(end - 1) === 13) end -= 1;
      if (end > segmentStart) this.#advanceNormal(chunk.slice(segmentStart, end));
      observe = this.#finishNormalLine() || observe;
      segmentStart = i + 1;
    }

    if (!this.insideSemanticFence) {
      let tailEnd = chunk.length;
      if (tailEnd > segmentStart && chunk.charCodeAt(tailEnd - 1) === 13) {
        tailEnd -= 1;
        this.pendingCR = true;
      }
      if (tailEnd > segmentStart) this.#advanceNormal(chunk.slice(segmentStart, tailEnd));
    }

    const utf8Bytes = ascii ? chunk.length : utf8Encoder.encode(chunk).length;
    return observe ? -(utf8Bytes + 1) : utf8Bytes;
  }

  inspect(chunk) {
    const packed = this.scan(chunk);
    return packed < 0
      ? { observe: true, utf8Bytes: -packed - 1 }
      : { observe: false, utf8Bytes: packed };
  }

  shouldObserve(chunk) {
    return this.scan(chunk) < 0;
  }

  reset() {
    this.linePrefix = "";
    this.headerLine = false;
    this.pendingCR = false;
    this.insideSemanticFence = false;
    this.fenceLineState = 0;
    this.fenceColonCount = 0;
  }

  #advanceNormal(segment) {
    if (segment === "" || this.headerLine || this.linePrefix === null) return;
    this.linePrefix = compactLinePrefix(this.linePrefix + segment);
    const classification = classifyLinePrefix(this.linePrefix);
    if (classification.confirmed) {
      this.headerLine = true;
      this.linePrefix = null;
    } else if (!classification.retain) {
      this.linePrefix = null;
    }
  }

  #finishNormalLine() {
    const openingFence = this.headerLine;
    const observe = openingFence || this.linePrefix === ":::";
    this.linePrefix = "";
    this.headerLine = false;
    if (openingFence) {
      this.insideSemanticFence = true;
      this.fenceLineState = 0;
      this.fenceColonCount = 0;
    }
    return observe;
  }

  #advanceFenceCode(code) {
    if (this.fenceLineState === 3) return;
    // Non-ASCII is treated conservatively as potential trim whitespace. This
    // can only cause a false-positive observation, never a missed close.
    const whitespace = asciiTrimWhitespace(code) || code > 0x7f;
    if (this.fenceLineState === 0) {
      if (code === 58) {
        this.fenceLineState = 1;
        this.fenceColonCount = 1;
      } else if (!whitespace) {
        this.fenceLineState = 3;
      }
      return;
    }
    if (this.fenceLineState === 1) {
      if (code === 58) {
        if (this.fenceColonCount < 3) this.fenceColonCount += 1;
      } else if (whitespace) {
        this.fenceLineState = 2;
      } else {
        this.fenceLineState = 3;
      }
      return;
    }
    if (this.fenceLineState === 2 && !whitespace) this.fenceLineState = 3;
  }

  #finishFenceLine() {
    const closes = (this.fenceLineState === 1 || this.fenceLineState === 2)
      && this.fenceColonCount >= 3;
    this.fenceLineState = 0;
    this.fenceColonCount = 0;
    if (closes) {
      // Streamdown supports longer colon fences. Treating any >=3-colon line
      // as a possible close is conservative for those fences: it may trigger
      // one extra semantic scan, but cannot miss the real closing fence.
      this.insideSemanticFence = false;
      this.linePrefix = "";
      this.headerLine = false;
    }
    return closes;
  }

  #finishLine() {
    return this.insideSemanticFence ? this.#finishFenceLine() : this.#finishNormalLine();
  }
}
