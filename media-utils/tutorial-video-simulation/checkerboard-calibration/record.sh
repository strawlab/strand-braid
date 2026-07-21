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
# frames (ci2-sim) cannot provide. Instead, it plays a real recorded
# checkerboard video directly through strand-cam's `video-file` backend
# (`camera/ci2-video-file`, --camera-backend video-file), which decodes the
# file itself via the `media-utils/frame-source` crate and paces playback to
# its own native frame rate -- no virtual camera device, `ffmpeg` feeder
# process, or `nokhwa` involved at all.
#
# (An earlier attempt fed the video through a `v4l2loopback` virtual webcam
# into strand-cam's `webcam` backend instead; `nokhwa` failed to open that
# device at all -- see checkerboard-calibration/POINTING-NOTES.md's BLOCKED
# section for the full diagnosis -- so this scenario switched to the
# video-file backend, added specifically to unblock this, instead.)
#
# Requires everything strand-cam-intro/braid-intro require (see
# ../README.md's Prerequisites -- ffmpeg, xdotool, Xvfb, openbox, ttyd,
# x11-utils, a browser), PLUS:
#
#   - CHECKERBOARD_VIDEO: a video file (any container/codec the
#     `media-utils/frame-source` crate can decode, e.g. mp4) of a real
#     checkerboard held at varying distances/angles, including into the
#     corners of frame, ideally with brief (>=1s) pauses at each distinct
#     pose -- strand-cam's checkerboard-detection loop only samples at most
#     once every 500ms (`checkerboard_loop_dur` in
#     ../../../strand-cam/src/frame_process_task.rs), so continuous fast
#     motion may never let it collect a clean detection at any single pose.
#     No default; the script errors out immediately if unset.
#   - A strand-cam build with both the `checkercal` cargo feature (NOT in
#     strand-cam's default feature set -- see ../../../strand-cam/Cargo.toml
#     and ../../../strand-cam/README.md's release build command) AND the
#     `video-file` backend (a plain dependency, not gated by any cargo
#     feature, but new enough that it's not yet in any *installed* build --
#     see BUILD_NEW_STRANDBRAID just below). If this script ends up building
#     from source (see TARGET_DIR resolution below) it adds --features
#     checkercal itself; if it finds strand-cam already installed/on PATH,
#     it trusts that build but VERIFIES the "Checkerboard Calibration" panel
#     actually renders once the BUI is up (see below) and errors out with a
#     clear message if not, rather than recording a video of a missing
#     feature.
#
# Usage:
#   CHECKERBOARD_VIDEO=/path/to/checkerboard.mp4 ./record.sh [OUTPUT_DIR]
#
# OUTPUT_DIR defaults to a directory named 'out' next to this script. It is
# created if missing and is not, and should not be, committed to the repo.
#
# BUILD_NEW_STRANDBRAID (default "true"): the `video-file` backend is new
# (added as part of this tutorial-video work) and not yet reviewed/merged
# upstream, so it's deliberately NOT part of whatever build is installed on
# PATH (e.g. the real .deb package, built by this project's primary
# developer -- this script must never rebuild or overwrite that). While
# true, this script builds and uses its own local copy from this repo
# instead (in $REPO_ROOT/target/release, never on PATH). Once the
# video-file backend is approved and lands in whatever build ends up
# installed, set BUILD_NEW_STRANDBRAID=false to switch back to the normal
# prefer-the-installed-build behavior strand-cam-intro/braid-intro already
# use.

set -o errexit
set -o nounset
set -o pipefail

SCRIPT_NAME="checkerboard-calibration"
SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/../../.." && pwd)
OUT_DIR=$(cd "$(dirname "${1:-$SCRIPT_DIR/out}")" && pwd)/$(basename "${1:-$SCRIPT_DIR/out}")
mkdir -p "$OUT_DIR"

