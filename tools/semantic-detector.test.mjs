import assert from "node:assert/strict";
import { SemanticChangeDetector } from "./semantic-detector.mjs";

function decisions(chunks) {
  const detector = new SemanticChangeDetector();
  return chunks.map((chunk) => detector.shouldObserve(chunk));
}

assert.deepEqual(decisions(["token ", "token ", "token "]), [false, false, false]);
assert.deepEqual(decisions([":::ll", "m tool id=x", " payload", "\n"]), [true, true, true, true]);
assert.deepEqual(decisions(["::::", "ll", "m artifact id=a", " depends=tool:x", "\n"]), [true, true, true, true, true]);
assert.deepEqual(decisions(["before ", "@[artifact:x", "] after"]), [false, true, true]);
assert.deepEqual(decisions(["plain", "\n", "plain"]), [false, true, false]);
assert.deepEqual(decisions([":::", "\n"]), [true, true]);

const reset = new SemanticChangeDetector();
reset.shouldObserve(":::ll");
reset.reset();
assert.equal(reset.shouldObserve("m tool id=x"), false);

console.log("semantic detector: ok");
