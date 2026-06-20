#!/bin/bash
#
# Regression reproduction for the live-vs-retrack trajectory FRAGMENTATION bug.
#
# Background: a real single-camera flydratrax recording produced 212 live
# trajectories that collapsed to 3 on retracking with the same parameters. Root
# cause (confirmed by instrumenting flydra2's track deaths): the live tracker's
# effective frame rate was wrong (too high), so the Kalman `dt = 1/fps` did not
# match the true frame spacing. With a too-small dt the process noise is too
# small, the filter becomes over-confident, and the real per-frame motion of a
# *maneuvering* target falls outside the acceptance gate -> observations are
# rejected -> the track coasts -> covariance kill -> re-birth, repeating into
# hundreds of fragments. Retracking at the correct fps tracks continuously.
#
# This script reproduces the mechanism deterministically in the simulation
# harness: it records a clean braidz of a *maneuvering* simulated insect at a
# true 30 fps, then retracks it at the matched fps (30) and at a mismatched,
# too-high fps (100). The mismatched retrack fragments into many more
# trajectories than the matched one. A correct fix (robust frame-rate handling /
# timestamp-derived dt) should remove that gap.
#
# Usage: smoke-tests/flydratrax-fps-fragmentation.sh
#
# Env: STRAND_BRAID_TARGET_DIR -- when unset (the default), the script builds
#      the binaries with `cargo build` (strand-cam with the `flydratrax`
#      feature) into <repo>/target/debug; when set, the binaries there are used
#      as-is and strand-cam must have been built with `flydratrax`.

set -o errexit
set -o nounset
set -o pipefail

REPO_DIR=$(cd "$(dirname "$0")/.." && pwd)
TARGET_DIR="${STRAND_BRAID_TARGET_DIR:-$REPO_DIR/target/debug}"
PORT="${PORT:-3479}"
WORK_DIR=$(mktemp -d -t flydratrax-fps-XXXXXX)
PIDS=()

cleanup() {
    local pid
    for pid in "${PIDS[@]}"; do kill -- "-$pid" 2>/dev/null || true; done
    sleep 1
    rm -rf "$WORK_DIR"
}
trap cleanup EXIT
fail() { echo "FAILED: $1" >&2; exit 1; }

# Build the binaries, unless the caller points us at a prebuilt directory.
# strand-cam needs the (non-default) flydratrax feature.
if [ -z "${STRAND_BRAID_TARGET_DIR:-}" ]; then
    echo "=== Building strand-cam (flydratrax), braid-sim, braid-offline, braidz-cli (cargo build) ==="
    ( cd "$REPO_DIR" && cargo build -p strand-cam --features flydratrax )
    ( cd "$REPO_DIR" && cargo build -p braid-sim -p braid-offline -p braidz-cli )
fi
for exe in strand-cam braid-sim braid-offline-retrack braidz-cli; do
    [ -x "$TARGET_DIR/$exe" ] || fail "$TARGET_DIR/$exe not found (build it; strand-cam needs the flydratrax feature)"
done
command -v uv >/dev/null 2>&1 || fail "'uv' is required (https://docs.astral.sh/uv/getting-started/installation/)"

# Run a Python helper through uv. RUST_LOG is cleared because uv is itself a Rust
# program and would otherwise emit its own logs at the verbose level we set below.
uv_run() { env -u RUST_LOG uv run --no-project "$@"; }

# A maneuvering insect at a true 30 fps. The small, fast 'maneuver' overlay gives
# high acceleration (sharp turns) that a constant-velocity filter cannot predict.
cat > "$WORK_DIR/sim.toml" <<'EOF'
seed = 1
fps = 30.0
[arena]
min = [-0.15, -0.15, 0.0]
max = [0.15, 0.15, 0.30]
[cameras]
count = 1
radius_m = 0.6
height_m = 0.7
focal_length_px = 900.0
image_width = 640
image_height = 512
[blob]
peak = 160
sigma = 1.5
background = 0
[[insects]]
id = 1
[insects.motion]
freq_hz = [0.11, 0.13, 0.07]
phase = [0.0, 1.0, 2.0]
fill = 0.5
maneuver_amp_m = 0.004
maneuver_freq_hz = 6.0
EOF

