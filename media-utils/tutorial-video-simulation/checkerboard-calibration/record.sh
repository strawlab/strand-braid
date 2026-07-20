#!/bin/bash
#
# Records a new "intrinsic (checkerboard) camera calibration" tutorial video:
# launch strand-cam, scroll to the "Checkerboard Calibration" panel, enable
# it, watch the live "checkerboards collected" counter increase as a
# checkerboard is shown to the camera at different distances/angles, then
# click "Perform and Save Calibration" and point at the terminal's own
# confirmation log line. See ../README.md for the general architecture and
# ../ONBOARDING.md for this scenario's current status -- unlike
# strand-cam-intro/braid-intro, this one is NOT regenerating a pre-existing
# tutorial video (no "Video_3.mp4" exists in this repo); it's new content,
# and this first pass is UNVERIFIED (written on macOS, where this pipeline
# cannot run at all -- see ../README.md's Prerequisites).
#
# Unlike strand-cam-intro (real Basler hardware or the hardware-free `sim`
# backend) and braid-intro (real Basler hardware only), this scenario has no
# camera hardware of its own at all: it needs a real, moving checkerboard on
# screen for strand-cam's calibration algorithm to actually detect corners
# from, which the synthetic `sim` backend's procedurally-generated insect-blob
# frames (ci2-sim) cannot provide. Instead, it feeds a real recorded video of
# a checkerboard into a `v4l2loopback` virtual video device via `ffmpeg`, and
# points strand-cam's `webcam` backend (ci2-webcam, backed by `nokhwa`'s
# native Linux/V4L2 enumeration) at that device -- the loopback device looks
# like an ordinary webcam to nokhwa, so no strand-cam/ci2 code changes are
# needed, only a new prerequisite (see below).
#
# Requires everything strand-cam-intro/braid-intro require (see
# ../README.md's Prerequisites -- ffmpeg, xdotool, Xvfb, openbox, ttyd,
# x11-utils, a browser), PLUS:
#
#   - The `v4l2loopback` kernel module, with a loopback device already
#     loaded under a known card_label (default "checkerboard-cam", override
#     via CHECKERBOARD_LOOPBACK_LABEL) -- e.g.:
#       sudo modprobe v4l2loopback video_nr=9 card_label="checkerboard-cam" exclusive_caps=1
#     This script deliberately does NOT modprobe it itself (that needs root,
#     and this project's other scripts never invoke sudo) -- it just checks
#     for the device and errors out with the exact command above if missing.
#     exclusive_caps=1 matters: without it, some v4l2 consumers (including,
#     per user reports of nokhwa/v4l on other loopback setups) fail to see
#     the device as a capture source at all.
#   - CHECKERBOARD_VIDEO: a video file (any container/codec ffmpeg can
#     decode -- mp4/webm, doesn't matter, see ../README.md discussion) of a
#     real checkerboard held at varying distances/angles, including into the
#     corners of frame, ideally with brief (>=1s) pauses at each distinct
#     pose -- strand-cam's checkerboard-detection loop only samples at most
#     once every 500ms (`checkerboard_loop_dur` in
#     ../../../strand-cam/src/frame_process_task.rs), so continuous fast
#     motion may never let it collect a clean detection at any single pose.
#     No default; the script errors out immediately if unset.
#   - A strand-cam build with the `checkercal` cargo feature (NOT in
#     strand-cam's default feature set -- see ../../../strand-cam/Cargo.toml
#     and ../../../strand-cam/README.md's release build command). If this
#     script ends up building from source (see TARGET_DIR resolution below)
#     it adds --features checkercal itself; if it finds strand-cam already
#     installed/on PATH, it trusts that build but VERIFIES the "Checkerboard
#     Calibration" panel actually renders once the BUI is up (see below) and
#     errors out with a clear message if not, rather than recording a video
#     of a missing feature.
#
# Usage:
#   CHECKERBOARD_VIDEO=/path/to/checkerboard.mp4 ./record.sh [OUTPUT_DIR]
#
# OUTPUT_DIR defaults to a directory named 'out' next to this script. It is
# created if missing and is not, and should not be, committed to the repo.

set -o errexit
set -o nounset
set -o pipefail

SCRIPT_NAME="checkerboard-calibration"
SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/../../.." && pwd)
OUT_DIR=$(cd "$(dirname "${1:-$SCRIPT_DIR/out}")" && pwd)/$(basename "${1:-$SCRIPT_DIR/out}")
mkdir -p "$OUT_DIR"

