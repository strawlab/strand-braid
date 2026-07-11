# /// script
# requires-python = ">=3.9"
# dependencies = ["hdrhistogram"]
# ///
"""Analyze reconstruct_latency_usec.hlog from a .braidz file.

Usage: uv run --no-project analyze_hlog.py <file.braidz> [more.braidz ...]

Prints per-interval and aggregate percentiles (values are microseconds; one
interval per <=60 s of recording).

Caveat: this histogram is stamped in the braidz writer thread, so it includes
writer-queue delay (the writer's ~1 s periodic gzip flush produces a tail of
tens of ms that live consumers never see). For the latency of the tracker's
published output, use the model-server SSE capture instead (analyze_sse.py).
"""
import os
import sys
import tempfile
import zipfile

from hdrh.histogram import HdrHistogram
from hdrh.log import HistogramLogReader

PCTS = [50, 90, 99, 99.9, 99.99, 100]


def fmt_ms(usec):
    return f"{usec / 1000.0:8.2f}"


def analyze(braidz_path):
    print(f"=== {braidz_path} ===")
    with zipfile.ZipFile(braidz_path) as zf:
        names = [n for n in zf.namelist() if n.endswith("reconstruct_latency_usec.hlog")]
        if not names:
            print("  no reconstruct_latency_usec.hlog found")
            return
        data = zf.read(names[0])

    with tempfile.NamedTemporaryFile(suffix=".hlog", delete=False) as f:
        f.write(data)
        tmp = f.name
    try:
        # Aggregate histogram: 1 usec .. 60 s, 3 sigfig (superset of writer's 2).
        total = HdrHistogram(1, 60_000_000, 3)
        reader = HistogramLogReader(tmp, total)
        intervals = []
        while True:
            h = reader.get_next_interval_histogram()
            if h is None:
                break
            intervals.append(h)
            total.add(h)

        if not intervals:
            print("  histogram log is EMPTY (no trigger timestamps recorded)")
            return
        hdr = "  {:<10s} {:>10s} ".format("interval", "count") + " ".join(
            f"P{p:<7g}" for p in PCTS
        ) + " (ms)"
        print(hdr)
        for i, h in enumerate(intervals):
            vals = " ".join(fmt_ms(h.get_value_at_percentile(p)) for p in PCTS)
            print(f"  {i:<10d} {h.get_total_count():>10d} {vals}")
        vals = " ".join(fmt_ms(total.get_value_at_percentile(p)) for p in PCTS)
        print(f"  {'TOTAL':<10s} {total.get_total_count():>10d} {vals}")
        print(f"  mean: {total.get_mean_value()/1000.0:.2f} ms   "
              f"stddev: {total.get_stddev()/1000.0:.2f} ms")
    finally:
        os.unlink(tmp)


if __name__ == "__main__":
    for p in sys.argv[1:]:
        analyze(p)
