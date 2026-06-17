#!/bin/bash
#
# End-to-end smoke test of the Braid live 3D simulation harness.
#
# Generates a synthetic calibration + Braid config from a sim.toml scenario
# (braid-sim generate), launches a real braid-run whose cameras are the
# synthetic-image `sim` backend (no hardware), waits for synchronization,
# records a .braidz over the HTTP control API, stops Braid, and verifies a
# .braidz file containing 3D tracking data was produced.
#
# See scratch/2026-06-17_braid-live-3d-sim-test-plan.md (milestone M3b).
#
# Usage:
#   smoke-tests/braid-sim.sh
#
# Environment variables:
#   STRAND_BRAID_TARGET_DIR  directory with braid-run, strand-cam, braid-sim
#                            (default: <repo>/target/debug)
#   BRAID_PORT               control-API port for the braid phase (44478)
#   RECORD_SECONDS           how long to record (default: 8)

set -o errexit
set -o nounset
set -o pipefail

REPO_DIR=$(cd "$(dirname "$0")/.." && pwd)
TARGET_DIR="${STRAND_BRAID_TARGET_DIR:-$REPO_DIR/target/debug}"
BRAID_PORT="${BRAID_PORT:-44478}"
RECORD_SECONDS="${RECORD_SECONDS:-8}"
SIM_TOML="$REPO_DIR/braid/braid-sim/example-sim.toml"

WORK_DIR=$(mktemp -d -t braid-sim-smoke-XXXXXX)
PIDS=()

cleanup() {
    local pid
    for pid in "${PIDS[@]}"; do
        # Kill the whole process group: braid-run spawns strand-cam children.
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

for exe in braid-run strand-cam braid-sim; do
    if [ ! -x "$TARGET_DIR/$exe" ]; then
        echo "ERROR: $TARGET_DIR/$exe not found. Build it first or set" >&2
        echo "STRAND_BRAID_TARGET_DIR. e.g.:" >&2
        echo "  cargo build -p braid-run -p strand-cam -p braid-sim" >&2
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

echo "=== Launching braid-run with sim cameras ==="
# braid-run finds strand-cam next to itself; the sim backend reads the scenario
# from STRAND_CAM_SIM_SPEC.
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
# GET / to obtain the auth cookie (localhost is trusted; no token needed).
session.get(braid_url).raise_for_status()

def callback(payload):
    r = session.post(urllib.parse.urljoin(braid_url, "callback"), json=payload)
    r.raise_for_status()

callback({"DoRecordCsvTables": True})
time.sleep(record_seconds)
callback({"DoRecordCsvTables": False})
# Give Braid a moment to finalize the .braidz.
time.sleep(2.0)
print("recording stopped")
PYEOF

echo "=== Verifying .braidz output ==="
BRAIDZ=$(find "$WORK_DIR/gen/braid-data" -name "*.braidz" | head -1 || true)
[ -n "$BRAIDZ" ] || fail "no .braidz file was produced" "$BRAID_LOG"
echo "produced: $BRAIDZ"

# A .braidz is a zip; it must contain kalman_estimates (the 3D tracking output).
if command -v unzip >/dev/null 2>&1; then
    unzip -l "$BRAIDZ" | grep -q "kalman_estimates" \
        || fail "braidz has no kalman_estimates (no 3D tracking)" "$BRAID_LOG"
    echo "braidz contains kalman_estimates (3D tracking present)"
fi

echo "PASS: braid-sim end-to-end live run produced a 3D-tracking .braidz"
