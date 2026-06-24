// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reproducible scaling/timing benchmark for Braid's in-process 3D tracker.
//!
//! Sweeps a grid of (camera count × insect count), tracks a fixed number of
//! synthetic frames at each grid point, and reports the tracker's throughput so
//! the cost of adding cameras or insects can be plotted.
//!
//! The workload is deterministic (seeded scenarios, fixed frame counts), so the
//! numbers are reproducible up to machine timing noise — run with `--reps > 1`
//! to report a median. Only the tracker (`consume_stream`: triangulation,
//! undistortion, the EKF, data association, ID management, braidz writing) is
//! timed; projecting ground truth to 2D detections and zipping the recording are
//! reported separately so they do not pollute the scaling metric. See
//! [`braid_sim::bench`] for the phase definitions.
//!
//! Example:
//!
//! ```text
//! cargo run --release -p braid-sim --features inprocess --bin braid-sim-bench -- \
//!     --cameras 2,3,4,6 --insects 1,2,4,8 --frames 1000 --reps 3 --csv scaling.csv
//! ```

use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use eyre::{Context, Result};

use braid_sim::bench::{BenchResult, bench_scenario, run_once};
use braid_sim::scenario::ObservationModel;

/// Detection-quality preset applied to the synthetic 2D observations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum Observation {
    /// No noise, dropout, or clutter — the clean perfect-world load.
    Perfect,
    /// A realistic imperfect detector: sub-pixel jitter, occasional misses, and
    /// a low rate of false positives. Stresses data association as well as the
    /// EKF, so it is the more representative end-to-end load.
    Realistic,
}

impl Observation {
    fn model(self) -> ObservationModel {
        match self {
            Observation::Perfect => ObservationModel::default(),
            Observation::Realistic => ObservationModel {
                pixel_noise_px: 0.5,
                dropout_prob: 0.02,
                clutter_per_frame: 0.1,
                ..Default::default()
            },
        }
    }
}

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Cli {
    /// Comma-separated camera counts to sweep (the rig sizes).
    #[arg(long, value_delimiter = ',', default_value = "2,3,4,5,6")]
    cameras: Vec<usize>,

    /// Comma-separated insect counts to sweep (simultaneous targets).
    #[arg(long, value_delimiter = ',', default_value = "1,2,4,8")]
    insects: Vec<usize>,

    /// Number of synchronized frames to track at each grid point.
    #[arg(long, default_value_t = 1000)]
    frames: usize,

    /// Repetitions per grid point; the reported timing is the median.
    #[arg(long, default_value_t = 3)]
    reps: usize,

    /// Simulated frame rate (frames per second). Sets the real-time baseline the
    /// real-time factor is measured against.
    #[arg(long, default_value_t = 100.0)]
    fps: f64,

    /// RNG seed for the deterministic scenarios.
    #[arg(long, default_value_t = 1)]
    seed: u64,

    /// Detection-quality preset.
    #[arg(long, value_enum, default_value_t = Observation::Perfect)]
    observation: Observation,

    /// Write the per-grid-point results as CSV to this path (in addition to the
    /// human-readable table on stdout).
    #[arg(long)]
    csv: Option<PathBuf>,

    /// Directory for the tracker's scratch `.braid`/`.braidz` output. Defaults
    /// to a temporary directory that is removed on exit. Point it at fast
    /// storage (tmpfs) so the `io` phase is not disk-bound.
    #[arg(long)]
    work_dir: Option<PathBuf>,
}

