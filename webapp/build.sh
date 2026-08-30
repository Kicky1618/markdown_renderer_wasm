#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
WEBAPP="$ROOT/webapp"
TMP="$ROOT/target/tmp"

mkdir -p "$TMP" "$WEBAPP/generative"

node "$WEBAPP/build_langpacks.mjs"

# Existing high-performance renderer.
TMPDIR="$TMP" cargo build \
  --manifest-path "$WEBAPP/Cargo.toml" \
  --release \
  --target wasm32-unknown-unknown
wasm-bindgen "$WEBAPP/target/wasm32-unknown-unknown/release/streamdown_web.wasm" \
  --out-dir "$WEBAPP/pkg" \
  --target web \
  --no-typescript

# Core incremental parser used by the Generative UI mode.
TMPDIR="$TMP" cargo build \
  --manifest-path "$ROOT/Cargo.toml" \
  --release \
  --target wasm32-unknown-unknown
cp "$ROOT/target/wasm32-unknown-unknown/release/streamdown.wasm" \
  "$WEBAPP/generative/streamdown.wasm"
cp "$ROOT/js/streamdown.js" "$WEBAPP/generative/streamdown.js"

echo "built viewer:      $WEBAPP/"
echo "built generative: $WEBAPP/generative/"
