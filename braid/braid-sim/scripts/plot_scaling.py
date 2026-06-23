#!/usr/bin/env python3
# Copyright (C) The Strand-Braid Authors
# SPDX-License-Identifier: MIT OR Apache-2.0
"""Turn a ``braid-sim-bench`` CSV into scaling plots (self-contained SVG).

This script has **no third-party dependencies** (Python 3 standard library
only), so it runs anywhere Python 3 is available — no numpy/matplotlib needed.
It emits two SVG line charts:

* ``<out>-vs-insects.svg`` — tracker real-time factor vs. insect count, one
  line per camera count. Shows how cost grows as more targets are tracked.
* ``<out>-vs-cameras.svg`` — tracker real-time factor vs. camera count, one
  line per insect count. Shows how cost grows as the rig gets bigger.

Usage::

    python3 plot_scaling.py scaling.csv --out scaling
    python3 plot_scaling.py scaling.csv --metric track_fps --out scaling

Metrics (CSV columns): ``realtime_x`` (default), ``track_fps``,
``us_per_cam_frame``, ``track_s``.
"""

import argparse
import csv
import sys
from collections import defaultdict

# A small categorical palette (color-blind friendly-ish), cycled per series.
PALETTE = [
    "#0072b2",
    "#d55e00",
    "#009e73",
    "#cc79a7",
    "#e69f00",
    "#56b4e9",
    "#999999",
    "#000000",
]

METRIC_LABELS = {
    "realtime_x": "real-time factor (× live)",
    "track_fps": "tracker throughput (frames/s)",
    "us_per_cam_frame": "cost (µs / camera-frame)",
    "track_s": "track wall-clock (s)",
}


def read_rows(path):
    with open(path, newline="") as f:
        return list(csv.DictReader(f))


def _nice_ticks(lo, hi, n=5):
    """A handful of round-ish tick values spanning [lo, hi]."""
    if hi <= lo:
        hi = lo + 1.0
    span = hi - lo
    raw = span / n
    mag = 10 ** _floor_log10(raw)
    for mult in (1, 2, 2.5, 5, 10):
        step = mult * mag
        if span / step <= n + 1:
            break
    start = step * int(lo / step)
    ticks = []
    v = start
    while v <= hi + step * 0.5:
        if v >= lo - step * 0.5:
            ticks.append(round(v, 10))
        v += step
    return ticks


def _floor_log10(x):
    import math

    return int(math.floor(math.log10(x))) if x > 0 else 0


