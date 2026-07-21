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
# Hold on the first frame instead of playing immediately (ci2-video-file's
# default) -- this scenario explicitly kicks off real playback itself, once
# every BUI setting is configured, via a "StartPlayback" command sent
# through post_cam_arg (see below) -- so the checkerboard-collected count
# only ever starts climbing from that moment, not from whenever the camera
# happened to open.
export STRAND_CAM_VIDEO_FILE_AUTOSTART=false
# Play through CHECKERBOARD_VIDEO exactly once rather than looping forever
# (ci2-video-file's default) -- this scenario waits for the whole video, not
# a fixed checkerboard count or timeout (see "Watching checkerboard
# detections accumulate" below), so looping would mean it never naturally
# finishes.
export STRAND_CAM_VIDEO_FILE_LOOP=false
# Where ci2-video-file writes an empty marker file the instant playback
# reaches the end (see camera/ci2-video-file/src/lib.rs's "Signaling end of
# playback" docs) -- record.sh waits on this file's existence (wait_for_file,
# below) rather than the terminal's own "holding on last frame" log line.
# That log line is real and does fire, but polling for it via cdp_locate.py
# against the ttyd-bridged terminal is unreliable: xterm.js's DOM renderer
# only materializes the currently visible viewport as DOM nodes, and the
# checkerboard corner-detection loop logs ~4 lines/second (both while the
# video plays and, since detection keeps running against the frozen last
# frame, after it ends too) -- easily enough to scroll that one-time line
# out of view, and thus out of reach of any DOM query, within a few seconds
# of it appearing. A plain file's existence can't scroll away.
CHECKERBOARD_DONE_MARKER="$SESSION_WORK_DIR/video-file-ended"
export STRAND_CAM_VIDEO_FILE_DONE_MARKER="$CHECKERBOARD_DONE_MARKER"
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

echo "=== Starting virtual display ==="
start_display
# Screen capture (start_capture, below) is deliberately deferred until both
# the terminal and BUI windows are open AND the BUI's other panels are
# already collapsed -- on request, this recording doesn't need to show
# strand-cam being launched or the collapsing itself, just the tidied-up
# layout, right before the Checkerboard Calibration tab gets clicked.
# log_event still needs a valid
# SESSION_CAPTURE_START_EPOCH before start_capture actually sets the real
# one, though: type_in below (launching strand-cam) captions the typed
# command and the Enter press via log_event, which crashes on an empty
# epoch. This placeholder just avoids that crash -- start_capture overwrites
# it with the real recording-start time, and the events file gets cleared
# right after, so none of this pre-recording captioning leaks into the
# actual video.
SESSION_CAPTURE_START_EPOCH=$(python3 -c 'import time; print(time.time())')

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

echo "=== Letting the real default BUI layout settle for a moment ==="
# Every top-level BUI section normally opens in the state a real strand-cam
# session actually starts in -- "Live view", "MP4 Recording Options", "Post
# Triggering", "Object Detection", and "Camera Settings" all default to
# expanded (main.rs's CheckboxLabel `initially_checked=true`, or `true` in
# VideoField's own case for "Live view"). None of this default state is
# shown in the recording (screen capture starts below, after collapsing --
# on request, so viewers only ever see the tidied-up layout) -- this pause is
# just settle time for the BUI's own initial render before the collapse
# clicks below, not for anything to appear on screen.
sleep 2

