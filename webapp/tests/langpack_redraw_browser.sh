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
  echo "langpack redraw browser: skipped (Chrome/Chromium not found)"
  exit 0
fi

if [ "${SKIP_BUILD:-0}" != "1" ]; then
  (cd "$ROOT/.." && ./webapp/build.sh)
fi

PORT=${PORT:-}
if [ -z "$PORT" ]; then
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
WORK=$(mktemp -d "$TMP_BASE/streamdown-langpack-redraw.XXXXXX")
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
url = f"http://127.0.0.1:{port}/tests/langpack_redraw_browser.html"
for _ in range(80):
    try:
        with urllib.request.urlopen(url, timeout=0.2) as response:
            if response.status == 200:
                raise SystemExit(0)
    except Exception:
        time.sleep(0.05)
raise SystemExit("langpack redraw browser: HTTP server did not start")
PY

HTML="$WORK/page.html"
ERR="$WORK/chrome.err"
"$CHROME" \
  --headless=new \
  --no-sandbox \
  --disable-dev-shm-usage \
  --disable-extensions \
  --disable-background-networking \
  --user-data-dir="$WORK/profile" \
  --use-angle=swiftshader \
  --enable-unsafe-swiftshader \
  --run-all-compositor-stages-before-draw \
  --virtual-time-budget=15000 \
  --dump-dom \
  "http://127.0.0.1:$PORT/tests/langpack_redraw_browser.html" \
  >"$HTML" 2>"$ERR" || true

grep -q 'data-langpack-redraw-probe="pass"' "$HTML" || {
  echo "langpack redraw browser: probe failed"
  grep -o '<html[^>]*>' "$HTML" | head -1 || true
  tail -40 "$ERR" || true
  cat "$WORK/http.log"
  exit 1
}
grep -q 'data-langpack-redraw-renderer="webgl"' "$HTML" || {
  echo "langpack redraw browser: WebGL2 renderer was not exercised"
  grep -o '<html[^>]*>' "$HTML" | head -1 || true
  exit 1
}
mutations=$(grep -o 'data-langpack-redraw-mutations="[0-9]*"' "$HTML" | head -1 | cut -d '"' -f2)
[ "${mutations:-0}" -ge 2 ] || {
  echo "langpack redraw browser: redraw handler saw ${mutations:-0} pause mutations"
  exit 1
}

echo "langpack redraw browser: WebGL langpack redraw reached renderer with pause state preserved"
