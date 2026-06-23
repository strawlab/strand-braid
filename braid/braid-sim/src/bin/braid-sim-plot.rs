// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Turn a `braid-sim-bench` CSV into scaling plots (self-contained SVG).
//!
//! This is a dependency-light port of the former `scripts/plot_scaling.py`: it
//! reads the benchmark CSV with the standard library only (no plotting crate)
//! and emits two SVG line charts:
//!
//! - `<out>-vs-insects.svg` — the chosen metric vs. insect count, one line per
//!   camera count. Shows how cost grows as more targets are tracked.
//! - `<out>-vs-cameras.svg` — the chosen metric vs. camera count, one line per
//!   insect count. Shows how cost grows as the rig gets bigger.
//!
//! Usage:
//!
//! ```text
//! cargo run -p braid-sim --bin braid-sim-plot -- scaling.csv --out scaling
//! cargo run -p braid-sim --bin braid-sim-plot -- scaling.csv --metric track-fps
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use clap::{Parser, ValueEnum};
use eyre::{Context, Result};

/// Which CSV column to plot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Metric {
    /// Real-time factor (× live).
    RealtimeX,
    /// Tracker throughput (frames/s).
    TrackFps,
    /// Cost (µs / camera-frame).
    UsPerCamFrame,
    /// Track wall-clock (s).
    TrackS,
}

impl Metric {
    /// The CSV column header for this metric.
    fn column(self) -> &'static str {
        match self {
            Metric::RealtimeX => "realtime_x",
            Metric::TrackFps => "track_fps",
            Metric::UsPerCamFrame => "us_per_cam_frame",
            Metric::TrackS => "track_s",
        }
    }

    /// The human-readable axis label for this metric.
    fn label(self) -> &'static str {
        match self {
            Metric::RealtimeX => "real-time factor (× live)",
            Metric::TrackFps => "tracker throughput (frames/s)",
            Metric::UsPerCamFrame => "cost (µs / camera-frame)",
            Metric::TrackS => "track wall-clock (s)",
        }
    }
}

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Cli {
    /// CSV produced by `braid-sim-bench --csv`.
    csv: PathBuf,
    /// Output SVG basename (two files `<out>-vs-{insects,cameras}.svg`).
    #[arg(long, default_value = "scaling")]
    out: String,
    /// Which column to plot.
    #[arg(long, value_enum, default_value_t = Metric::RealtimeX)]
    metric: Metric,
}

/// One parsed CSV row (only the columns the plotter uses, plus the requested
/// metric resolved by name).
struct Row {
    cameras: i64,
    insects: i64,
    metric: f64,
}

/// A categorical palette (color-blind friendly-ish), cycled per series.
const PALETTE: [&str; 8] = [
    "#0072b2", "#d55e00", "#009e73", "#cc79a7", "#e69f00", "#56b4e9", "#999999", "#000000",
];

