#!/bin/bash
#
# Regenerates the "launching Braid from the command line" tutorial video
# against the current repo, replicating the original Video_2.mp4 (see
# ../README.md for what this replaces and why) with today's Braid GUI, all
# 5 cameras cycled through (the original skips one), and a close/reopen
# demonstration added at the end (the original stops before showing this).
#
# Like strand-cam-intro, this scenario auto-detects real camera hardware and
# falls back to hardware-free simulated cameras if none is found (see
# BRAID_CAMERAS below) -- unlike strand-cam-intro's `sim` backend, which just
# swaps one `--camera-backend` flag, braid-run's real-hardware config (see
# BRAID_CONFIG_TOML below) is a whole TOML describing 5 real Basler cameras
# with PtpSync triggering and a real extrinsic calibration file, none of
# which a sim fallback can reuse -- so the fallback path instead generates a
# throwaway config from scratch via `braid-sim generate` (the same generator
# the project's own `smoke-tests/braid-sim.sh` uses): 5 `start_backend =
# "sim"` cameras (`camera/ci2-sim`, the same synthetic insect-blob backend
# strand-cam-intro's `sim` fallback uses) and `FakeSync` triggering (braid-run
# synthesizes a clock model for this immediately -- no PTP hardware/network
# involved, see braid/braid-run/src/mainbrain.rs's `needs_clock_model`/`Using
# fake synchronization method` path). See ../README.md's "Braid and camera
# hardware" section for the full story.
#
# Requires everything strand-cam-intro/record.sh requires (ffmpeg, xdotool,
# Xvfb, openbox, ttyd, xprop, a browser -- see ../README.md Prerequisites),
# plus a `braid-run` binary with a `strand-cam` binary alongside it in the
# same directory: `braid-run` resolves its own per-camera `strand-cam`
# child next to its own executable path (`std::env::current_exe().parent()`
# in braid-run/src/main.rs's `launch_strand_cam`), NOT via `$PATH` -- so an
# installed package needs to already ship both together (it does, via the
# .deb), and a from-source build needs both built into the same directory.
# The sim fallback additionally needs a `braid-sim` binary -- a dev-only
# generator tool, not shipped in the .deb, so it's built from source
# on-demand (see the `braid-sim` resolution below) regardless of whether
# braid-run/strand-cam themselves came from an installed package.
#
# Usage:
#   ./record.sh [OUTPUT_DIR]
#   BRAID_CONFIG_TOML=/path/to/other-config.TOML ./record.sh
#   BRAID_CAMERAS=sim ./record.sh    # hardware-free, even if real cameras are attached
#
# OUTPUT_DIR defaults to a directory named 'out' next to this script. It is
# created if missing and is not, and should not be, committed to the repo.
#
# BRAID_CONFIG_TOML, if set, always wins outright regardless of BRAID_CAMERAS
# -- same "explicit override always wins" precedent as strand-cam-intro's
# CAMERA_BACKEND -- and is used verbatim (error if missing), skipping the
# auto-detection below entirely.
#
# BRAID_CAMERAS selects real vs. simulated cameras. Left unset, record.sh
# auto-detects: real Basler (pylon) hardware (via --list-cameras, same check
# strand-cam-intro uses) *and* the default config file
# (/home/strawlab/BRAID_TOMLS/config.TOML, override via BRAID_CONFIG_TOML)
# both present -> real; either missing -> the hardware-free sim fallback.
# Set BRAID_CAMERAS=sim explicitly to force the sim fallback regardless of
# what's attached (e.g. to regenerate the hardware-free version on a machine
# that does have real cameras, or in CI).

# Tuned pixel/click-count constants -- expect to retune all of these after
# watching a first real run (see POINTING-NOTES.md). Units/convention match
# strand-cam-intro/record.sh's own header comment: standard top-left-origin
# screen pixels at lib/session.sh's SESSION_WIDTH/HEIGHT (1920x1200 as of
# this writing), +X right +Y down.

