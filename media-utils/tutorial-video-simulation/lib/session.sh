# Shared helpers for recording tutorial videos on a Linux host: a terminal
# and a browser window tiled side by side, simulated typing/keypresses via
# xdotool, and an ffmpeg screen capture with a timestamped event log for
# caption burn-in (see burn_captions.py).
#
# Runs entirely on its own disposable Xvfb + openbox display -- it never
# touches whatever real desktop session you're actually working in.
# xdotool automation and process cleanup are scoped to that virtual display,
# so there's no way for this to grab or kill a window that belongs to your
# real session (see git history for why that matters: an earlier version
# reused the real desktop when one was usable, and a window-targeting bug
# in that mode ended up killing an unrelated terminal on the real desktop).
#
# Hard requirements: ffmpeg (capture + caption burn-in), xdotool
# (window/keyboard automation), Xvfb + openbox (the virtual display + window
# manager), xterm (the terminal -- see open_terminal for why this can't be
# "whatever's installed" the way the others can be). Everything else is used
# if already present and otherwise falls back to installing its own minimal
# version:
#   - browser: prefers google-chrome/chromium over firefox (see open_browser
#     for why), launched with an isolated profile/`-no-remote` so it can't
#     hand off to (or get confused with) an instance already running on your
#     real desktop.
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
    local pid i
    for ((i = ${#SESSION_PIDS[@]} - 1; i >= 0; i--)); do
        pid="${SESSION_PIDS[$i]}"
        kill -- "-$pid" 2>/dev/null || kill "$pid" 2>/dev/null || true
    done

    # SIGTERM alone often isn't enough: a multi-process browser (zygote/GPU/
    # renderer/utility helpers) can take a couple of seconds to tear itself
    # down, longer than a token sleep -- so poll for actual exit first, and
    # only escalate to SIGKILL for anything still alive after that.
    for _ in $(seq 1 10); do
        local any_alive=0
        for pid in "${SESSION_PIDS[@]}"; do
            kill -0 -- "-$pid" 2>/dev/null && any_alive=1
        done
        [ "$any_alive" -eq 0 ] && break
        sleep 0.5
    done
    for pid in "${SESSION_PIDS[@]}"; do
        kill -s KILL -- "-$pid" 2>/dev/null || kill -s KILL "$pid" 2>/dev/null || true
    done

    # Everything under here (logs, the browser profile, the strand-cam
    # wrapper if a tutorial made one) is disposable once all our processes
    # are confirmed dead -- remove it so temp dirs don't pile up in /tmp
    # across repeated runs.
    rm -rf "$SESSION_WORK_DIR"
}
trap session_cleanup EXIT

# start_display: always starts a disposable Xvfb + openbox display of our
# own -- deliberately never reuses whatever real desktop session is already
# running, so this can't ever grab, move, type into, or kill a window that
# belongs to your actual session. Only ever kills what it itself started.
start_display() {
    local n=99
    while [ -e "/tmp/.X${n}-lock" ]; do
        n=$((n + 1))
    done
    export DISPLAY=":${n}"

    echo "Starting an isolated virtual display on $DISPLAY (Xvfb + openbox)" >&2
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

    # Plain black is a dead giveaway this isn't a real desktop; a solid
    # dark color (roughly matching this lab's actual desktop background)
    # reads much less obviously synthetic without needing a real desktop
    # shell running underneath. Must run after openbox starts -- openbox
    # sets its own root background on startup, which would otherwise
    # overwrite this.
    xsetroot -solid "#3d0b24" 2>/dev/null || true
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

# open_terminal: launches on the left half of the screen. Prints the X
# window id on stdout; capture it, e.g. TERM_WIN=$(open_terminal).
#
# Deliberately always xterm, never x-terminal-emulator: on a stock Debian/
# Ubuntu desktop that alternative resolves to gnome-terminal, which isn't a
# self-contained X client -- it just asks an already-running
# gnome-terminal-server daemon (started once, at login, bound to your real
# desktop) to open a window over D-Bus. That daemon ignores our exported
# DISPLAY, so the window opens on your real screen instead of the isolated
# virtual display, silently defeating start_display's whole point. xterm has
# no such daemon; it always opens directly on $DISPLAY.
open_terminal() {
    command -v xterm >/dev/null 2>&1 || {
        echo "ERROR: xterm not found (required; do not substitute x-terminal-emulator -- see comment above)" >&2
        exit 1
    }
    # Colors approximate Ubuntu's default terminal profile (dark purple/
    # aubergine background, plain white foreground) rather than xterm's
    # own stark black-on-white/black defaults, since that default is one of
    # the more obvious tells that this isn't a real desktop terminal.
    setsid xterm -bg '#300A24' -fg '#FFFFFF' -fa 'Monospace' -fs 11 \
        >"$SESSION_WORK_DIR/terminal.log" 2>&1 &
    SESSION_PIDS+=("$!")
    sleep 1.5
    local win
    win=$(xdotool getactivewindow)
    xdotool windowmove "$win" 0 0
    xdotool windowsize "$win" $((SESSION_WIDTH / 2)) "$SESSION_HEIGHT"
    echo "$win"
}

# open_browser URL TERM_WIN: launches on the right half of the screen, using
# whichever of chrome/chromium/firefox is already installed, and moves
# TERM_WIN (from open_terminal) back onto the left half in case opening the
# browser disturbed it. Prints the browser's X window id on stdout.
#
# Launched with an isolated profile and remote-control disabled: without
# that, Firefox/Chrome notice an instance already running on your real
# desktop (same user, default profile) and just forward "open a new window"
# to it over there instead of actually opening one on our virtual display --
# silently defeating the isolation start_display worked to set up.
#
# Chrome/Chromium variants are tried before Firefox on purpose: on a stock
# Ubuntu desktop, `firefox` is a snap package, and snap's confinement
# sandbox blocks it from reading/writing our temp profile dir under /tmp
# (outside its allow-list), so `-no-remote -profile <tmp>` fails with "Your
# Firefox profile cannot be loaded" instead of actually isolating it. Only
# falls back to firefox if no Chrome/Chromium variant is installed, in which
# case that failure mode is a known limitation, not a bug to chase here.
open_browser() {
    local url="$1" term_win="$2"
    local browser_cmd candidate
    for candidate in google-chrome google-chrome-stable chromium-browser chromium firefox; do
        if command -v "$candidate" >/dev/null 2>&1; then
            browser_cmd="$candidate"
            break
        fi
    done
    : "${browser_cmd:?ERROR: no browser found (looked for google-chrome, chromium, firefox)}"

    local profile_dir="$SESSION_WORK_DIR/browser-profile"
    mkdir -p "$profile_dir"
    local isolation_args=()
    case "$browser_cmd" in
    firefox)
        isolation_args=(-no-remote -profile "$profile_dir")
        export MOZ_NO_REMOTE=1
        ;;
    *)
        # chrome/chromium variants. Two flags, both required:
        #   --ozone-platform=x11: on a Wayland desktop, Chrome auto-detects
        #     $WAYLAND_DISPLAY and renders natively there, completely
        #     bypassing our isolated $DISPLAY (Wayland connections don't go
        #     through $DISPLAY at all) -- so without this it silently opens
        #     on the real desktop instead of the virtual one. This forces it
        #     onto X11/XWayland, which does honor $DISPLAY.
        #   --disable-gpu: with no real GPU under Xvfb, Chrome's default
        #     GPU-accelerated compositing path fails silently, leaving the
        #     window blank/black instead of falling back to software
        #     rendering on its own.
        isolation_args=(--user-data-dir="$profile_dir" --no-first-run --ozone-platform=x11 --disable-gpu)
        ;;
    esac

    setsid "$browser_cmd" "${isolation_args[@]}" --new-window "$url" \
        >"$SESSION_WORK_DIR/browser.log" 2>&1 &
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
#
# move_mouse_into WINDOW_ID: moves the mouse pointer to roughly the center
# of the given window. Purely cosmetic, but a cursor frozen in one spot for
# the entire recording is an obvious tell that nobody's really at the
# keyboard -- type_in/send_keys call this automatically before sending
# input, so every action reads as someone actually reaching for that window.
move_mouse_into() {
    local win="$1" geom w h
    geom=$(xdotool getwindowgeometry --shell "$win")
    w=$(echo "$geom" | sed -n 's/^WIDTH=//p')
    h=$(echo "$geom" | sed -n 's/^HEIGHT=//p')
    # No --sync: unlike windowactivate (which really must be confirmed
    # before typing), nothing downstream depends on confirming the pointer
    # physically arrived, and --sync here measurably slowed the whole
    # recording down for a purely cosmetic move.
    xdotool mousemove --window "$win" $((w / 2)) $((h / 2))
}

# Explicitly activates the target window first, then types with no --window
# (global XTEST input, delivered to whichever window currently has focus).
# `xdotool type/key --window WIN` sends via XSendEvent directly to that
# window id regardless of focus, but that isn't reliably honored once
# another window (e.g. the browser) has taken focus in the meantime --
# windowactivate first guarantees our target is the one that's focused when
# the global XTEST input actually lands.
type_in() {
    local win="$1" text="$2"
    move_mouse_into "$win"
    xdotool windowactivate --sync "$win"
    xdotool type --delay 120 -- "$text"
    # Leave the typed command visible and unexecuted for a beat before
    # hitting Enter, so a viewer has time to actually read it.
    sleep 3
    xdotool key Return
}

# send_keys WINDOW_ID KEYS: e.g. send_keys "$TERM_WIN" ctrl+c
send_keys() {
    local win="$1" keys="$2"
    move_mouse_into "$win"
    xdotool windowactivate --sync "$win"
    xdotool key "$keys"
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
