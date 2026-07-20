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

`strand-cam-intro` records against real camera hardware by default if any
is attached to the machine it's run on, falling back to the hardware-free
`sim` backend (`ci2-sim`, driven by `braid/braid-sim/example-sim.toml`)
otherwise — so the same command records the most realistic possible
version on a lab machine with a camera plugged in, and still just works
with no hardware at all (e.g. CI, a laptop). See `CAMERA_BACKEND` in step 4
of Running instructions to force one or the other explicitly.

`braid-intro` has no such fallback — see "Braid and camera hardware" below.

## Prerequisites (Linux only)

Six packages are hard requirements:

- `ffmpeg` — screen capture and caption burn-in
- `xdotool` — window placement, simulated typing, and keypresses
- `Xvfb` + `openbox` — a disposable virtual display and window manager
- `ttyd` — bridges the terminal's real PTY into a browser window instead of
  a native terminal emulator (see below for why)
- `xprop` (part of the `x11-utils` package) — reads a window's
  `_NET_FRAME_EXTENTS` so mouse-pointing can correct for a window-manager-
  added title bar on the terminal window (see below for why it has one at
  all)

**Recording always happens on its own isolated `Xvfb` display, never your
real desktop session.** This is deliberate: an earlier version reused
whatever real X11 session was already running (to avoid the extra
dependencies), but that made simulated typing/window automation a
window-targeting footgun — get it wrong and it can move, type into, or kill
a window on your *actual* desktop instead of the throwaway one it meant to
target. Isolation is worth the extra packages.

`ttyd` (not a native terminal emulator like `xterm`) is what `record.sh`
actually types into: it bridges a real PTY/shell into a browser tab, running
xterm.js in its **DOM render mode** rather than the default canvas/WebGL
one, so every line of terminal text becomes a real, queryable DOM element.
That's what lets `record.sh` point the mouse at real on-screen text (e.g. a
camera name in the terminal's own log output) via the Chrome DevTools
Protocol (`lib/cdp_locate.py`) the exact same way it already does for BUI
text, instead of a tuned pixel guess with no way to verify it. A native
terminal emulator has no DOM to query, so this only works because the
terminal is *also* just a browser page — see
`strand-cam-intro/POINTING-NOTES.md` for the full history of that decision.
The terminal's browser window is also launched in Chrome's **app mode**
(`--app=URL`), which hides the tab strip/address bar/back-forward buttons
entirely, so it reads as a real terminal window rather than an obvious
browser tab — the BUI window is deliberately left as a normal browser
window, since that's what a real user genuinely sees there.

App mode has one side effect worth knowing about: since Chrome no longer
draws its own window chrome, `openbox` decides this window needs a title
bar after all and adds one of its own (a normal Chrome window doesn't get
one, since openbox recognizes it as already decorated). That extra title
bar isn't visible to Chrome's own DOM/CDP measurements, so
`lib/session.sh`'s mouse-pointing math reads the window manager's own
`_NET_FRAME_EXTENTS` property (via `xprop`) to correct for it — a `0,0,0,0`
extent (the normal-window case) is a no-op, so this doesn't affect the BUI
window's already-correct pointing.

Everything else uses what's already installed instead of requiring anything
new, falling back to installing its own minimal version only if nothing
usable is found:

