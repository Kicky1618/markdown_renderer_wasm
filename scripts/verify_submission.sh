#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

RUN_BROWSER=${VERIFY_BROWSER:-0}
RUN_BENCH=${VERIFY_BENCH:-0}
for arg in "$@"; do
  case "$arg" in
    --browser) RUN_BROWSER=1 ;;
    --bench) RUN_BENCH=1 ;;
    --all) RUN_BROWSER=1; RUN_BENCH=1 ;;
    --no-browser) RUN_BROWSER=0 ;;
    --no-bench) RUN_BENCH=0 ;;
    -h|--help)
      cat <<'EOF'
Usage: scripts/verify_submission.sh [--browser] [--bench] [--all]

Runs the reproducible submission checks for Streamdown:
  - Rust parser/unit/integration tests
  - wasm32 build and JavaScript MDA1 tests
  - semantic graph/scheduler/runtime tests
  - webapp native tests and release build
  - Generative UI Node tests

Options:
  --browser  Also run the real Chrome/Chromium Viewer, Canvas2D, dynamic
             language-pack, and Generative UI smoke tests.
  --bench    Also print native parser and semantic-runtime performance snapshots.
  --all      Enable both --browser and --bench.

VERIFY_BROWSER=1 and VERIFY_BENCH=1 provide equivalent environment toggles.
EOF
      exit 0
      ;;
    *)
      echo "unknown argument: $arg" >&2
      exit 2
      ;;
  esac
done

for tool in cargo node npm wasm-bindgen; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "submission verify: required tool not found: $tool" >&2
    exit 127
  fi
done

VERIFY_TMP=${VERIFY_TMPDIR:-"$ROOT/target/submission-verify-tmp"}
mkdir -p "$VERIFY_TMP" "$ROOT/webapp/target/tmp"

section() {
  printf '\n== %s ==\n' "$1"
}

section "core Rust tests"
TMPDIR="$VERIFY_TMP" cargo test --lib --tests

section "core wasm32 build"
TMPDIR="$VERIFY_TMP" cargo build --release --target wasm32-unknown-unknown

section "JavaScript / MDA1"
npm test

section "semantic runtime"
node tools/semantic-detector.test.mjs
node tools/semantic-timeline-core.test.mjs
node tools/semantic-graph.test.mjs
node tools/semantic-scheduler.test.mjs
node tools/semantic-scheduler.integration.mjs
node tools/semantic-runtime.integration.mjs

section "webapp native tests"
TMPDIR="$ROOT/webapp/target/tmp" cargo test \
  --manifest-path webapp/Cargo.toml \
  --tests \
  --release

section "webapp release build"
./webapp/build.sh

section "Generative UI Node tests"
node webapp/generative/tests.mjs

if [ "$RUN_BENCH" = "1" ]; then
  section "native parser benchmark"
  node scripts/stream-bench-median.mjs

  section "semantic runtime benchmark"
  node tools/semantic-runtime-bench.mjs
fi

if [ "$RUN_BROWSER" = "1" ]; then
  # Keep Chrome's profile/socket paths short. Deep repository-local TMPDIRs can
  # exceed Chromium's Unix-domain socket length limit.
  BROWSER_TMP=${VERIFY_BROWSER_TMPDIR:-/tmp}

  # Headless GPU startup/reload can occasionally miss a virtual-time window.
  # Retry the whole smoke once; a persistent failure still fails verification.
  browser_smoke() {
    label=$1
    shift
    if "$@"; then
      return 0
    fi
    echo "submission verify: retrying browser smoke: $label" >&2
    sleep 0.2
    "$@"
  }

  section "Viewer browser smoke"
  browser_smoke viewer env TMPDIR="$BROWSER_TMP" ./webapp/tests/browser_smoke.sh

  section "Canvas2D browser smoke"
  browser_smoke canvas2d env TMPDIR="$BROWSER_TMP" ./webapp/tests/canvas2d_browser_smoke.sh

  section "dynamic language-pack browser smoke"
  browser_smoke langpack env TMPDIR="$BROWSER_TMP" SKIP_BUILD=1 ./webapp/tests/dynamic_langpack_browser.sh

  section "Generative UI browser smoke"
  browser_smoke generative env TMPDIR="$BROWSER_TMP" ./webapp/generative/browser_smoke.sh
fi

extras=""
[ "$RUN_BROWSER" = "1" ] && extras="${extras} browser"
[ "$RUN_BENCH" = "1" ] && extras="${extras} bench"
if [ -n "$extras" ]; then
  printf '\nsubmission verify: PASS (including%s)\n' "$extras"
else
  printf '\nsubmission verify: PASS\n'
fi
