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
# manager), ttyd (bridges the terminal's real PTY into a browser window --
# see open_terminal for why). Everything else is used if already present and
# otherwise falls back to installing its own minimal version:
#   - browser: prefers google-chrome/chromium over firefox (see
#     _open_isolated_browser_window for why), launched with an isolated
#     profile/`-no-remote` so it can't hand off to (or get confused with) an
#     instance already running on your real desktop. Used for both the BUI
#     window and (via ttyd) the terminal window.
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

SESSION_WIDTH=1920
SESSION_HEIGHT=1200
# Matches the original tutorial videos this harness regenerates
# (1920x1200) -- raised from 1280x800 on request, since strand-cam's BUI
# looked cramped (wrapping/tight spacing) at the smaller size. Every
# other pixel-based constant in this file and in each tutorial's
# record.sh was scaled by the same 1.5x factor (1920/1280 ==
# 1200/800 == 1.5) to keep proportions consistent -- rescale all of them
# together if this ever changes again, not just these two.
#
# Gap between/around the two windows and the screen edge. Real desktops
# never tile windows perfectly edge-to-edge with zero gap -- doing that here
# was one of the more obvious tells that this wasn't a real desktop.
SESSION_MARGIN=72
SESSION_PANE_WIDTH=$(((SESSION_WIDTH - 3 * SESSION_MARGIN) / 2))
SESSION_PANE_HEIGHT=$((SESSION_HEIGHT - 2 * SESSION_MARGIN))
# burn_captions.py draws caption text bottom-left of the *whole frame*
# (x=60, y=h-th-60 -- scaled 1.5x along with everything else, see above),
# which lands within the terminal's horizontal span -- so the terminal
# specifically (not the browser, which sits well to the right of x=60)
# needs to stop short of that zone vertically, or captions would get drawn
# on top of its bottom few lines.
SESSION_TERM_HEIGHT=$((SESSION_PANE_HEIGHT - 210))
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

# _open_isolated_browser_window URL: launches an isolated chrome/chromium/
# firefox window pointed at URL and prints "WINDOW_ID CDP_PORT" on stdout
# (CDP_PORT empty for firefox). Shared by open_terminal (pointed at ttyd)
# and open_browser (pointed at the BUI) -- both need the same isolation
# story, just against different URLs.
#
# Launched with an isolated profile and remote-control disabled: without
# that, Firefox/Chrome notice an instance already running on your real
# desktop (same user, default profile) and just forward "open a new window"
# to it over there instead of actually opening one on our virtual display --
# silently defeating the isolation start_display worked to set up. Each call
# gets its own profile dir (mktemp'd under SESSION_WORK_DIR), since two of
# these now run at once (terminal + BUI) and must not collide or share state.
#
# Chrome/Chromium variants are tried before Firefox on purpose: on a stock
# Ubuntu desktop, `firefox` is a snap package, and snap's confinement
# sandbox blocks it from reading/writing our temp profile dir under /tmp
# (outside its allow-list), so `-no-remote -profile <tmp>` fails with "Your
# Firefox profile cannot be loaded" instead of actually isolating it. Only
# falls back to firefox if no Chrome/Chromium variant is installed, in which
# case that failure mode is a known limitation, not a bug to chase here.
_open_isolated_browser_window() {
    local url="$1"
    local browser_cmd candidate
    for candidate in google-chrome google-chrome-stable chromium-browser chromium firefox; do
        if command -v "$candidate" >/dev/null 2>&1; then
            browser_cmd="$candidate"
            break
        fi
    done
    : "${browser_cmd:?ERROR: no browser found (looked for google-chrome, chromium, firefox)}"

    local profile_dir cdp_port=""
    profile_dir=$(mktemp -d "$SESSION_WORK_DIR/browser-profile-XXXXXX")
    local isolation_args=()
    case "$browser_cmd" in
    firefox)
        isolation_args=(-no-remote -profile "$profile_dir")
        export MOZ_NO_REMOTE=1
        ;;
    *)
        # chrome/chromium variants. Three flags:
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
        #   --remote-debugging-port: lets point_at_browser_text() (below)
        #     query the real DOM for exact text positions via the Chrome
        #     DevTools Protocol, instead of guessing tuned pixel offsets
        #     that break whenever the page layout changes. A free port is
        #     picked per call to avoid colliding with anything else on this
        #     machine, including the other isolated browser window from this
        #     same run (this is a real, non-virtual-display-scoped TCP port).
        cdp_port=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')
        isolation_args=(--user-data-dir="$profile_dir" --no-first-run --ozone-platform=x11 --disable-gpu --remote-debugging-port="$cdp_port")
        ;;
    esac

    setsid "$browser_cmd" "${isolation_args[@]}" --new-window "$url" \
        >"$SESSION_WORK_DIR/$(basename "$profile_dir").log" 2>&1 &
    SESSION_PIDS+=("$!")
    sleep 2

    local win
    win=$(xdotool getactivewindow)
    echo "$win $cdp_port"
}