: "${CHECKERBOARD_VIDEO:?ERROR: set CHECKERBOARD_VIDEO to a video of a checkerboard shown at varying distances/angles (see this script's own header comment)}"
[ -f "$CHECKERBOARD_VIDEO" ] || {
    echo "ERROR: CHECKERBOARD_VIDEO=$CHECKERBOARD_VIDEO not found" >&2
    exit 1
}

# Minimum "checkerboards collected" count to wait for before clicking
# "Perform and Save Calibration" -- matches the users-guide's own recommended
# minimum (docs/user-docs/users-guide/src/braid_calibration.md: "say, at
# least 10"), not a strand-cam-enforced minimum (the BUI's own Perform button
# is disabled only at exactly 0 collected).
CHECKERBOARD_MIN_COUNT="${CHECKERBOARD_MIN_COUNT:-10}"
CHECKERBOARD_LOOPBACK_LABEL="${CHECKERBOARD_LOOPBACK_LABEL:-checkerboard-cam}"

# shellcheck source=../lib/session.sh
source "$SCRIPT_DIR/../lib/session.sh"

# Rough pixel offsets, used only as a fallback for point_at_browser_text if
# its CDP text lookup fails -- see lib/session.sh's point_at_browser_text.
# UNTUNED: no visual review has happened yet (this was written on macOS,
# where this whole pipeline cannot run -- see ../README.md's Prerequisites),
# so these are deliberately left unset rather than guessed; a failed CDP
# lookup will just warn and skip that one point/click rather than aim
# somewhere wrong. Fill these in after watching the first real run, the same
# way strand-cam-intro/POINTING-NOTES.md and braid-intro/POINTING-NOTES.md
# were tuned -- see this scenario's own POINTING-NOTES.md.

# Prefer, in order: an explicit override, an already-installed strand-cam
# (e.g. via the .deb package) found on PATH, then finally a from-source
# build -- same resolution order as strand-cam-intro/braid-intro, so a normal
# desktop with the package installed never triggers an unnecessary cargo
# build. Whether the checkercal feature is actually present in the resulting
# binary is checked later, once the BUI is up (see "Verifying checkercal"
# below) -- --version/--help give no way to tell from here, since checkercal
# has no CLI surface of its own (it only changes what the BUI renders).
if [ -n "${STRAND_BRAID_TARGET_DIR:-}" ]; then
    TARGET_DIR="$STRAND_BRAID_TARGET_DIR"
elif command -v strand-cam >/dev/null 2>&1; then
    TARGET_DIR=$(dirname "$(command -v strand-cam)")
    echo "=== Using installed strand-cam: $TARGET_DIR/strand-cam ==="
else
    TARGET_DIR="$REPO_ROOT/target/release"
fi

if [ ! -x "$TARGET_DIR/strand-cam" ]; then
    # Unlike strand-cam-intro's plain build: checkercal is NOT a default
    # feature (strand-cam/Cargo.toml's `default = [...]` omits it), so a
    # from-source build here must ask for it explicitly or the "Checkerboard
    # Calibration" panel simply won't exist.
    echo "=== Building strand-cam (cargo build --release --features checkercal) ==="
    ( cd "$REPO_ROOT" && cargo build --release -p strand-cam --features checkercal )
fi
if [ ! -x "$TARGET_DIR/strand-cam" ]; then
    echo "ERROR: $TARGET_DIR/strand-cam not found after build." >&2
    exit 1
fi

STRAND_CAM_VERSION=$("$TARGET_DIR/strand-cam" --version)
echo "=== $STRAND_CAM_VERSION ==="

export DISABLE_VERSION_CHECK=1
export RUST_LOG=info

# Find the v4l2loopback device by its card_label (set at `modprobe` time --
# see this script's own header comment), rather than a hardcoded /dev/videoN
# number, so this doesn't depend on which numbers happen to be free on a
# given machine. Sysfs, not `v4l2-ctl` (which may not be installed) --
# every V4L2 capture device exposes its own name at
# /sys/class/video4linux/videoN/name.
LOOPBACK_DEVICE=""
for name_file in /sys/class/video4linux/video*/name; do
    [ -r "$name_file" ] || continue
    if [ "$(cat "$name_file")" = "$CHECKERBOARD_LOOPBACK_LABEL" ]; then
        LOOPBACK_DEVICE="/dev/$(basename "$(dirname "$name_file")")"
        break
    fi
