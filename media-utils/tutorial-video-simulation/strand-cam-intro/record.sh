#!/bin/bash
#
# Regenerates the "launching Strand Camera from the command line" tutorial
# video against the current repo, by default using the hardware-free `sim`
# camera backend (see ../README.md for what this replaces and why).
#
# Requires a Linux host with ffmpeg, xdotool, Xvfb, openbox, and xterm (hard
# requirements -- this always records on its own isolated virtual display,
# never your real desktop session), plus a browser (prefers an installed
# google-chrome/chromium, falls back to firefox) -- see ../README.md for the
# full story and how to review the output.
#
# Usage:
#   ./record.sh [OUTPUT_DIR]
#   CAMERA_BACKEND=pylon ./record.sh [OUTPUT_DIR]
#
# OUTPUT_DIR defaults to a directory named 'out' next to this script. It is
# created if missing and is not, and should not be, committed to the repo.
#
# CAMERA_BACKEND selects which strand-cam --camera-backend to actually run
# (defaults to "sim"). Set it to "pylon"/"vimba"/"webcam" to record against
# real camera hardware attached to this machine instead -- useful for
# regenerating the tutorial from an actual camera when one's available,
# rather than the hardware-free stand-in. Whichever backend is chosen, the
# on-screen commands always show the plain, unqualified form ("strand-cam",
# "strand-cam --camera-name <name>") a real user would type; see the
# CAMERA_BACKEND handling below for how that's kept true for non-default
# backends too.

# Rough pixel offsets (relative to each window's top-left) for pointing at
# on-screen camera-name text with point_at -- the browser's "Live view -
# <name>" heading, and a "run{cam=\"<name>\"}" occurrence in the terminal's
# log output. Tuned by eye from observed recordings at this window layout
# (see lib/session.sh's SESSION_MARGIN/SESSION_PANE_WIDTH); nudge these if a
# rerun shows the mouse missing the mark.
BROWSER_CAMNAME_X=100
BROWSER_CAMNAME_Y=400
# Command 1's own startup log (shorter scrollback so far) vs. Command 2's
# (typed further down, under all of Command 1's leftover output) settle at
# different heights, so these are two distinct points, not one reused twice.
TERM_CAMNAME_X=340
TERM_CAMNAME_Y=300
TERM_CAMNAME_Y2=500

set -o errexit
set -o nounset
set -o pipefail

SCRIPT_NAME="strand-cam-intro"
SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/../../.." && pwd)
OUT_DIR=$(cd "$(dirname "${1:-$SCRIPT_DIR/out}")" && pwd)/$(basename "${1:-$SCRIPT_DIR/out}")
mkdir -p "$OUT_DIR"

# shellcheck source=../lib/session.sh
source "$SCRIPT_DIR/../lib/session.sh"

CAMERA_BACKEND="${CAMERA_BACKEND:-sim}"

# Prefer, in order: an explicit override, an already-installed strand-cam
# (e.g. via the .deb package) found on PATH, then finally a from-source
# build -- so a normal desktop with the package installed never triggers an
# unnecessary cargo build.
if [ -n "${STRAND_BRAID_TARGET_DIR:-}" ]; then
    TARGET_DIR="$STRAND_BRAID_TARGET_DIR"
elif command -v strand-cam >/dev/null 2>&1; then
    TARGET_DIR=$(dirname "$(command -v strand-cam)")
    echo "=== Using installed strand-cam: $TARGET_DIR/strand-cam ==="
else
    TARGET_DIR="$REPO_ROOT/target/release"
fi

if [ ! -x "$TARGET_DIR/strand-cam" ]; then
    echo "=== Building strand-cam (cargo build --release) ==="
    ( cd "$REPO_ROOT" && cargo build --release -p strand-cam )
fi
if [ ! -x "$TARGET_DIR/strand-cam" ]; then
    echo "ERROR: $TARGET_DIR/strand-cam not found after build." >&2
    exit 1
fi

STRAND_CAM_VERSION=$("$TARGET_DIR/strand-cam" --version)
echo "=== $STRAND_CAM_VERSION ==="

export DISABLE_VERSION_CHECK=1
export RUST_LOG=info

if [ "$CAMERA_BACKEND" = "sim" ]; then
    export STRAND_CAM_SIM_SPEC="$REPO_ROOT/braid/braid-sim/example-sim.toml"
    SECOND_CAMERA_NAME="simcam0"
else
    echo "=== Detecting a camera for --camera-backend $CAMERA_BACKEND ==="
    SECOND_CAMERA_NAME=$("$TARGET_DIR/strand-cam" --camera-backend "$CAMERA_BACKEND" --list-cameras 2>/dev/null \
        | grep -E '^  [^ ]+  \(model:' | head -1 | awk '{print $1}')
    [ -n "$SECOND_CAMERA_NAME" ] || {
        echo "ERROR: no camera found for --camera-backend $CAMERA_BACKEND (checked via --list-cameras)" >&2
        exit 1
    }
    echo "=== Found real camera: $SECOND_CAMERA_NAME ==="
