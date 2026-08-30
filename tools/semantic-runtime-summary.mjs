import { parseLlmDescriptor } from "../js/streamdown.js";

function inlineText(nodes) {
  let text = "";
  for (const node of nodes ?? []) {
    if (node.type === "text" || node.type === "code" || node.type === "math") text += node.value;
    else if (node.type === "softBreak") text += " ";
    else if (node.type === "hardBreak") text += "\n";
    else if (node.children) text += inlineText(node.children);
  }
  return text;
}

function collectSemanticReferences(nodes, block, output) {
  for (const node of nodes ?? []) {
    if (node.type === "link"
      && typeof node.destination === "string"
      && node.destination.startsWith("llm:")
      && !node.destination.startsWith("llm:cite:")) {
      const body = node.destination.slice("llm:".length);
      const colon = body.indexOf(":");
      output.push({
        block,
        kind: colon < 0 ? body : body.slice(0, colon),
        id: colon < 0 ? "" : body.slice(colon + 1),
        label: inlineText(node.children),
      });
    }
    if (node.children) collectSemanticReferences(node.children, block, output);
  }
}

function scanBlock(block, index, llmBlocks, semanticReferences) {
  if (block.type === "codeBlock") {
    const descriptor = parseLlmDescriptor(block.language);
    if (descriptor) {
      llmBlocks.push({
        index,
        kind: descriptor.kind,
        attributes: descriptor.attributes,
        value: block.value,
        closed: block.closed,
      });
    }
  }

  if (block.children) collectSemanticReferences(block.children, index, semanticReferences);
  if (block.items) {
    for (const item of block.items) collectSemanticReferences(item, index, semanticReferences);
  }
  if (block.type === "table") {
    for (const cell of block.headers) collectSemanticReferences(cell, index, semanticReferences);
    for (const row of block.rows) {
      for (const cell of row) collectSemanticReferences(cell, index, semanticReferences);
    }
  }
}

/**
 * Incremental semantic projection of Streamdown's append-only document mirror.
 *
 * Streamdown may rewrite the current live tail block, but committed blocks do
 * not move. Re-scan that old tail plus newly appended blocks and retain the
 * already projected prefix. This removes the O(document) getLlmBlocks/getLinks
 * walk from every semantic observation.
 */
export class SemanticRuntimeSummary {
  constructor(document = []) {
    this.llmBlocks = [];
    this.semanticReferences = [];
    this.documentLength = 0;
    this.refresh(document, 0);
  }

  refreshTail(document, previousBlockCount = this.documentLength) {
    if (!Array.isArray(document)) throw new TypeError("semantic summary expects a document array");
    if (!Number.isSafeInteger(previousBlockCount) || previousBlockCount < 0) {
      throw new TypeError("previous block count must be a non-negative safe integer");
    }
    const dirtyFrom = Math.max(0, Math.min(previousBlockCount, document.length) - 1);
    return this.refresh(document, dirtyFrom);
  }

  refresh(document, dirtyFrom = 0) {
    if (!Array.isArray(document)) throw new TypeError("semantic summary expects a document array");
    if (!Number.isSafeInteger(dirtyFrom) || dirtyFrom < 0) {
      throw new TypeError("dirty block index must be a non-negative safe integer");
    }
    dirtyFrom = Math.min(dirtyFrom, document.length);

    while (this.llmBlocks.length && this.llmBlocks[this.llmBlocks.length - 1].index >= dirtyFrom) {
      this.llmBlocks.pop();
    }
    while (this.semanticReferences.length
      && this.semanticReferences[this.semanticReferences.length - 1].block >= dirtyFrom) {
      this.semanticReferences.pop();
    }

    for (let index = dirtyFrom; index < document.length; index += 1) {
      scanBlock(document[index], index, this.llmBlocks, this.semanticReferences);
    }
    this.documentLength = document.length;
    return this.current();
  }

  current() {
    return {
      llmBlocks: this.llmBlocks,
      semanticReferences: this.semanticReferences,
    };
  }
}