- **browser**: prefers `google-chrome`/`chromium` over `firefox`, launched
  with an isolated profile and remote-control disabled (`--user-data-dir`/
  `-no-remote`) so it can't hand off to — or get confused with — an instance
  already running on your real desktop. Used for *two* windows now: the BUI,
  and (via `ttyd`, above) the terminal itself — each gets its own isolated
  profile and, for Chrome/Chromium, its own `--remote-debugging-port` for
  CDP text lookups. `firefox` is tried last and only as a fallback: on stock
  Ubuntu it's a snap package, and snap's confinement sandbox blocks it from
  reading/writing the isolated profile dir under `/tmp`, so it fails with
  "Your Firefox profile cannot be loaded" instead of actually isolating — a
  known limitation of that packaging, not something this script works
  around (it also means CDP-based pointing silently falls back to a tuned
  pixel guess whenever firefox is the only browser available, since firefox
  doesn't speak CDP). Chrome/Chromium variants also get `--ozone-platform=x11
  --disable-gpu`: on a Wayland desktop, Chrome otherwise auto-detects
  `$WAYLAND_DISPLAY` and renders natively there, completely bypassing the
  isolated `$DISPLAY` (Wayland connections don't go through `$DISPLAY` at
  all) and opening a real, visible window on your actual desktop instead —
  `--ozone-platform=x11` forces it onto X11/XWayland so it actually honors
  the isolation. `--disable-gpu` is separately needed because Xvfb has no
  real GPU, and Chrome's default GPU-accelerated compositing path fails
  silently there, leaving the window blank/black instead of falling back to
  software rendering on its own.
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
  session.sh          # shared bash helpers: isolated virtual display, tiled
                       # terminal+browser windows, simulated typing/keys,
                       # screen capture, and a timestamped caption log
  burn_captions.py     # overlays lib/session.sh's caption log onto the
                       # captured video (no dependencies, run with python3)
strand-cam-intro/
  record.sh            # regenerates Video_1.mp4 (launching Strand Camera
                        # from the command line): `strand-cam --camera-backend
                        # sim`, watch the live view, Ctrl+C, relaunch with
                        # `--camera-name simcam0` explicit.
braid-intro/
  record.sh            # regenerates Video_2.mp4 (launching Braid from the
                        # command line): `braid-run config.TOML`, wait for
                        # all cameras to synchronize, scroll up to the QR
                        # code, open the GUI, cycle through every camera,
                        # Ctrl+C, relaunch, then (new, not in the original)
                        # close the GUI window and reopen it via the
                        # terminal's printed URL. Real camera hardware only
                        # -- see "Braid and camera hardware" below.
  POINTING-NOTES.md    # tuned constants (scroll-click counts, fallback
                        # pixel coordinates, per-camera dwell) that need
                        # retuning after watching a real run -- read this
                        # before touching record.sh's own tuned constants.
```

## Running instructions

### 1. Get the code

```sh
git clone git@github.com:Mharrap/strand-braid.git
cd strand-braid
```

Everything is on `main` — no branch to check out.

### 2. Install the hard requirements

```sh
sudo apt-get update
sudo apt-get install -y ffmpeg xdotool xvfb openbox ttyd x11-utils
```

That's everything needed for the display/terminal/capture side. See
Prerequisites above for when the browser fallback (`firefox`, if no Chrome/
Chromium variant is installed) needs installing too.

### 3. Point at a `strand-cam` build

`record.sh` picks a `strand-cam` binary itself, in this order:

1. `STRAND_BRAID_TARGET_DIR`, if you set it — an explicit override.
2. Otherwise, whatever `strand-cam` is already on `PATH` (e.g. installed via
   the `.deb` package) — the common case, and the fastest, since it skips
   building entirely.
3. Otherwise, `target/release`, building from source there first if it's
   empty — see Prerequisites above for the `trunk`/Rust-toolchain/
   `cargo fetch` requirements that build needs.

**If `strand-cam` is already installed**, sanity-check the `sim` backend is
compiled in before running the full script:

```sh
export STRAND_CAM_SIM_SPEC="$(pwd)/braid/braid-sim/example-sim.toml"
strand-cam --camera-backend sim --list-cameras   # should list simcam0..simcam4
```

If that errors, the installed build doesn't have `sim` compiled in — build
from source instead by setting `STRAND_BRAID_TARGET_DIR` to somewhere other
than that install (or uninstalling it isn't necessary; just don't rely on
step 2 above).

**Forcing real camera hardware explicitly** (`CAMERA_BACKEND=pylon`/
`vimba`/`webcam` — see step 4 and "A note on `--camera-backend sim`"
below)? Sanity-check the camera is actually detected first, the same way:

```sh
strand-cam --camera-backend pylon --list-cameras   # should list your camera(s)
```

If that lists nothing (or errors), an *explicit* `CAMERA_BACKEND=pylon`
will fail the same way once `record.sh` gets to actually launching
`strand-cam` — fix connectivity/drivers first rather than debugging it
through a full recording run. Left unset instead, `record.sh` runs this
same check itself to auto-detect: no camera found just means it quietly
falls back to `sim`, not an error.

Also worth checking before a real-hardware run, especially on a shared
machine: is a *different* `strand-cam` process already running against a
camera?

```sh
ss -ltnp | grep 3440
```

If something's already listening on port 3440, `record.sh`'s own instance
fails to bind it — but silently, not with an error: `wait_for_url` and the
browser both succeed anyway, just showing that *other* process's live feed
instead of the one this run actually launched. That looks like a
successful recording but isn't testing what this script did. Confirm the
port is free first.

### 4. Run it

```sh
cd media-utils/tutorial-video-simulation/strand-cam-intro
./record.sh

# Force one backend explicitly instead of auto-detecting:
CAMERA_BACKEND=sim ./record.sh    # hardware-free, even if a real camera is attached
CAMERA_BACKEND=pylon ./record.sh  # real Basler hardware, erroring out if none is found
```

`CAMERA_BACKEND` selects which `strand-cam --camera-backend` actually
runs. Left unset, `record.sh` auto-detects: real Basler (`pylon`) hardware
if `--list-cameras` finds any attached and responding, otherwise the
hardware-free `sim` backend — printed on stdout either way (`=== Real
camera hardware detected -- defaulting to CAMERA_BACKEND=pylon ===` or the
`sim` equivalent), so it's always obvious after the fact which one a given
run actually used. An explicit `CAMERA_BACKEND` always wins over
auto-detection, including `CAMERA_BACKEND=sim` on a machine that does have
a camera attached. See "A note on `--camera-backend sim`" below for how
the on-screen commands stay clean regardless of which backend ends up in
use.

Internally, step 3's binary selection does:
```sh
if [ -n "${STRAND_BRAID_TARGET_DIR:-}" ]; then
    TARGET_DIR="$STRAND_BRAID_TARGET_DIR"
elif command -v strand-cam >/dev/null 2>&1; then
    TARGET_DIR=$(dirname "$(command -v strand-cam)")
else
    TARGET_DIR="$REPO_ROOT/target/release"
fi
# ...build there if $TARGET_DIR/strand-cam doesn't exist yet...
export PATH="$TARGET_DIR:$PATH"   # so the terminal shows plain "strand-cam ...", not a full path
```
To force a from-source build even with a package installed (e.g. to test an
uncommitted change), point `STRAND_BRAID_TARGET_DIR` at `target/release`
explicitly: `STRAND_BRAID_TARGET_DIR="$(pwd)/../../../target/release" ./record.sh`.

### 5. Output

`out/strand-cam-intro.mp4` (plus `out/raw.mp4`, the pre-caption capture, and
`out/events.jsonl`, the caption log) next to `record.sh`. `out/` is a local
working directory — it is not, and should not be, committed. Compare the
result against the original tutorial video, tweak `sleep` durations/captions
in `record.sh` and rerun if needed, and hand the final `.mp4` off manually
once you're satisfied (the generated video files themselves are not part of
this repo).

The exact `strand-cam --version` output used to generate the video is
written into `strand-cam-intro.mp4`'s `comment` metadata tag (not burned
into the picture) -- check it with `ffprobe -v quiet -show_entries
format_tags=comment out/strand-cam-intro.mp4` if you need to know which
build a given output came from.

## A note on `--camera-backend sim`

`strand-cam-intro` auto-detects real Basler camera hardware and prefers it
over `--camera-backend sim` when it's available (see step 4), so the
recording is as realistic as possible on a machine that has one, while
still needing no camera hardware at all on one that doesn't. If you have a
real Basler camera, the equivalent of the old `strand-cam-pylon` command is
simply `strand-cam` on its own — `pylon` is still the default backend
(`strand-cam/src/cli_app.rs`), it's just no longer baked into the binary's
name. Set `CAMERA_BACKEND=vimba`/`webcam` to record against that kind of
hardware instead (auto-detection only ever probes for `pylon`), or
`CAMERA_BACKEND=sim`/`pylon` to force one of those explicitly rather than
auto-detecting — see step 4 above.

`strand-cam` has no environment variable for `--camera-backend`; it's
CLI-only, and always defaults to Pylon if omitted. So the terminal always
needs to show *some* version of the true command a real user with that
hardware would type — never a `--camera-backend` flag that's only an
artifact of this recording setup. `record.sh` handles this by generating a
tiny wrapper script named `strand-cam` (earlier on `PATH` than the real
binary, scoped to that one run only) that silently injects
`--camera-backend $CAMERA_BACKEND` while forwarding everything else — except
for `CAMERA_BACKEND=pylon`, where no wrapper is needed at all, since the
bare command is already exactly correct. Command 2's `--camera-name` is
`simcam0` for the sim backend, or auto-detected via `--list-cameras` for a
real one (so it always points at whichever real camera is actually
attached, not a hardcoded name).

## Braid and camera hardware

Unlike `strand-cam-intro`, `braid-intro` has **no hardware-free fallback**.
It replays a real config file (`/home/strawlab/BRAID_TOMLS/config.TOML` by
default, override with `BRAID_CONFIG_TOML`) that configures 5 real Basler
cameras with PTP-sync triggering and a real extrinsic calibration file.
`braid-run` only gets a `sim` backend for a camera if that camera's own
`[[cameras]]` entry sets `start_backend = "sim"` in the TOML — this config
doesn't, so every camera defaults to `Pylon` (`braid/braid-types/src/lib.rs`).
Running `braid-intro/record.sh` requires that hardware, PTP sync, and
calibration file to already be working on whatever machine you run it on;
there's no `CAMERA_BACKEND`-style override to fall back to.

`braid-run` also resolves its own per-camera `strand-cam` child next to its
own executable path (`std::env::current_exe().parent()` in
`braid-run/src/main.rs`'s `launch_strand_cam`), **not** via `$PATH` — so
whichever `braid-run` binary ends up on `PATH` (installed package or a
from-source build) needs a `strand-cam` binary sitting right next to it.
The `.deb` package ships both together already; a from-source build needs
`cargo build --release -p braid-run -p strand-cam` (not just `-p braid-run`).

## Running `braid-intro`

Same Prerequisites as `strand-cam-intro` (see above) — no extra packages
needed, just the real camera hardware described in "Braid and camera
hardware". Same `STRAND_BRAID_TARGET_DIR` override for picking a specific
`braid-run`/`strand-cam` build if you don't want to rely on an installed
package.

```sh
cd media-utils/tutorial-video-simulation/braid-intro
./record.sh

# Point at a different config file:
BRAID_CONFIG_TOML=/path/to/other-config.TOML ./record.sh
```

Output is `out/braid-intro.mp4` (plus `out/raw.mp4` and `out/events.jsonl`,
same as `strand-cam-intro`), with the `braid-run --version` output used to
generate it written into the `comment` metadata tag the same way. Given how
many pixel/scroll-count constants this scenario tunes by eye (see
`braid-intro/POINTING-NOTES.md`) and how slow each iteration is (real PTP
hardware has to actually resynchronize on every run), expect a first attempt
to need a few rounds of "watch the video, adjust a constant, rerun."

## Adding another tutorial

Create a new subdirectory with its own `record.sh` that sources
`../lib/session.sh` and sets `SCRIPT_NAME` before doing so (used to namespace
that script's temp/work directory). See `strand-cam-intro/record.sh` for the
pattern: `start_display` → `start_capture` → open windows → `type_in`/
`send_keys`/`log_event` for the actions being demonstrated → `stop_capture` →
`burn_captions.py`.
