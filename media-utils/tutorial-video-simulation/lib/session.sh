# Shared helpers for recording tutorial videos on a Linux X11 host: a
# terminal and a browser window tiled side by side, simulated
# typing/keypresses via xdotool, and an ffmpeg screen capture with a
# timestamped event log for caption burn-in (see burn_captions.py).
#
# Only two packages are hard requirements: ffmpeg (capture + caption
# burn-in) and xdotool (window/keyboard automation). Everything else is
# used if already present and otherwise falls back to installing its own
# minimal version:
#   - display: reuses the desktop's existing X11 session (assumes X11 or an
#     XWayland-compatible one -- this doesn't work under pure Wayland). Only
#     starts a virtual one (Xvfb + openbox) if no display is usable, e.g. on
#     a headless box or in CI.
#   - terminal: prefers `x-terminal-emulator` (already set up on any Debian/
#     Ubuntu desktop) over requiring `xterm` specifically.
#   - browser: uses whichever of firefox/chrome/chromium is already
#     installed, rather than requiring one specifically.
#   - caption burn-in: burn_captions.py has no third-party dependencies, so
#     it just needs `python3` (already present on essentially every Linux
#     install) -- no `uv`/venv needed.
#
# Not standalone: sourced by each tutorial's record.sh, which must set
# SCRIPT_NAME before sourcing this file (used to namespace temp files) and
# call start_display/start_capture before the other functions, and cleanup at
# the end (a trap is installed automatically once start_display runs).

set -o errexit
set -o nounset
set -o pipefail

: "${SCRIPT_NAME:?session.sh: set SCRIPT_NAME before sourcing}"

SESSION_WIDTH=1280
SESSION_HEIGHT=800
SESSION_PIDS=()
SESSION_WORK_DIR=$(mktemp -d -t "${SCRIPT_NAME}-XXXXXX")
SESSION_EVENTS_FILE="$SESSION_WORK_DIR/events.jsonl"
SESSION_CAPTURE_START_EPOCH=""
: > "$SESSION_EVENTS_FILE"