# How many scroll-wheel-up clicks it takes to jump from the bottom (where
# "All expected cameras synchronized" appears) back up near the top of the
# log (where the QR code / Predicted URL block was printed at startup).
# Real hardware logs periodic per-camera chatter throughout the run, so
# this can need to be a few hundred clicks -- harmless to overshoot
# (xterm.js clamps at the scrollback's top), only harmful if too small to
# reach the QR block at all. A short per-click delay keeps even a large
# count fast in the finished video; _scroll_clicks issues these via a
# single `xdotool click --repeat` call, not one process per click, so a
# large count here isn't as expensive as it looks.
QR_SCROLL_CLICKS=400
QR_SCROLL_DELAY=0.03

# Fallback pixel coordinates for point_at_browser_text's terminal-side
# lookups, used only if the CDP text lookup itself fails.
TERM_SYNC_FALLBACK_X=510
TERM_SYNC_FALLBACK_Y=750
TERM_QR_FALLBACK_X=510
TERM_QR_FALLBACK_Y=300

# Fallback pixel coordinates for the browser-side camera-link lookups.
BROWSER_CAMLINK_FALLBACK_X=150
BROWSER_CAMLINK_FALLBACK_Y=550

# How many scroll-wheel-down clicks bring every camera link into view
# before pointing at each one -- first guess, needs a real run to confirm
# it's enough (and not so much it scrolls a link back out at the bottom).
BROWSER_CAMLIST_SCROLL_CLICKS=3
BROWSER_CAMLIST_SCROLL_DELAY=0.25

# How many scroll-wheel-down clicks reach the bottom of the dashboard page
# (past the Recording/Cameras/Status sections) to reveal the "Quit Braid"
# button -- first guess, needs a real run to confirm it's enough.
BROWSER_QUIT_SCROLL_CLICKS=30
BROWSER_QUIT_SCROLL_DELAY=0.1

# Fallback pixel coordinates for the "Quit Braid" button lookup, used only
# if the CDP text lookup itself fails.
BROWSER_QUIT_FALLBACK_X=400
BROWSER_QUIT_FALLBACK_Y=900

# Chrome's own back-button chrome (top-left toolbar row, below the tab
# strip -- NOT the same row as BROWSER_CLOSE_Y, whose window-controls sit on
# the tab strip itself). Measured directly from a captured frame (raw.mp4 at
# t=52s, during camera 1's dwell, before the back-button click moves the
# cursor there): the real arrow centers at absolute (1023, 140); the
# browser window's own origin is (996, 72) (SESSION_MARGIN*2 +
# SESSION_PANE_WIDTH, SESSION_MARGIN -- zero frame-extent decoration, so
# also its content origin), giving this relative position.
BROWSER_BACK_X=27
BROWSER_BACK_Y=68

# How long to dwell on each camera's own live view before hitting back --
# brisk, matching the original Video_2.mp4's own ~2-3s-per-camera pacing.
PER_CAMERA_DWELL_SECONDS=4

set -o errexit
set -o nounset
set -o pipefail

SCRIPT_NAME="braid-intro"
SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/../../.." && pwd)
OUT_DIR=$(cd "$(dirname "${1:-$SCRIPT_DIR/out}")" && pwd)/$(basename "${1:-$SCRIPT_DIR/out}")
mkdir -p "$OUT_DIR"

# shellcheck source=../lib/session.sh
source "$SCRIPT_DIR/../lib/session.sh"

# Chrome's own window-close button -- see strand-cam-intro/record.sh for
# the full reasoning (empirically verified at two window widths: fixed
# ~23px in from the right/top edges regardless of window width).
BROWSER_CLOSE_X=$((SESSION_PANE_WIDTH - 23))
BROWSER_CLOSE_Y=23

# Prefer, in order: an explicit override, an already-installed braid-run
# (e.g. via the .deb package, which ships strand-cam alongside it), then
# finally a from-source build -- mirrors strand-cam-intro's own TARGET_DIR
# resolution, but must also verify strand-cam exists in the same directory
# (see the header comment above for why). Resolved before the camera-mode
# decision below, since auto-detection needs a real strand-cam binary to
# probe for hardware with (same ordering reason as strand-cam-intro's own
# CAMERA_BACKEND auto-detection).
if [ -n "${STRAND_BRAID_TARGET_DIR:-}" ]; then
    TARGET_DIR="$STRAND_BRAID_TARGET_DIR"
