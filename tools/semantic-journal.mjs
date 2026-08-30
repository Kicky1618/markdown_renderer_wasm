import { applyJsonMergePatch } from "./semantic-state.mjs";

function cloneJson(value) {
  if (value === undefined) return undefined;
  return JSON.parse(JSON.stringify(value));
}

function cloneJsonMaybe(value) {
  try {
    const cloned = cloneJson(value);
    return { ok: true, value: cloned };
  } catch {
    return { ok: false, value: undefined };
  }
}

function stableJson(value) {
  return JSON.stringify(value);
}

function assertJournal(journal) {
  if (!(journal instanceof SemanticJournal)) throw new TypeError("expected a SemanticJournal");
  return journal;
}

function applyStateEntry(entry, current) {
  const encoding = entry.encoding ?? "snapshot";
  if (encoding === "snapshot") return cloneJson(entry.value);
  if (encoding !== "patch") throw new Error(`${entry.key}: unknown state journal encoding ${JSON.stringify(encoding)}`);
  if (current === undefined) throw new Error(`${entry.key}: patch journal entry has no prior state snapshot`);
  const format = entry.format ?? "merge";
  if (["merge", "merge-patch", "application/merge-patch+json"].includes(format)) {
    return applyJsonMergePatch(current, entry.patch);
  }
  if (format === "replace") return cloneJson(entry.patch);
  throw new Error(`${entry.key}: unsupported journal patch format ${JSON.stringify(format)}`);
}

/**
 * Append-only execution journal for semantic scheduler and state transitions.
 *
 * Entries are deliberately JSON-only so they can be streamed as NDJSON, saved
 * with an LLM transcript, and replayed without the parser or WASM runtime.
 */
export class SemanticJournal {
  constructor(entries = []) {
    if (!Array.isArray(entries)) throw new TypeError("journal entries must be an array");
    this.entries = entries.map((entry) => cloneJson(entry));
    this.sequence = this.entries.reduce((max, entry) => Math.max(max, Number(entry?.seq) || 0), 0);
  }

  recordStateChange(change, { encoding = "snapshot" } = {}) {
    if (!change || typeof change !== "object") throw new TypeError("state change must be an object");
    if (typeof change.key !== "string" || change.key.length === 0) throw new TypeError("state change requires key");
    if (!Number.isSafeInteger(change.revision) || change.revision <= 0) throw new TypeError("state change requires a positive integer revision");
    if (encoding !== "snapshot" && encoding !== "delta") throw new TypeError(`state journal encoding must be "snapshot" or "delta", got ${JSON.stringify(encoding)}`);
    const action = change.type ?? "update";
    const entry = {
      seq: ++this.sequence,
      type: "state",
      key: change.key,
      revision: change.revision,
      action,
      node: change.node ?? null,
    };
    if (change.format !== undefined) entry.format = change.format;
    if (encoding === "delta" && action === "patch") {
      if (!Object.prototype.hasOwnProperty.call(change, "patch")) {
        throw new TypeError(`${change.key}: delta journal encoding requires patch metadata`);
      }
      entry.encoding = "patch";
      entry.patch = cloneJson(change.patch);
    } else {
      entry.encoding = "snapshot";
      entry.value = cloneJson(change.value);
    }
    this.entries.push(entry);
    return cloneJson(entry);
  }

  recordSchedulerTransition(transition) {
    if (!transition || typeof transition !== "object") throw new TypeError("scheduler transition must be an object");
    if (typeof transition.key !== "string" || transition.key.length === 0) throw new TypeError("scheduler transition requires key");
    if (typeof transition.status !== "string" || transition.status.length === 0) throw new TypeError("scheduler transition requires status");

    const entry = {
      seq: ++this.sequence,
      type: "scheduler",
      key: transition.key,
      status: transition.status,
      previousStatus: transition.previousStatus ?? null,
    };
    if (Number.isSafeInteger(transition.sequence)) entry.schedulerSequence = transition.sequence;
    if (transition.failedDependency !== undefined) entry.failedDependency = transition.failedDependency;
    if (transition.error !== undefined) {
      const cloned = cloneJsonMaybe(transition.error);
      if (cloned.ok) entry.error = cloned.value;
      else entry.errorOmitted = true;
    }
    if (Object.prototype.hasOwnProperty.call(transition, "result")) {
      const cloned = cloneJsonMaybe(transition.result);
      if (cloned.ok) entry.result = cloned.value;
      else entry.resultOmitted = true;
    }
    this.entries.push(entry);
    return cloneJson(entry);
  }

  snapshot() {
    return this.entries.map((entry) => cloneJson(entry));
  }

  toNDJSON() {
    return this.entries.map((entry) => JSON.stringify(entry)).join("\n") + (this.entries.length ? "\n" : "");
  }

