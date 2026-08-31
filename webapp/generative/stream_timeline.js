const DEFAULT_MAX_EVENTS = 64;

function finite(value, fallback = 0) {
  const number = Number(value);
  return Number.isFinite(number) ? number : fallback;
}

function pointOf({ blocks = 0, ui = 0, commit = "" } = {}) {
  return {
    blocks: Math.max(0, Math.floor(finite(blocks))),
    ui: Math.max(0, Math.floor(finite(ui))),
    commit: String(commit || "").slice(0, 24),
  };
}

function samePoint(a, b) {
  return Boolean(a && b)
    && a.blocks === b.blocks
    && a.ui === b.ui
    && a.commit === b.commit;
}

/** Stores only bounded stream milestones. It never stores chunk text. */
export class StreamTimelineRecorder {
  constructor({ maxEvents = DEFAULT_MAX_EVENTS, now = () => performance.now() } = {}) {
    this.maxEvents = Math.max(2, Math.min(256, Math.floor(finite(maxEvents, DEFAULT_MAX_EVENTS))));
    this.now = now;
    this.startedAt = this.now();
    this.chunks = 0;
    this.chars = 0;
    this.events = [];
    this.lastPoint = null;
    this.truncated = false;
  }

  observe(text, runtime = {}) {
    const value = String(text || "");
    this.chunks += 1;
    this.chars += value.length;
    const point = pointOf(runtime);
    const significant = this.chunks === 1 || !samePoint(point, this.lastPoint);
    this.lastPoint = point;
    if (!significant) return false;
    this.#push("chunk", point);
    return true;
  }

  finish(runtime = {}) {
    const point = pointOf(runtime);
    this.lastPoint = point;
    const event = this.#event("finish", point);
    const last = this.events.at(-1);
    if (last?.kind === "finish") this.events[this.events.length - 1] = event;
    else if (this.events.length < this.maxEvents) this.events.push(event);
    else {
      this.events[this.events.length - 1] = event;
      this.truncated = true;
    }
    return this.snapshot();
  }

  #event(kind, point) {
    return {
      kind,
      chunk: this.chunks,
      chars: this.chars,
      elapsedMs: Math.max(0, finite(this.now() - this.startedAt)),
      blocks: point.blocks,
      ui: point.ui,
      commit: point.commit,
    };
  }

  #push(kind, point) {
    const event = this.#event(kind, point);
    if (this.events.length < this.maxEvents) this.events.push(event);
    else this.truncated = true;
  }

  snapshot() {
    const events = this.events.map(event => ({ ...event }));
    const firstUi = events.find(event => event.ui > 0) || null;
    return {
      chunks: this.chunks,
      chars: this.chars,
      events,
      truncated: this.truncated,
      firstUiChunk: firstUi?.chunk ?? null,
      firstUiChars: firstUi?.chars ?? null,
      firstUiMs: firstUi?.elapsedMs ?? null,
      durationMs: events.at(-1)?.elapsedMs ?? Math.max(0, finite(this.now() - this.startedAt)),
    };
  }
}
