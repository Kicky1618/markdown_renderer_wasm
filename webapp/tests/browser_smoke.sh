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
  echo "browser smoke: skipped (Chrome/Chromium not found)"
  exit 0
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
WORK=$(mktemp -d "$TMP_BASE/streamdown-browser-smoke.XXXXXX")
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
sleep 0.05
if ! kill -0 "$SERVER_PID" 2>/dev/null; then
  echo "browser smoke: local HTTP server failed to start"
  cat "$WORK/http.log"
  exit 1
fi
python3 - "$PORT" <<'PY'
import sys, time, urllib.request
port = int(sys.argv[1])
url = f"http://127.0.0.1:{port}/"
for _ in range(50):
    try:
        with urllib.request.urlopen(url, timeout=0.2) as response:
            if response.status == 200:
                raise SystemExit(0)
    except Exception:
        time.sleep(0.05)
raise SystemExit("browser smoke: local HTTP server did not start")
PY

run_case() {
  name=$1
  query=$2
  requested=$3
  html="$WORK/$name.html"
  err="$WORK/$name.err"
  attempt=1
  while :; do
    "$CHROME" \
      --headless=new \
      --no-sandbox \
      --disable-dev-shm-usage \
      --virtual-time-budget=5000 \
      --dump-dom \
      "http://127.0.0.1:$PORT/?$query&smoke=1&doc=easy&tps=1000&repeat=1" \
      >"$html" 2>"$err" || true
    if grep -q 'data-smoke="pass"' "$html"; then
      break
    fi
    if [ "$attempt" -ge 3 ]; then
      echo "browser smoke: $name failed smoke probe"
      grep -o '<html[^>]*>' "$html" | head -1 || true
      grep -o '<canvas id="app"[^>]*>' "$html" | head -1 || true
      tail -30 "$err" || true
      exit 1
    fi
    attempt=$((attempt + 1))
    sleep 0.1
  done
  grep -q "data-renderer-requested=\"$requested\"" "$html" || {
    echo "browser smoke: $name did not preserve requested renderer '$requested'"
    grep -o '<canvas id="app"[^>]*>' "$html" | head -1 || true
    exit 1
  }
  renderer=$(grep -o 'data-smoke-renderer="[^"]*"' "$html" | head -1 | cut -d '"' -f2)
  depth=$(grep -o 'data-smoke-fallback-depth="[^"]*"' "$html" | head -1 | cut -d '"' -f2)
  echo "browser smoke: $name renderer=$renderer fallback_depth=$depth pass"
}

run_case canvas2d 'renderer=canvas2d' canvas2d
run_case webgl2 'renderer=webgl2' webgl2
run_case auto 'renderer=auto' auto


run_swiftshader_exact() {
  name=$1
  query=$2
  requested=$3
  expected_renderer=$4
  expected_depth=$5
  expected_runtime_depth=$6
  expected_runtime_origin=$7
  budget=$8
  html="$WORK/$name.html"
  err="$WORK/$name.err"
  attempt=1
  while :; do
    "$CHROME" \
      --headless=new \
      --no-sandbox \
      --disable-dev-shm-usage \
      --use-angle=swiftshader \
      --enable-unsafe-swiftshader \
      --enable-logging=stderr \
      --v=0 \
      --run-all-compositor-stages-before-draw \
      --virtual-time-budget="$budget" \
      --dump-dom \
      "http://127.0.0.1:$PORT/?$query&smoke=1&doc=easy&tps=1000&repeat=1" \
      >"$html" 2>"$err" || true
    if grep -q 'data-smoke="pass"' "$html"; then
      break
    fi
    if [ "$attempt" -ge 3 ]; then
      echo "browser smoke: $name failed smoke probe"
      grep -o '<html[^>]*>' "$html" | head -1 || true
      grep -o '<canvas id="app"[^>]*>' "$html" | head -1 || true
      tail -30 "$err" || true
      exit 1
    fi
    attempt=$((attempt + 1))
  done
  grep -q "data-renderer-requested=\"$requested\"" "$html" || {
    echo "browser smoke: $name final requested renderer mismatch"
    exit 1
  }
  renderer=$(grep -o 'data-smoke-renderer="[^"]*"' "$html" | head -1 | cut -d '"' -f2)
  depth=$(grep -o 'data-smoke-fallback-depth="[^"]*"' "$html" | head -1 | cut -d '"' -f2)
  runtime_depth=$(grep -o 'data-smoke-runtime-depth="[^"]*"' "$html" | head -1 | cut -d '"' -f2)
  runtime_origin=$(grep -o 'data-smoke-runtime-origin="[^"]*"' "$html" | head -1 | cut -d '"' -f2)
  [ "$renderer" = "$expected_renderer" ] || {
    echo "browser smoke: $name renderer=$renderer expected=$expected_renderer"
    exit 1
  }
  [ "$depth" = "$expected_depth" ] || {
    echo "browser smoke: $name fallback_depth=$depth expected=$expected_depth"
    exit 1
  }
  [ "$runtime_depth" = "$expected_runtime_depth" ] || {
    echo "browser smoke: $name runtime_depth=$runtime_depth expected=$expected_runtime_depth"
    exit 1
  }
  [ "$runtime_origin" = "$expected_runtime_origin" ] || {
    echo "browser smoke: $name runtime_origin=$runtime_origin expected=$expected_runtime_origin"
    exit 1
  }
  echo "browser smoke: $name renderer=$renderer fallback_depth=$depth runtime_depth=$runtime_depth pass"
}

run_swiftshader_exact webgl2-swiftshader \
  'renderer=webgl2' webgl2 webgl 0 0 '' 10000
run_swiftshader_exact webgl2-runtime-recovery \
  'renderer=webgl2&simulate_gpu_loss=webgl2' canvas2d canvas2d 0 1 webgl2 60000
grep -q 'WEBGL2 failed at runtime (device-lost); restarting with CANVAS2D' "$WORK/webgl2-runtime-recovery.err" || {
  echo "browser smoke: runtime recovery transition log missing"
  exit 1
}
