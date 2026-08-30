import assert from "node:assert/strict";
import { SemanticChangeDetector } from "./semantic-detector.mjs";

function decisions(chunks) {
  const detector = new SemanticChangeDetector();
  return chunks.map((chunk) => detector.shouldObserve(chunk));
}

assert.deepEqual(decisions(["token ", "token ", "token "]), [false, false, false]);
assert.deepEqual(decisions([":::ll", "m tool id=x", " payload", "\n"]), [false, false, false, true]);
assert.deepEqual(decisions(["::::", "ll", "m artifact id=a", " depends=tool:x", "\n"]), [false, false, false, false, true]);
assert.deepEqual(decisions(["before ", "@[artifact:x", "] after"]), [false, false, true]);
assert.deepEqual(decisions(["before @", "[artifact:x", "] after"]), [false, false, true]);
assert.deepEqual(decisions(["plain", "\n", "plain"]), [false, false, false]);
assert.deepEqual(decisions([":::", "\n"]), [false, true]);
assert.deepEqual(decisions([":::", "   ", "\n"]), [false, false, true]);
assert.deepEqual(decisions([":::llm tool id=x\r", "\n"]), [false, true]);
assert.deepEqual(decisions(["plain\r", "\n"]), [false, false]);
assert.deepEqual(
  decisions([":::llm tool id=x\n", "@[artifact:hidden]\n", '{"x":1}\n', ":::\n", "tail @[artifact:y", "]"]),
  [true, false, false, true, false, true],
);

// Ordinary Markdown closers and citations are not semantic runtime triggers.
assert.deepEqual(decisions(["[link](https://example.com)", " [x] ", "[[cite:doc]]"]), [false, false, false]);
assert.deepEqual(decisions(["@not-a-ref]", " @[bad kind:id]", " @[kind:]", " @[kind:id|"]), [false, false, false, false]);

// The semantic reference recognizer follows the parser grammar across chunks.
assert.deepEqual(decisions(["@", "[source:", "turn7search2", "]"]), [false, false, false, true]);
assert.deepEqual(decisions(["@[bad ", "@[source:ok", "]"]), [false, false, true]);

const boundedWhitespace = new SemanticChangeDetector();
for (let i = 0; i < 10000; i += 1) boundedWhitespace.shouldObserve(" ");
assert.equal(boundedWhitespace.linePrefix, "");

const boundedColons = new SemanticChangeDetector();
for (let i = 0; i < 10000; i += 1) boundedColons.shouldObserve(":");
assert.equal(boundedColons.linePrefix, ":::");
assert.equal(boundedColons.shouldObserve("llm tool id=x"), false);
assert.equal(boundedColons.shouldObserve("\n"), true);

const reset = new SemanticChangeDetector();
reset.shouldObserve(":::ll");
reset.shouldObserve("@[");
reset.reset();
assert.equal(reset.shouldObserve("m tool id=x"), false);
assert.equal(reset.referenceState, 0);

const knownBytes = new SemanticChangeDetector();
assert.equal(knownBytes.scan("日本語", 9), 9);
assert.throws(() => knownBytes.scan("x", -1), /non-negative safe integer/);

console.log("semantic detector: ok");