  verify() {
    const errors = [];
    const stateRevisions = new Map();
    const stateValues = new Map();
    const stateByNode = new Map();
    let schedulerSequence = 0;

    for (let index = 0; index < this.entries.length; index += 1) {
      const entry = this.entries[index];
      const expectedSeq = index + 1;
      if (entry.seq !== expectedSeq) errors.push(`entry ${index}: expected seq=${expectedSeq}, got ${JSON.stringify(entry.seq)}`);

      if (entry.type === "state") {
        const previous = stateRevisions.get(entry.key) ?? 0;
        if (entry.revision !== previous + 1) {
          errors.push(`${entry.key}: expected revision ${previous + 1}, got ${JSON.stringify(entry.revision)}`);
        }
        stateRevisions.set(entry.key, Number(entry.revision) || previous);
        try {
          const next = applyStateEntry(entry, stateValues.get(entry.key));
          stateValues.set(entry.key, next);
          if (entry.node) stateByNode.set(entry.node, next);
        } catch (error) {
          errors.push(error instanceof Error ? error.message : String(error));
        }
        continue;
      }

      if (entry.type === "scheduler") {
        if (entry.schedulerSequence !== undefined) {
          if (!Number.isSafeInteger(entry.schedulerSequence) || entry.schedulerSequence <= schedulerSequence) {
            errors.push(`${entry.key}: scheduler sequence must increase (previous ${schedulerSequence}, got ${JSON.stringify(entry.schedulerSequence)})`);
          } else {
            schedulerSequence = entry.schedulerSequence;
          }
        }
        if (entry.status === "completed" && stateByNode.has(entry.key) && Object.prototype.hasOwnProperty.call(entry, "result")) {
          const expected = stateByNode.get(entry.key);
          if (stableJson(entry.result) !== stableJson(expected)) {
            errors.push(`${entry.key}: completed result does not match recorded state change`);
          }
        }
        continue;
      }

      errors.push(`entry ${index}: unknown journal type ${JSON.stringify(entry.type)}`);
    }

    return { ok: errors.length === 0, errors };
  }

  replayState({ verify = true } = {}) {
    if (verify) {
      const result = this.verify();
      if (!result.ok) throw new Error(`semantic journal verification failed: ${result.errors.join("; ")}`);
    }
    const values = new Map();
    const revisions = new Map();
    for (const entry of this.entries) {
      if (entry.type !== "state") continue;
      values.set(entry.key, applyStateEntry(entry, values.get(entry.key)));
      revisions.set(entry.key, entry.revision);
    }
    return {
      values: Object.fromEntries(values),
      revisions: Object.fromEntries(revisions),
    };
  }

  static fromNDJSON(text) {
    if (typeof text !== "string") throw new TypeError("NDJSON input must be a string");
    const entries = [];
    const lines = text.split(/\r?\n/);
    for (let i = 0; i < lines.length; i += 1) {
      const line = lines[i].trim();
      if (!line) continue;
      let entry;
      try {
        entry = JSON.parse(line);
      } catch (error) {
        throw new SyntaxError(`invalid semantic journal NDJSON at line ${i + 1}: ${error.message}`);
      }
      entries.push(entry);
    }
    return new SemanticJournal(entries);
  }
}

/** Compose journal recording with optional existing callbacks. */
const JOURNAL_SCHEDULER_MODES = new Set(["all", "terminal", "none"]);
const TERMINAL_SCHEDULER_STATUSES = new Set(["completed", "failed", "blocked"]);

/** Compose journal recording with optional existing callbacks.
 *
 * `scheduler` controls only persisted scheduler entries; callbacks still see
 * every transition. `terminal` keeps completed/failed/blocked outcomes while
 * `none` produces a compact state-replay journal containing state changes only.
 */
export function createSemanticJournalHooks(journal, {
  onTransition = null,
  onStateChange = null,
  scheduler = "all",
  stateEncoding = "snapshot",
} = {}) {
  assertJournal(journal);
  if (!JOURNAL_SCHEDULER_MODES.has(scheduler)) {
    throw new TypeError(`scheduler journal mode must be "all", "terminal", or "none", got ${JSON.stringify(scheduler)}`);
  }
  if (stateEncoding !== "snapshot" && stateEncoding !== "delta") {
    throw new TypeError(`state journal encoding must be "snapshot" or "delta", got ${JSON.stringify(stateEncoding)}`);
  }
  return {
    onTransition(transition) {
      if (scheduler === "all" || (scheduler === "terminal" && TERMINAL_SCHEDULER_STATUSES.has(transition?.status))) {
        journal.recordSchedulerTransition(transition);
      }
      onTransition?.(transition);
    },
    onStateChange(change) {
      journal.recordStateChange(change, { encoding: stateEncoding });
      onStateChange?.(change);
    },
  };
}