echo "=== Collapsing other BUI panels (not relevant to this recording) ==="
# Every top-level BUI section (main.rs) is the same CSS-checkbox-hack
# collapsible as Checkerboard Calibration itself (see the comment further
# down at the panel-expand step) -- "Live view" specifically is
# VideoField's own wrap-collapsible (web/ads-webasm/src/components/
# video_field.rs), titled "Live view - {camera name}". All five collapsed
# here sit above Checkerboard Calibration in page order (or, for "Camera
# Settings", isn't worth leaving expanded either), so left alone they add
# scrolling/clutter before ever reaching the calibration content -- and
# "Live view" specifically needs to be out of the way before the
# checkerboard process (enabling calibration, playback) starts, so it
# doesn't compete for attention with the panel this recording is actually
# about. ("AprilTag Detection" isn't compiled into this build -- no
# `apriltag` feature -- so it never renders at all; "FMF & µFMF Recording",
# "ImOps Detection", "Kalman tracking", and "Online LED triggering" already
# default to collapsed, so there's nothing to do for those.) Collapsed via
# a plain click_browser_element, no visual pointing/captioning -- this is
# housekeeping for this recording's own clarity, not something the
# tutorial is about, and (like the checkerboard panel's own label) the
# click fires via CDP regardless of current scroll position, so there's no
# need to scroll to each one first.
for panel in "Live view" "MP4 Recording Options" "Post Triggering" "Object Detection" "Camera Settings"; do
    click_browser_element "$BROWSER_CDP_PORT" "$panel" label \
        || echo "WARNING: couldn't find/click the '$panel' panel label to collapse it -- continuing" >&2
done

echo "=== Starting screen capture (right-hand window's panels are all collapsed now) ==="
start_capture "$OUT_DIR/raw.mp4"
# Discard the caption events logged during setup above (typing the launch
# command, pressing Enter) -- those happened before real recording started,
# so their timestamps (relative to the placeholder epoch set near
# start_display) don't correspond to anything in raw.mp4.
: > "$SESSION_EVENTS_FILE"

echo "=== Suppressing frame-processing-error popups for this recording ==="
# strand-cam/yew_frontend/src/main.rs's frame_processing_error_dialog: a real,
# data-driven `.modal-container` that pops up whenever the backend reports
# `had_frame_processing_error`, which checkerboard corner-finding (CPU-heavy)
# can trigger under this pipeline's own recording overhead (Xvfb/xdotool/
# ffmpeg/Chrome all competing for CPU) -- not something a real user's
# strand-cam session would normally hit at all. An earlier version of this
# script waited (bounded to 8s) for the modal to appear right here, then
# clicked "Ignore all future errors" + "Dismiss" to suppress it -- but that
# 8s window is always well before checkerboard calibration is even enabled
# (let alone before the video starts playing), i.e. before the CPU load that
# actually triggers this error, so the modal was in practice never seen
# there and the click sequence never ran; it then went on to appear,
# undismissed, later in the recording with no code left to catch it.
#
# Fixed by suppressing it directly at the source instead of waiting for a
# UI element that may not exist yet: `CamArg::SetIngoreFutureFrameProcessingErrors(None)`
# (see camera/strand-cam-remote-control/src/lib.rs) sets the backend's
# `FrameProcessingErrorState` to `IgnoreAll` (strand-cam/src/cam_stream_task.rs),
# which suppresses `had_frame_processing_error` for the rest of this
# strand-cam process's life regardless of when/how often the underlying
# channel-full condition recurs -- the same "ignore forever" behavior the
# toggle-then-dismiss click sequence was trying to reach, just sent directly
# via post_cam_arg (the same `/callback` POST route used for the
# StartPlayback trigger below) instead of simulating clicks on a modal that
# may not have appeared yet. Sent early, before anything CPU-heavy starts,
# so the modal never has a chance to appear on screen at all -- which is
# also more faithful to what a real user would experience, since this error
# is purely an artifact of this recording pipeline's own overhead, not a
# normal part of the checkerboard calibration workflow.
post_cam_arg "$BUI_URL" '{"SetIngoreFutureFrameProcessingErrors":null}'

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
point_at_browser_text "$BROWSER_WIN" "$BROWSER_CDP_PORT" "Checkerboard Calibration" "" "" "0" "-2" 0
log_event "LEFT CLICK" 1.5
sleep 1.5
click_browser_element "$BROWSER_CDP_PORT" "Checkerboard Calibration" label
sleep 1

echo "=== Showing the checkerboard size fields (left at strand-cam's own 8x6 default) ==="
point_at_browser_text "$BROWSER_WIN" "$BROWSER_CDP_PORT" "Input: Checkerboard Size"
sleep 2