fn main() -> Result<()> {
    let cli = Cli::parse();

    let text = std::fs::read_to_string(&cli.csv)
        .with_context(|| format!("reading {}", cli.csv.display()))?;
    let rows = parse_csv(&text, cli.metric)?;
    if rows.is_empty() {
        eyre::bail!("no data rows in {}", cli.csv.display());
    }
    let ylabel = cli.metric.label();

    // vs insects, one line per camera count.
    let mut by_cam: BTreeMap<i64, BTreeMap<i64, f64>> = BTreeMap::new();
    let mut insect_vals: BTreeMap<i64, ()> = BTreeMap::new();
    for r in &rows {
        by_cam
            .entry(r.cameras)
            .or_default()
            .insert(r.insects, r.metric);
        insect_vals.insert(r.insects, ());
    }
    let cam_series: Vec<(String, &BTreeMap<i64, f64>)> = by_cam
        .iter()
        .map(|(cams, pts)| (format!("{cams} cams"), pts))
        .collect();
    let insect_xvals: Vec<i64> = insect_vals.keys().copied().collect();
    let vs_insects = format!("{}-vs-insects.svg", cli.out);
    svg_line_chart(
        Path::new(&vs_insects),
        &format!("Tracker scaling vs. insect count ({ylabel})"),
        "number of insects",
        ylabel,
        &cam_series,
        &insect_xvals,
    )
    .with_context(|| format!("writing {vs_insects}"))?;

    // vs cameras, one line per insect count.
    let mut by_ins: BTreeMap<i64, BTreeMap<i64, f64>> = BTreeMap::new();
    let mut cam_vals: BTreeMap<i64, ()> = BTreeMap::new();
    for r in &rows {
        by_ins
            .entry(r.insects)
            .or_default()
            .insert(r.cameras, r.metric);
        cam_vals.insert(r.cameras, ());
    }
    let ins_series: Vec<(String, &BTreeMap<i64, f64>)> = by_ins
        .iter()
        .map(|(ins, pts)| (format!("{ins} insects"), pts))
        .collect();
    let cam_xvals: Vec<i64> = cam_vals.keys().copied().collect();
    let vs_cameras = format!("{}-vs-cameras.svg", cli.out);
    svg_line_chart(
        Path::new(&vs_cameras),
        &format!("Tracker scaling vs. camera count ({ylabel})"),
        "number of cameras",
        ylabel,
        &ins_series,
        &cam_xvals,
    )
    .with_context(|| format!("writing {vs_cameras}"))?;

    println!("wrote {vs_insects} and {vs_cameras}");
    Ok(())
}

/// Parse the benchmark CSV, resolving the requested metric column by its header
/// name. Returns one [`Row`] per data line.
fn parse_csv(text: &str, metric: Metric) -> Result<Vec<Row>> {
    let mut lines = text.lines();
    let header = lines.next().ok_or_else(|| eyre::eyre!("empty CSV"))?;
    let cols: Vec<&str> = header.split(',').map(str::trim).collect();
    let col_idx = |name: &str| {
        cols.iter()
            .position(|c| *c == name)
            .ok_or_else(|| eyre::eyre!("CSV missing column `{name}`"))
    };
    let i_cameras = col_idx("cameras")?;
    let i_insects = col_idx("insects")?;
    let i_metric = col_idx(metric.column())?;

    let mut rows = Vec::new();
    for (lineno, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split(',').map(str::trim).collect();
        let get = |idx: usize| {
            f.get(idx)
                .ok_or_else(|| eyre::eyre!("row {} has too few fields", lineno + 2))
        };
        rows.push(Row {
            cameras: get(i_cameras)?
                .parse()
                .with_context(|| format!("parsing `cameras` on row {}", lineno + 2))?,
            insects: get(i_insects)?
                .parse()
                .with_context(|| format!("parsing `insects` on row {}", lineno + 2))?,
            metric: get(i_metric)?
                .parse()
                .with_context(|| format!("parsing `{}` on row {}", metric.column(), lineno + 2))?,
        });
    }
    Ok(rows)
}

