#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STUDIO="$ROOT/studio"
WASM="$ROOT/target/wasm32-unknown-unknown/release/streamdown.wasm"
TMP="$ROOT/target/tmp"
mkdir -p "$TMP"

if [[ "${SKIP_BUILD:-0}" != "1" ]]; then
  TMPDIR="$TMP" cargo build --manifest-path "$ROOT/Cargo.toml" --release --target wasm32-unknown-unknown
fi

if [[ ! -f "$WASM" ]]; then
  echo "missing $WASM" >&2
  echo "run rustup target add wasm32-unknown-unknown, then rerun this script" >&2
  exit 1
fi

cp "$WASM" "$STUDIO/streamdown.wasm"
cp "$ROOT/js/streamdown.js" "$STUDIO/streamdown.js"
echo "built $STUDIO"
echo "serve: python3 -m http.server 8080 --directory $STUDIO"