echo "=== Enabling checkerboard calibration and debug output ==="
# Deliberately BEFORE the StartPlayback trigger below, not after: enabling
# detection has some startup lag before it actually kicks in, and the held
# first frame (see STRAND_CAM_VIDEO_FILE_AUTOSTART above) never contains a
# checkerboard anyway, so there's no risk of a spurious detection against
# it. Enabling here and letting that lag happen against the still-held
# frame means detection is already warmed up and running by the time real
# playback starts, instead of missing the first however-many real frames
# while it spins up. Both toggles (same <Toggle> shape,
# checkerboard_calibration_ui in strand-cam/yew_frontend/src/main.rs) get
# their own "LEFT CLICK" caption, same as every other simulated click in
# this pipeline.
# Sweep width 0 -- about to click, not indicating text.
point_at_browser_text "$BROWSER_WIN" "$BROWSER_CDP_PORT" "Enable checkerboard calibration" "" "" "0" "-3" 0
log_event "LEFT CLICK" 1.5
sleep 1.5
# ANCESTOR_TAG "label", not the default "button" -- this is a <Toggle>
# (web/ads-webasm/src/components/toggle.rs), which renders
# <label><input type=checkbox></label> with no <button> in its DOM at all.
click_browser_element "$BROWSER_CDP_PORT" "Enable checkerboard calibration" label
point_at_browser_text "$BROWSER_WIN" "$BROWSER_CDP_PORT" "Save debug information" "" "" "0" "-3" 0
log_event "LEFT CLICK" 1.5
sleep 1.5
click_browser_element "$BROWSER_CDP_PORT" "Save debug information" label

# The toggle's checkbox mark ticks the instant the click lands (native
# browser checkbox behavior), but the orange ".toggle-on" styling and the
# "Saving debug data to {path}" line just below it are both driven by
# shared.checkerboard_save_debug coming back from the backend
# (CamArg::ToggleCheckerboardDebug -> cam_arg_task.rs creates the debug dir
# and updates shared state -> that state round-trips back to the browser and
# re-renders) -- not by the click itself. Wait for that real confirmation
# text before starting playback, so the recording shows the toggle's actual
# "on" appearance settled before the video begins, instead of the two
# appearing to happen back-to-back by coincidence. Not a hard gate: if this
# backend round-trip is ever unexpectedly slow, warn and proceed anyway
# rather than aborting an otherwise-fine recording over a cosmetic timing
# detail.
wait_for_browser_text "$BROWSER_CDP_PORT" "Saving debug data to" 20 1 \
    || echo "WARNING: 'Saving debug data to' never appeared within 20s -- proceeding anyway" >&2
sleep 1.5

echo "=== Starting checkerboard video playback ==="
# Everything is configured now (other panels collapsed, error modal handled,
# Checkerboard Calibration panel open, size fields shown, detection already
# enabled and running against the held frame above) -- tell ci2-video-file
# to stop holding on its first frame and begin real playback, via the exact
# same POST /callback route the BUI's own JS uses for every other camera
# command (see post_cam_arg in ../lib/session.sh), just called directly
# instead of through a simulated click. No on-screen caption for this one
# (deliberately removed on request) -- it's a plain HTTP call with nothing
# for a viewer to see happen on screen at the moment it fires anyway.
post_cam_arg "$BUI_URL" '{"ExecuteCommand":"StartPlayback"}'
sleep 1.5

echo "=== Watching checkerboard detections accumulate until the video ends ==="
# No checkerboard-count target and no fixed timeout -- CHECKERBOARD_VIDEO
# (STRAND_CAM_VIDEO_FILE_LOOP=false, set above) plays through exactly once,
# then ci2-video-file freezes on its last frame and creates
# CHECKERBOARD_DONE_MARKER (see the export above and ci2-video-file's own
# "Signaling end of playback" docs) instead of looping. Wait for that marker
# file -- the same "actual observed state, not a worst-case guess" principle
# scroll_until_visible/wait_for_browser_text already use elsewhere in this
# pipeline -- rather than polling a count or guessing a duration. A plain
# wait_for_file check, not wait_for_browser_text: the latter was tried first
# and reliably failed here, since the checkerboard-detection loop's own
# ~4 lines/second of logging scrolls a one-time terminal line like this out
# of the ttyd-rendered DOM long before a poll can see it (see the export
# comment above and POINTING-NOTES.md's dated update for the full
# diagnosis). Bounded to 200 tries * 1s = 200s: comfortably above
# CHECKERBOARD_VIDEO's own known ~120s duration (check with `ffprobe -v
# error -show_entries format=duration CHECKERBOARD_VIDEO` if using a
# different file) with margin, but not truly open-ended.
if ! wait_for_file "$CHECKERBOARD_DONE_MARKER" 200 1; then
    echo "ERROR: CHECKERBOARD_VIDEO never reached its end within the timeout." >&2
    echo "Is STRAND_CAM_VIDEO_FILE_LOOP=false actually reaching strand-cam? Check the terminal log." >&2
    exit 1