fi

# strand-cam has no env var for --camera-backend (it's CLI-only, defaulting
# to Pylon if omitted -- see ../README.md's "A note on --camera-backend
# sim"). The tutorial needs to show the plain, unqualified command a user
# with that hardware would actually type ("strand-cam", not "strand-cam
# --camera-backend <x>"): the flag is purely an artifact of selecting a
# *non-default* backend (sim, or a real backend other than pylon) and would
# confuse readers about what the real command is. A tiny wrapper script
# named `strand-cam`, placed earlier on PATH than the real binary, silently
# injects --camera-backend $CAMERA_BACKEND while forwarding everything else
# -- skipped entirely for "pylon", strand-cam's actual default, where the
# bare command is already exactly correct with no wrapper needed. This
# wrapper is only ever on PATH for this script's own process and the
# xterm/strand-cam it launches as children -- it cannot affect how
# strand-cam runs anywhere else on this machine, during or after this run,
# and session_cleanup deletes it (along with the rest of SESSION_WORK_DIR)
# once everything's confirmed stopped.
if [ "$CAMERA_BACKEND" = "pylon" ]; then
    export PATH="$TARGET_DIR:$PATH"
else
    WRAPPER_DIR="$SESSION_WORK_DIR/bin"
    mkdir -p "$WRAPPER_DIR"
    cat >"$WRAPPER_DIR/strand-cam" <<EOF
#!/bin/bash
exec "$TARGET_DIR/strand-cam" --camera-backend $CAMERA_BACKEND "\$@"
EOF
    chmod +x "$WRAPPER_DIR/strand-cam"
    export PATH="$WRAPPER_DIR:$TARGET_DIR:$PATH"
fi
BUI_URL="http://127.0.0.1:3440/"

echo "=== Starting virtual display and screen capture ==="
start_display
start_capture "$OUT_DIR/raw.mp4"

echo "=== Opening terminal and browser windows ==="
TERM_WIN=$(open_terminal)

# strand-cam itself runs as a child of the xterm's shell (it must, to be
# visible on screen), so session_cleanup's window-process kill won't reach
# it -- it's still running when the script exits normally after Command 2.
# Extend the trap to also stop it, scoped to xterm's own session id (set via
# setsid in open_terminal, so this xterm is that session's leader and
# strand-cam inherits the same SID as its descendant): safe regardless of
# --camera-backend, including "pylon" with no distinguishing command-line
# text of its own, and can never match an unrelated strand-cam elsewhere on
# this machine, which would belong to a different session entirely.
XTERM_PID=$(xdotool getwindowpid "$TERM_WIN")
trap "pkill -s $XTERM_PID -f strand-cam 2>/dev/null || true; session_cleanup" EXIT

echo "=== Command 1: launch with no --camera-name (auto-selects the first camera) ==="
type_in "$TERM_WIN" "strand-cam"
wait_for_url "$BUI_URL" || { echo "ERROR: strand-cam BUI did not come up"; exit 1; }
BROWSER_WIN=$(open_browser "$BUI_URL" "$TERM_WIN")

echo "=== Indicating the camera name (browser, then terminal) ==="
point_at_browser_text "$BROWSER_WIN" "Live view - " "$BROWSER_CAMNAME_X" "$BROWSER_CAMNAME_Y"
point_at "$TERM_WIN" "$TERM_CAMNAME_X" "$TERM_CAMNAME_Y"

echo "=== Watching the live view (scrolling the page down and back) ==="
scroll_page "$BROWSER_WIN"

echo "=== Ctrl+C ==="
log_event "Ctrl+C" 1.5
send_keys "$TERM_WIN" ctrl+c
sleep 2

echo "=== Command 2: relaunch with an explicit --camera-name ==="
type_only "$TERM_WIN" "strand-cam --camera-name $SECOND_CAMERA_NAME"

echo "=== Indicating the camera name (terminal), before activating ==="
point_at "$TERM_WIN" "$TERM_CAMNAME_X" "$TERM_CAMNAME_Y2"

xdotool key Return
wait_for_url "$BUI_URL" || { echo "ERROR: strand-cam BUI did not come back up"; exit 1; }

echo "=== Watching the live view again (scrolling the page down and back) ==="
scroll_page "$BROWSER_WIN"

echo "=== Stopping capture ==="
stop_capture

echo "=== Burning in captions ==="
python3 "$SCRIPT_DIR/../lib/burn_captions.py" \
    --events "$SESSION_EVENTS_FILE" \
    --input "$OUT_DIR/raw.mp4" \
    --output "$OUT_DIR/strand-cam-intro.mp4" \
    --comment "Generated by media-utils/tutorial-video-simulation/strand-cam-intro/record.sh using $STRAND_CAM_VERSION"

echo "=== Done: $OUT_DIR/strand-cam-intro.mp4 ==="
echo "Compare it against the original before deciding it's ready; adjust the"
echo "sleep durations above and rerun if the pacing looks off."
