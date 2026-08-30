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
  echo "canvas2d browser smoke: skipped (Chrome/Chromium not found)"
  exit 0
fi

PORT=${PORT:-$(python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
)}
TMP_BASE=${TMPDIR:-/tmp}
WORK=$(mktemp -d "$TMP_BASE/streamdown-canvas2d-smoke.XXXXXX")
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
raise SystemExit("canvas2d browser smoke: local HTTP server did not start")
PY

HTML="$WORK/result.html"
ERR="$WORK/chrome.err"
"$CHROME" \
  --headless=new \
  --no-sandbox \
  --disable-dev-shm-usage \
  --force-device-scale-factor=2 \
  --window-size=1000,800 \
  --virtual-time-budget=7000 \
  --dump-dom \
  "http://127.0.0.1:$PORT/tests/canvas2d_smoke.html" \
  >"$HTML" 2>"$ERR" || true

if ! grep -q 'data-canvas2d-smoke="pass"' "$HTML"; then
  echo "canvas2d browser smoke: failed"
  grep -o '<html[^>]*>' "$HTML" | head -1 || true
  tail -30 "$ERR" || true
  exit 1
fi

backing_x=$(grep -o 'data-canvas2d-backing-x="[^"]*"' "$HTML" | head -1 | cut -d '"' -f2)
backing_y=$(grep -o 'data-canvas2d-backing-y="[^"]*"' "$HTML" | head -1 | cut -d '"' -f2)
echo "canvas2d browser smoke: DPR backing=${backing_x}x${backing_y} fonts=ready touch=pass"