fi
echo "=== Video finished -- holding on its last frame ==="
# Log the final count for the record, but it's informational only now, not
# a gate -- however many checkerboards a full play-through of
# CHECKERBOARD_VIDEO happens to yield is however many there are.
FINAL_COUNT=$(get_browser_text "$BROWSER_CDP_PORT" "Number of checkerboards collected" 2>/dev/null) || FINAL_COUNT=""
echo "=== $FINAL_COUNT ==="
point_at_browser_text "$BROWSER_WIN" "$BROWSER_CDP_PORT" "Number of checkerboards collected" "" "" "15" "-10" 100
# The requested 1-second hold on the last frame before moving on.
sleep 1

echo "=== Disabling checkerboard calibration and debug output ==="
# The video has ended and no more checkerboards are coming, so turn both
# toggles back off before computing/saving the calibration (which runs on
# the corner sets already collected, not on live detection) -- same
# point/caption/click pattern as enabling them above.
point_at_browser_text "$BROWSER_WIN" "$BROWSER_CDP_PORT" "Enable checkerboard calibration" "" "" "0" "-3" 0
log_event "LEFT CLICK" 1.5
sleep 1.5
click_browser_element "$BROWSER_CDP_PORT" "Enable checkerboard calibration" label
point_at_browser_text "$BROWSER_WIN" "$BROWSER_CDP_PORT" "Save debug information" "" "" "0" "0" 0
log_event "LEFT CLICK" 1.5
sleep 1.5
click_browser_element "$BROWSER_CDP_PORT" "Save debug information" label
sleep 1.5

# Where CamArg::PerformCheckerboardCalibration (strand-cam/src/cam_arg_task.rs)
# saves the non-timestamped calibration file -- matches its own resolution
# exactly (directories::BaseDirs::config_dir().join(APP_INFO.name).join("camera_info"),
# APP_INFO.name == "strand-cam" -- see strand-cam/src/strand-cam.rs), and
# CHECKERBOARD_CAM_NAME (detected earlier via --list-cameras) is exactly the
# raw_cam_name that handler uses to name the file. Respect XDG_CONFIG_HOME
# if set, same as the `directories` crate does on Linux.
CHECKERBOARD_CAL_YAML="${XDG_CONFIG_HOME:-$HOME/.config}/strand-cam/camera_info/$CHECKERBOARD_CAM_NAME.yaml"
# Captured BEFORE clicking "Perform and Save Calibration": this file lives
# in a real, persistent config directory (unlike CHECKERBOARD_DONE_MARKER's
# fresh-per-run SESSION_WORK_DIR), so a from-a-previous-run file could
# already exist here -- a bare existence check afterward would report
# success immediately on that stale leftover, before this run's own save
# has actually happened. wait_for_file_newer_than (below) needs this
# baseline to tell "already there" apart from "just written".
CHECKERBOARD_CAL_YAML_BASELINE_MTIME=$(stat -c %Y "$CHECKERBOARD_CAL_YAML" 2>/dev/null) || CHECKERBOARD_CAL_YAML_BASELINE_MTIME=0

echo "=== Performing and saving the calibration ==="
# Sweep width 0 -- about to click, not indicating text.
point_at_browser_text "$BROWSER_WIN" "$BROWSER_CDP_PORT" "Perform and Save Calibration" "" "" "0" "-3" 0
log_event "LEFT CLICK" 1.5
sleep 1.5
click_browser_element "$BROWSER_CDP_PORT" "Perform and Save Calibration"

