# /// script
# requires-python = ">=3.10"
# dependencies = ["numpy", "pandas"]
# ///
"""Decompose the reconstruction-latency tail using per-camera 2D timestamps.

Usage: uv run --no-project decompose_latency.py <file.braidz>

For each synced frame:
  spread = max - min of cam_received_timestamp over cameras
The 3D bundle cannot complete before the last camera's packet, so if `spread`
has the same tail as the reconstruction latency histogram, the tail originates
camera-side (frame production in strand-cam); if `spread` is flat, the tail is
downstream (detection/UDP/tracker/writer). Note cam_received_timestamp is
stamped at frame *production*, before feature detection and UDP send.

Also reports per-camera frame-to-frame cadence jitter and the timing pattern
of the worst spikes.
"""
import sys
import zipfile

import numpy as np
import pandas as pd

PCTS = [50, 90, 99, 99.9, 99.99, 100]


def pct_ms(x):
    return " ".join(f"P{p:g}={np.percentile(x, p)*1e3:7.2f}" for p in PCTS)


braidz = sys.argv[1]
with zipfile.ZipFile(braidz) as zf:
    with zf.open("data2d_distorted.csv.gz") as f:
        d2 = pd.read_csv(f, compression="gzip",
                         usecols=["camn", "frame", "cam_received_timestamp"])
    with zf.open("kalman_estimates.csv.gz") as f:
        ke = pd.read_csv(f, compression="gzip",
                         usecols=["frame", "timestamp"])

print(f"2d rows: {len(d2)}, cameras: {sorted(d2.camn.unique())}, "
      f"frames: {d2.frame.min()}..{d2.frame.max()}")

# One timestamp per (camera, frame) (multiple detections share the stamp).
cf = d2.groupby(["camn", "frame"])["cam_received_timestamp"].first().reset_index()

# Per-frame spread across cameras (only frames seen by all cameras).
ncam = cf.camn.nunique()
per_frame = cf.groupby("frame")["cam_received_timestamp"].agg(["min", "max", "count"])
full = per_frame[per_frame["count"] == ncam]
spread = (full["max"] - full["min"]).to_numpy()
print(f"\nframes with all {ncam} cams: {len(full)} / {len(per_frame)}")
print(f"cross-camera spread (ms):   {pct_ms(spread)}")

# Per-camera cadence jitter: frame-to-frame delta (nominal = frame period).
print("\nper-camera cadence delta (ms):")
for camn, sub in cf.groupby("camn"):
    sub = sub.sort_values("frame")
    ok = np.diff(sub.frame.to_numpy()) == 1
    dt = np.diff(sub.cam_received_timestamp.to_numpy())[ok]
    print(f"  cam {camn}: {pct_ms(dt)}")

# Kalman bundle timestamp vs earliest camera stamp for that frame.
kf = ke.dropna().groupby("frame")["timestamp"].first()
joined = full.join(kf.rename("kts"), how="inner")
if len(joined):
    delta_first = (joined["kts"] - joined["min"]).to_numpy()
    print(f"\nkalman bundle ts - min cam ts (ms): {pct_ms(delta_first)}")

# Where are the spikes in time? Cluster the worst 1% of spreads.
thr = np.percentile(spread, 99)
spikes = full[(full["max"] - full["min"]) >= thr]
t0 = full["min"].min()
times = (spikes["min"] - t0).to_numpy()
print(f"\nworst-1% spread spikes: n={len(spikes)}, threshold={thr*1e3:.2f} ms")
gaps = np.diff(times)
if len(gaps):
    print(f"inter-spike gaps (s): median={np.median(gaps):.2f} "
          f"p90={np.percentile(gaps, 90):.2f} max={gaps.max():.2f}")
worst = spikes.assign(spread=spikes["max"] - spikes["min"]).nlargest(8, "spread")
print("\nworst spikes (per-camera arrival offsets ms relative to earliest):")
for frame in worst.index:
    row = cf[cf.frame == frame].sort_values("cam_received_timestamp")
    base = row.cam_received_timestamp.min()
    offs = ", ".join(f"cam{int(r.camn)}:{(r.cam_received_timestamp-base)*1e3:6.1f}"
                     for r in row.itertuples())
    print(f"  frame {frame} @t+{(base-t0):7.2f}s: {offs}")