/// Write one SVG line chart. `series` is an ordered list of
/// `(legend label, x -> y)` maps; `xvals` is the sorted list of all x positions
/// (categorical, evenly spaced on the axis).
fn svg_line_chart(
    path: &Path,
    title: &str,
    xlabel: &str,
    ylabel: &str,
    series: &[(String, &BTreeMap<i64, f64>)],
    xvals: &[i64],
) -> Result<()> {
    const W: f64 = 760.0;
    const H: f64 = 480.0;
    // Margins; the right margin holds the legend.
    const ML: f64 = 80.0;
    const MR: f64 = 170.0;
    const MT: f64 = 50.0;
    const MB: f64 = 60.0;
    let pw = W - ML - MR;
    let ph = H - MT - MB;

    let ymin = 0.0;
    let mut ymax = series
        .iter()
        .flat_map(|(_, pts)| pts.values())
        .copied()
        .fold(f64::MIN, f64::max);
    if !ymax.is_finite() {
        ymax = 1.0;
    }
    let yticks = nice_ticks(ymin, ymax, 5);
    if let Some(&last) = yticks.last() {
        ymax = ymax.max(last);
    }

    // Categorical x positions, evenly spaced.
    let px = |i: usize| -> f64 {
        if xvals.len() <= 1 {
            ML + pw / 2.0
        } else {
            ML + pw * i as f64 / (xvals.len() - 1) as f64
        }
    };
    let py = |y: f64| -> f64 {
        let denom = if ymax > ymin { ymax - ymin } else { 1.0 };
        MT + ph * (1.0 - (y - ymin) / denom)
    };

    let mut out = String::new();
    out.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{W}\" height=\"{H}\" \
         viewBox=\"0 0 {W} {H}\" font-family=\"sans-serif\">\n"
    ));
    out.push_str(&format!(
        "<rect width=\"{W}\" height=\"{H}\" fill=\"white\"/>\n"
    ));
    out.push_str(&format!(
        "<text x=\"{}\" y=\"26\" text-anchor=\"middle\" font-size=\"17\" \
         font-weight=\"bold\">{}</text>\n",
        W / 2.0,
        esc(title)
    ));

    // Axes.
    out.push_str(&format!(
        "<line x1=\"{ML}\" y1=\"{MT}\" x2=\"{ML}\" y2=\"{}\" stroke=\"#333\"/>\n",
        MT + ph
    ));
    out.push_str(&format!(
        "<line x1=\"{ML}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#333\"/>\n",
        MT + ph,
        ML + pw,
        MT + ph
    ));

    // Y grid + ticks.
    for &yt in &yticks {
        let y = py(yt);
        out.push_str(&format!(
            "<line x1=\"{ML}\" y1=\"{y:.1}\" x2=\"{}\" y2=\"{y:.1}\" stroke=\"#eee\"/>\n",
            ML + pw
        ));
        out.push_str(&format!(
            "<text x=\"{}\" y=\"{:.1}\" text-anchor=\"end\" font-size=\"11\">{}</text>\n",
            ML - 8.0,
            y + 4.0,
            fmt_num(yt)
        ));
    }

    // X ticks (categorical).
    for (i, &xv) in xvals.iter().enumerate() {
        out.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{}\" text-anchor=\"middle\" font-size=\"11\">{}</text>\n",
            px(i),
            MT + ph + 20.0,
            xv
        ));
    }

    // Axis labels.
    out.push_str(&format!(
        "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-size=\"13\">{}</text>\n",
        ML + pw / 2.0,
        H - 12.0,
        esc(xlabel)
    ));
    let yc = MT + ph / 2.0;
    out.push_str(&format!(
        "<text x=\"18\" y=\"{yc}\" text-anchor=\"middle\" font-size=\"13\" \
         transform=\"rotate(-90 18 {yc})\">{}</text>\n",
        esc(ylabel)
    ));

    // Series.
    let xindex: BTreeMap<i64, usize> = xvals.iter().enumerate().map(|(i, &x)| (x, i)).collect();
    for (s_i, (label, pts)) in series.iter().enumerate() {
        let color = PALETTE[s_i % PALETTE.len()];
        let points: Vec<(usize, f64)> = pts
            .iter()
            .filter_map(|(x, &y)| xindex.get(x).map(|&i| (i, y)))
            .collect();
        let d: Vec<String> = points
            .iter()
            .map(|&(i, y)| format!("{:.1},{:.1}", px(i), py(y)))
            .collect();
        out.push_str(&format!(
            "<polyline points=\"{}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"2\"/>\n",
            d.join(" ")
        ));
        for &(i, y) in &points {
            out.push_str(&format!(
                "<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"3\" fill=\"{color}\"/>\n",
                px(i),
                py(y)
            ));
        }
        let ly = MT + 10.0 + s_i as f64 * 20.0;
        out.push_str(&format!(
            "<line x1=\"{}\" y1=\"{ly}\" x2=\"{}\" y2=\"{ly}\" stroke=\"{color}\" \
             stroke-width=\"2\"/>\n",
            ML + pw + 16.0,
            ML + pw + 40.0
        ));
        out.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" font-size=\"11\">{}</text>\n",
            ML + pw + 46.0,
            ly + 4.0,
            esc(label)
        ));
    }

    out.push_str("</svg>\n");
    std::fs::write(path, out)?;
    Ok(())
}

