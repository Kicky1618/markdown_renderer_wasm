import { parseDependencies } from "./semantic-graph.mjs";

function semanticNode(block) {
  const id = block.attributes?.id;
  if (!id) return null;
  return {
    key: `${block.kind}:${id}`,
    kind: block.kind,
    id,
    block: block.index,
    closed: block.closed,
    attributes: block.attributes,
    value: block.value,
  };
}

function dependencyKeys(node) {
  const dependencies = parseDependencies(node.attributes?.depends);
  if (dependencies.some((dependency) => !dependency.parsed)) return null;
  return dependencies.map((dependency) => dependency.parsed.key);
}

function referenceKey(reference) {
  return `${reference.block}:${reference.kind}:${reference.id}:${reference.label}`;
}

function sameArray(a, b) {
  if (a === b) return true;
  if (!a || !b || a.length !== b.length) return false;
  for (let i = 0; i < a.length; i += 1) if (a[i] !== b[i]) return false;
  return true;
}

/**
 * Incremental counterpart to observeSemanticState().
 *
 * SemanticRuntimeSummary guarantees append-oriented arrays whose only mutable
 * portion is the current tail block. Revisit at most that previous tail plus
 * newly projected entries, then propagate readiness through a reverse
 * dependency worklist. Full graph diagnostics remain available at snapshot
 * time via buildSemanticGraph(); they are intentionally not rebuilt per token.
 */
export class IncrementalSemanticTimeline {
  constructor() {
    this.seenNodes = new Set();
    this.closedNodes = new Set();
    this.readyNodes = new Set();
    this.seenReferences = new Set();
    this.nodes = new Map();
    this.dependencies = new Map();
    this.dependents = new Map();
    this.llmCount = 0;
    this.referenceCount = 0;
  }

  observe(summary, observedAtByte, chunkIndex) {
    const llmBlocks = summary?.llmBlocks ?? [];
    const references = summary?.semanticReferences ?? [];
    const events = [];
    const changedNodes = [];
    const readyQueue = [];
    const queued = new Set();

    const enqueueReadyCheck = (key) => {
      if (!queued.has(key)) {
        queued.add(key);
        readyQueue.push(key);
      }
    };

    // The previous projected tail can change from open -> closed. If a caller
    // supplies a shorter projection, conservatively revisit everything; seen
    // sets keep lifecycle events idempotent.
    const nodeStart = llmBlocks.length < this.llmCount ? 0 : Math.max(0, this.llmCount - 1);
    for (let i = nodeStart; i < llmBlocks.length; i += 1) {
      const node = semanticNode(llmBlocks[i]);
      if (!node) continue;
      const previous = this.nodes.get(node.key);
      const dependencies = dependencyKeys(node);
      const oldDependencies = this.dependencies.get(node.key);
      const changed = !previous
        || previous.block !== node.block
        || previous.closed !== node.closed
        || previous.value !== node.value
        || !sameArray(oldDependencies, dependencies);

      if (changed) {
        if (oldDependencies) {
          for (const dependency of oldDependencies) this.dependents.get(dependency)?.delete(node.key);
        }
        this.nodes.set(node.key, node);
        this.dependencies.set(node.key, dependencies);
        if (dependencies) {
          for (const dependency of dependencies) {
            let reverse = this.dependents.get(dependency);
            if (!reverse) this.dependents.set(dependency, reverse = new Set());
            reverse.add(node.key);
          }
        }
        changedNodes.push({ node, dependencies });
      }

      if (!this.seenNodes.has(node.key)) {
        this.seenNodes.add(node.key);
        events.push({ type: "open", key: node.key, block: node.block, observedAtByte, chunkIndex });
      }
      if (node.closed && !this.closedNodes.has(node.key)) {
        this.closedNodes.add(node.key);
        events.push({ type: "close", key: node.key, block: node.block, observedAtByte, chunkIndex });
      }
      if (node.closed) enqueueReadyCheck(node.key);
    }
    this.llmCount = llmBlocks.length;

    const referenceStart = references.length < this.referenceCount ? 0 : this.referenceCount;
    for (let i = referenceStart; i < references.length; i += 1) {
      const reference = references[i];
      const identity = referenceKey(reference);
      if (this.seenReferences.has(identity)) continue;
      this.seenReferences.add(identity);
      events.push({
        type: "reference",
        key: `${reference.kind}:${reference.id}`,
        block: reference.block,
        label: reference.label,
        observedAtByte,
        chunkIndex,
      });
    }
    this.referenceCount = references.length;

    while (readyQueue.length) {
      const key = readyQueue.shift();
      queued.delete(key);
      if (this.readyNodes.has(key)) continue;
      const node = this.nodes.get(key);
      if (!node?.closed) continue;
      const dependencies = this.dependencies.get(key);
      if (dependencies === null || dependencies === undefined) continue;
      const ready = dependencies.every((dependencyKey) => {
        const dependency = this.nodes.get(dependencyKey);
        return dependency?.closed && this.readyNodes.has(dependencyKey);
      });
      if (!ready) continue;
      this.readyNodes.add(key);
      events.push({
        type: "ready",
        key,
        block: node.block,
        dependsOn: [...dependencies],
        observedAtByte,
        chunkIndex,
      });
      for (const dependent of this.dependents.get(key) ?? []) enqueueReadyCheck(dependent);
    }

    return { events, changedNodes, state: this };
  }
}