echo "=== Confirming the calibration was saved (checking the real file, not the terminal log) ==="
# An earlier version of this wait polled the terminal log for "Saved camera
# calibration to file" -- removed (see POINTING-NOTES.md's dated update):
# cdp_locate.py can only see whatever's currently in the ttyd-rendered
# viewport, so that wait could never reliably distinguish "calibration
# succeeded" from "the line already scrolled out of view before a poll
# checked" -- a source of false-negative warnings, not a real confirmation.
# Checking the actual file strand-cam writes is unambiguous: bounded to 15
# tries * 2s = 30s, same budget the old terminal-log wait used.
if wait_for_file_newer_than "$CHECKERBOARD_CAL_YAML" "$CHECKERBOARD_CAL_YAML_BASELINE_MTIME" 15 2; then
    echo "=== Saved: $CHECKERBOARD_CAL_YAML ==="

    echo "=== Opening a file navigator to browse to the calibration file ==="
    # Chrome's own built-in file:// directory listing, not a real file
    # manager (Nautilus et al. have no CDP/DOM to query, and turned out to
    # have real GApplication-singleton isolation problems -- see
    # POINTING-NOTES.md) -- --app mode hides the tab strip/address bar so
    # it doesn't read as an obvious browser tab, same trick open_terminal
    # uses for ttyd. Starts at $HOME so the recording shows real
    # step-by-step folder navigation down to the calibration file, the same
    # "watch a user browse there" principle as a real file manager demo,
    # just backed by real, CDP-verified clicks the whole way.
    move_mouse_gradual_into "$TERM_WIN"
    open_file_navigator "$HOME"
    sleep 1

    for folder in ".config" "strand-cam" "camera_info"; do
        point_at_browser_text "$NAV_WIN" "$NAV_CDP_PORT" "$folder" "" "" "0" "-10" 0
        log_event "LEFT CLICK" 1.5
        sleep 1.5
        # ANCESTOR_TAG "a": these are Chrome's own real <a> links for each
        # directory-listing row (confirmed via a live DOM query before
        # writing this), so clicking one fires a real navigation to that
        # subfolder, no different from any other link click in this
        # pipeline.
        click_browser_element "$NAV_CDP_PORT" "$folder" a
        sleep 1
    done

    echo "=== Selecting the calibration file (opens in a new window, like a real double-click) ==="
    # Sweep width 0 (about to "click"), and deliberately NOT a real
    # click_browser_element call: Chrome has no built-in viewer for .yaml,
    # confirmed it silently downloads the file instead of displaying it
    # (see POINTING-NOTES.md) -- so the actual "open" action below is a
    # generated viewer page, not letting this link's own href navigate.
    point_at_browser_text "$NAV_WIN" "$NAV_CDP_PORT" "$(basename "$CHECKERBOARD_CAL_YAML")" "" "" "0" "-10" 0
    log_event "LEFT CLICK" 1.5
    sleep 1.5
    open_file_viewer "$CHECKERBOARD_CAL_YAML"
    sleep 1

    echo "=== Pointing at the calibration's own quality metric ==="
    # "# Mean reprojection distance: ..." -- camcal::save_yaml's own leading
    # comment (lower is better; this is the real number a user would care
    # about most from this whole exercise), rendered as plain text inside
    # the generated viewer's <pre> -- pointable exactly like any other
    # on-screen text in this pipeline, since it's real HTML/DOM, not a
    # native app.
    point_at_browser_text "$VIEWER_WIN" "$VIEWER_CDP_PORT" "distance:" "" "" "50" "6"
    sleep 3
else
    echo "WARNING: $CHECKERBOARD_CAL_YAML was never (re)written within the timeout -- calibration may have failed (check the terminal log for an ERROR line, e.g. from too few/too degenerate checkerboard poses). Skipping the file-navigator steps." >&2
    move_mouse_gradual_into "$TERM_WIN"
    sleep 3
fi

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
