# Shared helpers for recording tutorial videos on a Linux X11 host: a virtual
# display, a terminal and a browser window tiled side by side, simulated
# typing/keypresses via xdotool, and an ffmpeg screen capture with a
# timestamped event log for caption burn-in (see burn_captions.py).
#
# Not standalone: sourced by each tutorial's record.sh, which must set
# SCRIPT_NAME before sourcing this file (used to namespace temp files) and
# call start_display/start_capture before the other functions, and cleanup at
# the end (a trap is installed automatically once start_display runs).

set -o errexit
set -o nounset
set -o pipefail

: "${SCRIPT_NAME:?session.sh: set SCRIPT_NAME before sourcing}"

SESSION_DISPLAY=":99"
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

start_display() {
    setsid Xvfb "$SESSION_DISPLAY" -screen 0 "${SESSION_WIDTH}x${SESSION_HEIGHT}x24" \
        >"$SESSION_WORK_DIR/xvfb.log" 2>&1 &
    SESSION_PIDS+=("$!")
    export DISPLAY="$SESSION_DISPLAY"

    for _ in $(seq 1 40); do
        if xdpyinfo >/dev/null 2>&1; then
            break
        fi
        sleep 0.25
    done
    xdpyinfo >/dev/null 2>&1 || {
        echo "ERROR: Xvfb on $SESSION_DISPLAY did not come up" >&2
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
    SESSION_CAPTURE_START_EPOCH=$(date +%s.%N)
    setsid ffmpeg -y \
        -f x11grab -video_size "${SESSION_WIDTH}x${SESSION_HEIGHT}" -framerate 30 -draw_mouse 1 \
        -i "$SESSION_DISPLAY" \
        -c:v libx264 -preset veryfast -pix_fmt yuv420p -crf 18 \
        "$out_mp4" \
        >"$SESSION_WORK_DIR/ffmpeg-capture.log" 2>&1 &
    SESSION_PIDS+=("$!")
    sleep 1
}

# stop_capture: ffmpeg needs a clean 'q' (or SIGINT) to finalize the mp4
# moov atom rather than being hard-killed.
stop_capture() {
    pkill -INT -f "ffmpeg .*x11grab.*$SESSION_DISPLAY" 2>/dev/null || true
    sleep 2
}

# open_xterm: launches on the left half of the screen. Prints the X window id
# on stdout; capture it, e.g. XTERM_WIN=$(open_xterm).
open_xterm() {
    setsid xterm -geometry 90x40+0+0 -fa 'Monospace' -fs 14 -bg black -fg white \
        >"$SESSION_WORK_DIR/xterm.log" 2>&1 &
    SESSION_PIDS+=("$!")
    sleep 1
    xdotool search --sync --onlyvisible --class xterm | tail -1
}

# open_browser URL: launches on the right half of the screen. Prints the X
# window id on stdout.
open_browser() {
    local url="$1"
    setsid firefox --new-window "$url" \
        >"$SESSION_WORK_DIR/firefox.log" 2>&1 &
    SESSION_PIDS+=("$!")
    sleep 2
    local win
    win=$(xdotool search --sync --onlyvisible --class firefox | tail -1)
    xdotool windowmove "$win" $((SESSION_WIDTH / 2)) 0
    xdotool windowsize "$win" $((SESSION_WIDTH / 2)) "$SESSION_HEIGHT"
    xdotool windowmove "$(xdotool search --onlyvisible --class xterm | tail -1)" 0 0
    xdotool windowsize "$(xdotool search --onlyvisible --class xterm | tail -1)" $((SESSION_WIDTH / 2)) "$SESSION_HEIGHT"
    echo "$win"
}

# type_in WINDOW_ID TEXT: simulates character-by-character typing, then Enter.
type_in() {
    local win="$1" text="$2"
    xdotool type --window "$win" --delay 60 -- "$text"
    xdotool key --window "$win" Return
}

# send_keys WINDOW_ID KEYS: e.g. send_keys "$XTERM_WIN" ctrl+c
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
    local text="$1" duration="$2" now elapsed
    now=$(date +%s.%N)
    elapsed=$(awk -v now="$now" -v start="$SESSION_CAPTURE_START_EPOCH" 'BEGIN { printf "%.2f", now - start }')
    printf '{"t": %s, "duration": %s, "text": %s}\n' \
        "$elapsed" "$duration" "$(printf '%s' "$text" | python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()))')" \
        >> "$SESSION_EVENTS_FILE"
}
