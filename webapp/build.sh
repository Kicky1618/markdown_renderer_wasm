#!/usr/bin/env sh
set -eu
cargo build --manifest-path webapp/Cargo.toml --release --target wasm32-unknown-unknown
wasm-bindgen webapp/target/wasm32-unknown-unknown/release/streamdown_web.wasm \
  --out-dir webapp/pkg --target web --no-typescript
