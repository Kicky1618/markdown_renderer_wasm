# Streamdown developer tools

## `streamdown-inspect.mjs`

`streamdown-inspect.mjs` runs the actual WASM parser against a Markdown file or stdin and prints a JSON report. It is intended for debugging LLM streaming protocols, not for rendering.

```sh
./scripts/build_wasm.sh
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

For execution-only or telemetry paths, snapshot components can be skipped independently. The default remains the full detached result, while `document: false` avoids the AST clone, `graph: false` / `diagnostics: false` avoid graph serialization, and `scheduler: false` avoids materializing scheduler records. `finish()` and `idle()` accept the same options; `consume()` forwards them through `snapshotOptions`.

```js
const result = await runtime.finish({
  document: false,
  graph: false,
  diagnostics: false,
  scheduler: false,
});
// { blockCount, semanticScans }
```

`node tools/semantic-runtime-snapshot-bench.mjs` measures these output costs. On the development host with 25k semantic blocks, the full snapshot took about 44.6 ms while the metadata-only form took about 0.07 ms.

```sh
node tools/semantic-runtime.integration.mjs
```

The integration test deliberately stalls `tool:search` until the parser has already observed the later `artifact:summary` block, proving that Markdown ingestion and semantic execution overlap.


### Incremental semantic scanning

`SemanticRuntime` defaults to `semanticScan: "incremental"`. Ordinary token chunks still go through `Streamdown.appendInPlace()`, but semantic graph/timeline reconstruction is skipped unless a chunk can change a `:{3,}llm` header, closing fence, or `@[kind:id]` reference. Use `semanticScan: "always"` as a correctness/reference mode.

```js
const runtime = await SemanticRuntime.load(wasm, {
  semanticScan: "incremental",
  runners,
});
```

The detector tracks split headers such as `:::ll` + `m tool id=x`, arbitrarily long header attributes, UTF-8 byte offsets, and semantic references without retaining ordinary long lines. A small `@[kind:id]` recognizer prevents ordinary Markdown links, checkboxes, and citations from causing graph rebuilds merely because they contain `]`. Regression tests compare incremental and always-scan events across fixed and pseudo-random chunk boundaries.

```sh
node tools/semantic-detector.test.mjs
node tools/semantic-runtime.incremental.mjs target/wasm32-unknown-unknown/release/streamdown.wasm
N=500000 REPEATS=7 node tools/semantic-runtime-bench.mjs
CHUNK="token " N=500000 REPEATS=7 node tools/semantic-runtime-bench.mjs
```

On the i7-12700 development host, the 500k-token / 7-repeat median measured about 6.10M append/s for incremental `SemanticRuntime` versus 1.21M append/s for always-scan mode and 6.71M append/s for bare `appendInPlace()` (about 5.0x over always-scan and 91% of bare-parser throughput).


## `semantic-state.mjs`

`:::llm state` initializes JSON state and `:::llm patch` updates it without adding a new parser AST or MDA1 variant. Patches default to RFC 7396 JSON Merge Patch semantics: object keys merge recursively, `null` deletes a key, and arrays/scalars replace the target value.

```md
:::llm state id=session
{"count":0,"status":"warming"}
:::

:::llm patch id=ready target=state:session depends=state:session if_revision=1
{"count":1,"status":"ready"}
:::
```

Patch ordering is expressed in the same semantic DAG. Multiple updates to one state should form a dependency chain (`patch:b depends=patch:a`) so concurrent scheduler execution cannot reorder them. `if_revision=N` adds an optimistic concurrency guard: the patch fails with `SemanticRevisionConflictError` unless the target state is still at revision `N`. `format=replace` replaces the whole state; `merge`, `merge-patch`, and `application/merge-patch+json` use Merge Patch. Unsafe JSON keys such as `__proto__`, `prototype`, and `constructor` are rejected.

The state store exposes monotonically increasing revisions and clones values at its public boundary so runner consumers cannot mutate canonical state by retaining a reference.

```js
import { SemanticStateStore, createStateRunners } from "./tools/semantic-state.mjs";

const store = new SemanticStateStore({
  onChange: ({ key, revision, value }) => updateUi(key, revision, value),
});
const runtime = await SemanticRuntime.load(wasm, {
  runners: { ...createStateRunners(store), ...otherRunners },
});
```

`streamdown-inspect.mjs --validate` additionally checks state/patch JSON safety, targets, formats, dependency paths, and warns when same-state patches are not serialized.

```sh
node tools/semantic-state.test.mjs
node tools/semantic-state.integration.mjs
node tools/streamdown-inspect.mjs examples/llm_state.md --chunk=5 --verify --validate --graph
```


## `stateful-semantic-runtime.mjs`

`StatefulSemanticRuntime` is a thin convenience wrapper around `SemanticRuntime`. It owns the built-in `state` and `patch` runner kinds, keeps the canonical `SemanticStateStore`, and adds state values plus revision numbers to every snapshot. Other semantic runners continue to use the normal scheduler and can consume completed patch results through `dependencyResults`.

```js
import { StatefulSemanticRuntime } from "./tools/stateful-semantic-runtime.mjs";

