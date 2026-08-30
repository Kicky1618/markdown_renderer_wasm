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
  echo "user language coverage browser: skipped (Chrome/Chromium not found)"
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
WORK=$(mktemp -d "$TMP_BASE/streamdown-user-language-browser.XXXXXX")
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
url = f"http://127.0.0.1:{port}/tests/user_language_coverage_browser.html"
for _ in range(100):
    try:
        with urllib.request.urlopen(url, timeout=0.2) as response:
            if response.status == 200:
                raise SystemExit(0)
    except Exception:
        time.sleep(0.05)
raise SystemExit("user language coverage browser: HTTP server did not start")
PY

HTML="$WORK/page.html"
ERR="$WORK/chrome.err"
"$CHROME" \
  --headless=new \
  --no-sandbox \
  --disable-dev-shm-usage \
  --virtual-time-budget=60000 \
  --dump-dom \
  "http://127.0.0.1:$PORT/tests/user_language_coverage_browser.html" \
  >"$HTML" 2>"$ERR" || true

if ! grep -q 'data-user-language-probe="pass"' "$HTML"; then
  echo "user language coverage browser: probe failed"
  grep -o '<html[^>]*>' "$HTML" | head -1 || true
  tail -30 "$ERR" || true
  tail -50 "$WORK/http.log" || true
  exit 1
fi

requested=$(grep -o 'data-user-language-count="[0-9]*"' "$HTML" | head -1 | tr -cd '0-9')
expected=$(grep -o 'data-user-language-expected-packs="[0-9]*"' "$HTML" | head -1 | tr -cd '0-9')
actual=$(grep -o 'data-user-language-actual-packs="[0-9]*"' "$HTML" | head -1 | tr -cd '0-9')
[ "$requested" = 100 ] || {
  echo "user language coverage browser: requested count=$requested expected=100"
  exit 1
}
[ "$expected" = "$actual" ] || {
  echo "user language coverage browser: canonical count expected=$expected actual=$actual"
  exit 1
}

pack_gets=$(grep -Ec 'GET /langpacks/[^? ]+\.slp\?v=[0-9a-f]+ HTTP/1\.1.* 200' "$WORK/http.log" || true)
[ "$pack_gets" -eq "$expected" ] || {
  echo "user language coverage browser: canonical pack GETs=$pack_gets expected=$expected"
  cat "$WORK/http.log"
  exit 1
}

if grep ' 404 ' "$WORK/http.log" | grep -v 'GET /favicon.ico ' >/dev/null 2>&1; then
  echo "user language coverage browser: unexpected HTTP 404"
  cat "$WORK/http.log"
  exit 1
fi

echo "user language coverage browser: $requested requested fences -> $actual canonical packs pass"