# open_terminal: launches a browser-based terminal as a floating window on
# the left, sized to SESSION_PANE_WIDTH/HEIGHT with SESSION_MARGIN of space
# around it (real desktops never tile windows edge-to-edge with zero gap).
# Sets the globals TERM_WIN (the window id), TERM_SESSION_PID (ttyd's own
# pid), and TERM_CDP_PORT (empty if firefox ended up as the fallback
# browser -- see _open_isolated_browser_window).
#
# Deliberately NOT called via command substitution (TERM_WIN=$(open_terminal)
# -- that would run this whole function in a subshell, silently discarding
# every global it sets, including TERM_WIN itself, once the subshell exits).
# Call it as a plain statement instead.
#
# Why a browser instead of a real terminal emulator (xterm): a real
# terminal has no DOM to query, so pointing at its own log output (e.g.
# strand-cam's "run{cam=...}" line) needed a tuned pixel guess with no way
# to verify it -- see strand-cam-intro/POINTING-NOTES.md. Bridging the
# terminal's real PTY into a browser tab via ttyd, using xterm.js's DOM
# renderer (each line/character becomes a real DOM element, unlike its
# default canvas/WebGL renderer), lets point_at_browser_text() query
# terminal text exactly the same way it already queries the BUI.
open_terminal() {
    command -v ttyd >/dev/null 2>&1 || {
        echo "ERROR: ttyd not found (required; bridges the terminal's PTY into a browser window -- see comment above)" >&2
        exit 1
    }
    local ttyd_port
    ttyd_port=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')

    # -i 127.0.0.1: never reachable beyond localhost. -W: writable -- ttyd
    # defaults to a read-only display, which would silently swallow every
    # xdotool keystroke typed into this window. rendererType=dom: forces
    # xterm.js's DOM renderer (see open_terminal's comment above).
    setsid ttyd -p "$ttyd_port" -i 127.0.0.1 -W -t rendererType=dom bash \
        >"$SESSION_WORK_DIR/ttyd.log" 2>&1 &
    TERM_SESSION_PID="$!"
    SESSION_PIDS+=("$TERM_SESSION_PID")
    sleep 1

    local win_and_port
    win_and_port=$(_open_isolated_browser_window "http://127.0.0.1:$ttyd_port/")
    TERM_WIN=$(echo "$win_and_port" | awk '{print $1}')
    TERM_CDP_PORT=$(echo "$win_and_port" | awk '{print $2}')
    sleep 1

    xdotool windowmove "$TERM_WIN" "$SESSION_MARGIN" "$SESSION_MARGIN"
    xdotool windowsize "$TERM_WIN" "$SESSION_PANE_WIDTH" "$SESSION_TERM_HEIGHT"
}

