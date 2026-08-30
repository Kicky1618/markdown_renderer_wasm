import { SemanticRuntime } from "./semantic-runtime.mjs";
import { createStateRunners, SemanticStateStore } from "./semantic-state.mjs";

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
    runners = {},
    ...runtimeOptions
  } = {}) {
    const store = stateStore ?? new SemanticStateStore({ onChange: onStateChange });
    if (!(store instanceof SemanticStateStore)) {
      throw new TypeError("stateStore must be a SemanticStateStore");
    }
    if (stateStore && onStateChange !== null) {
      throw new TypeError("onStateChange cannot be used with an explicit stateStore; configure the store callback directly");
    }

    const runtime = await SemanticRuntime.load(wasmSource, {
      ...runtimeOptions,
      runners: {
        ...runners,
        ...createStateRunners(store),
      },
    });
    return new StatefulSemanticRuntime(runtime, store);
  }

  constructor(runtime, stateStore) {
    if (!(runtime instanceof SemanticRuntime)) throw new TypeError("runtime must be a SemanticRuntime");
    if (!(stateStore instanceof SemanticStateStore)) throw new TypeError("stateStore must be a SemanticStateStore");
    this.runtime = runtime;
    this.stateStore = stateStore;
  }

  append(chunk) {
    return this.runtime.append(chunk);
  }

  consume(source, options) {
    return this.runtime.consume(source, options).then(() => this.snapshot());
  }

  async finish() {
    await this.runtime.finish();
    return this.snapshot();
  }

  async idle() {
    await this.runtime.idle();
    return this.snapshot();
  }

  snapshot() {
    const base = this.runtime.snapshot();
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
