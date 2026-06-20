#!/bin/bash
#
# End-to-end regression test for the hardware-timestamp frame-rate fix.
#
# Scenario: a maneuvering simulated insect at a true 30 fps, but the camera's
# *host* timestamps are bunched (reported as 100 fps) -- modeling a host clock
# distorted by buffering under load. The sim also emits a hardware (device)
# timestamp at the true cadence.
#
# With the fix (frame rate estimated from the hardware timestamp), live tracking
# stays continuous despite the bunched host clock. To show what the bunching
# would otherwise do, we retrack the same recording at the matched fps (30) and
# at the bunched fps (100): the bunched fps fragments the trajectory into many
# more tracks. The assertion is that the LIVE result is close to the matched
# retrack and far from the bunched one -- i.e. the fix neutralized the bunching.
# If the fix regresses (fps taken from the host clock again), the live result
# jumps toward the bunched count and this test fails.
#
# Usage: smoke-tests/flydratrax-fps-fix.sh
# Env:   STRAND_BRAID_TARGET_DIR (default <repo>/target/debug); strand-cam must
#        be built with the `flydratrax` feature.

set -o errexit
set -o nounset
set -o pipefail

REPO_DIR=$(cd "$(dirname "$0")/.." && pwd)
TARGET_DIR="${STRAND_BRAID_TARGET_DIR:-$REPO_DIR/target/debug}"
PORT="${PORT:-3489}"
WORK_DIR=$(mktemp -d -t flydratrax-fps-fix-XXXXXX)
PIDS=()
cleanup() { for pid in "${PIDS[@]}"; do kill -- "-$pid" 2>/dev/null || true; done; sleep 1; rm -rf "$WORK_DIR"; }
trap cleanup EXIT
fail() { echo "FAILED: $1" >&2; exit 1; }

for exe in strand-cam braid-offline-retrack braidz-cli; do
    [ -x "$TARGET_DIR/$exe" ] || fail "$TARGET_DIR/$exe not found (build it; strand-cam needs the flydratrax feature)"
done
python3 -c "import requests" || fail "python 'requests' is required"

# Maneuvering insect, true 30 fps, but host timestamps bunched to 100 fps. The
# sim emits a hardware timestamp at the true cadence.
cat > "$WORK_DIR/sim.toml" <<'EOF'
seed = 1
fps = 30.0
reported_fps = 100.0
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

export DISABLE_VERSION_CHECK=1
export RUST_LOG="${RUST_LOG:-error}"
STRAND_CAM_SIM_SPEC="$WORK_DIR/sim.toml" setsid "$TARGET_DIR/strand-cam" \
    --camera-backend sim --camera-name simcam0 --no-browser \
    --http-server-addr "127.0.0.1:$PORT" --csv-save-dir "$WORK_DIR" \
    > "$WORK_DIR/scam.log" 2>&1 &
PIDS+=($!)
for i in $(seq 1 60); do curl -fsS "http://127.0.0.1:$PORT/" >/dev/null 2>&1 && break; sleep 0.5; done

python3 - "$PORT" <<'PY'
import sys, time, urllib.parse, requests
u = "http://127.0.0.1:%s/" % sys.argv[1]
s = requests.session(); s.get(u).raise_for_status()
def cb(p): s.post(urllib.parse.urljoin(u, "callback"), json={"ToCamera": p}).raise_for_status()
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
time.sleep(6)
cb({"SetObjDetectionConfig": yaml_cfg}); cb({"SetIsDoingObjDetection": True})
time.sleep(2); cb({"SetIsSavingObjDetectionCsv": {"Saving": None}}); time.sleep(14)
cb({"SetIsSavingObjDetectionCsv": "NotSaving"}); time.sleep(2)
PY
kill -- "-${PIDS[0]}" 2>/dev/null || true; PIDS=(); sleep 1

LIVE=$(find "$WORK_DIR" -name "*.braidz" ! -name "*.retrack.braidz" | head -1)
[ -n "$LIVE" ] || fail "no live braidz produced (see $WORK_DIR/scam.log)"
ntraj() { "$TARGET_DIR/braidz-cli" "$1" 2>/dev/null | awk '/num_trajectories/{print $2}'; }

"$TARGET_DIR/braid-offline-retrack" --data-src "$LIVE" -o "$WORK_DIR/match.braidz"    --fps 30  >/dev/null 2>&1
"$TARGET_DIR/braid-offline-retrack" --data-src "$LIVE" -o "$WORK_DIR/mismatch.braidz" --fps 100 >/dev/null 2>&1

L=$(ntraj "$LIVE"); M=$(ntraj "$WORK_DIR/match.braidz"); X=$(ntraj "$WORK_DIR/mismatch.braidz")
echo "live (fix, host bunched to 100):   num_trajectories=$L"
echo "retrack --fps 30 (matched):        num_trajectories=$M"
echo "retrack --fps 100 (bunched/buggy): num_trajectories=$X"

# The fix must keep live tracking close to the matched retrack and far below the
# bunched one. Require live to be nearer M than X (midpoint test) and that the
# bunched count is meaningfully larger (so the scenario is actually exercising
# the bug).
[ "$X" -gt "$((M + 3))" ] || fail "scenario not exercising the bug: bunched=$X not >> matched=$M"
if [ "$L" -le "$(( (M + X) / 2 ))" ]; then
    echo "PASS: live ($L) tracks like the matched retrack ($M), not the bunched one ($X) -- fix works."
else
    fail "live ($L) is closer to the bunched retrack ($X) than the matched ($M); fps fix appears broken"
fi
