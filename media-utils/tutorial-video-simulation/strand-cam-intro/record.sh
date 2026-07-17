#!/bin/bash
#
# Regenerates the "launching Strand Camera from the command line" tutorial
# video against the current repo, by default using the hardware-free `sim`
# camera backend (see ../README.md for what this replaces and why).
#
# Requires a Linux host with ffmpeg, xdotool, Xvfb, openbox, and ttyd (hard
# requirements -- this always records on its own isolated virtual display,
# never your real desktop session), plus a browser (prefers an installed
# google-chrome/chromium, falls back to firefox) -- the terminal itself is a
# ttyd-bridged PTY running inside a browser window, not a native terminal
# emulator, so its on-screen text can be located the same way as the BUI's
# (see ../README.md for the full story and how to review the output).
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

# Rough pixel offsets (relative to each window's top-left), used only as a
# fallback for point_at_browser_text if its CDP text lookup fails (e.g.
# firefox ended up as the fallback browser, which doesn't speak CDP) --
# see lib/session.sh's point_at_browser_text/_open_isolated_browser_window.
# Tuned by eye from observed recordings at this window layout (see
# lib/session.sh's SESSION_MARGIN/SESSION_PANE_WIDTH); nudge these if a
# rerun ever needs the fallback and the mouse misses the mark.
BROWSER_CAMNAME_X=100
BROWSER_CAMNAME_Y=400
# Command 1's own startup log (shorter scrollback so far) vs. Command 2's
# (typed further down, under all of Command 1's leftover output) settle at
# different heights, so these are two distinct fallback points, not one
# reused twice.
TERM_CAMNAME_X=340
TERM_CAMNAME_Y=300
TERM_CAMNAME_Y2=500

# Chrome's own window-close button (top-right, part of the browser's own
# chrome, not a page DOM element -- cdp_locate.py can't query it the way it
# queries page text). Confirmed empirically from a real screenshot at this
# exact window geometry (SESSION_PANE_WIDTH x SESSION_PANE_HEIGHT): the "x"
# sits about 23px in from the right edge and 23px down from the top.
BROWSER_CLOSE_X=545
BROWSER_CLOSE_Y=23

# Per-point offsets added to point_at_browser_text's own located
# position (center-x, just-below-baseline-y -- see lib/session.sh),
# tunable independently for each of the three CDP-located points below.
# 0,6 matches point_at_browser_text's own defaults, i.e. "no adjustment
# yet" -- change a point's own pair here after a visual review rather than
# touching the shared default in session.sh.
#
# Units: pixels, standard top-left-origin screen coordinates (+X right,
# +Y down) -- CSS pixels as returned by Chrome's getBoundingClientRect()
# in cdp_locate.py, which equal physical/screen pixels here since nothing
# sets a device-pixel-ratio/scale-factor on this Xvfb display. They're
# added directly to window-relative pixel coordinates that get passed to
# `xdotool mousemove`, at the fixed 1280x800 resolution session.sh sets
# (SESSION_WIDTH/SESSION_HEIGHT) -- not a resolution-independent unit, so
# re-tune these if that display size ever changes. Same units as the
# BROWSER_CAMNAME_X/Y-style fallback constants above.
# Tuned per visual review 2026-07-17: point 1 (browser heading) and point 2
# (terminal "got camera") both needed to move up and right, closer to the
# indicated text -- up = decrease OFFSET_Y, right = increase OFFSET_X
# (standard screen convention: +X right, +Y down). Point 3's X was already
# fine; it just needed to move up a little.
BROWSER_HEADING_OFFSET_X=12
BROWSER_HEADING_OFFSET_Y=-6
TERM_GOTCAMERA_OFFSET_X=12
TERM_GOTCAMERA_OFFSET_Y=-6
TERM_CAMNAME2_OFFSET_X=0
TERM_CAMNAME2_OFFSET_Y=0

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
# Not "TERM_WIN=$(open_terminal)" -- open_terminal sets TERM_WIN,
# TERM_SESSION_PID, and TERM_CDP_PORT as globals; capturing it via command
# substitution would run it in a subshell and silently discard all of them.
open_terminal

# strand-cam itself runs as a child of the bash shell ttyd is bridging into
# the browser (it must, to be visible on screen), so session_cleanup's
# window-process kill won't reach it -- it's still running when the script
# exits normally after Command 2. Extend the trap to also stop it, scoped to
# ttyd's own session id (TERM_SESSION_PID, set by open_terminal via setsid,
# so ttyd is that session's leader and strand-cam inherits the same SID as
# its descendant): safe regardless of --camera-backend, including "pylon"
# with no distinguishing command-line text of its own, and can never match
# an unrelated strand-cam elsewhere on this machine, which would belong to a
# different session entirely.
trap "pkill -s $TERM_SESSION_PID -f strand-cam 2>/dev/null || true; session_cleanup" EXIT

echo "=== Command 1: launch with no --camera-name (auto-selects the first camera) ==="
type_in "$TERM_WIN" "strand-cam"
wait_for_url "$BUI_URL" || { echo "ERROR: strand-cam BUI did not come up"; exit 1; }
# Not "BROWSER_WIN=$(open_browser ...)" -- same subshell problem as
# open_terminal above; open_browser sets BROWSER_WIN/BROWSER_CDP_PORT itself.
open_browser "$BUI_URL" "$TERM_WIN"