elif command -v braid-run >/dev/null 2>&1; then
    TARGET_DIR=$(dirname "$(command -v braid-run)")
    echo "=== Using installed braid-run: $TARGET_DIR/braid-run ==="
else
    TARGET_DIR="$REPO_ROOT/target/release"
fi

if [ ! -x "$TARGET_DIR/braid-run" ] || [ ! -x "$TARGET_DIR/strand-cam" ]; then
    echo "=== Building braid-run + strand-cam (cargo build --release) ==="
    ( cd "$REPO_ROOT" && cargo build --release -p braid-run -p strand-cam )
fi
if [ ! -x "$TARGET_DIR/braid-run" ] || [ ! -x "$TARGET_DIR/strand-cam" ]; then
    echo "ERROR: $TARGET_DIR/braid-run and/or $TARGET_DIR/strand-cam not found after build." >&2
    exit 1
fi

BRAID_VERSION=$("$TARGET_DIR/braid-run" --version)
echo "=== $BRAID_VERSION ==="

DEFAULT_BRAID_CONFIG="/home/strawlab/BRAID_TOMLS/config.TOML"

if [ -n "${BRAID_CONFIG_TOML:-}" ]; then
    BRAID_CONFIG="$BRAID_CONFIG_TOML"
    [ -f "$BRAID_CONFIG" ] || { echo "ERROR: config file not found: $BRAID_CONFIG" >&2; exit 1; }
    CAMERAS_MODE=real
    echo "=== BRAID_CONFIG_TOML=$BRAID_CONFIG (explicit) ==="
elif [ "${BRAID_CAMERAS:-}" = "sim" ]; then
    CAMERAS_MODE=sim
    echo "=== BRAID_CAMERAS=sim (explicit) ==="
elif [ -f "$DEFAULT_BRAID_CONFIG" ] && "$TARGET_DIR/strand-cam" --camera-backend pylon --list-cameras 2>/dev/null \
    | grep -qE '^  [^ ]+  \(model:'; then
    BRAID_CONFIG="$DEFAULT_BRAID_CONFIG"
    CAMERAS_MODE=real
    echo "=== Real camera hardware + config detected -- using real cameras ($BRAID_CONFIG) ==="
else
    CAMERAS_MODE=sim
    echo "=== No real camera hardware/config detected -- falling back to simulated cameras ==="
fi

if [ "$CAMERAS_MODE" = "sim" ]; then
    # braid-sim isn't shipped in the .deb (a dev-only generator tool, only
    # ever used by this tutorial harness) -- build it from source on demand,
    # same idiom as braid-run/strand-cam's own from-source fallback above,
    # but independent of TARGET_DIR since an installed braid-run/strand-cam
    # would never have it alongside them.
    if command -v braid-sim >/dev/null 2>&1; then
        BRAID_SIM_BIN=$(command -v braid-sim)
    elif [ -x "$REPO_ROOT/target/release/braid-sim" ]; then
        BRAID_SIM_BIN="$REPO_ROOT/target/release/braid-sim"
    else
        echo "=== Building braid-sim (cargo build --release) ==="
        ( cd "$REPO_ROOT" && cargo build --release -p braid-sim )
        BRAID_SIM_BIN="$REPO_ROOT/target/release/braid-sim"
    fi
    [ -x "$BRAID_SIM_BIN" ] || { echo "ERROR: $BRAID_SIM_BIN not found after build." >&2; exit 1; }

    # Scenario file drives both config generation (camera count/geometry)
    # and, exported below, the actual sim cameras' own frame content at
    # runtime -- must be the same file for both, or camera count could
    # mismatch between the generated config and what ci2-sim actually
    # starts. Same env var strand-cam-intro's own `sim` backend reads, and
    # the same default scenario file it uses.
    export STRAND_CAM_SIM_SPEC="${STRAND_CAM_SIM_SPEC:-$REPO_ROOT/braid/braid-sim/example-sim.toml}"
    [ -f "$STRAND_CAM_SIM_SPEC" ] || { echo "ERROR: sim scenario not found: $STRAND_CAM_SIM_SPEC" >&2; exit 1; }

    echo "=== Generating a simulated Braid config from $STRAND_CAM_SIM_SPEC ==="
    # 0.0.0.0, not 127.0.0.1: braid-run's mainbrain only prints "QR code for
    # {url}" (the needle launch_braid's own scroll/point steps search for)
    # for a *non-loopback* URL (braid/braid-run/src/mainbrain.rs's
    # `is_loopback` check) -- binding all interfaces makes `build_urls`
    # expand to both a loopback entry (still used below to extract the
    # actual Predicted URL to navigate to) and a real LAN one, matching
    # real hardware's own config and what the rest of this script expects
    # to find on screen. Confirmed via a real run: a loopback-only address
    # never emits a QR line at all, so scroll_until_visible searches
    # forever and this script aborts with no ERROR message (its own
    # unguarded failure path).
    "$BRAID_SIM_BIN" generate "$STRAND_CAM_SIM_SPEC" \
        --out-dir "$OUT_DIR/sim-config-gen" \
        --http-api-server-addr "0.0.0.0:1234"
    BRAID_CONFIG="$OUT_DIR/sim-config-gen/braid-config.toml"
