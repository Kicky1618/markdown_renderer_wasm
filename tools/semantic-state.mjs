const FORBIDDEN_KEYS = new Set(["__proto__", "prototype", "constructor"]);
const PATCH_FORMATS = new Set(["merge", "merge-patch", "application/merge-patch+json", "replace"]);

function isJsonObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function cloneJson(value) {
  if (value === undefined) return undefined;
  return JSON.parse(JSON.stringify(value));
}

const cloneStateValue = typeof globalThis.structuredClone === "function"
  ? (value) => globalThis.structuredClone(value)
  : cloneJson;

function assertSafeJson(value, path = "$") {
  if (Array.isArray(value)) {
    for (let i = 0; i < value.length; i += 1) assertSafeJson(value[i], `${path}[${i}]`);
    return;
  }
  if (!isJsonObject(value)) return;
  for (const [key, child] of Object.entries(value)) {
    if (FORBIDDEN_KEYS.has(key)) throw new Error(`${path}: forbidden JSON key ${JSON.stringify(key)}`);
    assertSafeJson(child, `${path}.${key}`);
  }
}

function parseNodeJson(node) {
  if (!node || typeof node !== "object") throw new TypeError("semantic state runner requires a node");
  if (typeof node.value !== "string") throw new TypeError(`${node.key ?? "semantic node"}: missing JSON payload`);
  let value;
  try {
    value = JSON.parse(node.value.trim());
  } catch (error) {
    throw new SyntaxError(`${node.key ?? "semantic node"}: invalid JSON payload (${error.message})`);
  }
  assertSafeJson(value);
  return value;
}

function normalizeTarget(target) {
  if (typeof target !== "string" || !/^state:[^\s,]+$/.test(target)) {
    throw new Error(`patch target must be state:<id>, got ${JSON.stringify(target)}`);
  }
  return target;
}

export class SemanticRevisionConflictError extends Error {
  constructor(target, expected, actual) {
    super(`${target}: revision conflict (expected ${expected}, actual ${actual})`);
    this.name = "SemanticRevisionConflictError";
    this.target = target;
    this.expected = expected;
    this.actual = actual;
  }
}

function expectedRevision(attributes) {
  const raw = attributes?.if_revision;
  if (raw === undefined) return null;
  if (!/^[1-9][0-9]*$/.test(raw)) {
    throw new Error(`if_revision must be a positive integer, got ${JSON.stringify(raw)}`);
  }
  const value = Number(raw);
  if (!Number.isSafeInteger(value)) throw new Error(`if_revision is too large: ${JSON.stringify(raw)}`);
  return value;
}

/** Apply RFC 7396 JSON Merge Patch without mutating either input. */
export function applyJsonMergePatch(target, patch) {
  assertSafeJson(patch);
  if (!isJsonObject(patch)) return cloneJson(patch);

  const output = isJsonObject(target) ? cloneJson(target) : {};
  for (const [key, patchValue] of Object.entries(patch)) {
    if (patchValue === null) {
      delete output[key];
      continue;
    }
    output[key] = applyJsonMergePatch(output[key], patchValue);
  }
  return output;
}

function applyOwnedJsonMergePatch(target, patch) {
  if (!isJsonObject(patch)) return patch;
  const output = isJsonObject(target) ? { ...target } : {};
  for (const [key, patchValue] of Object.entries(patch)) {
    if (patchValue === null) delete output[key];
    else output[key] = applyOwnedJsonMergePatch(output[key], patchValue);
  }
  return output;
}

/**
 * Mutable state registry for `:::llm state` and `:::llm patch` runners.
 * Values crossing the public API are cloned so runner consumers cannot mutate
 * the canonical state by retaining a reference.
 */
export class SemanticStateStore {
  constructor({ onChange = null } = {}) {
    this.values = new Map();
    this.revisions = new Map();
    this.onChange = onChange;
  }

  has(key) {
    return this.values.has(key);
  }

  get(key) {
    return cloneStateValue(this.values.get(key));
  }

  revision(key) {
    return this.revisions.get(key) ?? 0;
  }

  snapshot() {
    return Object.fromEntries([...this.values].map(([key, value]) => [key, cloneStateValue(value)]));
  }

  revisionSnapshot() {
    return Object.fromEntries(this.revisions);
  }

  initialize(node) {
    if (node?.kind !== "state") throw new Error(`state runner received ${node?.kind ?? "unknown"} node`);
    if (typeof node.key !== "string" || !node.key.startsWith("state:")) throw new Error("state node requires an id");
    if (this.values.has(node.key)) throw new Error(`${node.key}: state is already initialized`);
    const value = parseNodeJson(node);
    return this.#commit(node.key, value, { type: "initialize", node: node.key });
  }

