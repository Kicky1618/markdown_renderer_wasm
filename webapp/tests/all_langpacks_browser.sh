#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
CHROME=${CHROME:-}
if [ -z "$CHROME" ]; then
  for candidate in google-chrome-stable google-chrome chromium chromium-browser; do
    if command -v "$candidate" >/dev/null 2>&1; then
      CHROME=$(command -v "$candidate")
      break
    fi
  done
fi
if [ -z "$CHROME" ]; then
  echo "all langpacks browser: skipped (Chrome/Chromium not found)"
  exit 0
fi

if [ "${SKIP_BUILD:-0}" != "1" ]; then
  node "$ROOT/build_langpacks.mjs"
  TMPDIR="${TMPDIR:-$ROOT/../target/tmp}" cargo build \
    --manifest-path "$ROOT/Cargo.toml" \
    --release \
    --target wasm32-unknown-unknown
  wasm-bindgen "$ROOT/target/wasm32-unknown-unknown/release/streamdown_web.wasm" \
    --out-dir "$ROOT/pkg" \
    --target web \
    --no-typescript
fi

PORT=${PORT:-18769}
TMP_BASE=${TMPDIR:-/tmp}
mkdir -p "$TMP_BASE"
WORK=$(mktemp -d "$TMP_BASE/streamdown-all-langpacks.XXXXXX")
SERVER_PID=
cleanup() {
  if [ -n "$SERVER_PID" ]; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  rm -rf "$WORK"
}
trap cleanup EXIT INT TERM

python3 -m http.server "$PORT" --bind 127.0.0.1 --directory "$ROOT" >"$WORK/http.log" 2>&1 &
SERVER_PID=$!

python3 - "$PORT" <<'PY'
import sys, time, urllib.request
port = int(sys.argv[1])
url = f"http://127.0.0.1:{port}/tests/all_langpacks_browser.html"
for _ in range(100):
    try:
        with urllib.request.urlopen(url, timeout=0.2) as response:
            if response.status == 200:
                raise SystemExit(0)
    except Exception:
        time.sleep(0.05)
raise SystemExit("all langpacks browser: HTTP server did not start")
PY

HTML="$WORK/page.html"
ERR="$WORK/chrome.err"
"$CHROME" \
  --headless=new \
  --no-sandbox \
  --disable-dev-shm-usage \
  --virtual-time-budget=90000 \
  --dump-dom \
  "http://127.0.0.1:$PORT/tests/all_langpacks_browser.html" \
  >"$HTML" 2>"$ERR" || true

if ! grep -q 'data-all-langpacks-probe="pass"' "$HTML"; then
  echo "all langpacks browser: probe failed"
  grep -o '<html[^>]*>' "$HTML" | head -1 || true
  tail -30 "$ERR" || true
  tail -30 "$WORK/http.log" || true
  exit 1
fi

count=$(grep -o 'data-all-langpacks-count="[0-9]*"' "$HTML" | sed 's/[^0-9]//g')
echo "all langpacks browser: $count canonical packs registered in real Chrome"
