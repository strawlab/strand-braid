# /// script
# requires-python = ">=3.10"
# dependencies = ["numpy", "pandas"]
# ///
"""Report percentiles of the model-server latency captured by sse_capture.py.

Usage: uv run --no-project analyze_sse.py <model-server-latency.csv>

This is the latency of the tracker's *published* output (frame acquisition to
pose-update publication), stamped upstream of the braidz writer.
"""
import sys

import numpy as np
import pandas as pd

PCTS = [50, 90, 99, 99.9, 99.99, 100]

d = pd.read_csv(sys.argv[1])
print("event counts:", d.msg_type.value_counts().to_dict())
u = d[d.msg_type == "Update"].dropna(subset=["latency_s"])
if not len(u):
    print("no Update events with latency (trigger timestamps missing?)")
    sys.exit(1)
lat = u.latency_s.to_numpy() * 1e3
print(f"pose updates: n={len(lat)}")
for p in PCTS:
    print(f"  P{p:<6g} {np.percentile(lat, p):8.2f} ms")

# Spike pattern: how are the worst-1% latencies distributed in time?
thr = np.percentile(lat, 99)
sp = u[u.latency_s * 1e3 >= thr]
t = sp.recv_time.to_numpy()
if len(t) > 1:
    g = np.diff(t)
    clusters = int((g > 0.05).sum()) + 1
    print(f"worst-1% (>= {thr:.2f} ms): n={len(sp)}, "
          f"{clusters} clusters (>50 ms apart), "
          f"inter-spike gap s: median={np.median(g):.2f} "
          f"p90={np.percentile(g, 90):.2f} max={g.max():.2f}")