: "${CHECKERBOARD_VIDEO:?ERROR: set CHECKERBOARD_VIDEO to a video of a checkerboard shown at varying distances/angles (see the header comment in this script)}"
[ -f "$CHECKERBOARD_VIDEO" ] || {
    echo "ERROR: CHECKERBOARD_VIDEO=$CHECKERBOARD_VIDEO not found" >&2
    exit 1
}

# Minimum "checkerboards collected" count to wait for before clicking
# "Perform and Save Calibration". The users-guide's own recommended minimum
# for a REAL calibration is higher (docs/user-docs/users-guide/src/
# braid_calibration.md: "say, at least 10"); this default is lower purely to
# bound how long this *recording* takes (see also the tightened
# wait_for_checkerboard_count/save-confirmation timeouts below) -- not a
# strand-cam-enforced minimum either way (the BUI's own Perform button is
# disabled only at exactly 0 collected). Override to 10+ if you actually
# care about the resulting calibration's quality rather than just the video.
CHECKERBOARD_MIN_COUNT="${CHECKERBOARD_MIN_COUNT:-5}"

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

# BUILD_NEW_STRANDBRAID: unlike strand-cam-intro/braid-intro, this scenario
# deliberately does NOT default to preferring an already-installed strand-cam
# (e.g. the real .deb package at /usr/bin/strand-cam, built by the project's
# primary developer -- never to be overwritten or rebuilt by this script).
# The `video-file` backend this scenario relies on is new, added as part of
# this tutorial-video work, and not yet reviewed/merged upstream -- so no
# installed build has it yet (confirmed: the installed /usr/bin/strand-cam on
# this machine rejects `--camera-backend video-file` outright, since it
# predates this backend entirely). Default `true` builds a local copy from
# this repo instead (into $REPO_ROOT/target/release, never on PATH, never
# touching the installed binary). Once the video-file backend has been
# reviewed and lands in whatever build is installed, set
# BUILD_NEW_STRANDBRAID=false to go back to the normal
# prefer-the-installed-binary behavior every other scenario here uses.
BUILD_NEW_STRANDBRAID="${BUILD_NEW_STRANDBRAID:-true}"

# Prefer, in order: an explicit STRAND_BRAID_TARGET_DIR override (always
# wins, regardless of BUILD_NEW_STRANDBRAID); else, if BUILD_NEW_STRANDBRAID
# is true, a from-source build in $REPO_ROOT/target/release; else (matching
# strand-cam-intro/braid-intro's own resolution order) an already-installed
# strand-cam found on PATH, falling back to a from-source build only if
# nothing is installed. Whether the checkercal feature is actually present
# in the resulting binary is checked later, once the BUI is up (see
# "Verifying checkercal" below) -- --version/--help give no way to tell from
# here, since checkercal has no CLI surface of its own (it only changes what
# the BUI renders).
if [ -n "${STRAND_BRAID_TARGET_DIR:-}" ]; then
    TARGET_DIR="$STRAND_BRAID_TARGET_DIR"
elif [ "$BUILD_NEW_STRANDBRAID" = "true" ]; then
    TARGET_DIR="$REPO_ROOT/target/release"
    echo "=== BUILD_NEW_STRANDBRAID=true: using/building a local strand-cam from this repo (not the installed one) ==="
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

