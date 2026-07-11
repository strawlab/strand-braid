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
2. **Writer-side histogram** (`analyze_hlog.py` on
   `reconstruct_latency_usec.hlog` from the `.braidz`): the same start point,
   but stamped when the braidz writer thread dequeues the kalman row. This
   *includes writer-queue delay*: the writer's ~1 s periodic gzip flush
   (`flydra2/src/write_data.rs`) produces a tail of tens of ms, quantized at
   frame-period multiples, that live consumers never see. Do not interpret
   this tail as tracking latency.

`decompose_latency.py` additionally separates camera-side timing (frame
production spread and cadence jitter, from `data2d_distorted` timestamps) from
downstream delay.

Trigger timestamps under FakeSync require the fix in commit "fix(braid):
populate trigger timestamps under FakeSync"; before it, simulated runs record
no latency data at all (empty hlog, NaN kalman timestamps and SSE latency).

## Baseline result (2026-07-11)

5 sim cameras, 2 insects, 100 fps, release, idle 16-core Linux machine
(~8% of one core per camera process):

| percentile | tracker output (SSE) | writer-side histogram |
|-----------:|---------------------:|----------------------:|
| P50        | 0.98 ms              | 0.96 ms               |
| P99        | 1.20 ms              | 20.4 ms               |
| P99.9      | 1.61 ms              | 60.7 ms               |
| max        | 3.84 ms (n=27k)      | 99.9 ms               |

The published tracking output never exceeded 3.84 ms; the writer-side tail is
entirely the flush artifact described above. Camera-side frame production was
synchronized to ≤0.5 ms across cameras with ≤0.8 ms cadence jitter, and no
packet drops or backpressure occurred.

To hunt for a real tail, increase load: more insects/cameras in the scenario,
higher fps, or run with the machine's cores contended (e.g. `stress-ng`).