done
[ -n "$LOOPBACK_DEVICE" ] || {
    echo "ERROR: no v4l2loopback device named '$CHECKERBOARD_LOOPBACK_LABEL' found." >&2
    echo "Set one up first (needs root -- this script deliberately doesn't do it for you):" >&2
    echo "  sudo modprobe v4l2loopback video_nr=9 card_label=\"$CHECKERBOARD_LOOPBACK_LABEL\" exclusive_caps=1" >&2
    exit 1
}
echo "=== Found v4l2loopback device: $LOOPBACK_DEVICE ($CHECKERBOARD_LOOPBACK_LABEL) ==="

BUI_URL="http://127.0.0.1:3440/"

echo "=== Starting virtual display and screen capture ==="
start_display
start_capture "$OUT_DIR/raw.mp4"

echo "=== Feeding $CHECKERBOARD_VIDEO into $LOOPBACK_DEVICE on a loop ==="
# -stream_loop -1: loop forever (session_cleanup stops it, like every other
# SESSION_PIDS entry -- it would otherwise run past ffmpeg's own EOF and this
# script has no fixed a-priori duration for the source video). -re: paces
# output at the video's own native frame rate, matching what a real live
# camera feed would deliver instead of dumping frames as fast as decode
# allows. -pix_fmt yuv420p: v4l2loopback consumers (including nokhwa) expect
# a raw pixel format, not whatever the source container's own codec used.
setsid ffmpeg -nostdin -y -stream_loop -1 -re -i "$CHECKERBOARD_VIDEO" \
    -pix_fmt yuv420p -f v4l2 "$LOOPBACK_DEVICE" \
    >"$SESSION_WORK_DIR/ffmpeg-loopback.log" 2>&1 &
SESSION_PIDS+=("$!")
sleep 2

echo "=== Opening terminal ==="
open_terminal

# strand-cam runs as a child of the bash shell ttyd is bridging into the
# browser, so session_cleanup's window-process kill won't reach it -- same
# reasoning as strand-cam-intro's own trap extension.
trap "pkill -s $TERM_SESSION_PID -f strand-cam 2>/dev/null || true; session_cleanup" EXIT

# strand-cam has no env var for --camera-backend (CLI-only, defaults to
# Pylon -- see ../README.md's "A note on --camera-backend sim"). The real
# hardware this tutorial is ultimately about is a physical Basler camera
# (see docs/user-docs/users-guide/src/braid_calibration.md), for which the
# plain, unqualified "strand-cam --camera-name <name>" is exactly correct --
# `--camera-backend webcam` is purely an artifact of this recording
# pipeline's v4l2loopback stand-in, not something a real user with that
# hardware would type. Same PATH-shadowing wrapper trick strand-cam-intro
# uses for its own non-default backends: a tiny wrapper named `strand-cam`,
# earlier on PATH than the real binary, silently injects
# --camera-backend webcam while forwarding everything else. Scoped to this
# script's own process and its ttyd/strand-cam children only; deleted by
# session_cleanup along with the rest of SESSION_WORK_DIR.
WRAPPER_DIR="$SESSION_WORK_DIR/bin"
mkdir -p "$WRAPPER_DIR"
cat >"$WRAPPER_DIR/strand-cam" <<EOF
#!/bin/bash
exec "$TARGET_DIR/strand-cam" --camera-backend webcam "\$@"
EOF
chmod +x "$WRAPPER_DIR/strand-cam"
export PATH="$WRAPPER_DIR:$TARGET_DIR:$PATH"

echo "=== Launching strand-cam against the checkerboard feed ==="
type_in "$TERM_WIN" "strand-cam --camera-name $CHECKERBOARD_LOOPBACK_LABEL"
wait_for_url "$BUI_URL" || { echo "ERROR: strand-cam BUI did not come up"; exit 1; }
open_browser "$BUI_URL" "$TERM_WIN"

echo "=== Verifying checkercal is compiled into this build ==="
if ! wait_for_browser_text "$BROWSER_CDP_PORT" "Checkerboard Calibration" 10 1; then
    echo "ERROR: no 'Checkerboard Calibration' panel found in the BUI." >&2
    echo "The strand-cam binary at $TARGET_DIR/strand-cam was not built with --features checkercal." >&2
    echo "Rebuild it with: cargo build --release -p strand-cam --features checkercal" >&2
    exit 1
fi

echo "=== Scrolling down to the Checkerboard Calibration panel ==="
scroll_until_visible "$BROWSER_WIN" "$BROWSER_CDP_PORT" down "Checkerboard Calibration" 60

