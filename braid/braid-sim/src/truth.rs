// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ground-truth oracle for the simulation harness.
//!
//! Unlike [`crate::score`], which compares a live recording against an offline
//! retrack of *itself*, this module scores a `.braidz` against the **known
//! ground truth** of the [`Scenario`] that generated it. Because the world is a
//! pure function of time ([`World::state_at`]), every reconstructed track can be
//! compared to where its insect actually was.
//!
//! It answers: how accurately, how completely, and how stably did Braid track
//! the simulated insects?
//!
//! - **Accuracy**: position RMSE / max error over matched object-frames.
//! - **Completeness** (coverage): the fraction of object-frames where an insect
//!   was present and some track was within the association gate.
//! - **Stability**: ID switches and track fragmentation — how many distinct
//!   Braid `obj_id`s ended up assigned to a single ground-truth insect. A
//!   perfectly stable run has one `obj_id` per insect (mean fragments = 1, zero
//!   switches); the live-vs-retrack fragmentation bug shows up here as many
//!   fragments per insect.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use braidz_parser::braidz_parse_path;

use crate::scenario::Scenario;
use crate::world::World;

/// Result of scoring a `.braidz` against ground truth.
#[derive(Debug, Clone, PartialEq)]
pub struct GroundTruthScore {
    /// Number of ground-truth insects in the scenario.
    pub num_truth: usize,
    /// Number of distinct Braid `obj_id`s (track fragments) in the recording.
    pub num_tracks: usize,
    /// Number of matched object-frames (a row associated to a present insect
    /// within the gate).
    pub num_matched: usize,
    /// Position root-mean-square error over matched object-frames, meters.
    pub rmse_m: f64,
    /// Worst single-frame position error over matched object-frames, meters.
    pub max_err_m: f64,
    /// Fraction in `[0, 1]` of present object-frames (insect present and the
    /// recording covers that frame) for which some track matched within the
    /// gate.
    pub coverage: f64,
    /// Total number of ID switches summed over insects: a switch is counted each
    /// time the `obj_id` matched to an insect changes from one matched frame to
    /// the next.
    pub id_switches: usize,
    /// Mean number of distinct `obj_id`s assigned to a single ground-truth
    /// insect. `1.0` is ideal (one unbroken track per insect); larger means more
    /// fragmentation.
    pub mean_fragments: f64,
    /// The frame offset (truth frame = braidz frame + offset) that best aligned
    /// the recording to ground truth. Usually `0`; nonzero absorbs any
    /// constant sync-establishment offset.
    pub frame_offset: i64,
}

/// One Kalman-estimate row reduced to what the oracle needs.
struct Row {
    frame: i64,
    obj_id: u32,
    pos: [f64; 3],
}

impl Row {
    fn new(frame: i64, obj_id: u32, pos: [f64; 3]) -> Self {
        Row { frame, obj_id, pos }
    }
}

/// Score `braidz_path` against the ground truth of `scenario`.
///
/// `gate_m` is the maximum 3D distance (meters) at which a reconstructed point
/// is associated to a ground-truth insect. `max_frame_offset` bounds the search
/// for a constant frame offset between the recording's synchronized frame
/// numbers and simulation time (`t = frame / fps`); pass `0` to disable the
/// search and assume exact alignment.
pub fn score_against_truth(
    braidz_path: &Path,
    scenario: &Scenario,
    gate_m: f64,
    max_frame_offset: i64,
) -> eyre::Result<GroundTruthScore> {
    let archive = braidz_parse_path(braidz_path)
        .map_err(|e| eyre::eyre!("opening braidz {}: {e}", braidz_path.display()))?;
    let krows = archive
        .kalman_estimates_table
        .as_ref()
        .ok_or_else(|| eyre::eyre!("braidz {} has no kalman_estimates", braidz_path.display()))?;

    let rows: Vec<Row> = krows
        .iter()
        .map(|r| Row::new(r.frame.0 as i64, r.obj_id, [r.x, r.y, r.z]))
        .collect();

    Ok(score_rows(&rows, scenario, gate_m, max_frame_offset))
}

