# tutorial-video-simulation

Scripts to regenerate the strand-braid tutorial videos (the ones showing
someone launching Strand Camera / Braid from the command line and clicking
through the GUI) against the *current* CLI and GUI, whenever those drift from
what an older recorded video shows. Not a Cargo crate — nothing here is added
to the workspace `[workspace] members` list.

## Why

Tutorial videos go stale: CLI flags get renamed, GUIs get redesigned. Rather
than manually re-recording (and re-typing, re-clicking, re-narrating) a video
by hand every time, each subdirectory here drives the real application
end-to-end — real CLI invocations, a real browser against the real BUI — on
a throwaway virtual display, and captures the result. Re-running a script
after a GUI/CLI change regenerates the video against whatever the repo does
today.

Camera-dependent tutorials use the hardware-free `sim` camera backend
(`ci2-sim`, driven by `braid/braid-sim/example-sim.toml`) instead of real
camera hardware, so these scripts run on any Linux box with no camera
attached.

## Prerequisites (Linux only)

Only two packages are hard requirements:

- `ffmpeg` — screen capture and caption burn-in
- `xdotool` — window placement, simulated typing, and keypresses

Everything else uses what's already on a normal Linux desktop instead of
requiring anything new, falling back to installing its own minimal version
only if nothing usable is found:

- **display**: reuses the desktop's existing X11 session (this is the
  expected case — these scripts are meant to run on a real Linux desktop,
  the same kind the original videos were recorded on; it assumes X11 or an
  XWayland-compatible session, not pure Wayland). Falls back to a disposable
  `Xvfb` + `openbox` only if there's no usable display (e.g. a headless box
  or CI).
- **terminal**: prefers `x-terminal-emulator` (already set up via
  update-alternatives on any Debian/Ubuntu desktop) over requiring `xterm`.
- **browser**: uses whichever of `firefox`/`google-chrome`/`chromium` is
  already installed, rather than requiring a specific one.
- **caption burn-in**: `burn_captions.py` has no third-party Python
  dependencies (unlike `docs/user-docs/scripts/record-mp4-video-ffmpeg.py`,
  which needs `requests` and so uses `uv`), so plain `python3` is enough —
  no `uv`/venv needed.

A `cargo build --release` of whatever binary the tutorial launches, unless
you point `STRAND_BRAID_TARGET_DIR` at an already-installed binary (each
script builds it automatically if missing otherwise). For `strand-cam` built
from source, that build needs everything in
[`docs/developer-docs/building-for-development.md`](../../docs/developer-docs/building-for-development.md)
(a recent enough Rust toolchain, `trunk`, and the `wasm32-unknown-unknown`
target, since the default `bundle_files` feature compiles the browser
frontend). If the very first build of the day is offline (e.g. `trunk`'s
nested `cargo metadata` call), run `cargo fetch` once first.

These scripts were developed and syntax-checked on macOS (where `x11grab`/
a real X11 session aren't available) but are meant to run on Linux. The first
full run of each script should be treated as a test: watch the resulting
video, adjust `sleep` durations/captions in that tutorial's `record.sh` as
needed, and only then treat the output as final.

## Layout

```
lib/
  session.sh          # shared bash helpers: display (existing desktop or a
                       # virtual fallback), tiled terminal+browser windows,
                       # simulated typing/keys, screen capture, and a
                       # timestamped caption log
  burn_captions.py     # overlays lib/session.sh's caption log onto the
                       # captured video (no dependencies, run with python3)
strand-cam-intro/
  record.sh            # regenerates Video_1.mp4 (launching Strand Camera
                        # from the command line): `strand-cam --camera-backend
                        # sim`, watch the live view, Ctrl+C, relaunch with
                        # `--camera-name simcam0` explicit.
```

## Running instructions

### 1. Get the code

```sh
git clone git@github.com:Mharrap/strand-braid.git
cd strand-braid
git checkout wip/tutorial-video-simulation
```

### 2. Install the two hard requirements

```sh
sudo apt-get update
sudo apt-get install -y ffmpeg xdotool
```

On a normal Linux desktop that's everything — see Prerequisites above for
when the terminal/browser/display fallbacks (`xterm`, `firefox`, `Xvfb` +
`openbox`) kick in and need installing too.

### 3. Point at a `strand-cam` build

**If `strand-cam` is already installed** (e.g. via the `.deb` package), skip
building it entirely — just tell `record.sh` where to find it:

```sh
export STRAND_CAM_SIM_SPEC="$(pwd)/braid/braid-sim/example-sim.toml"
strand-cam --camera-backend sim --list-cameras   # sanity check: should list simcam0..simcam4
```

If that errors, the installed build doesn't have the `sim` backend compiled
in and you'll need to build from source instead (below).

**Building from source**: `record.sh` does this automatically if it can't
find a binary — see Prerequisites above for the `trunk`/Rust-toolchain/
`cargo fetch` requirements that build needs.

### 4. Run it

```sh
cd media-utils/tutorial-video-simulation/strand-cam-intro

# Using an already-installed strand-cam:
STRAND_BRAID_TARGET_DIR=$(dirname "$(which strand-cam)") ./record.sh

# Or, to build from source (target/release):
./record.sh
```

`STRAND_BRAID_TARGET_DIR` tells `record.sh` where the `strand-cam` binary
lives, scoped to just this one invocation. Internally it does:
```sh
TARGET_DIR="${STRAND_BRAID_TARGET_DIR:-$REPO_ROOT/target/release}"
if [ ! -x "$TARGET_DIR/strand-cam" ]; then
    # cargo build --release -p strand-cam
fi
export PATH="$TARGET_DIR:$PATH"   # so the terminal shows plain "strand-cam ...", not a full path
```
so pointing it at an existing install (e.g. via `dirname "$(which
strand-cam)"`) skips the build step; leaving it unset builds from source
into `target/release` the first time and reuses that binary after.

### 5. Output

`out/strand-cam-intro.mp4` (plus `out/raw.mp4`, the pre-caption capture, and
`out/events.jsonl`, the caption log) next to `record.sh`. `out/` is a local
working directory — it is not, and should not be, committed. Compare the
result against the original tutorial video, tweak `sleep` durations/captions
in `record.sh` and rerun if needed, and hand the final `.mp4` off manually
once you're satisfied (the generated video files themselves are not part of
this repo).

## A note on `--camera-backend sim`

The tutorials here use `--camera-backend sim` purely so the recording needs
no camera hardware. If you have a real Basler camera, the equivalent of the
old `strand-cam-pylon` command is simply `strand-cam` on its own — `pylon` is
still the default backend (`strand-cam/src/cli_app.rs`), it's just no longer
baked into the binary's name.

## Adding another tutorial

Create a new subdirectory with its own `record.sh` that sources
`../lib/session.sh` and sets `SCRIPT_NAME` before doing so (used to namespace
that script's temp/work directory). See `strand-cam-intro/record.sh` for the
pattern: `start_display` → `start_capture` → open windows → `type_in`/
`send_keys`/`log_event` for the actions being demonstrated → `stop_capture` →
`burn_captions.py`.
