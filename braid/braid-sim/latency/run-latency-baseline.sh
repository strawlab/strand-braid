#!/bin/bash
#
# End-to-end latency measurement of Braid live 3D tracking, using the braid-sim
# image-level path (real detector, real UDP, real mainbrain — no hardware).
#
# Two latency measurement points are captured in a single run:
#
#   1. reconstruct_latency_usec.hlog inside the recorded .braidz — stamped in
#      the braidz writer thread. NOTE: this includes writer-queue delay; the
#      writer's ~1 s periodic gzip flush produces a tail (tens of ms) that live
#      consumers never see. See ../../flydra2/src/write_data.rs.
#   2. The `latency` field of the model-server SSE stream (:8397/events) —
#      stamped when the tracker's pose update is published. This is the true
#      "when was the 3D estimate available" latency.
#
# Requires trigger timestamps under FakeSync (braid-run >= the commit
# "fix(braid): populate trigger timestamps under FakeSync"); before that fix
# both measurement points are empty/NaN in simulated runs.
#
# Usage:
#   run-latency-baseline.sh [sim.toml] [out-dir]
#
# Environment variables:
#   STRAND_BRAID_TARGET_DIR  directory with braid-run, strand-cam, braid-sim.
#                            When unset, builds them with `cargo build
#                            --release` and uses <repo>/target/release.
#                            (Always measure latency on release builds.)
#   BRAID_PORT               control-API port for braid-run (default 44478)
#   MODEL_SERVER_PORT        model-server port, must match the Braid config
#                            default (8397)
#   RECORD_SECONDS           how long to record (default 125; >120 s yields
#                            multiple 60 s histogram intervals)

set -o errexit
set -o nounset
set -o pipefail

SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
REPO_DIR=$(cd "$SCRIPT_DIR/../../.." && pwd)
TARGET_DIR="${STRAND_BRAID_TARGET_DIR:-$REPO_DIR/target/release}"
BRAID_PORT="${BRAID_PORT:-44478}"
MODEL_SERVER_PORT="${MODEL_SERVER_PORT:-8397}"
RECORD_SECONDS="${RECORD_SECONDS:-125}"
SIM_TOML="${1:-$REPO_DIR/braid/braid-sim/example-sim-multi.toml}"
OUT_KEEP="${2:-latency-baseline-out}"

WORK_DIR=$(mktemp -d -t braid-sim-latency-XXXXXX)
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
    for i in $(seq 1 120); do
        if grep -q "$pattern" "$logfile" 2>/dev/null; then
            return 0
        fi
        sleep 0.5
    done
    return 1
}

if [ -z "${STRAND_BRAID_TARGET_DIR:-}" ]; then
    echo "=== Building braid-run, strand-cam, braid-sim (cargo build --release) ==="
    ( cd "$REPO_DIR" && cargo build --release -p braid-run -p strand-cam -p braid-sim )
fi
for exe in braid-run strand-cam braid-sim; do
    [ -x "$TARGET_DIR/$exe" ] || fail "$TARGET_DIR/$exe not found"
done
command -v uv >/dev/null 2>&1 || fail "'uv' is required (https://docs.astral.sh/uv/)"

# Run a Python helper through uv, without inheriting RUST_LOG (uv is a Rust
# program and would log at the level we set for the braid binaries).
uv_run() { env -u RUST_LOG uv run --no-project "$@"; }

mkdir -p "$OUT_KEEP"
OUT_KEEP=$(cd "$OUT_KEEP" && pwd)

export DISABLE_VERSION_CHECK=1
export RUST_LOG="${RUST_LOG:-info}"

echo "=== Generating calibration + Braid config from $SIM_TOML ==="
"$TARGET_DIR/braid-sim" generate "$SIM_TOML" \
    --out-dir "$WORK_DIR/gen" \
    --http-api-server-addr "127.0.0.1:$BRAID_PORT"

CONFIG="$WORK_DIR/gen/braid-config.toml"
BRAID_LOG="$OUT_KEEP/braid-run.log"

echo "=== Launching braid-run with sim cameras ==="
STRAND_CAM_SIM_SPEC="$SIM_TOML" setsid "$TARGET_DIR/braid-run" "$CONFIG" \
    > "$BRAID_LOG" 2>&1 &
PIDS+=($!)

wait_for_log_line "All expected cameras synchronized" "$BRAID_LOG" \
    || fail "cameras did not synchronize" "$BRAID_LOG"
echo "cameras synchronized"

# Let the pipeline settle (background model init, sync) before measuring.
sleep 10

echo "=== Starting model-server SSE latency capture ==="
uv_run "$SCRIPT_DIR/sse_capture.py" \
    "http://127.0.0.1:$MODEL_SERVER_PORT/events" "$((RECORD_SECONDS + 5))" \
    "$OUT_KEEP/model-server-latency.csv" &
SSE_PID=$!
sleep 2

echo "=== Recording ${RECORD_SECONDS}s via the HTTP control API ==="
uv_run - "$BRAID_PORT" "$RECORD_SECONDS" <<'PYEOF'
# /// script
# requires-python = ">=3.9"
# dependencies = ["requests"]
# ///
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
time.sleep(3.0)
print("recording stopped")
PYEOF

wait "$SSE_PID" || true

echo "=== Collecting .braidz output ==="
BRAIDZ=$(find "$WORK_DIR/gen/braid-data" -name "*.braidz" | head -1 || true)
[ -n "$BRAIDZ" ] || fail "no .braidz file was produced" "$BRAID_LOG"
cp "$BRAIDZ" "$OUT_KEEP/"
BRAIDZ="$OUT_KEEP/$(basename "$BRAIDZ")"
echo "kept: $BRAIDZ"

echo
echo "=== Writer-side histogram (includes braidz writer-queue delay) ==="
uv_run "$SCRIPT_DIR/analyze_hlog.py" "$BRAIDZ"
echo
echo "=== Tracker-output latency (model-server SSE) ==="
uv_run "$SCRIPT_DIR/analyze_sse.py" "$OUT_KEEP/model-server-latency.csv"
echo
echo "=== Camera-side decomposition (from data2d timestamps) ==="
uv_run "$SCRIPT_DIR/decompose_latency.py" "$BRAIDZ"
