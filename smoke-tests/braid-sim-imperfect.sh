#!/bin/bash
#
# End-to-end test that Braid still tracks accurately under observation-model
# imperfections, scored against the known ground truth.
#
# Generates a calibration + Braid config from an imperfect sim.toml scenario
# (detection noise, dropout, clutter, occlusion -- see example-sim-imperfect.toml),
# launches a real braid-run with the synthetic-image `sim` cameras, records a
# .braidz, then scores the recording against the known ground truth with
# `braid-sim score --sim-toml`. The imperfections flow through the real
# ci2-sim renderer, the feature detector, and data association; the oracle
# asserts that live tracking still recovers the insect with high coverage and
# low position error and without fragmenting into many tracks.
#
# Usage:
#   smoke-tests/braid-sim-imperfect.sh
#
# Environment variables:
#   STRAND_BRAID_TARGET_DIR  directory with braid-run, strand-cam, braid-sim,
#                            braid-offline-retrack (default: <repo>/target/debug)
#   BRAID_PORT               control-API port (default: 44479)
#   RECORD_SECONDS           how long to record (default: 8)
#   MIN_COVERAGE             minimum acceptable ground-truth coverage (default: 0.5)
#   MAX_RMSE_M               maximum acceptable position RMSE, meters (default: 0.02)
#   MAX_FRAGS                maximum acceptable mean fragments-per-insect (default: 5)

set -o errexit
set -o nounset
set -o pipefail

REPO_DIR=$(cd "$(dirname "$0")/.." && pwd)
TARGET_DIR="${STRAND_BRAID_TARGET_DIR:-$REPO_DIR/target/debug}"
BRAID_PORT="${BRAID_PORT:-44479}"
RECORD_SECONDS="${RECORD_SECONDS:-8}"
MIN_COVERAGE="${MIN_COVERAGE:-0.5}"
MAX_RMSE_M="${MAX_RMSE_M:-0.02}"
MAX_FRAGS="${MAX_FRAGS:-5}"
SIM_TOML="$REPO_DIR/braid/braid-sim/example-sim-imperfect.toml"

WORK_DIR=$(mktemp -d -t braid-sim-imperfect-XXXXXX)
PIDS=()

cleanup() {
    local pid
    for pid in "${PIDS[@]}"; do
        kill -- "-$pid" 2>/dev/null || true
    done
    sleep 1
    rm -rf "$WORK_DIR"
}
trap cleanup EXIT

fail() {
    echo "FAILED: $1" >&2
    if [ -n "${2:-}" ] && [ -f "${2:-}" ]; then
        echo "--- log tail ---" >&2
        tail -50 "$2" >&2
    fi
    exit 1
}

wait_for_log_line() {
    local pattern="$1" logfile="$2" i
    for i in $(seq 1 80); do
        if grep -q "$pattern" "$logfile" 2>/dev/null; then
            return 0
        fi
        sleep 0.5
    done
    return 1
}

for exe in braid-run strand-cam braid-sim braid-offline-retrack; do
    if [ ! -x "$TARGET_DIR/$exe" ]; then
        echo "ERROR: $TARGET_DIR/$exe not found. Build it first or set" >&2
        echo "STRAND_BRAID_TARGET_DIR. e.g.:" >&2
        echo "  cargo build -p braid-run -p strand-cam -p braid-sim -p braid-offline" >&2
        exit 1
    fi
done
python3 -c "import requests" || {
    echo "ERROR: the python 'requests' library is required." >&2
    exit 1
}

export DISABLE_VERSION_CHECK=1
export RUST_LOG="${RUST_LOG:-info}"

echo "=== Generating calibration + Braid config from $SIM_TOML ==="
"$TARGET_DIR/braid-sim" generate "$SIM_TOML" \
    --out-dir "$WORK_DIR/gen" \
    --http-api-server-addr "127.0.0.1:$BRAID_PORT"

