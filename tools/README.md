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


## `semantic-timeline.mjs`

`semantic-timeline.mjs` replays the real WASM parser and records when semantic blocks become visible, close, become dependency-ready, and are referenced from Markdown. This is useful for tool/artifact/UI runtimes that want to start work as soon as a streamed block is safe to consume.

```sh
node tools/semantic-timeline.mjs examples/llm_graph.md --chunk=5
node tools/semantic-timeline.mjs examples/llm_graph.md --chunk=5 --ndjson
```

Events contain `observedAtByte` and `chunkIndex`. A node emits `ready` only after it is closed and every declared `depends=kind:id` dependency exists, is closed, and is itself ready. Malformed or unresolved dependencies therefore never become ready.


## `semantic-scheduler.mjs`

`SemanticScheduler` executes semantic `ready` events only after runtime dependencies have completed successfully. Timeline readiness is syntactic; scheduler completion is operational. This prevents an artifact from running merely because its upstream tool block has finished streaming.

```js
import { SemanticScheduler } from "./tools/semantic-scheduler.mjs";

const scheduler = new SemanticScheduler({
  concurrency: 4,
  runners: {
    tool: async (node) => runTool(node),
    artifact: async (node, { dependencyResults }) =>
      buildArtifact(node, dependencyResults),
    ui: async (node, { dependencyResults }) =>
      renderUi(node, dependencyResults),
  },
});

scheduler.updateGraph(graph);
for (const event of timelineEvents) scheduler.accept(event);
await scheduler.idle();
```

Node states are `ready`, `queued`, `running`, `completed`, `failed`, and `blocked`. A failed/missing runner propagates `blocked` through known downstream dependencies. Ready events may arrive out of order; execution still follows completed dependencies. `concurrency` bounds independent branches.

Tests:

```sh
node tools/semantic-scheduler.test.mjs
node tools/semantic-scheduler.integration.mjs
```

The integration test streams `examples/llm_graph.md` through the real WASM parser in 5-byte chunks and verifies `tool:search -> artifact:summary -> ui:metric`, including dependency result passing.


## `semantic-runtime.mjs`

`SemanticRuntime` combines the real Streamdown WASM parser, semantic timeline, dependency graph, and scheduler. Parsing does not wait for semantic runners: a tool may still be running while later Markdown bytes continue to arrive. Downstream artifact/UI execution still waits for successful runtime completion of its dependencies.

```js
import { SemanticRuntime } from "./tools/semantic-runtime.mjs";

const runtime = await SemanticRuntime.load(wasmBytes, {
  concurrency: 4,
  runners: {
    tool: async (node) => executeTool(JSON.parse(node.value)),
    artifact: async (node, { dependencyResults }) =>
      createArtifact(JSON.parse(node.value), dependencyResults),
    ui: async (node, { dependencyResults }) =>
      renderSemanticUi(JSON.parse(node.value), dependencyResults),
  },
});

await runtime.consume(providerChunks);
```

Semantic graph nodes expose the closed fence body as `node.value`, so runners receive the exact streamed payload without reparsing the Markdown source. `snapshot()` returns the final AST, graph diagnostics, and scheduler states.

```sh
node tools/semantic-runtime.integration.mjs
```

The integration test deliberately stalls `tool:search` until the parser has already observed the later `artifact:summary` block, proving that Markdown ingestion and semantic execution overlap.
