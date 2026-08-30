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

/**
 * Cheap conservative detector for chunks that can change Streamdown's
 * semantic layer. `inspect()` also returns exact UTF-8 byte length so the
 * runtime only scans ordinary ASCII token chunks once.
 *
 * Streamdown only opens/closes semantic fences when a line completes, so the
 * detector tracks a bounded candidate prefix and does not rebuild the semantic
 * graph for ordinary newlines. Inline `@[kind:id]` references can become live
 * without a newline, so `]` remains an immediate observation trigger.
 */
export class SemanticChangeDetector {
  constructor() {
    this.linePrefix = "";
    this.headerLine = false;
    this.pendingCR = false;
  }

  inspect(chunk) {
    if (typeof chunk !== "string") throw new TypeError("semantic detector expects a string");

    let observe = false;
    let ascii = true;
    let segmentStart = 0;

    // A CR at the end of the previous chunk is only ignorable when this chunk
    // starts with LF. Otherwise it was ordinary line content and invalidates a
    // semantic header/close candidate exactly as the Rust parser would.
    if (this.pendingCR) {
      if (chunk.charCodeAt(0) === 10) {
        observe = this.#finishLine() || observe;
        segmentStart = 1;
      } else {
        this.#advance("\r");
      }
      this.pendingCR = false;
    }

    for (let i = segmentStart; i < chunk.length; i += 1) {
      const code = chunk.charCodeAt(i);
      if (code > 0x7f) ascii = false;
      if (code === 93) observe = true; // `]` may complete @[kind:id].
      if (code !== 10) continue;

      let end = i;
      if (end > segmentStart && chunk.charCodeAt(end - 1) === 13) end -= 1;
      if (end > segmentStart) this.#advance(chunk.slice(segmentStart, end));
      observe = this.#finishLine() || observe;
      segmentStart = i + 1;
    }

    let tailEnd = chunk.length;
    if (tailEnd > segmentStart && chunk.charCodeAt(tailEnd - 1) === 13) {
      tailEnd -= 1;
      this.pendingCR = true;
    }
    if (tailEnd > segmentStart) this.#advance(chunk.slice(segmentStart, tailEnd));

    return {
      observe,
      utf8Bytes: ascii ? chunk.length : utf8Encoder.encode(chunk).length,
    };
  }

  shouldObserve(chunk) {
    return this.inspect(chunk).observe;
  }

  reset() {
    this.linePrefix = "";
    this.headerLine = false;
    this.pendingCR = false;
  }

  #advance(segment) {
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

  #finishLine() {
    // A confirmed :::llm header changes the semantic graph. A colon-only line
    // may close an active semantic fence; observing it outside a fence is a
    // harmless conservative false positive.
    const observe = this.headerLine || this.linePrefix === ":::";
    this.linePrefix = "";
    this.headerLine = false;
    return observe;
  }
}
