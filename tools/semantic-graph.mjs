const LOCAL_KINDS = new Set(["tool", "artifact", "ui", "metric", "state", "data"]);

function semanticKey(kind, id) {
  return `${kind}:${id}`;
}

function parseDependency(value) {
  const separator = value.indexOf(":");
  if (separator <= 0 || separator === value.length - 1) return null;
  const kind = value.slice(0, separator);
  const id = value.slice(separator + 1);
  if (!/^[A-Za-z0-9_-]+$/.test(kind) || /[\s,]/.test(id)) return null;
  return { kind, id, key: semanticKey(kind, id) };
}

export function parseDependencies(value) {
  if (!value) return [];
  return value
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean)
    .map((item) => ({ raw: item, parsed: parseDependency(item) }));
}

export function buildSemanticGraph(summary) {
  const nodes = [];
  const nodeByKey = new Map();
  const duplicates = [];

  for (const block of summary.llmBlocks) {
    const id = block.attributes.id;
    if (!id) continue;
    const key = semanticKey(block.kind, id);
    const node = {
      key,
      kind: block.kind,
      id,
      block: block.index,
      closed: block.closed,
      attributes: block.attributes,
    };
    if (nodeByKey.has(key)) duplicates.push({ key, first: nodeByKey.get(key).block, duplicate: block.index });
    else nodeByKey.set(key, node);
    nodes.push(node);
  }

  const edges = [];
  const malformed = [];
  const unresolved = [];
  for (const node of nodes) {
    for (const dependency of parseDependencies(node.attributes.depends)) {
      if (!dependency.parsed) {
        malformed.push({ from: node.key, dependency: dependency.raw });
        continue;
      }
      const edge = { from: node.key, to: dependency.parsed.key, source: "depends" };
      edges.push(edge);
      if (LOCAL_KINDS.has(dependency.parsed.kind) && !nodeByKey.has(dependency.parsed.key)) {
        unresolved.push(edge);
      }
    }
  }

  for (const ref of summary.semanticReferences) {
    const key = semanticKey(ref.kind, ref.id);
    if (LOCAL_KINDS.has(ref.kind) && !nodeByKey.has(key)) {
      unresolved.push({ from: `markdown:block${ref.block}`, to: key, source: "inline" });
    }
  }

  const adjacency = new Map(nodes.map((node) => [node.key, []]));
  for (const edge of edges) {
    if (adjacency.has(edge.from) && adjacency.has(edge.to)) adjacency.get(edge.from).push(edge.to);
  }

  const cycles = [];
  const state = new Map();
  const stack = [];
  const stackIndex = new Map();
  const seenCycles = new Set();

  const visit = (key) => {
    state.set(key, 1);
    stackIndex.set(key, stack.length);
    stack.push(key);
    for (const next of adjacency.get(key) ?? []) {
      const nextState = state.get(next) ?? 0;
      if (nextState === 0) visit(next);
      else if (nextState === 1) {
        const start = stackIndex.get(next);
        const cycle = [...stack.slice(start), next];
        const canonical = [...new Set(cycle.slice(0, -1))].sort().join("|");
        if (!seenCycles.has(canonical)) {
          seenCycles.add(canonical);
          cycles.push(cycle);
        }
      }
    }
    stack.pop();
    stackIndex.delete(key);
    state.set(key, 2);
  };

  for (const node of nodes) if (!state.has(node.key)) visit(node.key);

  const executionOrder = [];
  if (cycles.length === 0) {
    const orderedState = new Set();
    const orderVisit = (key) => {
      if (orderedState.has(key)) return;
      orderedState.add(key);
      for (const dependency of adjacency.get(key) ?? []) orderVisit(dependency);
      executionOrder.push(key);
    };
    for (const node of nodes) orderVisit(node.key);
  }

  return { nodes, edges, duplicates, malformed, unresolved, cycles, executionOrder };
}

export function graphDiagnostics(graph) {
  const errors = [];
  const warnings = [];
  for (const duplicate of graph.duplicates) {
    errors.push(`${duplicate.key}: duplicate semantic id at blocks ${duplicate.first} and ${duplicate.duplicate}`);
  }
  for (const entry of graph.malformed) {
    errors.push(`${entry.from}: malformed dependency ${JSON.stringify(entry.dependency)} (expected kind:id)`);
  }
  for (const edge of graph.unresolved) {
    warnings.push(`${edge.from}: unresolved ${edge.source} dependency ${edge.to}`);
  }
  for (const cycle of graph.cycles) errors.push(`semantic dependency cycle: ${cycle.join(" -> ")}`);
  return { ok: errors.length === 0, errors, warnings };
}

export function graphToDot(graph) {
  const quote = (value) => JSON.stringify(value);
  const lines = ["digraph streamdown_llm {", "  rankdir=LR;"];
  for (const node of graph.nodes) {
    lines.push(`  ${quote(node.key)} [label=${quote(`${node.kind}:${node.id}`)}];`);
  }
  for (const edge of graph.edges) lines.push(`  ${quote(edge.from)} -> ${quote(edge.to)};`);
  lines.push("}");
  return lines.join("\n");
}
