import { buildSemanticGraph, parseDependencies } from "./semantic-graph.mjs";

export function semanticReferencesFromLinks(links) {
  return links
    .filter(({ destination }) => destination.startsWith("llm:") && !destination.startsWith("llm:cite:"))
    .map(({ block, text: label, destination }) => {
      const body = destination.slice("llm:".length);
      const colon = body.indexOf(":");
      return {
        block,
        kind: colon < 0 ? body : body.slice(0, colon),
        id: colon < 0 ? "" : body.slice(colon + 1),
        label,
      };
    });
}

export function createTimelineState() {
  return {
    seenNodes: new Set(),
    closedNodes: new Set(),
    readyNodes: new Set(),
    seenReferences: new Set(),
  };
}

function referenceKey(reference) {
  return `${reference.block}:${reference.kind}:${reference.id}:${reference.label}`;
}

function dependencyKeys(node) {
  const dependencies = parseDependencies(node.attributes.depends);
  if (dependencies.some((dependency) => !dependency.parsed)) return null;
  return dependencies.map((dependency) => dependency.parsed.key);
}

export function observeSemanticState(summary, state, observedAtByte, chunkIndex) {
  const graph = buildSemanticGraph(summary);
  const events = [];
  const nodeByKey = new Map(graph.nodes.map((node) => [node.key, node]));

  for (const node of graph.nodes) {
    if (!state.seenNodes.has(node.key)) {
      state.seenNodes.add(node.key);
      events.push({
        type: "open",
        key: node.key,
        block: node.block,
        observedAtByte,
        chunkIndex,
      });
    }
    if (node.closed && !state.closedNodes.has(node.key)) {
      state.closedNodes.add(node.key);
      events.push({
        type: "close",
        key: node.key,
        block: node.block,
        observedAtByte,
        chunkIndex,
      });
    }
  }

  for (const reference of summary.semanticReferences) {
    const key = referenceKey(reference);
    if (state.seenReferences.has(key)) continue;
    state.seenReferences.add(key);
    events.push({
      type: "reference",
      key: `${reference.kind}:${reference.id}`,
      block: reference.block,
      label: reference.label,
      observedAtByte,
      chunkIndex,
    });
  }

  // Iterate to a fixed point so a chain whose final dependency closes in this
  // chunk can become ready from dependency to dependent in the same observation.
  let changed = true;
  while (changed) {
    changed = false;
    for (const node of graph.nodes) {
      if (!node.closed || state.readyNodes.has(node.key)) continue;
      const dependencies = dependencyKeys(node);
      if (dependencies === null) continue;
      const ready = dependencies.every((key) => {
        const dependency = nodeByKey.get(key);
        return dependency?.closed && state.readyNodes.has(key);
      });
      if (!ready) continue;
      state.readyNodes.add(node.key);
      events.push({
        type: "ready",
        key: node.key,
        block: node.block,
        dependsOn: dependencies,
        observedAtByte,
        chunkIndex,
      });
      changed = true;
    }
  }

  return { graph, events, state };
}
