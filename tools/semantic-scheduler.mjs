function dependencyMap(graph) {
  const dependencies = new Map(graph.nodes.map((node) => [node.key, []]));
  for (const edge of graph.edges) {
    if (dependencies.has(edge.from)) dependencies.get(edge.from).push(edge.to);
  }
  return dependencies;
}

function dependentMap(graph) {
  const dependents = new Map(graph.nodes.map((node) => [node.key, new Set()]));
  for (const edge of graph.edges) {
    let reverse = dependents.get(edge.to);
    if (!reverse) dependents.set(edge.to, reverse = new Set());
    reverse.add(edge.from);
  }
  return dependents;
}

function publicError(error) {
  if (error instanceof Error) return { name: error.name, message: error.message };
  return { name: "Error", message: String(error) };
}

function sameDependencies(a, b) {
  if (a === b) return true;
  if (!a || !b || a.length !== b.length) return false;
  for (let i = 0; i < a.length; i += 1) if (a[i] !== b[i]) return false;
  return true;
}

export class SemanticScheduler {
  constructor({ concurrency = 4, runners = {}, onTransition = null } = {}) {
    if (!Number.isSafeInteger(concurrency) || concurrency <= 0) throw new RangeError("concurrency must be a positive integer");
    this.concurrency = concurrency;
    this.runners = new Map(Object.entries(runners));
    this.onTransition = onTransition;
    this.graph = { nodes: [], edges: [], executionOrder: [] };
    this.nodes = new Map();
    this.dependencies = new Map();
    this.dependents = new Map();
    this.ready = new Set();
    this.records = new Map();
    this.running = 0;
    this.sequence = 0;
    this.idleWaiters = [];
    this.pending = [];
    this.pendingSet = new Set();
  }

  updateGraph(graph) {
    this.graph = graph;
    this.nodes = new Map(graph.nodes.map((node) => [node.key, node]));
    this.dependencies = dependencyMap(graph);
    this.dependents = dependentMap(graph);
    for (const node of graph.nodes) {
      const failed = (this.dependencies.get(node.key) ?? []).find((key) => {
        const status = this.records.get(key)?.status;
        return status === "failed" || status === "blocked";
      });
      if (failed) this.#block(node.key, failed);
    }
    for (const key of this.ready) this.#enqueue(key);
    this.#pump();
    return this;
  }

  /**
   * Add or replace one semantic node without rebuilding the scheduler DAG.
   * `dependencies` is the already parsed list of `kind:id` keys. A malformed
   * dependency list may be passed as null; such a node will never dispatch.
   */
  upsertNode(node, dependencies = []) {
    if (!node || typeof node !== "object" || typeof node.key !== "string" || !node.key) {
      throw new TypeError("upsertNode() requires a semantic node with a key");
    }
    if (dependencies !== null && !Array.isArray(dependencies)) {
      throw new TypeError("upsertNode() dependencies must be an array or null");
    }

    const previousDependencies = this.dependencies.get(node.key);
    this.nodes.set(node.key, node);
    if (!sameDependencies(previousDependencies, dependencies)) {
      if (previousDependencies) {
        for (const dependency of previousDependencies) this.dependents.get(dependency)?.delete(node.key);
      }
      this.dependencies.set(node.key, dependencies);
      if (dependencies) {
        for (const dependency of dependencies) {
          let reverse = this.dependents.get(dependency);
          if (!reverse) this.dependents.set(dependency, reverse = new Set());
          reverse.add(node.key);
        }
      }
    }
    if (!this.dependents.has(node.key)) this.dependents.set(node.key, new Set());

    if (dependencies) {
      const failed = dependencies.find((key) => {
        const status = this.records.get(key)?.status;
        return status === "failed" || status === "blocked";
      });
      if (failed) this.#block(node.key, failed);
    }
    if (this.ready.has(node.key)) this.#enqueue(node.key);
    this.#pump();
    return this;
  }

  accept(event) {
    if (!event || typeof event !== "object") throw new TypeError("event must be an object");
    if (event.type !== "ready") return false;
    if (typeof event.key !== "string" || event.key.length === 0) throw new TypeError("ready event requires a semantic key");
    this.ready.add(event.key);
    const existing = this.records.get(event.key);
    if (!existing || existing.status === "waiting") this.#transition(event.key, "ready", { readyEvent: event });
    this.#enqueue(event.key);
    this.#pump();
    return true;
  }