session_cleanup() {
    local pid
    for ((i = ${#SESSION_PIDS[@]} - 1; i >= 0; i--)); do
        pid="${SESSION_PIDS[$i]}"
        kill -- "-$pid" 2>/dev/null || kill "$pid" 2>/dev/null || true
    done
    sleep 1
}
trap session_cleanup EXIT

# start_display: uses the current desktop session if one is usable (the
# common case -- this is meant to run on a real Linux desktop), only
# starting a disposable Xvfb + openbox if there's no display to reuse (e.g.
# a headless box or CI runner). Only ever kills what it itself started, so
# it never touches a real desktop's X server or window manager.
start_display() {
    if [ -n "${DISPLAY:-}" ] && xdpyinfo >/dev/null 2>&1; then
        echo "Using the existing display $DISPLAY" >&2
        local dims
        dims=$(xdpyinfo | awk '/dimensions:/ { print $2; exit }')
        SESSION_WIDTH="${dims%x*}"
        SESSION_HEIGHT="${dims#*x}"
        return
    fi

    echo "No usable display found; starting a virtual one (Xvfb + openbox)" >&2
    export DISPLAY=":99"
    setsid Xvfb "$DISPLAY" -screen 0 "${SESSION_WIDTH}x${SESSION_HEIGHT}x24" \
        >"$SESSION_WORK_DIR/xvfb.log" 2>&1 &
    SESSION_PIDS+=("$!")

    for _ in $(seq 1 40); do
        if xdpyinfo >/dev/null 2>&1; then
            break
        fi
        sleep 0.25
    done
    xdpyinfo >/dev/null 2>&1 || {
        echo "ERROR: Xvfb on $DISPLAY did not come up" >&2
        exit 1
    }

    setsid openbox >"$SESSION_WORK_DIR/openbox.log" 2>&1 &
    SESSION_PIDS+=("$!")
    sleep 1
}

# start_capture OUT_MP4
# Also records the capture's start time so later log_event calls can convert
# wall-clock time into "seconds since the recording started".
start_capture() {
    local out_mp4="$1"
    SESSION_CAPTURE_START_EPOCH=$(python3 -c 'import time; print(time.time())')
    setsid ffmpeg -y \
        -f x11grab -video_size "${SESSION_WIDTH}x${SESSION_HEIGHT}" -framerate 30 -draw_mouse 1 \
        -i "$DISPLAY" \
        -c:v libx264 -preset veryfast -pix_fmt yuv420p -crf 18 \
        "$out_mp4" \
        >"$SESSION_WORK_DIR/ffmpeg-capture.log" 2>&1 &
    SESSION_PIDS+=("$!")
    sleep 1
}

# stop_capture: ffmpeg needs a clean 'q' (or SIGINT) to finalize the mp4
# moov atom rather than being hard-killed.
stop_capture() {
    pkill -INT -f "ffmpeg .*x11grab.*$DISPLAY" 2>/dev/null || true
    sleep 2
}

# open_terminal: launches on the left half of the screen, using whatever
# terminal emulator is already available. Prints the X window id on stdout;
# capture it, e.g. TERM_WIN=$(open_terminal).
open_terminal() {
    local term_cmd
    term_cmd=$(command -v x-terminal-emulator || command -v xterm || true)
    [ -n "$term_cmd" ] || {
        echo "ERROR: no terminal emulator found (looked for x-terminal-emulator, xterm)" >&2
        exit 1
    }
    setsid "$term_cmd" >"$SESSION_WORK_DIR/terminal.log" 2>&1 &
    SESSION_PIDS+=("$!")
    sleep 1.5
    local win
    win=$(xdotool getactivewindow)
    xdotool windowmove "$win" 0 0
    xdotool windowsize "$win" $((SESSION_WIDTH / 2)) "$SESSION_HEIGHT"
    echo "$win"
}

# open_browser URL TERM_WIN: launches on the right half of the screen, using
# whichever of firefox/chrome/chromium is already installed, and moves
# TERM_WIN (from open_terminal) back onto the left half in case opening the
# browser disturbed it. Prints the browser's X window id on stdout.
open_browser() {
    local url="$1" term_win="$2"
    local browser_cmd candidate
    for candidate in firefox google-chrome google-chrome-stable chromium-browser chromium; do
        if command -v "$candidate" >/dev/null 2>&1; then
            browser_cmd="$candidate"
            break
        fi
    done
    : "${browser_cmd:?ERROR: no browser found (looked for firefox, chrome, chromium)}"

    setsid "$browser_cmd" --new-window "$url" >"$SESSION_WORK_DIR/browser.log" 2>&1 &
    SESSION_PIDS+=("$!")
    sleep 2

    local win
    win=$(xdotool getactivewindow)
    xdotool windowmove "$win" $((SESSION_WIDTH / 2)) 0
    xdotool windowsize "$win" $((SESSION_WIDTH / 2)) "$SESSION_HEIGHT"
    xdotool windowmove "$term_win" 0 0
    xdotool windowsize "$term_win" $((SESSION_WIDTH / 2)) "$SESSION_HEIGHT"
    echo "$win"
}

# type_in WINDOW_ID TEXT: simulates character-by-character typing, then Enter.
type_in() {
    local win="$1" text="$2"
    xdotool type --window "$win" --delay 60 -- "$text"
    xdotool key --window "$win" Return
}

# send_keys WINDOW_ID KEYS: e.g. send_keys "$TERM_WIN" ctrl+c
send_keys() {
    local win="$1" keys="$2"
    xdotool key --window "$win" "$keys"
}

# wait_for_url URL [TIMEOUT_TRIES]
wait_for_url() {
    local url="$1" tries="${2:-80}" i
    for ((i = 0; i < tries; i++)); do
        if curl --fail --silent --output /dev/null "$url"; then
            return 0
        fi
        sleep 0.5
    done
    return 1
}

# log_event TEXT DURATION_SECONDS: records an on-screen caption event, timed
# relative to start_capture, for burn_captions.py to overlay afterward. Only
# use this for actions that leave no visible trace on screen (e.g. Ctrl+C) --
# typed commands are already visible as terminal text and don't need one.
log_event() {
    local text="$1" duration="$2"
    python3 -c '
import json, sys, time
text, duration, start = sys.argv[1], float(sys.argv[2]), float(sys.argv[3])
print(json.dumps({"t": round(time.time() - start, 2), "duration": duration, "text": text}))
' "$text" "$duration" "$SESSION_CAPTURE_START_EPOCH" >> "$SESSION_EVENTS_FILE"
}