const runtime = await StatefulSemanticRuntime.load(wasm, {
  onStateChange: ({ key, revision, value }) => updateUi(key, revision, value),
  runners: {
    artifact: async (node, { dependencyResults }) => ({
      config: JSON.parse(node.value),
      state: dependencyResults["patch:ready"],
    }),
  },
});

const result = await runtime.consume(providerChunks);
console.log(result.state.values["state:session"]);
console.log(result.state.revisions["state:session"]);
```

The wrapper deliberately reserves the `state` and `patch` runner kinds so its snapshot always reflects the state actually applied by the scheduler. Use the base `SemanticRuntime` directly when custom state semantics are required. A `SemanticJournal` can also be auto-wired at construction time. `journalScheduler: "terminal"` keeps only completed/failed/blocked scheduler outcomes in the persisted journal while optional callbacks still receive every transition.

```js
const journal = new SemanticJournal();
const runtime = await StatefulSemanticRuntime.load(wasm, {
  journal,
  journalScheduler: "terminal",
  journalStateEncoding: "delta",
  runners,
});
await runtime.consume(providerChunks);
console.log(journal.toNDJSON());
```

`journalStateEncoding: "snapshot"` (the compatibility default) stores the full canonical state at every revision. `"delta"` stores the initial state snapshot once and then records the original Merge Patch / replace payload for later revisions. Replay reconstructs the same canonical state and journal verification compares reconstructed patch results with scheduler completion results. Delta encoding is especially useful when a large state receives many small patches.

```sh
node tools/stateful-semantic-runtime.integration.mjs
```


## Semantic journal and replay

`SemanticJournal` records scheduler transitions and canonical state revisions as JSON-only append entries. The journal can be streamed or stored as NDJSON beside an LLM transcript, then verified and replayed without loading the Markdown parser or WASM module. State entries contain the full canonical value at each revision; scheduler entries record status transitions and JSON-serializable results when available.

```js
import { SemanticJournal, createSemanticJournalHooks } from "./tools/semantic-journal.mjs";
import { StatefulSemanticRuntime } from "./tools/stateful-semantic-runtime.mjs";

const journal = new SemanticJournal();
const hooks = createSemanticJournalHooks(journal);
const runtime = await StatefulSemanticRuntime.load(wasm, {
  onTransition: hooks.onTransition,
  onStateChange: hooks.onStateChange,
  runners,
});

await runtime.consume(providerChunks);
await fs.writeFile("run.ndjson", journal.toNDJSON());
```

`verify()` checks global entry ordering, monotonic state revisions, scheduler sequence ordering, and consistency between state changes and completed state/patch runner results. `replayState()` reconstructs final state/revisions from journal entries only. Non-JSON tool results are marked `resultOmitted` instead of breaking journal recording.

For long-lived sessions, `createSemanticJournalHooks()` can reduce persisted scheduler detail without changing downstream callbacks. The default `scheduler: "all"` records every transition. `scheduler: "terminal"` keeps only `completed` / `failed` / `blocked`, while `scheduler: "none"` stores state revisions only and remains sufficient for deterministic state replay.

```js
const compactHooks = createSemanticJournalHooks(journal, { scheduler: "terminal" });
const replayOnlyHooks = createSemanticJournalHooks(journal, { scheduler: "none" });
```

A synthetic 50k-update benchmark can be reproduced with `node tools/semantic-journal-bench.mjs`. On the development host, full journaling used about 713 bytes/update, terminal-only about 342 bytes/update, and state-only about 168 bytes/update. These entries are emitted on semantic transitions, not on ordinary LLM token appends.

For state-size scaling, `node tools/semantic-journal-state-bench.mjs` compares full state snapshots with delta encoding. With a 64 KiB state and 1000 small patches, the local run produced about 65.8 MB of snapshot NDJSON versus 226 KB of delta NDJSON, a 99.66% reduction. Override `STATE_BYTES` and `N` to reproduce other workloads.

```sh
node tools/semantic-replay.mjs run.ndjson
node tools/semantic-replay.mjs run.ndjson --state-only
node tools/semantic-replay.mjs run.ndjson --entries
```

Replay verification failures exit with status 3. `--no-verify` is available for forensic inspection of a damaged journal but should not be used for trusted replay.

```sh
node tools/semantic-journal.test.mjs
node tools/semantic-journal.integration.mjs
node tools/semantic-replay.test.mjs
```
