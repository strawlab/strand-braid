#!/bin/bash
#
# Regenerates the "launching Braid from the command line" tutorial video
# against the current repo, replicating the original Video_2.mp4 (see
# ../README.md for what this replaces and why) with today's Braid GUI, all
# 5 cameras cycled through (the original skips one), and a close/reopen
# demonstration added at the end (the original stops before showing this).
#
# Unlike strand-cam-intro, this scenario has NO hardware-free fallback: the
# config it replays (see BRAID_CONFIG_TOML below) configures 5 real Basler
# cameras with PtpSync triggering and a real extrinsic calibration file,
# none of which `braid-run` can substitute a `sim` backend for unless the
# config itself opts a camera into `start_backend = "sim"` (it doesn't).
# Requires that hardware, PTP sync, and calibration file to already be
# working on whatever machine runs this -- see ../README.md's "Braid and
# camera hardware" section.
#
# Requires everything strand-cam-intro/record.sh requires (ffmpeg, xdotool,
# Xvfb, openbox, ttyd, xprop, a browser -- see ../README.md Prerequisites),
# plus a `braid-run` binary with a `strand-cam` binary alongside it in the
# same directory: `braid-run` resolves its own per-camera `strand-cam`
# child next to its own executable path (`std::env::current_exe().parent()`
# in braid-run/src/main.rs's `launch_strand_cam`), NOT via `$PATH` -- so an
# installed package needs to already ship both together (it does, via the
# .deb), and a from-source build needs both built into the same directory.
#
# Usage:
#   ./record.sh [OUTPUT_DIR]
#   BRAID_CONFIG_TOML=/path/to/other-config.TOML ./record.sh
#
# OUTPUT_DIR defaults to a directory named 'out' next to this script. It is
# created if missing and is not, and should not be, committed to the repo.

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

# Chrome's own back-button chrome (top-left toolbar row) -- by analogy with
# BROWSER_CLOSE_X/Y below (a fixed offset from the window's own edge,
# empirically verified for the close button in strand-cam-intro; NOT yet
# verified for this one -- see POINTING-NOTES.md). Y matches
# BROWSER_CLOSE_Y, the same toolbar row.
BROWSER_BACK_X=40
BROWSER_BACK_Y=23

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

BRAID_CONFIG="${BRAID_CONFIG_TOML:-/home/strawlab/BRAID_TOMLS/config.TOML}"
[ -f "$BRAID_CONFIG" ] || { echo "ERROR: config file not found: $BRAID_CONFIG" >&2; exit 1; }

# Parsed straight out of the config file at runtime, rather than hardcoded
# -- stays correct if the config's cameras ever change, matching this
# project's existing "discover, don't hardcode" approach (c.f.
# strand-cam-intro's own --list-cameras detection).
mapfile -t CAMERA_NAMES < <(grep -oP '(?<=^name = ")[^"]+' "$BRAID_CONFIG")
[ "${#CAMERA_NAMES[@]}" -gt 0 ] || { echo "ERROR: no camera names found in $BRAID_CONFIG" >&2; exit 1; }
echo "=== Found ${#CAMERA_NAMES[@]} camera(s) in config: ${CAMERA_NAMES[*]} ==="

# Prefer, in order: an explicit override, an already-installed braid-run
# (e.g. via the .deb package, which ships strand-cam alongside it), then
# finally a from-source build -- mirrors strand-cam-intro's own TARGET_DIR
# resolution, but must also verify strand-cam exists in the same directory
# (see the header comment above for why).
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

export PATH="$TARGET_DIR:$PATH"
# Required: env_tracing_logger's EnvFilter::from_default_env() means every
# info!() line this script depends on (Predicted URL, All expected cameras
# synchronized) is otherwise invisible in both the terminal and the
# ~/.braid-*.log file braid-run writes on its own.
export RUST_LOG=info
export DISABLE_VERSION_CHECK=1

echo "=== Starting virtual display and screen capture ==="
start_display
start_capture "$OUT_DIR/raw.mp4"

echo "=== Opening terminal window ==="
open_terminal