/// The median of a slice of durations (lower-median for even counts).
fn median(mut v: Vec<Duration>) -> Duration {
    v.sort();
    v[(v.len() - 1) / 2]
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // A held-open tempdir (dropped at the end) unless the user picks a work dir.
    let scratch = match &cli.work_dir {
        Some(_) => None,
        None => Some(tempfile::tempdir().context("creating scratch tempdir")?),
    };
    let work_dir = match &cli.work_dir {
        Some(p) => {
            std::fs::create_dir_all(p).context("creating --work-dir")?;
            p.clone()
        }
        None => scratch.as_ref().unwrap().path().to_path_buf(),
    };

    eprintln!(
        "braid-sim-bench: cameras={:?} insects={:?} frames={} reps={} fps={} obs={:?} seed={}",
        cli.cameras, cli.insects, cli.frames, cli.reps, cli.fps, cli.observation, cli.seed
    );

    let mut results: Vec<BenchResult> = Vec::new();
    for &num_cameras in &cli.cameras {
        for &num_insects in &cli.insects {
            let scenario = bench_scenario(
                num_cameras,
                num_insects,
                cli.fps,
                cli.seed,
                cli.observation.model(),
            );

            // Repeat and keep the run whose `track` time is the median.
            let mut runs: Vec<BenchResult> = Vec::with_capacity(cli.reps);
            for rep in 0..cli.reps.max(1) {
                let out = work_dir.join(format!("c{num_cameras}_i{num_insects}_r{rep}.braid"));
                let r = run_once(&scenario, cli.frames, &out)
                    .await
                    .with_context(|| {
                        format!("benchmarking {num_cameras} cameras × {num_insects} insects")
                    })?;
                // Clean each rep's output so the grid does not accumulate disk.
                let _ = std::fs::remove_dir_all(&out);
                let _ = std::fs::remove_file(out.with_extension("braidz"));
                runs.push(r);
            }
            let median_track = median(runs.iter().map(|r| r.track).collect());
            let chosen = runs
                .into_iter()
                .find(|r| r.track == median_track)
                .expect("median is one of the runs");

            eprintln!(
                "  cams={num_cameras} insects={num_insects}: \
                 track {:.3}s ({:.0} fps, {:.1}× realtime, {:.2} µs/cam-frame), \
                 prep {:.3}s io {:.3}s, objs={} rows={}",
                chosen.track.as_secs_f64(),
                chosen.track_fps(),
                chosen.realtime_factor(),
                chosen.us_per_camera_frame(),
                chosen.prep.as_secs_f64(),
                chosen.io.as_secs_f64(),
                chosen.num_objects,
                chosen.total_rows,
            );
            results.push(chosen);
        }
    }

    print_table(&results);
    if let Some(csv_path) = &cli.csv {
        write_csv(csv_path, &results)
            .with_context(|| format!("writing CSV {}", csv_path.display()))?;
        eprintln!("wrote CSV: {}", csv_path.display());
    }

    Ok(())
}

/// Print an aligned human-readable summary table to stdout.
fn print_table(results: &[BenchResult]) {
    println!(
        "\n{:>5} {:>7} {:>7} {:>10} {:>11} {:>14} {:>9} {:>5} {:>8}",
        "cams", "insects", "frames", "track_s", "track_fps", "realtime_x", "us/cf", "objs", "rows"
    );
    for r in results {
        println!(
            "{:>5} {:>7} {:>7} {:>10.3} {:>11.0} {:>14.1} {:>9.2} {:>5} {:>8}",
            r.num_cameras,
            r.num_insects,
            r.num_frames,
            r.track.as_secs_f64(),
            r.track_fps(),
            r.realtime_factor(),
            r.us_per_camera_frame(),
            r.num_objects,
            r.total_rows,
        );
    }
}

/// Write the results as a CSV suitable for `scripts/plot_scaling.py`.
fn write_csv(path: &std::path::Path, results: &[BenchResult]) -> Result<()> {
    let mut f = std::fs::File::create(path)?;
    writeln!(
        f,
        "cameras,insects,frames,fps,track_s,prep_s,io_s,track_fps,realtime_x,us_per_cam_frame,num_objects,total_rows"
    )?;
    for r in results {
        writeln!(
            f,
            "{},{},{},{},{:.6},{:.6},{:.6},{:.3},{:.4},{:.4},{},{}",
            r.num_cameras,
            r.num_insects,
            r.num_frames,
            r.fps,
            r.track.as_secs_f64(),
            r.prep.as_secs_f64(),
            r.io.as_secs_f64(),
            r.track_fps(),
            r.realtime_factor(),
            r.us_per_camera_frame(),
            r.num_objects,
            r.total_rows,
        )?;
    }
    Ok(())
}
