#!/bin/bash
#
# Regenerates the "launching Strand Camera from the command line" tutorial
# video against the current repo, using the hardware-free `sim` camera
# backend (see ../README.md for what this replaces and why).
#
# Requires a Linux host with ffmpeg and xdotool (hard requirements), plus
# either a running desktop session or Xvfb+openbox as a fallback, and either
# an existing terminal/browser or xterm/firefox as a fallback -- see
# ../README.md for the full story and how to review the output.
#
# Usage:
#   ./record.sh [OUTPUT_DIR]
#
# OUTPUT_DIR defaults to a directory named 'out' next to this script. It is
# created if missing and is not, and should not be, committed to the repo.

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

# strand-cam itself runs as a child of the xterm shell (it must, to be
# visible on screen), so session_cleanup's window-process kill won't reach
# it. Extend the trap to also stop it directly.
trap 'pkill -f "strand-cam --camera-backend sim" 2>/dev/null || true; session_cleanup' EXIT

TARGET_DIR="${STRAND_BRAID_TARGET_DIR:-$REPO_ROOT/target/release}"
if [ ! -x "$TARGET_DIR/strand-cam" ]; then
    echo "=== Building strand-cam (cargo build --release) ==="
    ( cd "$REPO_ROOT" && cargo build --release -p strand-cam )
fi
if [ ! -x "$TARGET_DIR/strand-cam" ]; then
    echo "ERROR: $TARGET_DIR/strand-cam not found after build." >&2
    exit 1
fi

export DISABLE_VERSION_CHECK=1
export RUST_LOG=info
export STRAND_CAM_SIM_SPEC="$REPO_ROOT/braid/braid-sim/example-sim.toml"
# Put strand-cam on PATH so the terminal window shows the same short command
# ("strand-cam ...") the original video typed, not an absolute build path.
export PATH="$TARGET_DIR:$PATH"
BUI_URL="http://127.0.0.1:3440/"

echo "=== Starting virtual display and screen capture ==="
start_display
start_capture "$OUT_DIR/raw.mp4"

echo "=== Opening terminal and browser windows ==="
TERM_WIN=$(open_terminal)

echo "=== Command 1: launch with no --camera-name (auto-selects the first camera) ==="
type_in "$TERM_WIN" "strand-cam --camera-backend sim"
wait_for_url "$BUI_URL" || { echo "ERROR: strand-cam BUI did not come up"; exit 1; }
BROWSER_WIN=$(open_browser "$BUI_URL" "$TERM_WIN")

echo "=== Watching the live view ==="
sleep 10

echo "=== Ctrl+C ==="
log_event "Ctrl+C" 1.5
send_keys "$TERM_WIN" ctrl+c
sleep 2

echo "=== Command 2: relaunch with an explicit --camera-name ==="
type_in "$TERM_WIN" "strand-cam --camera-backend sim --camera-name simcam0"
wait_for_url "$BUI_URL" || { echo "ERROR: strand-cam BUI did not come back up"; exit 1; }

echo "=== Watching the live view again ==="
sleep 10

echo "=== Stopping capture ==="
stop_capture

echo "=== Burning in captions ==="
python3 "$SCRIPT_DIR/../lib/burn_captions.py" \
    --events "$SESSION_EVENTS_FILE" \
    --input "$OUT_DIR/raw.mp4" \
    --output "$OUT_DIR/strand-cam-intro.mp4"

echo "=== Done: $OUT_DIR/strand-cam-intro.mp4 ==="
echo "Compare it against the original before deciding it's ready; adjust the"
echo "sleep durations above and rerun if the pacing looks off."
