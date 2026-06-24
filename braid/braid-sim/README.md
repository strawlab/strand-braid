# braid-sim

Simulation harness for end-to-end testing of Braid's live 3D tracking. A
`sim.toml` scenario is the single source of truth for a run: it describes the
arena, a ring of synthetic cameras, the insects and their motion, the detector
imperfections, and the frame rate. From it the harness derives a synthetic
multi-camera calibration and (deterministically) the ground-truth 3D world, so
the *same* world can be projected into fake cameras and then reconstructed and
scored.

There are two ways to drive the tracker:

- **Image-level (`ci2-sim` backend)** — render insects as blobs, run the real
  feature detector, and feed a full `braid run`. This is the most faithful path;
  use the `braid-sim generate` subcommand to emit the calibration XML and Braid
  config.
- **In-process injector (`inprocess` feature)** — project ground truth straight
  to 2D detections and feed them into the real `flydra2` tracker in-process,
  skipping image rendering, the detector, UDP, and camera registration. This is
  fast and deterministic, so it is ideal for regression tests and for the
  **timing benchmark** below.

## Scaling / timing benchmark

`braid-sim-bench` measures realistic end-to-end **tracking** performance and
produces data for scaling plots as the number of insects and the number of
cameras grow. It drives the real `flydra2` 3D core — triangulation,
undistortion, the EKF, nearest-neighbor data association, multi-target ID
management, and braidz writing — over deterministic synthetic detections.

It does **not** measure image rendering, the feature detector, or the network
(those belong to the image-level path). What it isolates is the 3D
reconstruction core, which is where cost grows with cameras and insects. To keep
the metric clean, each run's wall-clock is split into three phases (see
[`src/bench.rs`](src/bench.rs)):

| phase   | what it is                                            | counted as |
|---------|-------------------------------------------------------|------------|
| `prep`  | project ground truth to 2D detections (the simulator) | *not* tracker |
| `track` | `CoordProcessor::consume_stream` (the tracker)        | **the metric** |
| `io`    | drain the braidz writer + zip to disk                 | reported separately |

Detections are pre-generated and the writer buffer is sized to hold the whole
run, so neither the simulator nor the disk pollutes the `track` measurement.

### Running

The benchmark needs the non-default `inprocess` feature and should be built
`--release`:

```bash
cargo run --release -p braid-sim --features inprocess --bin braid-sim-bench -- \
    --cameras 2,3,4,5,6 --insects 1,2,4,8,16 \
    --frames 2000 --reps 3 --csv scaling.csv
```

Key flags (see `--help` for all):

- `--cameras` / `--insects`: comma-separated grids to sweep.
- `--frames`: synchronized frames tracked per grid point.
- `--reps`: repetitions per point; the **median** `track` time is reported.
- `--fps`: simulated cadence; sets the real-time baseline (default 100).
- `--observation perfect|realistic`: clean detections, or sub-pixel jitter +
  dropout + clutter (stresses data association too).
- `--csv PATH`: also write machine-readable results.
- `--work-dir PATH`: scratch dir for the tracker's `.braid`/`.braidz` (defaults
  to a tempdir; point it at tmpfs to keep `io` fast).

Reported columns: `track_fps` (frames tracked per wall-second), `realtime_x`
(simulated seconds ÷ track wall-clock; `>1` means the tracker keeps up live),
`us/cf` (microseconds per camera-frame — the most load-normalized cost), plus
`objs`/`rows` as a sanity check that tracking actually succeeded.

### Plotting

`braid-sim-plot` turns the CSV into two SVG line charts (vs. insects, one line
per camera count; and vs. cameras, one line per insect count). It only reads the
CSV and writes SVG, so it builds in the default (lightweight) crate — no
`inprocess`/flydra2/tokio and no plotting dependency:

```bash
cargo run -p braid-sim --bin braid-sim-plot -- scaling.csv --out scaling
# -> scaling-vs-insects.svg and scaling-vs-cameras.svg
# choose the metric with --metric {realtime-x,track-fps,us-per-cam-frame,track-s}
```

### Reproducibility

Scenarios are fully seeded and the workload (cameras × insects × frames) is
fixed, so a given invocation defines an identical amount of tracker work every
time; only wall-clock timing varies with the machine. Use `--reps` to report a
median and reduce that noise.
