// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Oracle for the simulation harness: summarize the 3D tracks in a `.braidz`,
//! and compare a live recording against an offline retrack of the same file.
//!
//! The headline question (see the bug-1 plan) is whether *live* tracking
//! produces shorter / more fragmented trajectories than *retracking* the same
//! data. This module computes per-recording track statistics and the
//! live-vs-retrack differential that answers it.

use std::collections::BTreeMap;
use std::path::Path;

use braidz_parser::braidz_parse_path;

/// Statistics about the 3D tracks in one `.braidz` recording.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackStats {
    /// Number of distinct `obj_id`s (track fragments).
    pub num_objects: usize,
    /// Total number of Kalman-estimate rows (tracked object-frames).
    pub total_rows: usize,
    /// Longest single-object frame span (max_frame - min_frame + 1) over all
    /// `obj_id`s. This is the key "how long is the longest trajectory" number.
    pub longest_span: u64,
    /// Sum over objects of each object's frame span.
    pub total_span: u64,
    /// Overall frame range actually covered (min and max frame across all rows).
    pub frame_range: Option<(u64, u64)>,
}

/// Compute [`TrackStats`] from a `.braidz` file's `kalman_estimates` table.
pub fn track_stats(braidz_path: &Path) -> eyre::Result<TrackStats> {
    let archive = braidz_parse_path(braidz_path)
        .map_err(|e| eyre::eyre!("opening braidz {}: {e}", braidz_path.display()))?;

    let rows = archive
        .kalman_estimates_table
        .as_ref()
        .ok_or_else(|| eyre::eyre!("braidz {} has no kalman_estimates", braidz_path.display()))?;

    // Per-object min/max frame and row count.
    let mut per_obj: BTreeMap<u32, (u64, u64, usize)> = BTreeMap::new();
    let mut global_min = u64::MAX;
    let mut global_max = 0u64;
    for row in rows {
        let f = row.frame.0;
        global_min = global_min.min(f);
        global_max = global_max.max(f);
        let e = per_obj.entry(row.obj_id).or_insert((f, f, 0));
        e.0 = e.0.min(f);
        e.1 = e.1.max(f);
        e.2 += 1;
    }

    let mut longest_span = 0u64;
    let mut total_span = 0u64;
    for (lo, hi, _n) in per_obj.values() {
        let span = hi - lo + 1;
        longest_span = longest_span.max(span);
        total_span += span;
    }

    Ok(TrackStats {
        num_objects: per_obj.len(),
        total_rows: rows.len(),
        longest_span,
        total_span,
        frame_range: if rows.is_empty() {
            None
        } else {
            Some((global_min, global_max))
        },
    })
}

/// The result of comparing a live recording against an offline retrack.
#[derive(Debug, Clone)]
pub struct Differential {
    /// Stats from the live recording.
    pub live: TrackStats,
    /// Stats from the offline retrack of the same recording.
    pub retrack: TrackStats,
}

impl Differential {
    /// Whether the live recording's tracks are meaningfully shorter or more
    /// fragmented than the retrack's — the signature of the bug under
    /// investigation. `span_frac` is how much shorter the live longest span may
    /// be before we flag it (e.g. 0.9 means live < 90% of retrack flags).
    pub fn live_is_shortened(&self, span_frac: f64) -> bool {
        let live = self.live.longest_span as f64;
        let retrack = self.retrack.longest_span as f64;
        let shorter = retrack > 0.0 && live < span_frac * retrack;
        let more_fragmented = self.live.num_objects > self.retrack.num_objects;
        shorter || more_fragmented
    }

    /// A human-readable summary table.
    pub fn report(&self) -> String {
        format!(
            "                    {:>12} {:>12}\n\
             objects (frags)     {:>12} {:>12}\n\
             longest span        {:>12} {:>12}\n\
             total span          {:>12} {:>12}\n\
             total rows          {:>12} {:>12}\n\
             frame range         {:>12} {:>12}",
            "live",
            "retrack",
            self.live.num_objects,
            self.retrack.num_objects,
            self.live.longest_span,
            self.retrack.longest_span,
            self.live.total_span,
            self.retrack.total_span,
            self.live.total_rows,
            self.retrack.total_rows,
            fmt_range(self.live.frame_range),
            fmt_range(self.retrack.frame_range),
        )
    }
}

fn fmt_range(r: Option<(u64, u64)>) -> String {
    match r {
        Some((a, b)) => format!("{a}-{b}"),
        None => "(none)".to_string(),
    }
}

/// Run `braid-offline-retrack` on `live_braidz`, writing to `out_braidz`, then
/// return the live-vs-retrack [`Differential`].
///
/// `retrack_exe` is the path to the `braid-offline-retrack` binary.
pub fn differential(
    retrack_exe: &Path,
    live_braidz: &Path,
    out_braidz: &Path,
) -> eyre::Result<Differential> {
    if out_braidz.exists() {
        std::fs::remove_file(out_braidz)?;
    }
    let status = std::process::Command::new(retrack_exe)
        .arg("--data-src")
        .arg(live_braidz)
        .arg("--output")
        .arg(out_braidz)
        .status()
        .map_err(|e| eyre::eyre!("running {}: {e}", retrack_exe.display()))?;
    if !status.success() {
        eyre::bail!("braid-offline-retrack failed with status {status}");
    }

    Ok(Differential {
        live: track_stats(live_braidz)?,
        retrack: track_stats(out_braidz)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(num_objects: usize, longest_span: u64) -> TrackStats {
        TrackStats {
            num_objects,
            total_rows: longest_span as usize,
            longest_span,
            total_span: longest_span,
            frame_range: Some((0, longest_span.saturating_sub(1))),
        }
    }

    #[test]
    fn agreeing_recordings_are_not_flagged() {
        let diff = Differential {
            live: stats(1, 800),
            retrack: stats(1, 798),
        };
        assert!(!diff.live_is_shortened(0.9));
    }

    #[test]
    fn much_shorter_live_span_is_flagged() {
        let diff = Differential {
            live: stats(1, 300),
            retrack: stats(1, 800),
        };
        assert!(diff.live_is_shortened(0.9));
    }

    #[test]
    fn more_live_fragments_is_flagged() {
        // Same total span, but live split the trajectory into more obj_ids.
        let diff = Differential {
            live: stats(4, 800),
            retrack: stats(1, 800),
        };
        assert!(diff.live_is_shortened(0.9));
    }
}
