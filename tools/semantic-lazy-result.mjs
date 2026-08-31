const LAZY_SEMANTIC_RESULT = Symbol("streamdown.lazySemanticResult");

export function createLazySemanticResult(factory) {
  if (typeof factory !== "function") throw new TypeError("lazy semantic result factory must be a function");
  let materialized = false;
  let value;
  return Object.freeze({
    [LAZY_SEMANTIC_RESULT]: true,
    materialize() {
      if (!materialized) {
        value = factory();
        materialized = true;
      }
      return value;
    },
    get materialized() { return materialized; },
  });
}

export function isLazySemanticResult(value) {
  return !!value && typeof value === "object" && value[LAZY_SEMANTIC_RESULT] === true
    && typeof value.materialize === "function";
}