# The video-file backend's only "camera" is the file itself, named by the
# STRAND_CAM_VIDEO_FILE environment variable (camera/ci2-video-file/src/lib.rs) --
# export it before probing, then detect the exact --camera-name it expects
# (the file's own stem; see VideoFileCameraInfo::default_name) the same way
# strand-cam-intro/braid-intro detect a real camera's name for non-default
# backends, via --list-cameras, rather than re-deriving the stem in bash and
# risking it drifting from Rust's own file_stem() logic.
export STRAND_CAM_VIDEO_FILE="$CHECKERBOARD_VIDEO"
echo "=== Detecting the video-file camera name ==="
CHECKERBOARD_LIST_OUTPUT=$("$TARGET_DIR/strand-cam" --camera-backend video-file --list-cameras 2>&1) || true
CHECKERBOARD_CAM_NAME=$(echo "$CHECKERBOARD_LIST_OUTPUT" | grep -E '^  [^ ]+  \(model:' | head -1 | awk '{print $1}')
[ -n "$CHECKERBOARD_CAM_NAME" ] || {
    echo "ERROR: no camera found for --camera-backend video-file (checked via --list-cameras)." >&2
    if echo "$CHECKERBOARD_LIST_OUTPUT" | grep -q "invalid value 'video-file'"; then
        echo "$TARGET_DIR/strand-cam doesn't recognize --camera-backend video-file at all -- it predates" >&2
        echo "that backend. Set BUILD_NEW_STRANDBRAID=true (the default) so this script builds its own" >&2
        echo "local copy from this repo instead of using that one." >&2
    else
        echo "Is CHECKERBOARD_VIDEO=$CHECKERBOARD_VIDEO a video file strand-cam's video-file backend can open?" >&2
    fi
    exit 1
}
echo "=== Found video-file camera: $CHECKERBOARD_CAM_NAME ==="

BUI_URL="http://127.0.0.1:3440/"

echo "=== Starting virtual display and screen capture ==="
start_display
start_capture "$OUT_DIR/raw.mp4"

# strand-cam has no env var for --camera-backend (CLI-only, defaults to
# Pylon -- see ../README.md's "A note on --camera-backend sim"). The real
# hardware this tutorial is ultimately about is a physical Basler camera
# (see docs/user-docs/users-guide/src/braid_calibration.md), for which the
# plain, unqualified "strand-cam --camera-name <name>" is exactly correct --
# `--camera-backend video-file` is purely an artifact of this recording
# pipeline's playback stand-in, not something a real user with that hardware
# would type. Same PATH-shadowing wrapper trick strand-cam-intro uses for its
# own non-default backends: a tiny wrapper named `strand-cam`, earlier on
# PATH than the real binary, silently injects --camera-backend video-file
# while forwarding everything else. Scoped to this script's own process and
# its ttyd/strand-cam children only; deleted by session_cleanup along with
# the rest of SESSION_WORK_DIR.
#
# Built and exported to PATH *before* open_terminal, not after: open_terminal
# launches ttyd's shell as its own process, which only ever sees the PATH
# record.sh had at that moment -- a later `export PATH=...` in record.sh's
# own shell doesn't retroactively reach an already-running child (this bit
# strand-cam-intro's own version of this trick too, which is why its wrapper
# setup comes before its own open_terminal call).
WRAPPER_DIR="$SESSION_WORK_DIR/bin"
mkdir -p "$WRAPPER_DIR"
cat >"$WRAPPER_DIR/strand-cam" <<EOF
#!/bin/bash
exec "$TARGET_DIR/strand-cam" --camera-backend video-file "\$@"
EOF
chmod +x "$WRAPPER_DIR/strand-cam"
export PATH="$WRAPPER_DIR:$TARGET_DIR:$PATH"

echo "=== Opening terminal ==="
open_terminal

# strand-cam runs as a child of the bash shell ttyd is bridging into the
# browser, so session_cleanup's window-process kill won't reach it -- same
# reasoning as strand-cam-intro's own trap extension.
trap "pkill -s $TERM_SESSION_PID -f strand-cam 2>/dev/null || true; session_cleanup" EXIT

echo "=== Launching strand-cam against the checkerboard video ==="
type_in "$TERM_WIN" "strand-cam --camera-name $CHECKERBOARD_CAM_NAME"
wait_for_url "$BUI_URL" || { echo "ERROR: strand-cam BUI did not come up"; exit 1; }
open_browser "$BUI_URL" "$TERM_WIN"

