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
assert.deepEqual(decisions(["plain", "\n", "plain"]), [false, false, false]);
assert.deepEqual(decisions([":::", "\n"]), [false, true]);
assert.deepEqual(decisions([":::", "   ", "\n"]), [false, false, true]);
assert.deepEqual(decisions([":::llm tool id=x\r", "\n"]), [false, true]);
assert.deepEqual(decisions(["plain\r", "\n"]), [false, false]);

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
reset.reset();
assert.equal(reset.shouldObserve("m tool id=x"), false);

console.log("semantic detector: ok");
