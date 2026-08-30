#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
TARGET_DIR=${CARGO_TARGET_DIR:-"$ROOT/target"}
case "$TARGET_DIR" in
  /*) ;;
  *) TARGET_DIR="$ROOT/$TARGET_DIR" ;;
esac

OUT_DIR="$TARGET_DIR/wasm32-unknown-unknown/release"
OUT="$OUT_DIR/streamdown.wasm"
mkdir -p "$OUT_DIR"

# The core crate is rlib-only so native `cargo test` never races two unhashed
# cdylib/test outputs. Request cdylib only for this wasm build and write the
# linked module to the stable path consumed by js/streamdown.js and tools.
CARGO_TARGET_DIR="$TARGET_DIR" cargo rustc \
  --manifest-path "$ROOT/Cargo.toml" \
  --release \
  --target wasm32-unknown-unknown \
  --lib \
  -- \
  --crate-type=cdylib \
  "--emit=link=$OUT"

printf 'built %s\n' "$OUT"