/// Core scoring over already-extracted rows. Separated from
/// [`score_against_truth`] so it can be unit-tested with synthetic rows.
fn score_rows(
    rows: &[Row],
    scenario: &Scenario,
    gate_m: f64,
    max_frame_offset: i64,
) -> GroundTruthScore {
    let world = World::new(scenario.clone());
    let fps = scenario.fps;
    let num_tracks = rows.iter().map(|r| r.obj_id).collect::<BTreeSet<_>>().len();

    // Pick the integer frame offset minimizing mean matched error. Position
    // changes slowly between frames, so even a coarse search robustly absorbs a
    // constant sync-establishment offset; ties (and the zero-match case) keep
    // the smallest |offset|.
    let mut best: Option<(f64, i64)> = None; // (mean_err, offset)
    for offset in -max_frame_offset..=max_frame_offset {
        let mut sum = 0.0;
        let mut n = 0usize;
        for row in rows {
            let t = (row.frame + offset) as f64 / fps;
            if let Some((_id, d)) = nearest_truth(&world, t, &row.pos, gate_m) {
                sum += d;
                n += 1;
            }
        }
        if n == 0 {
            continue;
        }
        let mean = sum / n as f64;
        let better = match best {
            None => true,
            Some((bmean, boff)) => {
                mean < bmean - 1e-12 || ((mean - bmean).abs() <= 1e-12 && offset.abs() < boff.abs())
            }
        };
        if better {
            best = Some((mean, offset));
        }
    }
    let frame_offset = best.map(|(_, o)| o).unwrap_or(0);

    // Final pass at the chosen offset: gather matches and accuracy.
    let mut sq_sum = 0.0;
    let mut max_err = 0.0f64;
    let mut num_matched = 0usize;
    // Per insect, ordered by frame, the matched obj_id (nearest wins per frame).
    let mut per_insect: BTreeMap<u32, BTreeMap<i64, (u32, f64)>> = BTreeMap::new();
    for row in rows {
        let t = (row.frame + frame_offset) as f64 / fps;
        if let Some((id, d)) = nearest_truth(&world, t, &row.pos, gate_m) {
            num_matched += 1;
            sq_sum += d * d;
            max_err = max_err.max(d);
            let slot = per_insect.entry(id).or_default().entry(row.frame);
            slot.and_modify(|(oid, best_d)| {
                if d < *best_d {
                    *oid = row.obj_id;
                    *best_d = d;
                }
            })
            .or_insert((row.obj_id, d));
        }
    }
    let rmse_m = if num_matched > 0 {
        (sq_sum / num_matched as f64).sqrt()
    } else {
        0.0
    };

    // Stability: ID switches and fragments per insect.
    let mut id_switches = 0usize;
    let mut frag_total = 0usize;
    for assignments in per_insect.values() {
        let mut prev: Option<u32> = None;
        let mut distinct: BTreeSet<u32> = BTreeSet::new();
        for (oid, _d) in assignments.values() {
            distinct.insert(*oid);
            if let Some(p) = prev
                && p != *oid
            {
                id_switches += 1;
            }
            prev = Some(*oid);
        }
        frag_total += distinct.len();
    }

    // Coverage: matched object-frames over present object-frames within the
    // recording's frame range.
    let coverage = if rows.is_empty() {
        0.0
    } else {
        let min_f = rows.iter().map(|r| r.frame).min().unwrap();
        let max_f = rows.iter().map(|r| r.frame).max().unwrap();
        let mut present = 0usize;
        for f in min_f..=max_f {
            let t = (f + frame_offset) as f64 / fps;
            present += world.state_at(t).len();
        }
        // matched object-frames = number of (insect, frame) pairs we matched.
        let matched_obj_frames: usize = per_insect.values().map(|m| m.len()).sum();
        if present > 0 {
            (matched_obj_frames as f64 / present as f64).min(1.0)
        } else {
            0.0
        }
    };

    let num_truth = scenario.insects.len();
    let mean_fragments = if per_insect.is_empty() {
        0.0
    } else {
        frag_total as f64 / per_insect.len() as f64
    };

    GroundTruthScore {
        num_truth,
        num_tracks,
        num_matched,
        rmse_m,
        max_err_m: max_err,
        coverage,
        id_switches,
        mean_fragments,
        frame_offset,
    }
}

