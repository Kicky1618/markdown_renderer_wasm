#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
[ -f "$ROOT/generative/streamdown.wasm" ] && [ -f "$ROOT/generative/streamdown.js" ] || {
  echo "generative browser smoke: run ./webapp/build.sh first" >&2
  exit 2
}
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
  echo "generative browser smoke: skipped (Chrome/Chromium not found)"
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
WORK=$(mktemp -d "$TMP_BASE/streamdown-gen-smoke.XXXXXX")
SERVER_PID=
cleanup() {
  if [ -n "$SERVER_PID" ]; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  rm -rf "$WORK"
}
trap cleanup EXIT INT TERM

python3 "$ROOT/generative/fixtures/llm_proxy.py" --port "$PORT" >"$WORK/http.log" 2>&1 &
SERVER_PID=$!
python3 - "$PORT" <<'PY'
import sys, time, urllib.request
port = int(sys.argv[1])
url = f"http://127.0.0.1:{port}/generative/"
for _ in range(60):
    try:
        with urllib.request.urlopen(url, timeout=0.2) as response:
            if response.status == 200:
                raise SystemExit(0)
    except Exception:
        time.sleep(0.05)
raise SystemExit("generative browser smoke: local server did not start")
PY

html="$WORK/dom.html"
err="$WORK/chrome.err"
attempt=1
while :; do
  "$CHROME" \
    --headless=new \
    --no-sandbox \
    --disable-dev-shm-usage \
    --virtual-time-budget=10000 \
    --dump-dom \
    "http://127.0.0.1:$PORT/generative/?smoke=1" \
    >"$html" 2>"$err" || true
  if grep -q 'data-generative-smoke="pass"' "$html"; then
    break
  fi
  if [ "$attempt" -ge 3 ]; then
    echo "generative browser smoke: failed"
    grep -o '<html[^>]*>' "$html" | head -1 || true
    tail -30 "$err" || true
    exit 1
  fi
  attempt=$((attempt + 1))
  sleep 0.1
done

grep -q 'class="generated-layout"' "$html" || {
  echo "generative browser smoke: generated layout missing"
  exit 1
}
grep -q 'data-ui-id="fahrenheit"' "$html" || {
  echo "generative browser smoke: derived metric missing"
  exit 1
}
grep -q 'data-ui-id="warning"' "$html" || {
  echo "generative browser smoke: conditional metric missing"
  exit 1
}

echo "generative browser smoke: semantic UI + reactive interactions pass"

remote_html="$WORK/remote-dom.html"
remote_err="$WORK/remote-chrome.err"
attempt=1
while :; do
  "$CHROME" \
    --headless=new \
    --no-sandbox \
    --disable-dev-shm-usage \
    --virtual-time-budget=12000 \
    --dump-dom \
    "http://127.0.0.1:$PORT/generative/?remote_smoke=1" \
    >"$remote_html" 2>"$remote_err" || true
  if grep -q 'data-remote-smoke="pass"' "$remote_html"; then
    break
  fi
  if [ "$attempt" -ge 3 ]; then
    echo "generative browser smoke: remote SSE failed"
    grep -o '<html[^>]*>' "$remote_html" | head -1 || true
    tail -30 "$remote_err" || true
    exit 1
  fi
  attempt=$((attempt + 1))
  sleep 0.1
done

grep -q 'data-ui-id="remote"' "$remote_html" || {
  echo "generative browser smoke: remote metric missing"
  exit 1
}
grep -q 'REMOTE' "$remote_html" || {
  echo "generative browser smoke: remote metric value missing"
  exit 1
}

echo "generative browser smoke: SSE -> WASM -> semantic UI pass"

post_html="$WORK/post-dom.html"
post_err="$WORK/post-chrome.err"
attempt=1
while :; do
  "$CHROME" \
    --headless=new \
    --no-sandbox \
    --disable-dev-shm-usage \
    --virtual-time-budget=12000 \
    --dump-dom \
    "http://127.0.0.1:$PORT/generative/?llm_smoke=1" \
    >"$post_html" 2>"$post_err" || true
  if grep -q 'data-llm-smoke="pass"' "$post_html"; then
    break
  fi
  if [ "$attempt" -ge 3 ]; then
    echo "generative browser smoke: Chat POST failed"
    grep -o '<html[^>]*>' "$post_html" | head -1 || true
    tail -30 "$post_err" || true
    exit 1
  fi
  attempt=$((attempt + 1))
  sleep 0.1
done

grep -q 'data-ui-id="post-remote"' "$post_html" || {
  echo "generative browser smoke: POST metric missing"
  exit 1
}
grep -q '>POST<' "$post_html" || {
  echo "generative browser smoke: POST metric value missing"
  exit 1
}

echo "generative browser smoke: Chat POST -> SSE -> WASM -> semantic UI pass"

interaction_html="$WORK/interaction-dom.html"
interaction_err="$WORK/interaction-chrome.err"
attempt=1
while :; do
  "$CHROME" \
    --headless=new \
    --no-sandbox \
    --disable-dev-shm-usage \
    --virtual-time-budget=14000 \
    --dump-dom \
    "http://127.0.0.1:$PORT/generative/?interaction_smoke=1" \
    >"$interaction_html" 2>"$interaction_err" || true
  if grep -q 'data-interaction-smoke="pass"' "$interaction_html"; then
    break
  fi
  if [ "$attempt" -ge 3 ]; then
    echo "generative browser smoke: action=llm round trip failed"
    grep -o '<html[^>]*>' "$interaction_html" | head -1 || true
    tail -30 "$interaction_err" || true
    exit 1
  fi
  attempt=$((attempt + 1))
  sleep 0.1
done

grep -q 'data-ui-id="interaction-result"' "$interaction_html" || {
  echo "generative browser smoke: action=llm continuation metric missing"
  exit 1
}
grep -q '>58<' "$interaction_html" || {
  echo "generative browser smoke: action=llm continuation value missing"
  exit 1
}
grep -q 'data-history-smoke="pass"' "$interaction_html" || {
  echo "generative browser smoke: model response undo/redo history failed"
  exit 1
}
grep -q 'Model continuation' "$interaction_html" || {
  echo "generative browser smoke: action=llm source continuation missing"
  exit 1
}

echo "generative browser smoke: action=llm state -> POST -> SSE -> appended UI pass"