fi

# Parsed straight out of the config file at runtime, rather than hardcoded
# -- stays correct if the config's cameras ever change, matching this
# project's existing "discover, don't hardcode" approach (c.f.
# strand-cam-intro's own --list-cameras detection). Works the same whether
# $BRAID_CONFIG is the real hardware config or the sim-generated one above:
# both are `[[cameras]]` tables with an un-indented `name = "..."` line.
mapfile -t CAMERA_NAMES < <(grep -oP '(?<=^name = ")[^"]+' "$BRAID_CONFIG")
[ "${#CAMERA_NAMES[@]}" -gt 0 ] || { echo "ERROR: no camera names found in $BRAID_CONFIG" >&2; exit 1; }
echo "=== Found ${#CAMERA_NAMES[@]} camera(s) in config: ${CAMERA_NAMES[*]} ==="

export PATH="$TARGET_DIR:$PATH"
# Required: env_tracing_logger's EnvFilter::from_default_env() means every
# info!() line this script depends on (Predicted URL, All expected cameras
# synchronized) is otherwise invisible in both the terminal and the
# ~/.braid-*.log file braid-run writes on its own.
export RUST_LOG=info
export DISABLE_VERSION_CHECK=1

echo "=== Starting virtual display ==="
start_display

echo "=== Opening terminal window ==="
# Placed and resized to its final SESSION_MARGIN/SESSION_PANE_WIDTH geometry
# before capture starts below, so the recording never shows the window
# appearing at Chrome's own default size/position and jumping into place.
open_terminal

echo "=== Starting screen capture ==="
start_capture "$OUT_DIR/raw.mp4"
# Half a second holding on the placed, empty terminal before anything is
# typed -- reads as a real pause before starting to type, not a cut.
sleep 0.5

# braid-run spawns each camera's own strand-cam as a real child process of
# itself (std::process::Command::spawn in launch_strand_cam), itself a
# child of the ttyd-bridged shell -- pkill -f matching EITHER binary name,
# scoped to TERM_SESSION_PID's session id (set by open_terminal via
# setsid), covers both without ever risking an unrelated braid-run/
# strand-cam elsewhere on this machine.
trap "pkill -s $TERM_SESSION_PID -f 'braid-run|strand-cam' 2>/dev/null || true; session_cleanup" EXIT

