function validId(value) {
  return /^[A-Za-z0-9_.-]{1,40}$/.test(value);
}

/** Parse a bounded, line-oriented graph DSL. */
export function parseGraph(source) {
  const nodes = new Map();
  const edges = [];
  for (const raw of String(source).split(/\r?\n/).slice(0, 512)) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;
    const parts = line.split(/\s+/);
    const command = parts.shift();
    if (command === "node") {
      const id = parts.shift();
      if (!id || !validId(id) || nodes.size >= 128) continue;
      const label = parts.join(" ").slice(0, 96) || id;
      nodes.set(id, { id, label });
    } else if (command === "edge") {
      const from = parts.shift();
      const to = parts.shift();
      if (!from || !to || !validId(from) || !validId(to) || edges.length >= 256) continue;
      const label = parts.join(" ").slice(0, 64);
      if (!nodes.has(from) && nodes.size < 128) nodes.set(from, { id: from, label: from });
      if (!nodes.has(to) && nodes.size < 128) nodes.set(to, { id: to, label: to });
      if (nodes.has(from) && nodes.has(to)) edges.push({ from, to, label });
    }
  }
  return { nodes: [...nodes.values()], edges };
}

/** Deterministic layered layout; cycles fall into the last layer. */
export function layoutGraph(graph, width = 640, height = 320) {
  const nodes = graph.nodes.map(node => ({ ...node }));
  const byId = new Map(nodes.map(node => [node.id, node]));
  const incoming = new Map(nodes.map(node => [node.id, 0]));
  const outgoing = new Map(nodes.map(node => [node.id, []]));
  for (const edge of graph.edges) {
    if (!byId.has(edge.from) || !byId.has(edge.to)) continue;
    outgoing.get(edge.from).push(edge.to);
    incoming.set(edge.to, incoming.get(edge.to) + 1);
  }

  const level = new Map();
  const queue = nodes.filter(node => incoming.get(node.id) === 0).map(node => node.id);
  for (const id of queue) level.set(id, 0);
  for (let head = 0; head < queue.length; head++) {
    const id = queue[head];
    const base = level.get(id) || 0;
    for (const next of outgoing.get(id)) {
      level.set(next, Math.max(level.get(next) || 0, base + 1));
      incoming.set(next, incoming.get(next) - 1);
      if (incoming.get(next) === 0) queue.push(next);
    }
  }
  const resolvedMax = Math.max(0, ...level.values());
  for (const node of nodes) if (!level.has(node.id)) level.set(node.id, resolvedMax + 1);

  const groups = new Map();
  for (const node of nodes) {
    const l = level.get(node.id);
    if (!groups.has(l)) groups.set(l, []);
    groups.get(l).push(node);
  }
  const levels = [...groups.keys()].sort((a, b) => a - b);
  const padX = 72;
  const padY = 48;
  const spanX = Math.max(1, width - padX * 2);
  const spanY = Math.max(1, height - padY * 2);
  for (let li = 0; li < levels.length; li++) {
    const group = groups.get(levels[li]);
    const x = levels.length === 1 ? width / 2 : padX + spanX * li / (levels.length - 1);
    for (let i = 0; i < group.length; i++) {
      const y = group.length === 1 ? height / 2 : padY + spanY * i / (group.length - 1);
      group[i].x = x;
      group[i].y = y;
    }
  }
  return { nodes, edges: graph.edges.map(edge => ({ ...edge })), width, height };
}