  get(key) {
    const record = this.records.get(key);
    return record ? { ...record } : null;
  }

  getResult(key) {
    return this.records.get(key)?.result;
  }

  snapshot() {
    const output = {};
    const keys = new Set([...this.nodes.keys(), ...this.records.keys()]);
    for (const key of keys) {
      const record = this.records.get(key);
      output[key] = record ? { ...record } : { key, status: "waiting" };
    }
    return output;
  }

  async idle() {
    this.#pump();
    if (this.running === 0) return this.snapshot();
    return new Promise((resolve) => this.idleWaiters.push(resolve));
  }

  #runnerFor(node) {
    return this.runners.get(node.kind) ?? this.runners.get("*") ?? null;
  }

  #transition(key, status, extra = {}) {
    const previous = this.records.get(key);
    const record = { ...(previous ?? { key }), ...extra, key, status, sequence: ++this.sequence };
    this.records.set(key, record);
    this.onTransition?.({ ...record, previousStatus: previous?.status ?? null });
    return record;
  }

  #block(key, failedDependency) {
    const current = this.records.get(key)?.status;
    if (["completed", "failed", "blocked", "running"].includes(current)) return;
    this.#transition(key, "blocked", { failedDependency });
    for (const dependent of this.dependents.get(key) ?? []) this.#block(dependent, key);
  }

  #enqueue(key) {
    const status = this.records.get(key)?.status;
    if (["queued", "running", "completed", "failed", "blocked"].includes(status)) return;
    if (this.pendingSet.has(key)) return;
    this.pendingSet.add(key);
    this.pending.push(key);
  }

  #pump() {
    while (this.running < this.concurrency && this.pending.length) {
      const key = this.pending.shift();
      this.pendingSet.delete(key);
      if (!this.ready.has(key)) continue;
      const node = this.nodes.get(key);
      if (!node) continue;
      const status = this.records.get(key)?.status;
      if (["queued", "running", "completed", "failed", "blocked"].includes(status)) continue;
      const dependencies = this.dependencies.get(key);
      if (dependencies === null) continue;
      const dependencyList = dependencies ?? [];
      const failed = dependencyList.find((dependency) => {
        const dependencyStatus = this.records.get(dependency)?.status;
        return dependencyStatus === "failed" || dependencyStatus === "blocked";
      });
      if (failed) {
        this.#block(key, failed);
        continue;
      }
      if (!dependencyList.every((dependency) => this.records.get(dependency)?.status === "completed")) {
        // Do not spin on a waiting dependent. Completion of any dependency will
        // enqueue its reverse dependents again.
        continue;
      }
      const runner = this.#runnerFor(node);
      if (!runner) {
        this.#transition(key, "failed", { error: { name: "MissingRunnerError", message: `no runner registered for semantic kind ${node.kind}` } });
        for (const dependent of this.dependents.get(key) ?? []) this.#block(dependent, key);
        continue;
      }
      this.#transition(key, "queued", { dependencies: [...dependencyList] });
      this.#start(node, runner, dependencyList);
    }
    this.#resolveIdleIfNeeded();
  }

  #start(node, runner, dependencies) {
    this.running += 1;
    this.#transition(node.key, "running", { dependencies: [...dependencies] });
    const dependencyResults = Object.fromEntries(dependencies.map((key) => [key, this.records.get(key)?.result]));
    Promise.resolve()
      .then(() => runner(node, { key: node.key, dependencies: [...dependencies], dependencyResults, getResult: (key) => this.getResult(key) }))
      .then((result) => {
        this.#transition(node.key, "completed", { result });
        for (const dependent of this.dependents.get(node.key) ?? []) this.#enqueue(dependent);
      }, (error) => {
        this.#transition(node.key, "failed", { error: publicError(error) });
        for (const dependent of this.dependents.get(node.key) ?? []) this.#block(dependent, node.key);
      })
      .finally(() => {
        this.running -= 1;
        this.#pump();
      });
  }

  #resolveIdleIfNeeded() {
    if (this.running !== 0 || this.idleWaiters.length === 0) return;
    const waiters = this.idleWaiters.splice(0);
    const snapshot = this.snapshot();
    for (const resolve of waiters) resolve(snapshot);
  }
}

export function createSemanticScheduler(options) {
  return new SemanticScheduler(options);
}