echo "=== Recording a clean live braidz (flydratrax, single sim camera, 30 fps) ==="
export DISABLE_VERSION_CHECK=1
export RUST_LOG="${RUST_LOG:-error}"
STRAND_CAM_SIM_SPEC="$WORK_DIR/sim.toml" setsid "$TARGET_DIR/strand-cam" \
    --camera-backend sim --camera-name simcam0 --no-browser \
    --http-server-addr "127.0.0.1:$PORT" --csv-save-dir "$WORK_DIR" \
    > "$WORK_DIR/scam.log" 2>&1 &
PIDS+=($!)

for i in $(seq 1 60); do curl -fsS "http://127.0.0.1:$PORT/" >/dev/null 2>&1 && break; sleep 0.5; done

uv_run - "$PORT" <<'PY'
# /// script
# requires-python = ">=3.9"
# dependencies = ["requests"]
# ///
import sys, time, urllib.parse, requests
u = "http://127.0.0.1:%s/" % sys.argv[1]
s = requests.session(); s.get(u).raise_for_status()
def cb(p): s.post(urllib.parse.urljoin(u, "callback"), json={"ToCamera": p}).raise_for_status()
# A circular valid region provides the pseudo-calibration flydratrax needs.
yaml_cfg = """do_update_background_model: true
polarity: DetectAbsDiff
alpha: 0.01
n_sigma: 7.0
bright_non_gaussian_cutoff: 255
bright_non_gaussian_replacement: 5
bg_update_interval: 200
diff_threshold: 30
use_cmp: true
max_num_points: 1
feature_window_size: 30
clear_fraction: 0.3
despeckle_threshold: 5
valid_region:
  Circle:
    center_x: 320
    center_y: 256
    radius: 230
"""
time.sleep(6)  # let the measured fps settle before tracking starts
cb({"SetObjDetectionConfig": yaml_cfg})
cb({"SetIsDoingObjDetection": True})
time.sleep(2)
cb({"SetIsSavingObjDetectionCsv": {"Saving": None}})
time.sleep(14)
cb({"SetIsSavingObjDetectionCsv": "NotSaving"})
time.sleep(2)
PY

kill -- "-${PIDS[0]}" 2>/dev/null || true; PIDS=(); sleep 1

LIVE=$(find "$WORK_DIR" -name "*.braidz" ! -name "*.retrack.braidz" | head -1)
[ -n "$LIVE" ] || fail "no live braidz produced (see $WORK_DIR/scam.log)"

ntraj() { "$TARGET_DIR/braidz-cli" "$1" 2>/dev/null | awk '/num_trajectories/{print $2}'; }

echo "=== Retracking at matched (30) and mismatched (100) fps ==="
"$TARGET_DIR/braid-offline-retrack" --data-src "$LIVE" -o "$WORK_DIR/match.braidz"    --fps 30  >/dev/null 2>&1
"$TARGET_DIR/braid-offline-retrack" --data-src "$LIVE" -o "$WORK_DIR/mismatch.braidz" --fps 100 >/dev/null 2>&1

M=$(ntraj "$WORK_DIR/match.braidz")
X=$(ntraj "$WORK_DIR/mismatch.braidz")
echo "matched   (--fps 30):  num_trajectories=$M"
echo "mismatched (--fps 100): num_trajectories=$X"

# The bug: a wrong (too-high) fps fragments the same data into many more tracks.
# This assertion documents the CURRENT (buggy) behavior; a fix that makes the
# tracker robust to a wrong fps should shrink this gap (flip the comparison).
if [ "$X" -gt "$M" ]; then
    echo "PASS: mismatched fps fragments more ($X > $M) -- bug reproduced."
else
    fail "expected mismatched-fps fragmentation ($X > $M) but did not see it"
fi
