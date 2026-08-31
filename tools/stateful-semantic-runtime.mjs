import { SemanticRuntime } from "./semantic-runtime.mjs";
import { createStateRunners, SemanticStateStore } from "./semantic-state.mjs";
import { createSemanticJournalHooks, SemanticJournal } from "./semantic-journal.mjs";

const INTERNAL_LIGHT_SNAPSHOT = Object.freeze({
  document: false,
  graph: false,
  diagnostics: false,
  scheduler: false,
});

/**
 * Convenience wrapper that wires `:::llm state` / `:::llm patch` into the
 * streaming SemanticRuntime while preserving the base runtime API.
 *
 * The wrapper deliberately owns the `state` and `patch` runner kinds so the
 * state snapshot and revision map always describe the values actually applied
 * by the scheduler. Other runner kinds can still be supplied normally.
 */
export class StatefulSemanticRuntime {
  static async load(wasmSource, {
    stateStore = null,
    onStateChange = null,
    onTransition = null,
    journal = null,
    journalScheduler = "all",
    journalStateEncoding = "snapshot",
    runners = {},
    ...runtimeOptions
  } = {}) {
    if (journal !== null && !(journal instanceof SemanticJournal)) {
      throw new TypeError("journal must be a SemanticJournal");
    }
    if (stateStore && onStateChange !== null) {
      throw new TypeError("onStateChange cannot be used with an explicit stateStore; configure the store callback directly");
    }
    if (stateStore && journal !== null) {
      throw new TypeError("journal cannot be auto-wired with an explicit stateStore; use createSemanticJournalHooks() on that store instead");
    }

    const hooks = journal === null
      ? { onTransition, onStateChange }
      : createSemanticJournalHooks(journal, {
          scheduler: journalScheduler,
          stateEncoding: journalStateEncoding,
          onTransition,
          onStateChange,
        });
    const store = stateStore ?? new SemanticStateStore({ onChange: hooks.onStateChange });
    if (!(store instanceof SemanticStateStore)) {
      throw new TypeError("stateStore must be a SemanticStateStore");
    }

    const runtime = await SemanticRuntime.load(wasmSource, {
      ...runtimeOptions,
      onTransition: hooks.onTransition,
      runners: {
        ...runners,
        ...createStateRunners(store),
      },
    });
    return new StatefulSemanticRuntime(runtime, store, journal);
  }

  constructor(runtime, stateStore, journal = null) {
    if (!(runtime instanceof SemanticRuntime)) throw new TypeError("runtime must be a SemanticRuntime");
    if (!(stateStore instanceof SemanticStateStore)) throw new TypeError("stateStore must be a SemanticStateStore");
    if (journal !== null && !(journal instanceof SemanticJournal)) throw new TypeError("journal must be a SemanticJournal");
    this.runtime = runtime;
    this.stateStore = stateStore;
    this.journal = journal;
  }

  append(chunk) {
    return this.runtime.append(chunk);
  }

  consume(source, options = {}) {
    const { snapshotOptions, ...consumeOptions } = options ?? {};
    return this.runtime.consume(source, {
      ...consumeOptions,
      snapshotOptions: INTERNAL_LIGHT_SNAPSHOT,
    }).then(() => this.snapshot(snapshotOptions));
  }

  async finish(snapshotOptions = undefined) {
    await this.runtime.finish(INTERNAL_LIGHT_SNAPSHOT);
    return this.snapshot(snapshotOptions);
  }

  async idle(snapshotOptions = undefined) {
    await this.runtime.idle(INTERNAL_LIGHT_SNAPSHOT);
    return this.snapshot(snapshotOptions);
  }

  snapshot(snapshotOptions = undefined) {
    const base = this.runtime.snapshot(snapshotOptions);
    return {
      ...base,
      state: {
        values: this.stateStore.snapshot(),
        revisions: this.stateStore.revisionSnapshot(),
      },
    };
  }

  dispose() {
    this.runtime.dispose();
  }

  get parser() {
    return this.runtime.parser;
  }

  get scheduler() {
    return this.runtime.scheduler;
  }

  get graph() {
    return this.runtime.graph;
  }
}

export async function createStatefulSemanticRuntime(wasmSource, options = {}) {
  return StatefulSemanticRuntime.load(wasmSource, options);
}
