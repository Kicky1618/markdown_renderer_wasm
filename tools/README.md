# Streamdown developer tools

## `streamdown-inspect.mjs`

`streamdown-inspect.mjs` runs the actual WASM parser against a Markdown file or stdin and prints a JSON report. It is intended for debugging LLM streaming protocols, not for rendering.

```sh
cargo build --release --target wasm32-unknown-unknown
node tools/streamdown-inspect.mjs examples/llm_protocol.md --chunk=7 --verify --validate
```

Useful options:

- `--chunk=N`: split input into N-byte chunks before feeding it through the streaming `TextDecoder`. This can deliberately cut inside UTF-8 code points.
- `--verify`: compare the selected chunking with both one-shot parsing and one-byte streaming. A mismatch exits with status 2.
- `--validate`: check that semantic fences are closed, `(kind,id)` pairs are unique, JSON-looking payloads parse, `depends=` targets resolve, and the semantic dependency graph is acyclic. Validation errors exit with status 3.
- `--graph`: include semantic graph nodes, edges, unresolved dependencies, cycles, and dependency-first `executionOrder` in JSON output.
- `--dot`: print the semantic dependency graph as Graphviz DOT.
- `--deltas`: include the delta operation names emitted for each streamed chunk.
- `--wasm=PATH`: use a non-default WASM artifact.

The report includes:

- final AST
- `:::llm` semantic blocks and parsed attributes
- `[[cite:...]]` citations
- `@[kind:id]` semantic references
- plain text
- optional chunk-boundary verification and protocol diagnostics

Example from stdin:

```sh
printf '%s\n' 'Fact [[cite:doc-1|spec]].' | \
  node tools/streamdown-inspect.mjs --chunk=1 --verify
```

### Semantic dependencies

LLM blocks can declare local dependencies without changing the core AST or MDA1 wire format:

```md
:::llm tool id=search
{"query":"streaming markdown"}
:::

:::llm artifact id=summary depends=tool:search
{"answer":"..."}
:::

:::llm ui id=result depends=artifact:summary
{"type":"metric"}
:::
```

`depends=` accepts comma-separated `kind:id` values. The inspector resolves these against local semantic blocks, reports dangling references and cycles, and emits a dependency-first execution order suitable for a runtime scheduler. See `examples/llm_graph.md`.

```sh
node tools/streamdown-inspect.mjs examples/llm_graph.md --chunk=5 --verify --validate --graph
node tools/streamdown-inspect.mjs examples/llm_graph.md --dot
```

## `wasm-bench.mjs`

`wasm-bench.mjs` measures the JavaScript-facing hot path end to end: UTF-8 encoding, WASM input transport, Rust parsing, MDA1 encoding/decoding, and JavaScript AST mirror updates. This complements `cargo run --release --bin stream-bench`, which measures the Rust parser itself.

```sh
node tools/wasm-bench.mjs --rounds=100000 --warmup=5000
node tools/wasm-bench.mjs --rounds=100000 --json
```

It currently measures plain token streaming, syntax-heavy Markdown boundaries, an open fenced code block, and a `:::llm` semantic payload. Each scenario is measured through both `append()` and the allocation-light `appendInPlace()` path when available. Use `--wasm=PATH` to compare two WASM builds without changing the tool.