echo "=== Enabling checkerboard calibration ==="
point_at_browser_text "$BROWSER_WIN" "$BROWSER_CDP_PORT" "Enable checkerboard calibration"
log_event "LEFT CLICK" 1.5
sleep 1.5
# ANCESTOR_TAG "label", not the default "button" -- this is a <Toggle>
# (web/ads-webasm/src/components/toggle.rs), which renders
# <label><input type=checkbox></label> with no <button> in its DOM at all.
click_browser_element "$BROWSER_CDP_PORT" "Enable checkerboard calibration" label

echo "=== Showing the checkerboard size fields (left at strand-cam's own 8x6 default) ==="
point_at_browser_text "$BROWSER_WIN" "$BROWSER_CDP_PORT" "Input: Checkerboard Size"
sleep 2

echo "=== Watching checkerboard detections accumulate ==="
# No fixed sleep -- polls the live "Number of checkerboards collected: N"
# counter (strand-cam/yew_frontend/src/main.rs) until N reaches
# CHECKERBOARD_MIN_COUNT, the same "actual on-screen state, not a worst-case
# guess" principle scroll_until_visible/wait_for_browser_text already use
# elsewhere in this pipeline. No fixed upper bound here either (matches
# wait_for_browser_text's own "no fixed upper bound" default of 150 tries *
# 2s = 5 minutes) -- how long this takes depends entirely on
# CHECKERBOARD_VIDEO's own content, which this script doesn't control.
wait_for_checkerboard_count() {
    local min_count="$1" tries="${2:-150}" interval="${3:-2}" i text n
    for ((i = 0; i < tries; i++)); do
        text=$(get_browser_text "$BROWSER_CDP_PORT" "Number of checkerboards collected" 2>/dev/null) || text=""
        if [[ "$text" =~ collected:\ ([0-9]+) ]]; then
            n="${BASH_REMATCH[1]}"
            echo "  ...checkerboards collected so far: $n" >&2
            if [ "$n" -ge "$min_count" ]; then
                echo "$n"
                return 0
            fi
        fi
        sleep "$interval"
    done
    return 1
}
COLLECTED=$(wait_for_checkerboard_count "$CHECKERBOARD_MIN_COUNT") || {
    echo "ERROR: only collected fewer than $CHECKERBOARD_MIN_COUNT checkerboards within the timeout." >&2
    echo "CHECKERBOARD_VIDEO may need more/longer distinct checkerboard poses -- see this script's own header comment." >&2
    exit 1
}
echo "=== Collected $COLLECTED checkerboards ==="
point_at_browser_text "$BROWSER_WIN" "$BROWSER_CDP_PORT" "Number of checkerboards collected"
sleep 2

echo "=== Performing and saving the calibration ==="
point_at_browser_text "$BROWSER_WIN" "$BROWSER_CDP_PORT" "Perform and Save Calibration"
log_event "LEFT CLICK" 1.5
sleep 1.5
click_browser_element "$BROWSER_CDP_PORT" "Perform and Save Calibration"

echo "=== Confirming the save in the terminal log ==="
# "Saved camera calibration to file" (from the `info!(...)` in
# strand-cam/src/cam_arg_task.rs's CamArg::PerformCheckerboardCalibration
# handler) -- a one-time message logged exactly once per successful save,
# the same "unique anchor, not a repeated prefix" reasoning strand-cam-intro
# already applies to its own "got camera" lookup (see its POINTING-NOTES.md
# history) rather than something like "computing calibration", which would
# also fire once per attempt but reads less clearly as success.
move_mouse_gradual_into "$TERM_WIN"
if wait_for_browser_text "$TERM_CDP_PORT" "Saved camera calibration"; then
    point_at_browser_text "$TERM_WIN" "$TERM_CDP_PORT" "Saved camera calibration"
else
    echo "WARNING: 'Saved camera calibration to file' never appeared in the terminal log -- calibration may have failed (check for an ERROR line, e.g. from too few/too degenerate checkerboard poses)." >&2
fi
sleep 3

echo "=== Stopping capture ==="
stop_capture

echo "=== Burning in captions ==="
python3 "$SCRIPT_DIR/../lib/burn_captions.py" \
    --events "$SESSION_EVENTS_FILE" \
    --input "$OUT_DIR/raw.mp4" \
    --output "$OUT_DIR/checkerboard-calibration.mp4" \
    --comment "Generated by media-utils/tutorial-video-simulation/checkerboard-calibration/record.sh using $STRAND_CAM_VERSION"

echo "=== Done: $OUT_DIR/checkerboard-calibration.mp4 ==="
echo "This is a first pass with UNTUNED pointing constants (see this script's own"
echo "header comment and POINTING-NOTES.md) -- watch it, expect to need to fix up"
echo "point_at_browser_text fallbacks and pacing before treating it as final."
