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
- `--validate`: check that semantic fences are closed, `(kind,id)` pairs are unique, and JSON-looking / JSON-declared payloads parse successfully. Validation errors exit with status 3.
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

## `wasm-bench.mjs`

`wasm-bench.mjs` measures the JavaScript-facing hot path end to end: UTF-8 encoding, WASM input transport, Rust parsing, MDA1 encoding/decoding, and JavaScript AST mirror updates. This complements `cargo run --release --bin stream-bench`, which measures the Rust parser itself.

```sh
node tools/wasm-bench.mjs --rounds=100000 --warmup=5000
node tools/wasm-bench.mjs --rounds=100000 --json
```

It currently measures plain token streaming, syntax-heavy Markdown boundaries, an open fenced code block, and a `:::llm` semantic payload. Each scenario is measured through both `append()` and the allocation-light `appendInPlace()` path when available. Use `--wasm=PATH` to compare two WASM builds without changing the tool.