echo "=== Verifying checkercal is compiled into this build ==="
if ! wait_for_browser_text "$BROWSER_CDP_PORT" "Checkerboard Calibration" 10 1; then
    echo "ERROR: no 'Checkerboard Calibration' panel found in the BUI." >&2
    echo "The strand-cam binary at $TARGET_DIR/strand-cam was not built with --features checkercal." >&2
    echo "Rebuild it with: cargo build --release -p strand-cam --features checkercal" >&2
    exit 1
fi

echo "=== Collapsing other BUI panels (not relevant to this recording) ==="
# Every top-level BUI section (main.rs) is the same CSS-checkbox-hack
# collapsible as Checkerboard Calibration itself (see the comment further
# down at the panel-expand step). Of the ones this build actually renders,
# "MP4 Recording Options", "Post Triggering", and "Object Detection" default
# to expanded (`initially_checked=true`) and sit above Checkerboard
# Calibration in page order, so left alone they add scrolling/clutter before
# ever reaching the calibration content; "Camera Settings" also defaults to
# expanded, further down. ("AprilTag Detection" isn't compiled into this
# build -- no `apriltag` feature -- so it never renders at all; "FMF & µFMF
# Recording", "ImOps Detection", "Kalman tracking", and "Online LED
# triggering" already default to collapsed, so there's nothing to do for
# those.) Collapsed via a plain click_browser_element, no visual
# pointing/captioning -- these are housekeeping for this recording's own
# clarity, not something the tutorial is about, and (like the checkerboard
# panel's own label) the click fires via CDP regardless of current scroll
# position, so there's no need to scroll to each one first.
for panel in "MP4 Recording Options" "Post Triggering" "Object Detection" "Camera Settings"; do
    click_browser_element "$BROWSER_CDP_PORT" "$panel" label \
        || echo "WARNING: couldn't find/click the '$panel' panel label to collapse it -- continuing" >&2
done

echo "=== Checking for a stuck 'frame processing too slow' error modal ==="
# strand-cam/yew_frontend/src/main.rs's frame_processing_error_dialog: a real,
# data-driven `.modal-container` (fixed near the top of the viewport,
# regardless of scroll position -- ads-webasm/scss/_base.scss) that pops up
# whenever the backend reports `had_frame_processing_error`, which checkerboard
# corner-finding (CPU-heavy) can trigger under this pipeline's own recording
# overhead (Xvfb/xdotool/ffmpeg/Chrome all competing for CPU). Left alone, it
# sits on screen indefinitely and can recur once calibration starts. Dismissed
# here, before touching the Checkerboard Calibration panel at all, with
# "Ignore all future errors" also toggled on so it can't reappear mid-run.
# Bounded wait (not wait_for_browser_text's open-ended default) since this
# modal may not appear at all on a lightly-loaded run -- that's fine, just
# move on if it doesn't show up within a few seconds.
if wait_for_browser_text "$BROWSER_CDP_PORT" "frame processing too slow" 8 1; then
    echo "=== Dismissing it: toggling 'Ignore all future errors', then Dismiss ==="
    # Sweep width 0 -- about to click, not indicating text (see
    # point_at_browser_text's own doc comment in lib/session.sh: the
    # left-right wiggle reads as "look at this text," not "about to click
    # this").
    point_at_browser_text "$BROWSER_WIN" "$BROWSER_CDP_PORT" "Ignore all future errors" "" "" "" "" 0
    log_event "LEFT CLICK" 1.5
    sleep 1.5
    # ANCESTOR_TAG "label" -- same <Toggle> shape as "Enable checkerboard
    # calibration" below (web/ads-webasm/src/components/toggle.rs).
    click_browser_element "$BROWSER_CDP_PORT" "Ignore all future errors" label
    point_at_browser_text "$BROWSER_WIN" "$BROWSER_CDP_PORT" "Dismiss" "" "" "" "" 0
    log_event "LEFT CLICK" 1.5
    sleep 1.5
    # Needle "Dismiss" alone is ambiguous in principle (main.rs has two other
    # "Dismiss" buttons: a JSON-decode-error modal and a version-update
    # banner) -- not ambiguous in practice here, since DISABLE_VERSION_CHECK=1
    # (set above) keeps the version banner from ever rendering, and a JSON
    # decode error is not expected in normal operation, so this modal's
    # Dismiss is the only one actually in the DOM.
    click_browser_element "$BROWSER_CDP_PORT" "Dismiss"
    sleep 1