# open_browser URL TERM_WIN: launches as a floating window on the right
# (same SESSION_MARGIN gap/sizing as open_terminal), and moves TERM_WIN
# (from open_terminal) back onto the left in case opening this window
# disturbed it. Sets the globals BROWSER_WIN (the window id) and
# BROWSER_CDP_PORT (empty if firefox -- see _open_isolated_browser_window).
#
# Deliberately NOT called via command substitution -- see open_terminal's
# comment above; the same subshell problem applies here.
open_browser() {
    local url="$1" term_win="$2"
    local win_and_port right_x
    win_and_port=$(_open_isolated_browser_window "$url")
    BROWSER_WIN=$(echo "$win_and_port" | awk '{print $1}')
    BROWSER_CDP_PORT=$(echo "$win_and_port" | awk '{print $2}')

    right_x=$((SESSION_MARGIN * 2 + SESSION_PANE_WIDTH))
    xdotool windowmove "$BROWSER_WIN" "$right_x" "$SESSION_MARGIN"
    xdotool windowsize "$BROWSER_WIN" "$SESSION_PANE_WIDTH" "$SESSION_PANE_HEIGHT"
    xdotool windowmove "$term_win" "$SESSION_MARGIN" "$SESSION_MARGIN"
    xdotool windowsize "$term_win" "$SESSION_PANE_WIDTH" "$SESSION_TERM_HEIGHT"
}

# move_mouse_to WINDOW_ID X Y: moves the mouse pointer to a specific pixel
# offset within the given window (top-left origin) -- e.g. to point at a
# particular piece of text, like a camera name, rather than just "somewhere
# in this window." No --sync: unlike windowactivate (which really must be
# confirmed before typing), nothing downstream depends on confirming the
# pointer physically arrived, and --sync here measurably slowed the whole
# recording down for a purely cosmetic move.
move_mouse_to() {
    local win="$1" x="$2" y="$3"
    xdotool mousemove --window "$win" "$x" "$y"
}

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
    move_mouse_to "$win" $((w / 2)) $((h / 2))
}

# move_mouse_gradual_into WINDOW_ID: like move_mouse_into (moves to roughly
# the center of WINDOW_ID), but travels there via move_mouse_gradual
# (visible, interpolated motion across the screen) instead of an instant
# jump -- use when the mouse is already visible somewhere else (e.g. just
# finished pointing at something) and the transition itself should read as
# someone moving the mouse there, not teleporting.
move_mouse_gradual_into() {
    local win="$1" geom win_x win_y w h
    geom=$(xdotool getwindowgeometry --shell "$win")
    win_x=$(echo "$geom" | sed -n 's/^X=//p')
    win_y=$(echo "$geom" | sed -n 's/^Y=//p')
    w=$(echo "$geom" | sed -n 's/^WIDTH=//p')
    h=$(echo "$geom" | sed -n 's/^HEIGHT=//p')
    move_mouse_gradual $((win_x + w / 2)) $((win_y + h / 2))
}

# move_mouse_gradual TARGET_X TARGET_Y [STEPS] [STEP_DELAY]: moves the mouse
# from its current position to an absolute screen position in small
# interpolated steps, so the motion reads as someone actually dragging the
# mouse across the screen (including across window boundaries) rather than
# teleporting there in one jump.
move_mouse_gradual() {
    local target_x="$1" target_y="$2" steps="${3:-20}" step_delay="${4:-0.05}"
    local cur cur_x cur_y i x y
    cur=$(xdotool getmouselocation --shell)
    cur_x=$(echo "$cur" | sed -n 's/^X=//p')
    cur_y=$(echo "$cur" | sed -n 's/^Y=//p')
    for ((i = 1; i <= steps; i++)); do
        x=$((cur_x + (target_x - cur_x) * i / steps))
        y=$((cur_y + (target_y - cur_y) * i / steps))
        xdotool mousemove "$x" "$y"
        sleep "$step_delay"
    done
}

# point_at WINDOW_ID REL_X REL_Y [SWEEP_WIDTH]: gradually moves the mouse to
# a point within WINDOW_ID (top-left origin) and then slowly sweeps it left
# and right under that point a couple of times, e.g. to indicate a specific
# piece of text (a camera name) without covering it up -- REL_Y should
# already be offset a little *below* the text's baseline by the caller.
# SWEEP_WIDTH=0 skips the sweep entirely (just the move, then done) -- use
# that for a simulated click on a specific spot (e.g. a close button, a
# link), where the wiggle reads as "pointing at something" rather than
# "about to click it."
point_at() {
    local win="$1" rel_x="$2" rel_y="$3" sweep_width="${4:-50}"
    local geom win_x win_y abs_x abs_y half
    geom=$(xdotool getwindowgeometry --shell "$win")
    win_x=$(echo "$geom" | sed -n 's/^X=//p')
    win_y=$(echo "$geom" | sed -n 's/^Y=//p')
    abs_x=$((win_x + rel_x))
    abs_y=$((win_y + rel_y))
    move_mouse_gradual "$abs_x" "$abs_y"
    if [ "$sweep_width" -gt 0 ]; then
        half=$((sweep_width / 2))
        move_mouse_gradual $((abs_x + half)) "$abs_y" 12 0.08
        move_mouse_gradual $((abs_x - half)) "$abs_y" 12 0.08
        move_mouse_gradual $((abs_x + half)) "$abs_y" 12 0.08
        move_mouse_gradual "$abs_x" "$abs_y" 8 0.08
    fi
}

