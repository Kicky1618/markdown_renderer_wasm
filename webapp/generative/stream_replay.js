const DEFAULT_MAX_CHARS = 2 * 1024 * 1024;
const DEFAULT_MAX_CHUNKS = 8192;

function finite(value, fallback = 0) {
  const number = Number(value);
  return Number.isFinite(number) ? number : fallback;
}

export class StreamReplayRecorder {
  constructor({ maxChars = DEFAULT_MAX_CHARS, maxChunks = DEFAULT_MAX_CHUNKS, now = () => performance.now() } = {}) {
    this.maxChars = Math.max(1, finite(maxChars, DEFAULT_MAX_CHARS));
    this.maxChunks = Math.max(1, finite(maxChunks, DEFAULT_MAX_CHUNKS));
    this.now = now;
    this.startedAt = this.now();
    this.lastAt = this.startedAt;
    this.chars = 0;
    this.chunks = [];
    this.truncated = false;
  }

  push(text) {
    const value = String(text || "");
    if (!value) return true;
    if (this.chunks.length >= this.maxChunks || this.chars + value.length > this.maxChars) {
      this.truncated = true;
      return false;
    }
    const current = this.now();
    const delayMs = Math.max(0, Math.min(5000, finite(current - this.lastAt)));
    this.lastAt = current;
    this.chars += value.length;
    this.chunks.push({ text: value, delayMs });
    return true;
  }

  snapshot() {
    return {
      chunks: this.chunks.map(chunk => ({ ...chunk })),
      chars: this.chars,
      truncated: this.truncated,
      durationMs: Math.max(0, finite(this.lastAt - this.startedAt)),
    };
  }
}

function abortError() {
  const error = new Error("replay aborted");
  error.name = "AbortError";
  return error;
}

const defaultSleep = ms => new Promise(resolve => setTimeout(resolve, ms));

/** Replay already-decoded Markdown chunks without touching the network. */
export async function replayDecodedChunks(recording, {
  onText,
  speed = 4,
  maxDelayMs = 120,
  signal,
  sleep = defaultSleep,
} = {}) {
  if (typeof onText !== "function") throw new TypeError("onText callback is required");
  const factor = Math.max(0.01, finite(speed, 4));
  const delayCap = Math.max(0, finite(maxDelayMs, 120));
  let chunks = 0;
  let chars = 0;
  for (const chunk of recording?.chunks || []) {
    if (signal?.aborted) throw signal.reason || abortError();
    const delay = Math.min(delayCap, Math.max(0, finite(chunk.delayMs))) / factor;
    if (delay > 0) await sleep(delay);
    if (signal?.aborted) throw signal.reason || abortError();
    const text = String(chunk.text || "");
    if (!text) continue;
    onText(text);
    chunks += 1;
    chars += text.length;
  }
  return { chunks, chars };
}
