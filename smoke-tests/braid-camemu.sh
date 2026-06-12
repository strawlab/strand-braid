#!/bin/bash
#
# Smoke test for Strand Camera and Braid using emulated Basler Pylon cameras.
#
# This launches real `strand-cam` and `braid-run` binaries against cameras
# emulated by the Basler Pylon driver (PYLON_CAMEMU), then exercises the HTTP
# control API with the background-reset demo scripts from
# docs/user-docs/scripts/ and verifies in the program logs that the commands
# took effect. No camera hardware is required, but the Pylon SDK and the
# libpylon-cabi shim library must be installed (see
# docs/developer-docs/testing-with-emulated-cameras.md).
#
# Usage:
#   smoke-tests/braid-camemu.sh
#
# Environment variables:
#   STRAND_BRAID_TARGET_DIR  directory with strand-cam and braid-run binaries
#                            (default: <repo>/target/release)
#   PYLON_CABI               path to the libpylon-cabi shim library, if it is
#                            not installed in a standard location
#   STRAND_CAM_PORT          port for the standalone strand-cam phase (3477)
#   BRAID_PORT               port for the braid phase (44477)

set -o errexit
set -o nounset
set -o pipefail

REPO_DIR=$(cd "$(dirname "$0")/.." && pwd)
TARGET_DIR="${STRAND_BRAID_TARGET_DIR:-$REPO_DIR/target/release}"
STRAND_CAM_PORT="${STRAND_CAM_PORT:-3477}"
BRAID_PORT="${BRAID_PORT:-44477}"
SCRIPTS_DIR="$REPO_DIR/docs/user-docs/scripts"

WORK_DIR=$(mktemp -d -t strand-braid-smoke-XXXXXX)
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
    echo "--- log tail ---" >&2
    tail -50 "$2" >&2
    exit 1
}

wait_for_url() {
    local url="$1"
    local i
    for i in $(seq 1 60); do
        if curl --fail --silent --output /dev/null "$url"; then
            return 0
        fi
        sleep 0.5
    done
    return 1
}

wait_for_log_line() {
    local pattern="$1"
    local logfile="$2"
    local count="${3:-1}"
    local i
    for i in $(seq 1 60); do
        if [ "$(grep -c "$pattern" "$logfile" || true)" -ge "$count" ]; then
            return 0
        fi
        sleep 0.5
    done
    return 1
}

for exe in strand-cam braid-run; do
    if [ ! -x "$TARGET_DIR/$exe" ]; then
        echo "ERROR: $TARGET_DIR/$exe not found. Build it first (see" >&2
        echo "docs/developer-docs/building-for-development.md) or set" >&2
        echo "STRAND_BRAID_TARGET_DIR." >&2
        exit 1
    fi
done
python3 -c "import requests" || {
    echo "ERROR: the python 'requests' library is required." >&2
    exit 1
}

# Always pass --camera-name: on machines with real cameras attached, the
# emulated cameras are enumerated alongside the real ones, and we must never
# open a real camera from a smoke test.
export PYLON_CAMEMU=2
export DISABLE_VERSION_CHECK=1
# The verification below greps for debug-level messages of the feature
# detector, so enable them.
export RUST_LOG="info,flydra_feature_detector=debug"

cd "$WORK_DIR"

#
# Phase 1: standalone Strand Camera.
#
echo "=== Phase 1: standalone strand-cam with one emulated camera ==="
SCAM_LOG="$WORK_DIR/strand-cam.log"
setsid "$TARGET_DIR/strand-cam" \
    --camera-backend pylon \
    --camera-name Basler-0815-0000 \
    --no-browser \
    --http-server-addr "127.0.0.1:$STRAND_CAM_PORT" \
    > "$SCAM_LOG" 2>&1 &
PIDS+=($!)

wait_for_url "http://127.0.0.1:$STRAND_CAM_PORT/" \
    || fail "strand-cam HTTP server did not come up" "$SCAM_LOG"

python3 "$SCRIPTS_DIR/reset-background.py" \
    --strand-cam-url "http://127.0.0.1:$STRAND_CAM_PORT/"
python3 "$SCRIPTS_DIR/reset-background.py" \
    --strand-cam-url "http://127.0.0.1:$STRAND_CAM_PORT/" --clear-to-value 127

wait_for_log_line "taking bg image" "$SCAM_LOG" 1 \
    || fail "TakeCurrentImageAsBackground not seen by detector" "$SCAM_LOG"
wait_for_log_line "clearing bg image to 127" "$SCAM_LOG" 1 \
    || fail "ClearBackground not seen by detector" "$SCAM_LOG"
echo "Phase 1 OK"

kill -- "-${PIDS[0]}" 2>/dev/null || true
PIDS=()
sleep 1

#
# Phase 2: Braid with two emulated cameras.
#
echo "=== Phase 2: braid with two emulated cameras ==="
BRAID_LOG="$WORK_DIR/braid-run.log"
cat > "$WORK_DIR/braid-camemu.toml" <<EOF
[mainbrain]
http_api_server_addr = "127.0.0.1:$BRAID_PORT"
output_base_dirname = "$WORK_DIR/BRAID-DATA"

[[cameras]]
name = "Basler-0815-0000"

[[cameras]]
name = "Basler-0815-0001"
EOF
mkdir -p "$WORK_DIR/BRAID-DATA"

# braid-run finds the strand-cam executable next to itself.
setsid "$TARGET_DIR/braid-run" "$WORK_DIR/braid-camemu.toml" \
    > "$BRAID_LOG" 2>&1 &
PIDS+=($!)

wait_for_url "http://127.0.0.1:$BRAID_PORT/" \
    || fail "braid HTTP server did not come up" "$BRAID_LOG"
wait_for_log_line "All expected cameras synchronized" "$BRAID_LOG" 1 \
    || fail "cameras did not synchronize" "$BRAID_LOG"

python3 "$SCRIPTS_DIR/reset-background-braid-all-cams.py" \
    --braid-url "http://127.0.0.1:$BRAID_PORT/"
python3 "$SCRIPTS_DIR/reset-background-braid-all-cams.py" \
    --braid-url "http://127.0.0.1:$BRAID_PORT/" --clear-to-value 127

# Each command must have reached the feature detector of both cameras.
wait_for_log_line "taking bg image" "$BRAID_LOG" 2 \
    || fail "TakeCurrentImageAsBackground not seen by both detectors" "$BRAID_LOG"
wait_for_log_line "clearing bg image to 127" "$BRAID_LOG" 2 \
    || fail "ClearBackground not seen by both detectors" "$BRAID_LOG"
echo "Phase 2 OK"

echo "PASSED"