# point_at_browser_text WINDOW_ID CDP_PORT NEEDLE [FALLBACK_X] [FALLBACK_Y]
# [OFFSET_X] [OFFSET_Y] [SWEEP_WIDTH]: finds the on-screen text containing
# NEEDLE via the Chrome DevTools Protocol (cdp_locate.py, against CDP_PORT
# -- e.g. BROWSER_CDP_PORT for the BUI window or TERM_CDP_PORT for the
# ttyd terminal window, both set by _open_isolated_browser_window) and
# points at it precisely instead of a tuned pixel guess -- falls back to
# point_at with FALLBACK_X/FALLBACK_Y if CDP isn't available (e.g.
# CDP_PORT is empty because firefox ended up as the fallback browser) or
# the lookup fails for any reason, so a rerun degrades gracefully rather
# than erroring out.
#
# OFFSET_X/OFFSET_Y (default 0, 6) are added to the located text's own
# center-x/bottom-y, per call site -- e.g. a caller can pass a larger
# OFFSET_Y to point further below a tall heading, or a nonzero OFFSET_X to
# favor one side of a long matched string instead of its exact center.
# Units: pixels, top-left-origin screen coordinates (+X right, +Y down) --
# CSS pixels from Chrome's getBoundingClientRect(), which equal physical
# screen pixels on this Xvfb display (no device-pixel-ratio/scale-factor
# set anywhere), at the fixed resolution start_display sets up
# (SESSION_WIDTH/SESSION_HEIGHT, 1920x1200 as of this writing). Same units
# as point_at's FALLBACK_X/Y. Defaults match "centered horizontally, just
# below the baseline" (see below for why +6 specifically).
#
# SWEEP_WIDTH is forwarded to point_at as-is (default 50, its own default;
# pass 0 to disable the sweep entirely -- see point_at).
point_at_browser_text() {
    local win="$1" cdp_port="$2" needle="$3" fallback_x="${4:-}" fallback_y="${5:-}"
    local offset_x="${6:-0}" offset_y="${7:-6}" sweep_width="${8:-50}"
    local lib_dir result rel_x rel_y
    lib_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

    if [ -n "$cdp_port" ]; then
        result=$(python3 "$lib_dir/cdp_locate.py" --port "$cdp_port" --contains "$needle" 2>"$SESSION_WORK_DIR/cdp_locate.log") || result=""
    fi

    if [ -n "${result:-}" ]; then
        rel_x=$(echo "$result" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(int(d["x"]+d["width"]/2))')
        # +6 base (not the old +15): now that cdp_locate.py measures the
        # exact text substring (a Range) rather than a whole enclosing
        # element, the returned height is already just the text's own line
        # height, so a smaller "below the baseline" buffer is enough to
        # clear the text without the sweep covering it. OFFSET_Y adds to
        # this base per call site, OFFSET_X to the horizontal center.
        rel_y=$(echo "$result" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(int(d["chromeY"]+d["y"]+d["height"]+6))')
        rel_x=$((rel_x + offset_x))
        rel_y=$((rel_y + offset_y))
        point_at "$win" "$rel_x" "$rel_y" "$sweep_width"
    elif [ -n "$fallback_x" ]; then
        echo "WARNING: CDP text lookup for '$needle' failed, using fallback coordinates (see $SESSION_WORK_DIR/cdp_locate.log)" >&2
        point_at "$win" "$fallback_x" "$fallback_y" "$sweep_width"
    else
        echo "WARNING: CDP text lookup for '$needle' failed and no fallback coordinates given; skipping" >&2
    fi
}

# scroll_page WINDOW_ID: slowly scrolls the page down then back up over
# about 10s (20 clicks each direction, 0.25s apart), so a "watching the live
# view" pause shows something happening instead of a completely static
# window -- and incidentally shows off the whole page, not just whatever
# was in view when it opened. Scroll-wheel clicks (buttons 4/5), not
# --window-targeted, matching the windowactivate-then-global-input pattern
# type_in/send_keys use, since XSendEvent-style --window targeting isn't
# reliably honored by every app.
scroll_page() {
    local win="$1" i
    xdotool windowactivate --sync "$win"
    move_mouse_into "$win"
    for ((i = 0; i < 20; i++)); do
        xdotool click 5
        sleep 0.25
    done
    for ((i = 0; i < 20; i++)); do
        xdotool click 4
        sleep 0.25
    done
    # Brief hold once back at the top, rather than immediately cutting to
    # the next action.
    sleep 3
}

# type_only WINDOW_ID TEXT: like type_in, but stops after typing -- no
# pause, no Enter. Use this (plus a separate `press_return`) instead of
# type_in when something else needs to happen in between, e.g.
# point_at()-ing a piece of on-screen text while the command sits typed but
# not yet run.
#
# Explicitly activates the target window first, then types with no --window
# (global XTEST input, delivered to whichever window currently has focus).
# `xdotool type/key --window WIN` sends via XSendEvent directly to that
# window id regardless of focus, but that isn't reliably honored once
# another window (e.g. the browser) has taken focus in the meantime --
# windowactivate first guarantees our target is the one that's focused when
# the global XTEST input actually lands.
#
# Also logs a caption event of the typed text itself, the same yellow
# burned-in style Ctrl+C already gets -- the original tutorial videos this
# harness reproduces caption keystrokes this way too, not just untyped
# actions. (The terminal's own on-screen echo of the typed characters isn't
# a substitute for this: it's easy to miss against a busy log, and it's
# xterm.js/ttyd rendering, not a caption -- this is a second, deliberately
# redundant indicator.) Fixed 3s, matching type_in's own dwell before
# Return; a caller that does something longer before pressing Return (e.g.
# point_at_browser_text) will just see the caption disappear a bit early.
type_only() {
    local win="$1" text="$2"
    move_mouse_into "$win"
    xdotool windowactivate --sync "$win"
    xdotool type --delay 120 -- "$text"
    log_event "$text" 3
}

# type_in WINDOW_ID TEXT: simulates character-by-character typing, then a
# pause, then Enter (via press_return, below).
type_in() {
    local win="$1" text="$2"
    type_only "$win" "$text"
    # Leave the typed command visible and unexecuted for a beat before
    # hitting Enter, so a viewer has time to actually read it.
    sleep 3
    press_return "$win"
}

# press_return WINDOW_ID: sends Return, captioned "Enter" the same way
# Ctrl+C is captioned in record.sh -- both are discrete, momentary
# keypresses with no letter-by-letter on-screen trace of their own.
press_return() {
    local win="$1"
    log_event "Enter" 1.5
    xdotool key Return
}

# send_keys WINDOW_ID KEYS: e.g. send_keys "$TERM_WIN" ctrl+c. Does NOT log
# a caption event itself (unlike type_only/press_return) -- callers vary too
# much in what a given key combo means on screen (record.sh's Ctrl+C use
# logs its own caption right before calling this).
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
# relative to start_capture, for burn_captions.py to overlay afterward.
# Called by type_only/press_return for keystrokes, and directly by callers
# for other discrete actions with no on-screen trace of their own (e.g.
# record.sh's Ctrl+C).
log_event() {
    local text="$1" duration="$2"
    python3 -c '
import json, sys, time
text, duration, start = sys.argv[1], float(sys.argv[2]), float(sys.argv[3])
print(json.dumps({"t": round(time.time() - start, 2), "duration": duration, "text": text}))
' "$text" "$duration" "$SESSION_CAPTURE_START_EPOCH" >> "$SESSION_EVENTS_FILE"
}
