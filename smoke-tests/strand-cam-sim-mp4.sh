#!/bin/bash
#
# Smoke test: record a color (RGB8) MP4 from the synthetic-image `sim` camera
# backend using the ffmpeg codec, with no camera hardware and no Pylon SDK.
#
# This is the hardware-free end-to-end regression test for the color ffmpeg
# recording path of strawlab/strand-braid#29. It launches a standalone
# strand-cam whose frames come from the ci2-sim backend rendering RGB8, drives
# it over the HTTP control API with
# docs/user-docs/scripts/record-mp4-video-ffmpeg.py, and verifies that a
# non-empty .mp4 is produced. Because it uses the sim backend (not
# Pylon-emulated cameras) it needs no Pylon SDK and so runs on GitHub CI. See
# also smoke-tests/braid-camemu.sh, which covers the real Pylon backend (and
# thus the ci2-pylon pixel-format-name selection) on GitLab CI.
#
# Usage:
#   smoke-tests/strand-cam-sim-mp4.sh
#
# Environment variables:
#   STRAND_BRAID_TARGET_DIR  directory with the strand-cam binary. When unset
#                            (the default), it is built with `cargo build` and
#                            <repo>/target/debug is used. When set, the binary
#                            there is used as-is.
#   STRAND_CAM_PORT          HTTP control-API port (default: 3479)
#   RECORD_SECONDS           how long to record (default: 3)

set -o errexit
set -o nounset
set -o pipefail

REPO_DIR=$(cd "$(dirname "$0")/.." && pwd)
TARGET_DIR="${STRAND_BRAID_TARGET_DIR:-$REPO_DIR/target/debug}"
STRAND_CAM_PORT="${STRAND_CAM_PORT:-3479}"
RECORD_SECONDS="${RECORD_SECONDS:-3}"
SCRIPTS_DIR="$REPO_DIR/docs/user-docs/scripts"
SIM_TOML="$REPO_DIR/braid/braid-sim/example-sim.toml"

WORK_DIR=$(mktemp -d -t strand-cam-sim-mp4-XXXXXX)
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

wait_for_url() {
    local url="$1" i
    for i in $(seq 1 80); do
        if curl --fail --silent --output /dev/null "$url"; then
            return 0
        fi
        sleep 0.5
    done
    return 1
}

# Build strand-cam, unless the caller points us at a prebuilt directory
# (e.g. CI, which sets STRAND_BRAID_TARGET_DIR to its build directory).
if [ -z "${STRAND_BRAID_TARGET_DIR:-}" ]; then
    echo "=== Building strand-cam (cargo build) ==="
    ( cd "$REPO_DIR" && cargo build -p strand-cam )
fi
if [ ! -x "$TARGET_DIR/strand-cam" ]; then
    echo "ERROR: $TARGET_DIR/strand-cam not found. Build it first (see" >&2
    echo "docs/developer-docs/building-for-development.md) or set" >&2
    echo "STRAND_BRAID_TARGET_DIR." >&2
    exit 1
fi
command -v uv >/dev/null 2>&1 || {
    echo "ERROR: 'uv' is required to run the Python helper with a pinned" >&2
    echo "Python version and the 'requests' dependency. Install it from" >&2
    echo "https://docs.astral.sh/uv/getting-started/installation/" >&2
    exit 1
}
command -v ffmpeg >/dev/null 2>&1 || {
    echo "ERROR: 'ffmpeg' is required for the Ffmpeg-codec recording test." >&2
    exit 1
}

# Run the Python helper through uv. RUST_LOG is cleared because uv is itself a
# Rust program and would otherwise emit its own logs at the verbose level.
uv_run() { env -u RUST_LOG uv run --no-project "$@"; }

export DISABLE_VERSION_CHECK=1
export RUST_LOG="info"
# The sim backend renders the scenario named by this variable.
export STRAND_CAM_SIM_SPEC="$SIM_TOML"

cd "$WORK_DIR"

echo "=== Recording a color (RGB8) MP4 from the sim backend via ffmpeg ==="
SCAM_LOG="$WORK_DIR/strand-cam.log"
DATA_DIR="$WORK_DIR/recording"
mkdir -p "$DATA_DIR"

# `simcam0` is the first camera of the scenario. If strand-cam cannot set the
# RGB8 pixel format it exits during startup, so the HTTP server never comes up
# and wait_for_url below fails.
setsid "$TARGET_DIR/strand-cam" \
    --camera-backend sim \
    --camera-name simcam0 \
    --pixel-format RGB8 \
    --no-browser \
    --http-server-addr "127.0.0.1:$STRAND_CAM_PORT" \
    --data-dir "$DATA_DIR" \
    > "$SCAM_LOG" 2>&1 &
PIDS+=($!)

wait_for_url "http://127.0.0.1:$STRAND_CAM_PORT/" \
    || fail "strand-cam HTTP server did not come up (RGB8 rejected?)" "$SCAM_LOG"

uv_run --with requests "$SCRIPTS_DIR/record-mp4-video-ffmpeg.py" \
    --strand-cam-url "http://127.0.0.1:$STRAND_CAM_PORT/" \
    --codec libx264 --duration "$RECORD_SECONDS" --verify-dir "$DATA_DIR" \
    || fail "ffmpeg RGB8 recording did not produce a valid MP4" "$SCAM_LOG"

echo "PASSED"