CONFIG="$WORK_DIR/gen/braid-config.toml"
BRAID_LOG="$WORK_DIR/braid-run.log"

echo "=== Launching braid-run with imperfect sim cameras ==="
STRAND_CAM_SIM_SPEC="$SIM_TOML" setsid "$TARGET_DIR/braid-run" "$CONFIG" \
    > "$BRAID_LOG" 2>&1 &
PIDS+=($!)

wait_for_log_line "All expected cameras synchronized" "$BRAID_LOG" \
    || fail "cameras did not synchronize" "$BRAID_LOG"
echo "cameras synchronized"

echo "=== Recording a .braidz for ${RECORD_SECONDS}s via the HTTP control API ==="
python3 - "$BRAID_PORT" "$RECORD_SECONDS" <<'PYEOF'
import sys, time, urllib.parse
import requests

braid_url = "http://127.0.0.1:%s/" % sys.argv[1]
record_seconds = float(sys.argv[2])

session = requests.session()
session.get(braid_url).raise_for_status()

def callback(payload):
    r = session.post(urllib.parse.urljoin(braid_url, "callback"), json=payload)
    r.raise_for_status()

callback({"DoRecordCsvTables": True})
time.sleep(record_seconds)
callback({"DoRecordCsvTables": False})
time.sleep(2.0)
print("recording stopped")
PYEOF

echo "=== Locating .braidz output ==="
BRAIDZ=$(find "$WORK_DIR/gen/braid-data" -name "*.braidz" | head -1 || true)
[ -n "$BRAIDZ" ] || fail "no .braidz file was produced" "$BRAID_LOG"
echo "produced: $BRAIDZ"

echo "=== Scoring against ground truth ==="
SCORE_OUT="$WORK_DIR/score.txt"
"$TARGET_DIR/braid-sim" score "$BRAIDZ" \
    --sim-toml "$SIM_TOML" \
    --retrack-exe "$TARGET_DIR/braid-offline-retrack" \
    --retrack-out "$WORK_DIR/retrack.braidz" \
    | tee "$SCORE_OUT"

# Parse the ground-truth oracle block.
get() { grep -E "$1" "$SCORE_OUT" | head -1 | grep -oE '[0-9]+(\.[0-9]+)?' | tail -1; }
MATCHED=$(get '^\s*matched frames')
RMSE=$(get '^\s*position RMSE')
COVERAGE_PCT=$(get '^\s*coverage')
FRAGS=$(get '^\s*frags / insect')

echo "--- parsed: matched=$MATCHED rmse=${RMSE}m coverage=${COVERAGE_PCT}% frags=$FRAGS ---"

[ -n "$MATCHED" ] && [ "$MATCHED" -gt 0 ] 2>/dev/null \
    || fail "ground-truth oracle matched no frames (tracking failed)" "$BRAID_LOG"

awk -v c="$COVERAGE_PCT" -v minc="$MIN_COVERAGE" \
    -v r="$RMSE" -v maxr="$MAX_RMSE_M" \
    -v f="$FRAGS" -v maxf="$MAX_FRAGS" 'BEGIN {
        ok = 1
        if (c/100.0 < minc) { printf("coverage %.3f < %.3f\n", c/100.0, minc); ok = 0 }
        if (r > maxr)       { printf("rmse %.4f > %.4f\n", r, maxr); ok = 0 }
        if (f > maxf)       { printf("frags %.2f > %.2f\n", f, maxf); ok = 0 }
        exit (ok ? 0 : 1)
    }' || fail "live tracking under imperfections did not meet thresholds" "$BRAID_LOG"

echo "PASS: Braid tracked the insect under detection noise/dropout/clutter"
echo "      (coverage ${COVERAGE_PCT}% >= $(awk -v m=$MIN_COVERAGE 'BEGIN{printf "%.0f", m*100}')%, RMSE ${RMSE}m <= ${MAX_RMSE_M}m, frags ${FRAGS} <= ${MAX_FRAGS})"
