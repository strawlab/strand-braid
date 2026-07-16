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
camera hardware, so these scripts run on any Linux box, including CI runners,
with no camera attached.

## Prerequisites (Linux only)

- `Xvfb`, `openbox`, `xterm`, a browser (Firefox), `ffmpeg`, `xdotool`
- [`uv`](https://docs.astral.sh/uv/getting-started/installation/) (runs the
  Python caption-burning helper with a pinned Python + dependencies, same as
  `docs/user-docs/scripts/record-mp4-video-ffmpeg.py`)
- A `cargo build --release` of whatever binary the tutorial launches (each
  script builds it automatically if missing). For `strand-cam`, that build
  needs everything in
  [`docs/developer-docs/building-for-development.md`](../../docs/developer-docs/building-for-development.md)
  (a recent enough Rust toolchain, `trunk`, and the `wasm32-unknown-unknown`
  target, since the default `bundle_files` feature compiles the browser
  frontend). If the very first build of the day is offline (e.g. `trunk`'s
  nested `cargo metadata` call), run `cargo fetch` once first.

These scripts were developed and syntax-checked on macOS (where `Xvfb`/
`xdotool`/`x11grab` aren't available) but are meant to run on Linux. The first
full run of each script should be treated as a test: watch the resulting
video, adjust `sleep` durations/captions in that tutorial's `record.sh` as
needed, and only then treat the output as final.

## Layout

```
lib/
  session.sh          # shared bash helpers: virtual display, tiled
                       # terminal+browser windows, simulated typing/keys,
                       # screen capture, and a timestamped caption log
  burn_captions.py     # overlays lib/session.sh's caption log onto the
                       # captured video (uv script, run via `uv run --no-project`)
strand-cam-intro/
  record.sh            # regenerates Video_1.mp4 (launching Strand Camera
                        # from the command line): `strand-cam --camera-backend
                        # sim`, watch the live view, Ctrl+C, relaunch with
                        # `--camera-name simcam0` explicit.
```

## Running a tutorial

```sh
cd strand-cam-intro
./record.sh          # writes ./out/strand-cam-intro.mp4 (and raw.mp4, events.jsonl)
```

`out/` is a local working directory — it is not, and should not be,
committed. Compare the result against the original tutorial video, tweak
timings, and hand the final `.mp4` off manually once you're satisfied (the
generated video files themselves are not part of this repo).

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