def svg_line_chart(path, title, xlabel, ylabel, series, xvals):
    """Write one SVG line chart.

    ``series`` maps a legend label -> dict{x: y}. ``xvals`` is the sorted list
    of all x positions (categorical, evenly spaced on the axis).
    """
    W, H = 760, 480
    ml, mr, mt, mb = 80, 170, 50, 60  # margins (right margin holds the legend)
    pw, ph = W - ml - mr, H - mt - mb

    ys = [y for s in series.values() for y in s.values()]
    ymin = 0.0
    ymax = max(ys) if ys else 1.0
    yticks = _nice_ticks(ymin, ymax)
    ymax = max(ymax, yticks[-1]) if yticks else ymax

    def px(i):
        # Evenly space categorical x positions.
        if len(xvals) == 1:
            return ml + pw / 2
        return ml + pw * i / (len(xvals) - 1)

    def py(y):
        return mt + ph * (1 - (y - ymin) / (ymax - ymin if ymax > ymin else 1))

    out = []
    out.append(
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" '
        f'viewBox="0 0 {W} {H}" font-family="sans-serif">'
    )
    out.append(f'<rect width="{W}" height="{H}" fill="white"/>')
    out.append(
        f'<text x="{W / 2}" y="26" text-anchor="middle" '
        f'font-size="17" font-weight="bold">{_esc(title)}</text>'
    )

    # Axes.
    out.append(
        f'<line x1="{ml}" y1="{mt}" x2="{ml}" y2="{mt + ph}" stroke="#333"/>'
    )
    out.append(
        f'<line x1="{ml}" y1="{mt + ph}" x2="{ml + pw}" y2="{mt + ph}" stroke="#333"/>'
    )

    # Y grid + ticks.
    for yt in yticks:
        y = py(yt)
        out.append(
            f'<line x1="{ml}" y1="{y:.1f}" x2="{ml + pw}" y2="{y:.1f}" '
            f'stroke="#eee"/>'
        )
        out.append(
            f'<text x="{ml - 8}" y="{y + 4:.1f}" text-anchor="end" '
            f'font-size="11">{_fmt(yt)}</text>'
        )

    # X ticks (categorical).
    for i, xv in enumerate(xvals):
        x = px(i)
        out.append(
            f'<text x="{x:.1f}" y="{mt + ph + 20}" text-anchor="middle" '
            f'font-size="11">{_fmt(xv)}</text>'
        )

    # Axis labels.
    out.append(
        f'<text x="{ml + pw / 2}" y="{H - 12}" text-anchor="middle" '
        f'font-size="13">{_esc(xlabel)}</text>'
    )
    out.append(
        f'<text x="18" y="{mt + ph / 2}" text-anchor="middle" font-size="13" '
        f'transform="rotate(-90 18 {mt + ph / 2})">{_esc(ylabel)}</text>'
    )

    # Series.
    xindex = {xv: i for i, xv in enumerate(xvals)}
    for s_i, (label, pts) in enumerate(sorted(series.items())):
        color = PALETTE[s_i % len(PALETTE)]
        ordered = [(xindex[x], y) for x, y in sorted(pts.items())]
        d = " ".join(f"{px(i):.1f},{py(y):.1f}" for i, y in ordered)
        out.append(
            f'<polyline points="{d}" fill="none" stroke="{color}" '
            f'stroke-width="2"/>'
        )
        for i, y in ordered:
            out.append(
                f'<circle cx="{px(i):.1f}" cy="{py(y):.1f}" r="3" '
                f'fill="{color}"/>'
            )
        ly = mt + 10 + s_i * 20
        out.append(
            f'<line x1="{ml + pw + 16}" y1="{ly}" x2="{ml + pw + 40}" '
            f'y2="{ly}" stroke="{color}" stroke-width="2"/>'
        )
        out.append(
            f'<text x="{ml + pw + 46}" y="{ly + 4}" font-size="11">'
            f'{_esc(label)}</text>'
        )

    out.append("</svg>")
    with open(path, "w") as f:
        f.write("\n".join(out))


def _esc(s):
    return (
        str(s)
        .replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
    )


def _fmt(v):
    f = float(v)
    if f == int(f):
        return str(int(f))
    return f"{f:.3g}"


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("csv", help="CSV produced by braid-sim-bench --csv")
    ap.add_argument("--out", default="scaling", help="output SVG basename")
    ap.add_argument(
        "--metric",
        default="realtime_x",
        choices=sorted(METRIC_LABELS),
        help="which column to plot",
    )
    args = ap.parse_args(argv)

    rows = read_rows(args.csv)
    if not rows:
        print("no data rows in CSV", file=sys.stderr)
        return 1
    ylabel = METRIC_LABELS[args.metric]

    # vs insects, one line per camera count.
    by_cam = defaultdict(dict)
    insect_vals = set()
    for r in rows:
        cams = int(r["cameras"])
        ins = int(r["insects"])
        by_cam[f"{cams} cams"][ins] = float(r[args.metric])
        insect_vals.add(ins)
    svg_line_chart(
        f"{args.out}-vs-insects.svg",
        f"Tracker scaling vs. insect count ({ylabel})",
        "number of insects",
        ylabel,
        dict(by_cam),
        sorted(insect_vals),
    )

    # vs cameras, one line per insect count.
    by_ins = defaultdict(dict)
    cam_vals = set()
    for r in rows:
        cams = int(r["cameras"])
        ins = int(r["insects"])
        by_ins[f"{ins} insects"][cams] = float(r[args.metric])
        cam_vals.add(cams)
    svg_line_chart(
        f"{args.out}-vs-cameras.svg",
        f"Tracker scaling vs. camera count ({ylabel})",
        "number of cameras",
        ylabel,
        dict(by_ins),
        sorted(cam_vals),
    )

    print(f"wrote {args.out}-vs-insects.svg and {args.out}-vs-cameras.svg")
    return 0


if __name__ == "__main__":
    sys.exit(main())
