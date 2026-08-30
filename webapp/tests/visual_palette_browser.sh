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
  echo "visual palette browser: skipped (Chrome/Chromium not found)"
  exit 0
fi

PORT=$(python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
)
WORK=$(mktemp -d "${TMPDIR:-/tmp}/streamdown-visual-palette.XXXXXX")
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
for _ in range(60):
    try:
        with urllib.request.urlopen(url, timeout=0.2) as response:
            if response.status == 200:
                raise SystemExit(0)
    except Exception:
        time.sleep(0.05)
raise SystemExit("visual palette browser: local HTTP server did not start")
PY

capture() {
  backend=$1
  doc=$2
  output=$3
  profile="$WORK/profile-$backend-$doc"
  extra=""
  if [ "$backend" = webgl2 ]; then
    extra="--use-angle=swiftshader --enable-unsafe-swiftshader"
  fi
  # shellcheck disable=SC2086
  "$CHROME" \
    --headless=new \
    --no-sandbox \
    --disable-dev-shm-usage \
    --disable-extensions \
    --disable-background-networking \
    --user-data-dir="$profile" \
    $extra \
    --window-size=1200,900 \
    --run-all-compositor-stages-before-draw \
    --virtual-time-budget=8000 \
    --screenshot="$output" \
    "http://127.0.0.1:$PORT/?renderer=$backend&doc=$doc&tps=10000000&repeat=1&autoscroll=0&fade=0" \
    >/dev/null 2>"$WORK/$backend-$doc.err"
}

capture canvas2d default "$WORK/canvas-table.png"
capture webgl2 default "$WORK/webgl-table.png"
capture canvas2d code "$WORK/canvas-code.png"
capture webgl2 code "$WORK/webgl-code.png"

python3 - "$WORK" <<'PY'
import collections
import struct
import sys
import zlib
from pathlib import Path

work = Path(sys.argv[1])

def pixels(path):
    data = path.read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise SystemExit(f"not PNG: {path}")
    pos = 8
    width = height = color_type = bit_depth = interlace = None
    raw = bytearray()
    while pos < len(data):
        length = struct.unpack(">I", data[pos:pos+4])[0]
        kind = data[pos+4:pos+8]
        chunk = data[pos+8:pos+8+length]
        pos += 12 + length
        if kind == b"IHDR":
            width, height, bit_depth, color_type, _, _, interlace = struct.unpack(">IIBBBBB", chunk)
        elif kind == b"IDAT":
            raw.extend(chunk)
        elif kind == b"IEND":
            break
    if bit_depth != 8 or interlace != 0 or color_type not in (2, 6):
        raise SystemExit(f"unsupported PNG layout in {path}: depth={bit_depth} color={color_type} interlace={interlace}")
    channels = 3 if color_type == 2 else 4
    stride = width * channels
    decoded = zlib.decompress(bytes(raw))
    prev = bytearray(stride)
    offset = 0
    counts = collections.Counter()
    for _ in range(height):
        filt = decoded[offset]
        offset += 1
        scan = bytearray(decoded[offset:offset+stride])
        offset += stride
        for i in range(stride):
            a = scan[i-channels] if i >= channels else 0
            b = prev[i]
            c = prev[i-channels] if i >= channels else 0
            if filt == 1:
                scan[i] = (scan[i] + a) & 255
            elif filt == 2:
                scan[i] = (scan[i] + b) & 255
            elif filt == 3:
                scan[i] = (scan[i] + ((a + b) >> 1)) & 255
            elif filt == 4:
                p = a + b - c
                pa, pb, pc = abs(p-a), abs(p-b), abs(p-c)
                pr = a if pa <= pb and pa <= pc else b if pb <= pc else c
                scan[i] = (scan[i] + pr) & 255
            elif filt != 0:
                raise SystemExit(f"unknown PNG filter {filt}")
        for x in range(width):
            base = x * channels
            counts[(scan[base], scan[base+1], scan[base+2])] += 1
        prev = scan
    return width, height, counts

def check_pair(label, a_path, b_path, expected):
    aw, ah, a = pixels(a_path)
    bw, bh, b = pixels(b_path)
    if (aw, ah) != (bw, bh):
        raise SystemExit(f"{label}: screenshot dimensions differ: {(aw, ah)} vs {(bw, bh)}")
    print(f"{label}: {aw}x{ah}")
    for name, rgb, minimum in expected:
        ac = a[rgb]
        bc = b[rgb]
        print(f"  {name:16} rgb={rgb} canvas={ac} webgl={bc}")
        if ac < minimum or bc < minimum:
            raise SystemExit(f"{label}: {name} missing/too sparse; canvas={ac}, webgl={bc}, minimum={minimum}")

check_pair(
    "table palette",
    work / "canvas-table.png",
    work / "webgl-table.png",
    [
        ("background", (0x09, 0x0C, 0x12), 100_000),
        ("table header", (0x1A, 0x30, 0x40), 5_000),
        ("table stripe", (0x0E, 0x16, 0x1F), 5_000),
        ("table rule", (0x78, 0x87, 0x9E), 500),
        ("foreground", (0xD1, 0xDB, 0xE8), 100),
        ("cyan", (0x40, 0xDC, 0xDF), 20),
    ],
)
check_pair(
    "code palette",
    work / "canvas-code.png",
    work / "webgl-code.png",
    [
        ("panel", (0x0E, 0x1A, 0x25), 5_000),
        ("foreground", (0xD1, 0xDB, 0xE8), 100),
        ("keyword", (0xC7, 0x9E, 0xF2), 10),
        ("type", (0x40, 0xDC, 0xDF), 10),
        ("function", (0x73, 0xB8, 0xFA), 10),
        ("string", (0x73, 0xE0, 0x9E), 10),
        ("number", (0xF2, 0xAB, 0x61), 10),
        ("comment", (0x78, 0x87, 0x9E), 10),
        ("operator", (0xAD, 0xBC, 0xCC), 10),
    ],
)
print("visual palette browser: pass")
PY