# launch_braid COMMAND_TEXT: types COMMAND_TEXT into the terminal, waits
# for all cameras to report synchronized (real PTP hardware, or the
# sim fallback's instant FakeSync -- no fixed timeout beyond
# wait_for_browser_text's own generous default), points at
# that message, scrolls up to reveal the QR/token block printed at startup
# and pauses on it, then extracts THIS launch's own loopback Predicted URL
# (with its token) from braid-run's own ~/.braid-*.log -- never parsed off
# the visible on-screen command -- and simulates a click on whichever
# QR/URL text ended up on screen (a real click would trigger ttyd's own
# xterm.js WebLinksAddon into a new tab in the terminal's own browser
# process, same reasoning as strand-cam-intro's reopen-link step). Prints
# the extracted loopback URL on stdout for the caller to open_browser;
# returns non-zero (via `return`, not `exit`, so the caller's own `|| ...`
# handles the failure the same way the rest of this script does) if
# anything along the way times out.
launch_braid() {
    local command_text="$1" demo="${2:-1}" launch_epoch braid_log log_line braid_url
    launch_epoch=$(date +%s)
    type_in "$TERM_WIN" "$command_text"

    echo "Waiting for all cameras to report synchronized (no fixed timeout)..." >&2
    wait_for_browser_text "$TERM_CDP_PORT" "All expected cameras synchronized" || {
        echo "ERROR: cameras never reported synchronized" >&2
        return 1
    }
    # demo=0 (launch 2): the browser is already open from launch 1 and never
    # closed, so none of the "look, here's how you'd open it" business below
    # (indicating the sync line, scrolling to the QR code, pointing at the
    # link) needs repeating -- only the URL still needs extracting, so the
    # caller can navigate the existing window to it.
    if [ "$demo" = "1" ]; then
        point_at_browser_text "$TERM_WIN" "$TERM_CDP_PORT" "All expected cameras synchronized" \
            "$TERM_SYNC_FALLBACK_X" "$TERM_SYNC_FALLBACK_Y" 0 1 60
    fi
    sleep 1

    if [ "$demo" = "1" ]; then
        echo "Scrolling up to reveal the QR code..." >&2
        # Needle is "r http://", not the bare "http://" -- the terminal also
        # prints "Predicted URL: http://..." (once per launch) and, far more
        # often, "Will connect to braid at "http://127.0.0.1:PORT/..."" (once
        # per camera, repeatedly) -- both contain "http://" too, and the
        # latter especially is recent/bottom-anchored enough that a bare
        # "http://" needle was satisfied almost immediately without any real
        # scrolling, then had the click below land on that loopback
        # strand-cam URL instead of the actual QR line. Only "QR code for
        # {url}" contains "r http://" (the tail of "for" + the URL) -- the
        # other two lines read ": http://" and "\"http://" at that position.
        scroll_until_visible "$TERM_WIN" "$TERM_CDP_PORT" up "r http://" "$QR_SCROLL_CLICKS" \
            15 "$QR_SCROLL_DELAY" "Scroll wheel"
        sleep 0.5
    fi

    braid_log=$(newest_file_matching "$HOME/.braid-*.log" "$launch_epoch")
    [ -n "$braid_log" ] || {
        echo "ERROR: no ~/.braid-*.log created for this launch" >&2
        return 1
    }
    log_line=$(wait_for_log_match "$braid_log" 'Predicted URL: http://127\.0\.0\.1:') || {
        echo "ERROR: no loopback Predicted URL found in $braid_log" >&2
        return 1
    }
    braid_url=$(echo "$log_line" | sed -n 's/.*Predicted URL: //p')
    [ -n "$braid_url" ] || {
        echo "ERROR: could not parse Predicted URL from log line: $log_line" >&2
        return 1
    }

    if [ "$demo" = "1" ]; then
        # Sweep width 0 -- this is "about to click," not "indicating text"
        # (see point_at's own convention, already used this way for
        # strand-cam-intro's close/reopen clicks). Needle is "r http://",
        # not the bare "http://" -- see scroll_until_visible's own comment
        # above for why the bare form matches the wrong (loopback,
        # constantly-repeated) line instead of this one.
        point_at_browser_text "$TERM_WIN" "$TERM_CDP_PORT" "r http://" \
            "$TERM_QR_FALLBACK_X" "$TERM_QR_FALLBACK_Y" 0 -6 0
        log_event "Ctrl + LEFT CLICK" 1.5
        sleep 1.5
    fi

    echo "$braid_url"
}

echo "=== Launch 1: braid-run ==="
BRAID_URL_1=$(launch_braid "braid-run '$BRAID_CONFIG'") || { echo "ERROR: launch 1 failed"; exit 1; }
open_browser "$BRAID_URL_1" "$TERM_WIN"
wait_for_url "$BRAID_URL_1" || { echo "ERROR: Braid GUI did not come up"; exit 1; }
move_mouse_gradual_into "$BROWSER_WIN"