else
    echo "=== Not present within the timeout -- continuing ==="
fi

echo "=== Scrolling down to the Checkerboard Calibration panel ==="
scroll_until_visible "$BROWSER_WIN" "$BROWSER_CDP_PORT" down "Checkerboard Calibration" 60

echo "=== Expanding the Checkerboard Calibration panel ==="
# The whole panel is a CSS-checkbox-hack collapsible
# (web/ads-webasm/scss/_wrap_collapsible.scss's `.wrap-collapsible`): a
# <CheckboxLabel label="Checkerboard Calibration"> renders a hidden
# <input type=checkbox> immediately followed by a sibling <label>, and every
# control inside (the toggles, size fields, Perform button) sits in a
# `display:none` sibling <div> until that checkbox is checked. Scrolling to
# the heading text above only confirms it's present in the DOM -- it does
# NOT expand the section, so without this click every following interaction
# targets elements with zero layout box (which is why point_at_browser_text
# below can't find them to visually point at, even though the underlying
# click_browser_element calls still fire on the hidden DOM nodes) and,
# worse, the panel visibly stays closed for the whole recording. Click the
# label (not the hidden input directly) to toggle it open, same
# click-a-<label>-to-activate-its-<input> mechanism as the Toggle components
# below.
# Sweep width 0 -- about to click, not indicating text.
point_at_browser_text "$BROWSER_WIN" "$BROWSER_CDP_PORT" "Checkerboard Calibration" "" "" "" "" 0
log_event "LEFT CLICK" 1.5
sleep 1.5
click_browser_element "$BROWSER_CDP_PORT" "Checkerboard Calibration" label
sleep 1

echo "=== Enabling checkerboard calibration ==="
# Sweep width 0 -- about to click, not indicating text.
point_at_browser_text "$BROWSER_WIN" "$BROWSER_CDP_PORT" "Enable checkerboard calibration" "" "" "" "" 0
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
# Polls the live "Number of checkerboards collected: N" counter
# (strand-cam/yew_frontend/src/main.rs) until N reaches CHECKERBOARD_MIN_COUNT,
# the same "actual on-screen state, not a worst-case guess" principle
# scroll_until_visible/wait_for_browser_text already use elsewhere in this
# pipeline. Bounded to 60 tries * 1s = 60s worst case (not
# wait_for_browser_text's own 150-tries-*-2s = 5 minute default) -- part of
# keeping this scenario's overall recording length reasonable (target ~2m30s
# total; see also the tightened "Saved camera calibration" wait below), since
# an open-ended wait here would otherwise let a slow/sparse CHECKERBOARD_VIDEO
# blow that budget with no feedback until the very end.
wait_for_checkerboard_count() {
    local min_count="$1" tries="${2:-60}" interval="${3:-1}" i text n
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
# Sweep width 0 -- about to click, not indicating text.
point_at_browser_text "$BROWSER_WIN" "$BROWSER_CDP_PORT" "Perform and Save Calibration" "" "" "" "" 0
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
# also fire once per attempt but reads less clearly as success. Bounded to
# 15 tries * 2s = 30s (not the 150-tries-*-2s = 5 minute default) -- a
# successful save confirms almost immediately, and this scenario's overall
# recording length is meant to stay near ~2m30s (see wait_for_checkerboard_count
# above); sitting idle for the full 5 minutes on a failed calibration would
# blow that budget for no benefit, since the WARNING below already covers
# the "it didn't save" case just as well at 30s as at 5 minutes.
move_mouse_gradual_into "$TERM_WIN"
if wait_for_browser_text "$TERM_CDP_PORT" "Saved camera calibration" 15 2; then
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