# braid-run spawns each camera's own strand-cam as a real child process of
# itself (std::process::Command::spawn in launch_strand_cam), itself a
# child of the ttyd-bridged shell -- pkill -f matching EITHER binary name,
# scoped to TERM_SESSION_PID's session id (set by open_terminal via
# setsid), covers both without ever risking an unrelated braid-run/
# strand-cam elsewhere on this machine.
trap "pkill -s $TERM_SESSION_PID -f 'braid-run|strand-cam' 2>/dev/null || true; session_cleanup" EXIT

# launch_braid COMMAND_TEXT: types COMMAND_TEXT into the terminal, waits
# for real PTP hardware to report all cameras synchronized (no fixed
# timeout beyond wait_for_browser_text's own generous default), points at
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
    local command_text="$1" launch_epoch braid_log log_line braid_url
    launch_epoch=$(date +%s)
    type_in "$TERM_WIN" "$command_text"

    echo "Waiting for all cameras to report synchronized (real PTP hardware, no fixed timeout)..." >&2
    wait_for_browser_text "$TERM_CDP_PORT" "All expected cameras synchronized" || {
        echo "ERROR: cameras never reported synchronized" >&2
        return 1
    }
    point_at_browser_text "$TERM_WIN" "$TERM_CDP_PORT" "All expected cameras synchronized" \
        "$TERM_SYNC_FALLBACK_X" "$TERM_SYNC_FALLBACK_Y"
    sleep 1

    echo "Scrolling up to reveal the QR code..." >&2
    scroll_by "$TERM_WIN" up "$QR_SCROLL_CLICKS" "$QR_SCROLL_DELAY" "Scroll wheel"
    sleep 1

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

    # Sweep width 0 -- this is "about to click," not "indicating text" (see
    # point_at's own convention, already used this way for strand-cam-
    # intro's close/reopen clicks).
    point_at_browser_text "$TERM_WIN" "$TERM_CDP_PORT" "QR code for" \
        "$TERM_QR_FALLBACK_X" "$TERM_QR_FALLBACK_Y" 0 6 0
    log_event "LEFT CLICK" 1.5
    sleep 1.5

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
    # Sweep width 0 -- indicating this link right before "clicking" it.
    point_at_browser_text "$BROWSER_WIN" "$BROWSER_CDP_PORT" "$cam" \
        "$BROWSER_CAMLINK_FALLBACK_X" "$BROWSER_CAMLINK_FALLBACK_Y" 0 6 0
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
sleep 1

log_event "LEFT CLICK" 1.5
sleep 1.5

echo "=== Ctrl+C ==="
log_event "Ctrl+C" 1.5
send_keys "$TERM_WIN" ctrl+c
sleep 2

echo "=== Launch 2: braid-run (relaunch) ==="
BRAID_URL_2=$(launch_braid "braid-run '$BRAID_CONFIG'") || { echo "ERROR: launch 2 failed"; exit 1; }
open_browser "$BRAID_URL_2" "$TERM_WIN"
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
# Needle is "token=", not the full $BRAID_URL_2 -- confirmed live (first
# real run) that the full URL is long enough to wrap across two terminal
# rows, which cdp_locate.py's Range-based matching can't match at all
# (same wrapping failure strand-cam-intro's own history already
# documents for a spanning needle), falling back to a tuned pixel guess.
# "token=" is short, guaranteed to sit within one row, and appears in
# several lines sharing this launch's own token -- cdp_locate.py's
# last-match-wins tie-break resolves to whichever is bottom-most in the
# current (already-scrolled-up) viewport, any of which is a correct,
# representative thing to point at.
point_at_browser_text "$TERM_WIN" "$TERM_CDP_PORT" "token=" \
    "$TERM_QR_FALLBACK_X" "$TERM_QR_FALLBACK_Y" 0 6 0
log_event "LEFT CLICK" 1.5
sleep 1.5
open_browser "$BRAID_URL_2" "$TERM_WIN"
wait_for_url "$BRAID_URL_2" || { echo "ERROR: Braid GUI did not reconnect after reopening"; exit 1; }
sleep 3

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