echo "=== Cycling through each camera ==="
for cam in "${CAMERA_NAMES[@]}"; do
    echo "--- $cam ---"
    # browser_back below does a full page reload (navigate_browser sets
    # window.location.href), which resets scroll to the top -- so this
    # scroll has to happen every iteration, not just once before the loop,
    # or later cameras would fall back out of view exactly like before.
    scroll_by "$BROWSER_WIN" down "$BROWSER_CAMLIST_SCROLL_CLICKS" "$BROWSER_CAMLIST_SCROLL_DELAY"
    # Sweep width 0 -- indicating this link right before "clicking" it.
    # OFFSET_Y=-12: point_at_browser_text's own bounding-box calculation
    # always adds a fixed +6 baseline buffer below the measured text (to
    # clear the glyphs' own descenders); -6 cancelled that exactly (landing
    # right at the text's own bottom edge) but still read as too low, so
    # this goes a further -6 up, into the name itself rather than just its
    # bottom edge.
    point_at_browser_text "$BROWSER_WIN" "$BROWSER_CDP_PORT" "$cam" \
        "$BROWSER_CAMLINK_FALLBACK_X" "$BROWSER_CAMLINK_FALLBACK_Y" 0 -12 0
    log_event "LEFT CLICK" 1.5
    sleep 0.5
    # Real click, not simulated -- but performed via navigate_browser
    # rather than an actual xdotool click: a literal click on this
    # project's Yew/WASM-rendered camera link did not reliably trigger
    # real navigation in testing, so this reads the link's own real
    # (browser-resolved, already-encoded) target via get_browser_href and
    # navigates there directly, achieving the same real result.
    cam_url=$(get_browser_href "$BROWSER_CDP_PORT" "$cam") || {
        echo "WARNING: could not resolve a link for camera '$cam', skipping" >&2
        continue
    }
    navigate_browser "$BROWSER_CDP_PORT" "$cam_url"
    sleep "$PER_CAMERA_DWELL_SECONDS"
    browser_back "$BROWSER_WIN" "$BROWSER_CDP_PORT" "$BRAID_URL_1" "$BROWSER_BACK_X" "$BROWSER_BACK_Y"
    # Confirm the list page has actually re-rendered before the next
    # camera's lookup runs against it; short timeout and tolerated failure
    # -- worst case the next point_at_browser_text falls back to its own
    # tuned pixel guess instead of blocking indefinitely.
    wait_for_browser_text "$BROWSER_CDP_PORT" "cameras:" 10 0.5 || true
done

echo "=== Moving back to the terminal ==="
move_mouse_gradual_into "$TERM_WIN"

# Critical, not just cosmetic: the terminal has sat scrolled up (from
# revealing launch 1's QR code) for the whole camera-cycling phase above.
# ttyd/xterm.js only auto-follows new output while already scrolled to the
# bottom -- if we retyped the relaunch command from here, launch 2's own
# "All expected cameras synchronized" line would never render into the
# live DOM for wait_for_browser_text to find (it isn't virtualized into
# existence until scrolled into view), and it'd time out. Scrolling back
# down first, before Ctrl+C, keeps us at the bottom through the retype.
echo "=== Scrolling the terminal back to the bottom ==="
scroll_by "$TERM_WIN" down "$QR_SCROLL_CLICKS" "$QR_SCROLL_DELAY" "Scroll wheel"
sleep 0.5

log_event "LEFT CLICK" 1.5
sleep 1.5

echo "=== Ctrl+C ==="
log_event "Ctrl+C" 1.5
send_keys "$TERM_WIN" ctrl+c
sleep 2

echo "=== Launch 2: braid-run (relaunch) ==="
BRAID_URL_2=$(launch_braid "braid-run '$BRAID_CONFIG'" 0) || { echo "ERROR: launch 2 failed"; exit 1; }
# The browser window from launch 1 was never closed -- navigate it to the
# new session's URL directly (same CDP mechanism as camera navigation)
# rather than opening a second new window via open_browser.
navigate_browser "$BROWSER_CDP_PORT" "$BRAID_URL_2"
wait_for_url "$BRAID_URL_2" || { echo "ERROR: Braid GUI did not come back up"; exit 1; }
sleep 3