  patch(node) {
    if (node?.kind !== "patch") throw new Error(`patch runner received ${node?.kind ?? "unknown"} node`);
    const target = normalizeTarget(node.attributes?.target);
    if (!this.values.has(target)) {
      throw new Error(`${node.key}: target ${target} is not initialized; declare an execution dependency on it`);
    }
    const expected = expectedRevision(node.attributes);
    const actual = this.revision(target);
    if (expected !== null && expected !== actual) {
      throw new SemanticRevisionConflictError(target, expected, actual);
    }

    const patch = parseNodeJson(node);
    const format = node.attributes?.format ?? node.attributes?.op ?? "merge";
    let next;
    if (["merge", "merge-patch", "application/merge-patch+json"].includes(format)) {
      next = applyOwnedJsonMergePatch(this.values.get(target), patch);
    } else if (format === "replace") {
      next = patch;
    } else {
      throw new Error(`${node.key}: unsupported patch format ${JSON.stringify(format)}`);
    }
    return this.#commit(target, next, {
      type: "patch",
      node: node.key,
      format,
      patch,
    });
  }

  #commit(key, value, metadata) {
    assertSafeJson(value);
    const stored = value;
    this.values.set(key, stored);
    const revision = (this.revisions.get(key) ?? 0) + 1;
    this.revisions.set(key, revision);
    const result = cloneStateValue(stored);
    if (this.onChange) {
      const change = { key, revision, value: cloneStateValue(stored), ...metadata };
      if (Object.prototype.hasOwnProperty.call(metadata, "patch")) change.patch = cloneStateValue(metadata.patch);
      this.onChange(change);
    }
    return result;
  }
}

export function createStateRunners(store = new SemanticStateStore()) {
  if (!(store instanceof SemanticStateStore)) throw new TypeError("createStateRunners expects a SemanticStateStore");
  return {
    state: (node) => store.initialize(node),
    patch: (node) => store.patch(node),
  };
}


function graphDependsOn(graph, from, target) {
  if (from === target) return true;
  const adjacency = new Map(graph.nodes.map((node) => [node.key, []]));
  for (const edge of graph.edges) {
    if (adjacency.has(edge.from) && adjacency.has(edge.to)) adjacency.get(edge.from).push(edge.to);
  }
  const seen = new Set();
  const stack = [from];
  while (stack.length) {
    const key = stack.pop();
    if (seen.has(key)) continue;
    seen.add(key);
    for (const dependency of adjacency.get(key) ?? []) {
      if (dependency === target) return true;
      stack.push(dependency);
    }
  }
  return false;
}

function validateJsonBlock(block, errors) {
  const id = block.attributes.id;
  const where = id ? `${block.kind}:${id}` : `${block.kind}@block${block.index}`;
  let value;
  try {
    value = JSON.parse(block.value.trim());
    assertSafeJson(value);
  } catch (error) {
    errors.push(`${where}: invalid state JSON (${error.message})`);
  }
}

/** Validate the execution contract for `state` and `patch` semantic blocks. */
export function validateSemanticStateProtocol(summary, graph) {
  const errors = [];
  const warnings = [];
  const states = new Map();
  const patchesByTarget = new Map();

  for (const block of summary.llmBlocks ?? []) {
    if (block.kind === "state") {
      const id = block.attributes.id;
      if (!id) errors.push(`state@block${block.index}: state block requires id`);
      else states.set(`state:${id}`, block);
      validateJsonBlock(block, errors);
      continue;
    }
    if (block.kind !== "patch") continue;

    const id = block.attributes.id;
    const where = id ? `patch:${id}` : `patch@block${block.index}`;
    if (!id) errors.push(`${where}: patch block requires id`);
    let target = null;
    try {
      target = normalizeTarget(block.attributes.target);
    } catch (error) {
      errors.push(`${where}: ${error.message}`);
    }
    const format = block.attributes.format ?? block.attributes.op ?? "merge";
    if (!PATCH_FORMATS.has(format)) errors.push(`${where}: unsupported patch format ${JSON.stringify(format)}`);
    if (block.attributes.if_revision !== undefined) {
      try {
        expectedRevision(block.attributes);
      } catch (error) {
        errors.push(`${where}: ${error.message}`);
      }
    }
    validateJsonBlock(block, errors);

    if (target) {
      if (!states.has(target)) errors.push(`${where}: target ${target} has no local state initializer`);
      if (id && graph && !graphDependsOn(graph, `patch:${id}`, target)) {
        errors.push(`${where}: no dependency path reaches ${target}; patch execution may precede state initialization`);
      }
      const list = patchesByTarget.get(target) ?? [];
      list.push(block);
      patchesByTarget.set(target, list);
    }
  }

  if (graph) {
    for (const [target, patches] of patchesByTarget) {
      patches.sort((a, b) => a.index - b.index);
      for (let i = 1; i < patches.length; i += 1) {
        const previous = patches[i - 1];
        const current = patches[i];
        const previousId = previous.attributes.id;
        const currentId = current.attributes.id;
        if (!previousId || !currentId) continue;
        if (!graphDependsOn(graph, `patch:${currentId}`, `patch:${previousId}`)) {
          warnings.push(`patch:${currentId}: updates ${target} without depending on earlier patch:${previousId}; concurrent execution may reorder updates`);
        }
      }
    }
  }

  return { ok: errors.length === 0, errors, warnings };
}
