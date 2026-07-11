# Live-tracking latency measurement

Measures the end-to-end latency of Braid's live 3D tracking with the braid-sim
image-level path (real feature detector, real UDP transport, real mainbrain —
no camera hardware), and analyzes where any latency tail originates.

## Running

```bash
braid/braid-sim/latency/run-latency-baseline.sh [sim.toml] [out-dir]
```

Defaults: `../example-sim-multi.toml` (5 cameras, 2 insects, 100 fps), a
release build (built automatically unless `STRAND_BRAID_TARGET_DIR` is set),
and a 125 s recording (`RECORD_SECONDS`). Requires
[`uv`](https://docs.astral.sh/uv/) for the Python helpers. Always measure on
release builds.

One run captures **two latency measurement points** and prints percentile
tables for both:

1. **Tracker-output latency** (`analyze_sse.py` on the model-server SSE
   capture): frame acquisition → detection → UDP → bundling → Kalman →
   pose-update publication. This is the latency a live consumer of the
   `/events` stream experiences.
2. **Reconstruction-latency histogram** (`analyze_hlog.py` on
   `reconstruct_latency_usec.hlog` from the `.braidz`): frame acquisition →
   kalman-estimate production in the tracker. Since the fix in commit
   "fix(flydra2): stamp reconstruction latency at estimate production", the
   two measurement points agree to within ~0.1 ms. (Before that fix the
   histogram was stamped when the braidz writer thread dequeued the row,
   which added the writer's queue delay — its ~1 s periodic gzip flush
   produced a spurious tail of tens of ms, quantized at frame-period
   multiples, that live consumers never saw.)

`decompose_latency.py` additionally separates camera-side timing (frame
production spread and cadence jitter, from `data2d_distorted` timestamps) from
downstream delay.

Trigger timestamps under FakeSync require the fix in commit "fix(braid):
populate trigger timestamps under FakeSync"; before it, simulated runs record
no latency data at all (empty hlog, NaN kalman timestamps and SSE latency).

## Baseline result (2026-07-11)

5 sim cameras, 2 insects, 100 fps, release, idle 16-core Linux machine
(~8% of one core per camera process):

| percentile | tracker output (SSE) | histogram (fixed) | histogram (before fix) |
|-----------:|---------------------:|------------------:|-----------------------:|
| P50        | 0.98 ms              | 0.87 ms           | 0.96 ms                |
| P99        | 1.20 ms              | 1.05 ms           | 20.4 ms                |
| P99.9      | 1.61 ms              | 1.39 ms           | 60.7 ms                |
| max        | 3.84 ms (n=27k)      | 2.26 ms (65 s run)| 99.9 ms                |

The published tracking output never exceeded 3.84 ms; the pre-fix histogram
tail was entirely the writer-flush artifact described above. Camera-side frame
production was synchronized to ≤0.5 ms across cameras with ≤0.8 ms cadence
jitter, and no packet drops or backpressure occurred.

Caveat observed once in ~7 runs: cameras can synchronize with an off-by-one
frame offset under FakeSync (a subset of cameras' synced frame numbers lag one
frame period). The run then reports a consistent ~1-frame-period latency floor
(bundling waits for the lagging cameras) and `decompose_latency.py` shows a
cross-camera spread with P50 of one frame period. Discard such runs when
measuring pipeline latency — or investigate them: it is a real sync race, and
it also degrades data association (cameras observe instants 10 ms apart).

To hunt for a real tail, increase load: more insects/cameras in the scenario,
higher fps, or run with the machine's cores contended (e.g. `stress-ng`).