/// A handful of round-ish tick values spanning `[lo, hi]`.
fn nice_ticks(lo: f64, mut hi: f64, n: usize) -> Vec<f64> {
    if hi <= lo {
        hi = lo + 1.0;
    }
    let span = hi - lo;
    let raw = span / n as f64;
    let mag = 10f64.powi(if raw > 0.0 {
        raw.log10().floor() as i32
    } else {
        0
    });
    let mut step = mag;
    for mult in [1.0, 2.0, 2.5, 5.0, 10.0] {
        step = mult * mag;
        if span / step <= n as f64 + 1.0 {
            break;
        }
    }
    let mut ticks = Vec::new();
    let mut v = step * (lo / step).trunc();
    while v <= hi + step * 0.5 {
        if v >= lo - step * 0.5 {
            // Round away float fuzz so the label is clean (matches the old
            // Python `round(v, 10)`).
            ticks.push((v * 1e10).round() / 1e10);
        }
        v += step;
    }
    ticks
}

/// Escape the XML special characters that can appear in chart text.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Format a tick value: integers without a decimal point, otherwise the
/// shortest round-tripping representation (Rust's default float `Display`).
fn fmt_num(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_csv_resolves_metric_by_name() {
        let csv = "cameras,insects,frames,fps,track_s,realtime_x\n\
                   2,1,2000,100,0.007,2790.8\n\
                   3,4,2000,100,0.023,852.5\n";
        let rows = parse_csv(csv, Metric::RealtimeX).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].cameras, rows[0].insects), (2, 1));
        assert!((rows[0].metric - 2790.8).abs() < 1e-6);
        assert_eq!((rows[1].cameras, rows[1].insects), (3, 4));

        // A different metric column resolves independently.
        let rows = parse_csv(csv, Metric::TrackS).unwrap();
        assert!((rows[0].metric - 0.007).abs() < 1e-9);
    }

    #[test]
    fn parse_csv_errors_on_missing_column() {
        let csv = "cameras,insects\n2,1\n";
        assert!(parse_csv(csv, Metric::RealtimeX).is_err());
    }

    #[test]
    fn nice_ticks_span_the_range_and_are_clean() {
        let ticks = nice_ticks(0.0, 2790.8, 5);
        assert!(ticks.len() >= 2);
        assert!(*ticks.first().unwrap() <= 0.0);
        assert!(*ticks.last().unwrap() >= 2790.8);
        // Steps are uniform.
        let step = ticks[1] - ticks[0];
        for w in ticks.windows(2) {
            assert!((w[1] - w[0] - step).abs() < step * 1e-6);
        }
    }

    #[test]
    fn fmt_num_drops_integer_decimals() {
        assert_eq!(fmt_num(100.0), "100");
        assert_eq!(fmt_num(2.5), "2.5");
        assert_eq!(fmt_num(0.0), "0");
    }

    #[test]
    fn esc_escapes_xml() {
        assert_eq!(esc("a & b < c > d"), "a &amp; b &lt; c &gt; d");
    }

    #[test]
    fn svg_line_chart_writes_a_file() {
        let tmp = std::env::temp_dir().join("braid-sim-plot-test-vs.svg");
        let mut pts = BTreeMap::new();
        pts.insert(1i64, 10.0);
        pts.insert(2i64, 20.0);
        let series = vec![("2 cams".to_string(), &pts)];
        svg_line_chart(&tmp, "t", "x", "y", &series, &[1, 2]).unwrap();
        let body = std::fs::read_to_string(&tmp).unwrap();
        assert!(body.starts_with("<svg"));
        assert!(body.contains("</svg>"));
        assert!(body.contains("polyline"));
        let _ = std::fs::remove_file(&tmp);
    }
}