echo "=== Indicating the camera name (browser, then terminal) ==="
point_at_browser_text "$BROWSER_WIN" "$BROWSER_CDP_PORT" "Live view - " "$BROWSER_CAMNAME_X" "$BROWSER_CAMNAME_Y" "$BROWSER_HEADING_OFFSET_X" "$BROWSER_HEADING_OFFSET_Y"
# "got camera" (from `info!("  got camera {}", raw_name)` in
# strand-cam/src/strand-cam.rs) rather than the tracing span-context prefix
# 'run{cam="...'": that prefix is repeated on EVERY log line emitted from
# within the `run` span, not just once, so it's not actually a unique
# anchor -- confirmed live: it matched whichever such line happened to be
# last, nowhere near where "got camera" itself is. "got camera" is a
# one-time message, logged exactly once. Previously a tuned pixel guess
# with no way to verify it (see POINTING-NOTES.md); now a real CDP text
# lookup against the ttyd terminal, the same as the browser heading above.
point_at_browser_text "$TERM_WIN" "$TERM_CDP_PORT" "got camera" "$TERM_CAMNAME_X" "$TERM_CAMNAME_Y" "$TERM_GOTCAMERA_OFFSET_X" "$TERM_GOTCAMERA_OFFSET_Y"

# Visible travel from the terminal (where the mouse just was) to the
# browser, rather than letting scroll_page's own move_mouse_into jump
# there instantly -- scroll_page still calls move_mouse_into itself right
# after this, but by then it's already there, so that becomes a no-op.
move_mouse_gradual_into "$BROWSER_WIN"

echo "=== Watching the live view (scrolling the page down and back) ==="
scroll_page "$BROWSER_WIN"

# Simulated click back into the terminal (same style as the close/reopen
# clicks: move the mouse there, caption "LEFT CLICK", no literal xdotool
# click needed -- send_keys below activates the window for real), so it
# reads clearly that we've returned to the terminal before Ctrl+C, rather
# than Ctrl+C appearing to come out of nowhere right after scrolling the
# browser.
echo "=== Moving back to the terminal ==="
move_mouse_gradual_into "$TERM_WIN"
log_event "LEFT CLICK" 1.5
sleep 1.5
sleep 1

echo "=== Ctrl+C ==="
log_event "Ctrl+C" 1.5
send_keys "$TERM_WIN" ctrl+c
sleep 2

echo "=== Command 2: relaunch with an explicit --camera-name ==="
type_only "$TERM_WIN" "strand-cam --camera-name $SECOND_CAMERA_NAME"

echo "=== Indicating the camera name (terminal), before activating ==="
# Needle is just the camera name, not "--camera-name $SECOND_CAMERA_NAME"
# -- the full typed command is long enough to wrap across two terminal
# rows (the shell prompt alone is most of a row), and a needle spanning
# that wrap boundary matches no single row, only some much larger ancestor
# (confirmed: a spanning needle here returned a ~530x540 box, essentially
# the whole terminal pane, not a specific line). $SECOND_CAMERA_NAME alone
# is short enough to reliably land within one row. It can still match an
# earlier occurrence in Command 1's scrollback above (e.g. "got camera
# simcam0") if that's still visible, but cdp_locate.py's tie-break (last
# match in document order wins among equal-area candidates) resolves that
# to the bottom-most -- i.e. most recent -- occurrence.
point_at_browser_text "$TERM_WIN" "$TERM_CDP_PORT" "$SECOND_CAMERA_NAME" "$TERM_CAMNAME_X" "$TERM_CAMNAME_Y2" "$TERM_CAMNAME2_OFFSET_X" "$TERM_CAMNAME2_OFFSET_Y"

press_return "$TERM_WIN"
wait_for_url "$BUI_URL" || { echo "ERROR: strand-cam BUI did not come back up"; exit 1; }

echo "=== Watching the live view again ==="
# No scroll_page here (unlike Command 1's live-view pause above) -- removed
# on request. A plain pause instead of cutting straight to the next step,
# so the reconnected live view is visible for a beat first. Reduced from
# 10 to 4 (~40%) on request.
sleep 4

# Closing the BUI window, then reopening it via the terminal's printed URL
# -- both "clicks" below are simulated, not real: point_at/point_at_browser_text
# move the mouse there and log_event captions "LEFT CLICK" (same style as
# Ctrl+C/Enter), but the actual close/reopen happens programmatically
# (xdotool windowclose / open_browser) rather than via a literal xdotool
# click, because neither target is something this harness can click for
# real: Chrome's own close button is browser chrome, not a page DOM element
# cdp_locate.py can query, and actually clicking the terminal's printed URL
# would trigger ttyd's own link-opening (xterm.js's WebLinksAddon, loaded
# unconditionally) -- which opens a new tab in the *terminal's* browser
# window/process, not a new window in the BUI's usual right-hand pane.
# Trailing "0" arg on both calls below: disables point_at's left-right
# sweep (see lib/session.sh) -- that wiggle reads as "indicating text,"
# not "about to click something," which is what these two are.
echo "=== Closing the Strand Cam browser window ==="
point_at "$BROWSER_WIN" "$BROWSER_CLOSE_X" "$BROWSER_CLOSE_Y" 0
log_event "LEFT CLICK" 1.5
sleep 1.5
xdotool windowclose "$BROWSER_WIN" 2>/dev/null || true
sleep 1

echo "=== Reopening it via the terminal's printed URL ==="
point_at_browser_text "$TERM_WIN" "$TERM_CDP_PORT" "$BUI_URL" "$TERM_CAMNAME_X" "$TERM_CAMNAME_Y2" 0 -10 0
log_event "LEFT CLICK" 1.5
sleep 1.5
open_browser "$BUI_URL" "$TERM_WIN"
wait_for_url "$BUI_URL" || { echo "ERROR: strand-cam BUI did not reconnect after reopening"; exit 1; }
sleep 3

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