# New relative to the original Video_2.mp4 (which stops here): demonstrate
# that closing the GUI browser window does not end the braid-run process,
# reusing strand-cam-intro's exact close/reopen pattern.
echo "=== Closing the Braid browser window ==="
point_at "$BROWSER_WIN" "$BROWSER_CLOSE_X" "$BROWSER_CLOSE_Y" 0
log_event "LEFT CLICK" 1.5
sleep 1.5
xdotool windowclose "$BROWSER_WIN" 2>/dev/null || true
sleep 1

echo "=== Reopening it via the terminal's printed URL ==="
# demo=0 skipped launch 2's own QR-reveal scroll (see launch_braid), so the
# URL line was never scrolled into view and isn't rendered in xterm.js's DOM
# at all -- scroll up again here, on-screen like launch 1's, so the lookup
# below has something real to find.
echo "Scrolling up to reveal the URL again..." >&2
# scroll_until_visible, not scroll_by -- stops as soon as the QR/URL line is
# rendered, landing on THIS launch's own nearest one instead of overshooting
# all the way to the absolute top (launch 1's older one). Needle is
# "r http://", not the bare "http://" -- see launch_braid's own comment for
# why the bare form matches the wrong, constantly-repeated loopback line.
scroll_until_visible "$TERM_WIN" "$TERM_CDP_PORT" up "r http://" "$QR_SCROLL_CLICKS" \
    15 "$QR_SCROLL_DELAY" "Scroll wheel"
sleep 0.5

# Needle is "r http://", not the full $BRAID_URL_2, "token=", or the bare
# "http://" -- same reasoning as the QR-code link fix in launch_braid: land
# on the link itself (not preceding/surrounding text) and avoid matching
# the wrong, more-recently-printed loopback line.
point_at_browser_text "$TERM_WIN" "$TERM_CDP_PORT" "r http://" \
    "$TERM_QR_FALLBACK_X" "$TERM_QR_FALLBACK_Y" 0 -6 0
log_event "Ctrl + LEFT CLICK" 1.5
sleep 1.5
open_browser "$BRAID_URL_2" "$TERM_WIN"
wait_for_url "$BRAID_URL_2" || { echo "ERROR: Braid GUI did not reconnect after reopening"; exit 1; }
sleep 3

echo "=== Scrolling to the bottom to quit Braid ==="
move_mouse_gradual_into "$BROWSER_WIN"
scroll_by "$BROWSER_WIN" down "$BROWSER_QUIT_SCROLL_CLICKS" "$BROWSER_QUIT_SCROLL_DELAY" "Scroll wheel"
sleep 0.5

# Sweep width 0 -- about to click, not indicating text.
point_at_browser_text "$BROWSER_WIN" "$BROWSER_CDP_PORT" "Quit Braid" \
    "$BROWSER_QUIT_FALLBACK_X" "$BROWSER_QUIT_FALLBACK_Y" 0 -4 0
log_event "LEFT CLICK" 1.5
sleep 0.5

# Real click, performed programmatically via click_browser_element (see
# lib/session.sh) rather than a literal xdotool click -- same "simulate
# visually, act via a separately-verified mechanism" convention as the
# camera links' navigate_browser. Also auto-accepts the native
# window.confirm() dialog the app's DoQuit handler pops (see
# cdp_locate.py --click), which this hand-rolled CDP client has no way to
# intercept if it were left to actually appear.
click_browser_element "$BROWSER_CDP_PORT" "Quit Braid" || echo "WARNING: could not click Quit Braid button" >&2
wait_for_browser_text "$BROWSER_CDP_PORT" "Braid has quit" 10 0.5 || echo "WARNING: quit confirmation text not seen" >&2
sleep 1.5

echo "=== Stopping capture ==="
stop_capture

echo "=== Burning in captions ==="
python3 "$SCRIPT_DIR/../lib/burn_captions.py" \
    --events "$SESSION_EVENTS_FILE" \
    --input "$OUT_DIR/raw.mp4" \
    --output "$OUT_DIR/braid-intro.mp4" \
    --comment "Generated by media-utils/tutorial-video-simulation/braid-intro/record.sh using $BRAID_VERSION"

echo "=== Done: $OUT_DIR/braid-intro.mp4 ==="
echo "Compare it against the original before deciding it's ready; adjust the"
echo "tuned constants at the top of this script and rerun if the pointing/"
echo "scrolling/pacing looks off -- see POINTING-NOTES.md."
