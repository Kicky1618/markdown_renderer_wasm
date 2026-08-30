function normalizeConcurrency(value) {
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new RangeError("concurrency must be a positive integer");
  }
  return value;
}

function dependencyMap(graph) {
  const dependencies = new Map(graph.nodes.map((node) => [node.key, []]));
  for (const edge of graph.edges) {
    if (dependencies.has(edge.from) && dependencies.has(edge.to)) dependencies.get(edge.from).push(edge.to);
  }
  return dependencies;
}

function dependentMap(graph) {
  const dependents = new Map(graph.nodes.map((node) => [node.key, []]));
  for (const edge of graph.edges) {
    if (dependents.has(edge.from) && dependents.has(edge.to)) dependents.get(edge.to).push(edge.from);
  }
  return dependents;
}

function publicError(error) {
  if (error instanceof Error) return { name: error.name, message: error.message };
  return { name: "Error", message: String(error) };
}

export class SemanticScheduler {
  constructor({ concurrency = 4, runners = {}, onTransition = null } = {}) {
    if (!Number.isSafeInteger(concurrency) || concurrency <= 0) throw new RangeError("concurrency must be a positive integer");
    this.concurrency = concurrency;
    this.runners = new Map(Object.entries(runners));
    this.onTransition = onTransition;
    this.graph = { nodes: [], edges: [] };
    this.nodes = new Map();
    this.dependencies = new Map();
    this.dependents = new Map();
    this.ready = new Set();
    this.records = new Map();
    this.running = 0;
    this.sequence = 0;
    this.idleWaiters = [];
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

  #candidateKeys() {
    const ordered = this.graph.executionOrder?.length ? this.graph.executionOrder : this.graph.nodes.map((node) => node.key);
    const known = new Set(ordered);
    return [...ordered, ...[...this.ready].filter((key) => !known.has(key))];
  }

  #pump() {
    let madeProgress = true;
    while (this.running < this.concurrency && madeProgress) {
      madeProgress = false;
      for (const key of this.#candidateKeys()) {
        if (this.running >= this.concurrency) break;
        if (!this.ready.has(key)) continue;
        const node = this.nodes.get(key);
        if (!node) continue;
        const status = this.records.get(key)?.status;
        if (["queued", "running", "completed", "failed", "blocked"].includes(status)) continue;
        const dependencies = this.dependencies.get(key) ?? [];
        const failed = dependencies.find((dependency) => {
          const dependencyStatus = this.records.get(dependency)?.status;
          return dependencyStatus === "failed" || dependencyStatus === "blocked";
        });
        if (failed) {
          this.#block(key, failed);
          madeProgress = true;
          continue;
        }
        if (!dependencies.every((dependency) => this.records.get(dependency)?.status === "completed")) continue;
        const runner = this.#runnerFor(node);
        if (!runner) {
          this.#transition(key, "failed", { error: { name: "MissingRunnerError", message: `no runner registered for semantic kind ${node.kind}` } });
          for (const dependent of this.dependents.get(key) ?? []) this.#block(dependent, key);
          madeProgress = true;
          continue;
        }
        this.#transition(key, "queued", { dependencies: [...dependencies] });
        this.#start(node, runner, dependencies);
        madeProgress = true;
      }
    }
    this.#resolveIdleIfNeeded();
  }

  #start(node, runner, dependencies) {
    this.running += 1;
    this.#transition(node.key, "running", { dependencies: [...dependencies] });
    const dependencyResults = Object.fromEntries(dependencies.map((key) => [key, this.records.get(key)?.result]));
    Promise.resolve()
      .then(() => runner(node, { key: node.key, dependencies: [...dependencies], dependencyResults, getResult: (key) => this.getResult(key) }))
      .then((result) => this.#transition(node.key, "completed", { result }), (error) => {
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
