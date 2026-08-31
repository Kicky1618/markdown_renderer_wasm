# stream-mecab

Clean-room, streaming-first Japanese morphological tokenizer.

This crate does **not** link to MeCab/libmecab, copy MeCab source code, bundle IPA/JUMAN/UniDic data, or consume their dictionary binaries. The implementation is original and dependency-free. The crate itself is `MIT OR Apache-2.0`; dictionary data is a separate input and its license must be checked independently.

## Why it is streaming-first

`StreamAnalyzer::append()` returns only a token-tail delta:

- `retract`: remove N tokens from the end of the previously published result.
- `push`: append replacement tokens.

The analyzer Viterbi-parses only an ambiguous tail. A prefix is permanently committed only when every live frontier path shares it and it is farther from the stream edge than the maximum candidate length. Committed input is drained and is never parsed again.

For high-frequency callers use `append_into()` with a reusable `StreamDelta`; this keeps the delta vector capacity between calls.

`Model::stream_delta()` returns `DeltaStreamAnalyzer`, the bounded-memory variant used by the WASM ABI. It assumes the consumer applies every delta, so committed token history is not duplicated inside the analyzer. Only the provisional tail remains buffered.

## Engine

The implementation uses:

- a UTF-8 byte trie with small sorted edge vectors;
- first-order tag transition costs;
- bounded unknown-word candidates by Unicode character class;
- incremental Viterbi with backpointer-LCA stable-prefix detection;
- `Arc<str>` metadata sharing for dictionary tokens;
- reusable frontier/path/candidate/best-path scratch arenas;
- a reusable `retract + push` delta hot path.

No external Rust crate is required.

## Dictionary formats

The source TSV format is intentionally not MeCab-compatible:

```text
surface<TAB>lemma<TAB>reading<TAB>tag-id<TAB>word-cost
```

Tag IDs `0..=8` are reserved for BOS/EOS and built-in unknown classes. User tags start at `FIRST_USER_TAG` (`9`). Transition costs are configured with `Model::set_transition(previous, next, cost)`.

For deployment, compile TSV to the crate's own `SMD1` binary format:

```bash
cargo run --manifest-path stream_mecab/Cargo.toml --release \
  --example compile_dict -- dictionary.tsv dictionary.smd1
```

`SMD1` stores this crate's entries, UTF-8 trie and transitions directly. It is not compatible with MeCab/IPADIC/UniDic formats.

## Rust

```rust
use stream_mecab::{Model, StreamDelta};

let mut model = Model::new();
model.add_entry("東京大学", "東京大学", "トウキョウダイガク", 9, 100)?;

let mut analyzer = model.stream_delta();
let mut delta = StreamDelta::default();
analyzer.append_into("東京", &mut delta);
analyzer.append_into("大学", &mut delta);
# Ok::<(), stream_mecab::ModelError>(())
```

Use `Model::stream()` instead when the analyzer itself should retain the complete token history and expose it through `tokens()`.

## Raw WASM

The crate builds as `rlib + cdylib` with no wasm-bindgen dependency:

```bash
cargo build --manifest-path stream_mecab/Cargo.toml \
  --release --target wasm32-unknown-unknown
```

`js/stream_mecab.js` directly instantiates the raw WebAssembly module. It supports both source TSV (`addTsv`) and compiled SMD1 (`loadCompiled`) and decodes compact `SMT1` token deltas.

## Verification

```bash
cargo test --manifest-path stream_mecab/Cargo.toml --release
cargo clippy --manifest-path stream_mecab/Cargo.toml --all-targets -- -D warnings
cargo build --manifest-path stream_mecab/Cargo.toml --release --target wasm32-unknown-unknown
node stream_mecab/tests/wasm.mjs
node stream_mecab/tests/wasm_compiled.mjs
```

The tests include UTF-8 split invariance, per-character streaming-vs-batch equivalence, bounded-tail checks, SMD1 round trips/corruption rejection, SMT1 wire validation, and raw-WASM end-to-end tests.

## JavaScript fast paths

`StreamMecab.append(text)` decodes the complete token metadata (`surface`, `lemma`, `reading`, tag, cost, origin). If a caller only needs token boundaries/surfaces, `appendSurfaces(text)` skips lemma/reading decoding and returns the same `retract + push` shape with strings in `push`.

The raw WASM API uses the same reusable input/output buffers as the native hot path. `Model::stream_delta()` / the WASM handle use the history-free analyzer: committed tokens are not retained internally, so analyzer memory is bounded by the ambiguous tail instead of total document length.

## Compiled dictionary

`Model::to_compiled()` emits the project-specific `SMD1` format. `Model::from_compiled()` validates and loads it. This format is intentionally unrelated to MeCab/IPADIC/UniDic formats. The `compile_dict` example converts this crate's TSV format to SMD1; WASM can load SMD1 directly with `StreamMecab.loadCompiled(bytes)`, avoiding runtime TSV parsing.

## Measured performance

Environment: Intel Core i7-12700, release build, CPU affinity fixed to logical CPU 2, 160,000 appends per run, 9 runs. On the synthetic overlapping Japanese test lexicon used by the included benchmarks:

- Native history-retaining stream: median ~1.595 M append/s.
- Native history-free `stream_delta()`: median ~1.630 M append/s, max 4 buffered tokens and 27-byte reparsed tail.
- Raw WASM + JavaScript full metadata decode: median ~0.499 M append/s.
- Raw WASM + JavaScript surface-only decode: median ~0.638 M append/s.

These numbers measure the included synthetic dictionary, not a production Japanese dictionary. Dictionary size, ambiguity and transition density materially affect throughput.

## Licensing boundary

The engine source is dual-licensed under MIT or Apache-2.0; see `LICENSE-MIT` and `LICENSE-APACHE`. No MeCab source, MeCab dictionary binary, IPADIC, UniDic or JUMAN dictionary data is bundled. Dictionary data supplied by an application is a separate work and its license must be checked independently.
