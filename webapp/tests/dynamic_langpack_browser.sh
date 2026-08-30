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
  echo "dynamic langpack browser: skipped (Chrome/Chromium not found)"
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
WORK=$(mktemp -d "$TMP_BASE/streamdown-langpack-browser.XXXXXX")
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
url = f"http://127.0.0.1:{port}/"
for _ in range(80):
    try:
        with urllib.request.urlopen(url, timeout=0.2) as response:
            if response.status == 200:
                raise SystemExit(0)
    except Exception:
        time.sleep(0.05)
raise SystemExit("dynamic langpack browser: HTTP server did not start")
PY

HTML="$WORK/page.html"
ERR="$WORK/chrome.err"
"$CHROME" \
  --headless=new \
  --no-sandbox \
  --disable-dev-shm-usage \
  --virtual-time-budget=30000 \
  --dump-dom \
  "http://127.0.0.1:$PORT/tests/langpack_probe.html?renderer=canvas2d&doc=easy&tps=1000000&repeat=1&fade=0" \
  >"$HTML" 2>"$ERR" || true

grep -q 'data-langpack-probe="pass"' "$HTML" || {
  echo "dynamic langpack browser: probe did not complete successfully"
  grep -o '<html[^>]*>' "$HTML" | head -1 || true
  tail -30 "$ERR" || true
  cat "$WORK/http.log"
  exit 1
}

js_count=$(grep -c 'GET /langpacks/javascript.langpack HTTP/1.1.* 200' "$WORK/http.log" || true)
if [ "$js_count" -ne 1 ]; then
  echo "dynamic langpack browser: javascript fetched $js_count times (expected once for TypeScript + ts aliases)"
  cat "$WORK/http.log"
  exit 1
fi

if grep -q 'GET /langpacks/.*rust' "$WORK/http.log"; then
  echo "dynamic langpack browser: unsafe ../rust probe reached the HTTP server"
  cat "$WORK/http.log"
  exit 1
fi

echo "dynamic langpack browser: alias dedupe + binary registration + path sanitization pass"