/// The nearest present insect to `pos` at time `t`, and its distance, if within
/// `gate_m`.
fn nearest_truth(world: &World, t: f64, pos: &[f64; 3], gate_m: f64) -> Option<(u32, f64)> {
    let mut best: Option<(u32, f64)> = None;
    for ins in world.state_at(t) {
        let c = &ins.pos.coords;
        let dx = c.x - pos[0];
        let dy = c.y - pos[1];
        let dz = c.z - pos[2];
        let d = (dx * dx + dy * dy + dz * dz).sqrt();
        if d <= gate_m && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((ins.id, d));
        }
    }
    best
}

impl GroundTruthScore {
    /// A human-readable summary.
    pub fn report(&self) -> String {
        format!(
            "ground-truth oracle:\n\
             \x20 truth insects   {}\n\
             \x20 braid tracks    {}\n\
             \x20 matched frames  {}\n\
             \x20 frame offset    {}\n\
             \x20 position RMSE   {:.4} m\n\
             \x20 position max    {:.4} m\n\
             \x20 coverage        {:.1}%\n\
             \x20 id switches     {}\n\
             \x20 frags / insect  {:.2}",
            self.num_truth,
            self.num_tracks,
            self.num_matched,
            self.frame_offset,
            self.rmse_m,
            self.max_err_m,
            100.0 * self.coverage,
            self.id_switches,
            self.mean_fragments,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{
        Arena, BlobParams, CameraRig, InsectSpec, Lissajous, ObservationModel, TimingModel,
    };

    /// A two-insect scenario whose insects follow distinct (out-of-phase) paths.
    fn two_insect_scenario() -> Scenario {
        let motion = |phase: [f64; 3]| Lissajous {
            freq_hz: [0.11, 0.13, 0.07],
            phase,
            fill: 0.7,
            maneuver_amp_m: 0.0,
            maneuver_freq_hz: 0.0,
        };
        Scenario {
            seed: 1,
            fps: 100.0,
            arena: Arena {
                min: [-0.5, -0.5, -0.2],
                max: [0.5, 0.5, 0.2],
            },
            cameras: CameraRig {
                count: 4,
                radius_m: 2.0,
                height_m: 0.5,
                focal_length_px: 800.0,
                image_width: 640,
                image_height: 480,
            },
            insects: vec![
                InsectSpec {
                    id: 0,
                    enter_t: 0.0,
                    exit_t: None,
                    motion: motion([0.0, 0.0, 0.0]),
                },
                InsectSpec {
                    id: 1,
                    enter_t: 0.0,
                    exit_t: None,
                    motion: motion([2.0, 0.5, 1.0]),
                },
            ],
            blob: BlobParams::default(),
            bg_warmup_frames: 0,
            timing: TimingModel::default(),
            observation: ObservationModel::default(),
            reported_fps: None,
            calibration_perturbation: Default::default(),
        }
    }

    /// Synthesize `.braidz`-style rows that track each insect exactly, mapping
    /// ground-truth `insect_id` to a Braid `obj_id` via `obj_id_for`. Returns one
    /// row per (present insect, frame) for `frames` consecutive frames starting
    /// at `start_frame`.
    fn rows_from_truth(
        scenario: &Scenario,
        start_frame: i64,
        frames: i64,
        mut obj_id_for: impl FnMut(u32, i64) -> u32,
    ) -> Vec<Row> {
        let world = World::new(scenario.clone());
        let mut out = Vec::new();
        for k in 0..frames {
            let frame = start_frame + k;
            let t = frame as f64 / scenario.fps;
            for ins in world.state_at(t) {
                let c = &ins.pos.coords;
                out.push(Row::new(frame, obj_id_for(ins.id, frame), [c.x, c.y, c.z]));
            }
        }
        out
    }

    #[test]
    fn perfect_tracking_scores_perfectly() {
        let s = two_insect_scenario();
        // Each insect tracked by one stable obj_id (10 and 11).
        let rows = rows_from_truth(&s, 0, 200, |id, _f| 10 + id);
        let score = score_rows(&rows, &s, 0.01, 0);
        assert_eq!(score.num_truth, 2);
        assert_eq!(score.num_tracks, 2);
        assert!(score.rmse_m < 1e-9, "rmse {}", score.rmse_m);
        assert!(score.coverage > 0.999, "coverage {}", score.coverage);
        assert_eq!(score.id_switches, 0);
        assert!(
            (score.mean_fragments - 1.0).abs() < 1e-9,
            "frags {}",
            score.mean_fragments
        );
    }

    #[test]
    fn fragmentation_is_detected() {
        let s = two_insect_scenario();
        // Break each insect's track into a new obj_id every 50 frames: 4 frags
        // per insect over 200 frames, with a switch at each break.
        let rows = rows_from_truth(&s, 0, 200, |id, f| id * 100 + (f / 50) as u32);
        let score = score_rows(&rows, &s, 0.01, 0);
        assert!(
            (score.mean_fragments - 4.0).abs() < 1e-9,
            "frags {}",
            score.mean_fragments
        );
        // 3 switches per insect * 2 insects.
        assert_eq!(score.id_switches, 6);
        assert!(score.coverage > 0.999, "coverage {}", score.coverage);
    }

    #[test]
    fn missed_frames_lower_coverage() {
        let s = two_insect_scenario();
        // Only emit even frames: ~half of object-frames are present-but-unmatched.
        let mut rows = rows_from_truth(&s, 0, 200, |id, _f| 10 + id);
        rows.retain(|r| r.frame % 2 == 0);
        let score = score_rows(&rows, &s, 0.01, 0);
        assert!(
            (0.45..0.6).contains(&score.coverage),
            "coverage {}",
            score.coverage
        );
    }

    #[test]
    fn frame_offset_is_recovered() {
        let s = two_insect_scenario();
        // Rows are labeled with frames shifted +7 from the truth they depict.
        // With max_frame_offset >= 7, the search should recover offset = -7 and
        // still score perfectly.
        let world = World::new(s.clone());
        let mut rows = Vec::new();
        for k in 0..200i64 {
            let truth_frame = k;
            let t = truth_frame as f64 / s.fps;
            for ins in world.state_at(t) {
                let c = &ins.pos.coords;
                rows.push(Row::new(truth_frame + 7, 10 + ins.id, [c.x, c.y, c.z]));
            }
        }
        let score = score_rows(&rows, &s, 0.01, 12);
        assert_eq!(score.frame_offset, -7);
        assert!(score.rmse_m < 1e-9, "rmse {}", score.rmse_m);
    }

    #[test]
    fn unmatched_points_are_not_credited() {
        let s = two_insect_scenario();
        // All points far outside the arena -> nothing within the gate.
        let rows: Vec<Row> = (0..50)
            .map(|f| Row::new(f, 1, [100.0, 100.0, 100.0]))
            .collect();
        let score = score_rows(&rows, &s, 0.05, 0);
        assert_eq!(score.num_matched, 0);
        assert_eq!(score.coverage, 0.0);
        assert_eq!(score.id_switches, 0);
    }
}
