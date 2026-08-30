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
  echo "language matrix browser: skipped (Chrome/Chromium not found)"
  exit 0
fi

if [ "${SKIP_BUILD:-0}" != "1" ]; then
  (cd "$ROOT/.." && ./webapp/build.sh)
fi

if [ -z "${PORT:-}" ]; then
  PORT=$(python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
)
fi

TMP_BASE=${TMPDIR:-/tmp}
mkdir -p "$TMP_BASE"
WORK=$(mktemp -d "$TMP_BASE/streamdown-language-matrix-browser.XXXXXX")
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
url = f"http://127.0.0.1:{port}/tests/language_matrix_browser.html"
for _ in range(80):
    try:
        with urllib.request.urlopen(url, timeout=0.2) as response:
            if response.status == 200:
                raise SystemExit(0)
    except Exception:
        time.sleep(0.05)
raise SystemExit("language matrix browser: HTTP server did not start")
PY

HTML="$WORK/page.html"
ERR="$WORK/chrome.err"
"$CHROME" \
  --headless=new \
  --no-sandbox \
  --disable-dev-shm-usage \
  --virtual-time-budget=30000 \
  --dump-dom \
  "http://127.0.0.1:$PORT/tests/language_matrix_browser.html" \
  >"$HTML" 2>"$ERR" || true

grep -q 'data-language-matrix-probe="pass"' "$HTML" || {
  echo "language matrix browser: probe failed"
  grep -o '<html[^>]*>' "$HTML" | head -1 || true
  tail -30 "$ERR" || true
  cat "$WORK/http.log"
  exit 1
}

index_count=$(grep -c 'GET /langpacks/_index.slp HTTP/1.1.* 200' "$WORK/http.log" || true)
if [ "$index_count" -ne 1 ]; then
  echo "language matrix browser: alias index fetched $index_count times (expected once)"
  cat "$WORK/http.log"
  exit 1
fi

for alias in kt c%23 c%2B%2B f%23 rb hs sv webgpu tf ps1; do
  count=$(grep -c "GET /langpacks/$alias.slp?v=[0-9a-f]* HTTP/1.1.* 200" "$WORK/http.log" || true)
  if [ "$count" -ne 1 ]; then
    echo "language matrix browser: alias $alias pack fetched $count times (expected once)"
    cat "$WORK/http.log"
    exit 1
  fi
done

if grep -q 'GET /langpacks/.*\.\.' "$WORK/http.log"; then
  echo "language matrix browser: unsafe path probe reached HTTP server"
  cat "$WORK/http.log"
  exit 1
fi

echo "language matrix browser: 10 expanded/special aliases + dedupe pass"
