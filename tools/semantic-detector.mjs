const utf8Encoder = new TextEncoder();

function compactLinePrefix(line) {
  const trimmed = line.trimStart();
  if (trimmed === "") return "";

  let colons = 0;
  while (colons < trimmed.length && trimmed.charCodeAt(colons) === 58) colons += 1;
  if (colons >= 3 && colons === trimmed.length) return ":::";
  if (colons > 3) return `:::${trimmed.slice(colons)}`;
  return trimmed;
}

function classifyLinePrefix(line) {
  const trimmed = line;
  if (trimmed === "") return { retain: true, forceNext: false, confirmed: false };

  let colons = 0;
  while (colons < trimmed.length && trimmed.charCodeAt(colons) === 58) colons += 1;
  if (colons === 0) return { retain: false, forceNext: false, confirmed: false };
  if (colons < 3) {
    return colons === trimmed.length
      ? { retain: true, forceNext: false, confirmed: false }
      : { retain: false, forceNext: false, confirmed: false };
  }

  const tail = trimmed.slice(colons);
  if (tail === "" || tail === "l" || tail === "ll") {
    return { retain: true, forceNext: true, confirmed: false };
  }
  if (tail === "llm" || /^llm\s/.test(tail)) {
    return { retain: false, forceNext: true, confirmed: true };
  }
  return { retain: false, forceNext: false, confirmed: false };
}

/**
 * Cheap conservative detector for chunks that can change Streamdown's
 * semantic layer. `inspect()` also returns exact UTF-8 byte length so the
 * runtime only scans ordinary ASCII token chunks once.
 *
 * Once a `:{3,}llm` header is confirmed, the detector keeps only one boolean
 * and forces semantic observation until newline. Arbitrarily long attributes
 * therefore do not grow detector memory or delay the header's events.
 */
export class SemanticChangeDetector {
  constructor() {
    this.linePrefix = "";
    this.headerCandidate = false;
    this.headerLine = false;
  }

  inspect(chunk) {
    if (typeof chunk !== "string") throw new TypeError("semantic detector expects a string");

    let triggered = false;
    let ascii = true;
    let lastNewline = -1;
    for (let i = 0; i < chunk.length; i += 1) {
      const code = chunk.charCodeAt(i);
      if (code > 0x7f) ascii = false;
      if (code === 10 || code === 13) {
        triggered = true;
        lastNewline = i;
      } else if (code === 58 || code === 64 || code === 93) {
        triggered = true;
      }
    }

    const forcedByCarry = this.headerCandidate || this.headerLine;
    if (forcedByCarry || triggered || this.linePrefix !== null) {
      this.#advance(chunk, lastNewline);
    }

    return {
      observe: forcedByCarry || triggered,
      utf8Bytes: ascii ? chunk.length : utf8Encoder.encode(chunk).length,
    };
  }

  shouldObserve(chunk) {
    return this.inspect(chunk).observe;
  }

  reset() {
    this.linePrefix = "";
    this.headerCandidate = false;
    this.headerLine = false;
  }

  #advance(chunk, lastNewline) {
    if (lastNewline >= 0) {
      this.headerLine = false;
      this.linePrefix = chunk.slice(lastNewline + 1);
    } else if (this.headerLine) {
      this.headerCandidate = false;
      return;
    } else if (this.linePrefix !== null) {
      this.linePrefix += chunk;
    } else {
      this.headerCandidate = false;
      return;
    }

    this.linePrefix = compactLinePrefix(this.linePrefix);
    const classification = classifyLinePrefix(this.linePrefix);
    this.headerCandidate = classification.forceNext;
    this.headerLine = classification.confirmed;
    if (!classification.retain) this.linePrefix = null;
  }
}
