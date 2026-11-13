// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use anyhow::Context;
use std::{
    collections::BTreeMap,
    io::{Read, Seek, Write},
};
use tabled::{Table, Tabled, settings::style::Style};

use braid_types::{Data2dDistortedRow, DataAssocRow, KalmanEstimatesRow};

#[derive(Tabled, Default)]
struct FrameByFrameRow {
    /// Synchronized frame number.
    frame: u64,
    #[tabled(display = "display_data2d_distorted_rows")]
    data2d_distorted_rows: Vec<DisplayData2dDistortedRow>,
    #[tabled(display = "display_data_association")]
    data_association: Vec<DisplayDataAssocRow>,
    #[tabled(display = "display_kalman_estimates")]
    kalman_estimates: Vec<DisplayKalmanEstimates>,
}

#[derive(Tabled, Clone)]
struct DisplayKalmanEstimates {
    obj_id: u32,
    x: String,
    y: String,
    z: String,
}

impl From<&KalmanEstimatesRow> for DisplayKalmanEstimates {
    fn from(orig: &KalmanEstimatesRow) -> Self {
        Self {
            obj_id: orig.obj_id,
            x: format!("{:.04}", orig.x),
            y: format!("{:.04}", orig.y),
            z: format!("{:.04}", orig.z),
        }
    }
}

#[derive(Tabled, Clone)]
struct DisplayDataAssocRow {
    obj_id: u32,
    cam_num: u8,
    pt_idx: u8,
}

impl From<&DataAssocRow> for DisplayDataAssocRow {
    fn from(orig: &DataAssocRow) -> Self {
        Self {
            obj_id: orig.obj_id,
            cam_num: orig.cam_num.0,
            pt_idx: orig.pt_idx,
        }
    }
}

#[derive(Tabled, Clone)]
struct DisplayData2dDistortedRow {
    camn: u8,
    /// Time of day of the trigger timestamp, in the display timezone.
    #[tabled(display = "display_time")]
    timestamp: Option<chrono::NaiveTime>,
    /// Time from the trigger timestamp until the camera frame was received.
    #[tabled(rename = "cam latency (msec)", display = "display_option")]
    cam_latency: Option<i64>,
    x: String,
    y: String,
}

impl DisplayData2dDistortedRow {
    fn new<Tz: chrono::TimeZone>(orig: Data2dDistortedRow, tz: &Tz) -> Self {
        let trigger: Option<chrono::DateTime<chrono::Utc>> =
            orig.timestamp.as_ref().map(Into::into);
        let cam_received: chrono::DateTime<chrono::Utc> = (&orig.cam_received_timestamp).into();
        Self {
            camn: orig.camn.0,
            timestamp: trigger.map(|t| t.with_timezone(tz).time()),
            cam_latency: trigger.map(|t| cam_received.signed_duration_since(t).num_milliseconds()),
            x: format!("{:.1}", orig.x),
            y: format!("{:.1}", orig.y),
        }
    }
}

fn display_kalman_estimates(d: &[DisplayKalmanEstimates]) -> String {
    if d.is_empty() {
        return "".to_string();
    }
    // sort by obj_id for reproducibility
    let mut d = d.to_vec();
    d.sort_by_key(|row| row.obj_id);
    Table::new(d).with(Style::empty()).to_string()
}

fn display_data_association(d: &[DisplayDataAssocRow]) -> String {
    if d.is_empty() {
        return "".to_string();
    }
    // sort by cam_num for reproducibility
    let mut d = d.to_vec();
    d.sort_by_key(|row| row.cam_num);
    Table::new(d).with(Style::empty()).to_string()
}

fn display_data2d_distorted_rows(d: &[DisplayData2dDistortedRow]) -> String {
    if d.is_empty() {
        return "".to_string();
    }
    // sort by camn for reproducibility
    let mut d = d.to_vec();
    d.sort_by_key(|row| row.camn);
    Table::new(d).with(Style::empty()).to_string()
}

fn display_time(timestamp: &Option<chrono::NaiveTime>) -> String {
    timestamp.map(|t| t.to_string()).unwrap_or_default()
}

fn display_option<T: std::fmt::Display>(opt: &Option<T>) -> String {
    opt.as_ref().map(|v| v.to_string()).unwrap_or_default()
}

/// The row for `frame`, created empty if needed.
fn row_for(rows: &mut BTreeMap<u64, FrameByFrameRow>, frame: u64) -> &mut FrameByFrameRow {
    rows.entry(frame).or_insert_with(|| FrameByFrameRow {
        frame,
        ..Default::default()
    })
}

/// Collect all rows of the three tables, keyed by synchronized frame number.
///
/// 2D rows without a detection (`x` is NaN) are skipped.
fn group_by_frame<Tz, E>(
    data_association: &[DataAssocRow],
    data2d_distorted: impl IntoIterator<Item = Result<Data2dDistortedRow, E>>,
    kalman_estimates: &[KalmanEstimatesRow],
    tz: &Tz,
) -> anyhow::Result<BTreeMap<u64, FrameByFrameRow>>
where
    Tz: chrono::TimeZone,
    E: std::error::Error + Send + Sync + 'static,
{
    let mut rows = BTreeMap::<u64, FrameByFrameRow>::new();

    for row in data_association {
        row_for(&mut rows, row.frame.0)
            .data_association
            .push(row.into());
    }

    for row in data2d_distorted {
        let row = row?;
        if !row.x.is_nan() {
            let frame = u64::try_from(row.frame)
                .with_context(|| format!("negative frame number {}", row.frame))?;
            row_for(&mut rows, frame)
                .data2d_distorted_rows
                .push(DisplayData2dDistortedRow::new(row, tz));
        }
    }

    for row in kalman_estimates {
        row_for(&mut rows, row.frame.0)
            .kalman_estimates
            .push(row.into());
    }

    Ok(rows)
}

/// Write a table with one row per frame, showing all data from that frame.
pub(crate) fn write_frame_by_frame<R, Tz, W>(
    archive: &mut braidz_parser::BraidzArchive<R>,
    tz: &Tz,
    out: &mut W,
) -> anyhow::Result<()>
where
    R: Read + Seek,
    Tz: chrono::TimeZone,
    W: Write,
{
    // Take the owned tables out first: iterating `data2d_distorted` borrows
    // the archive mutably.
    let data_association = archive.data_association.take().unwrap_or_default();
    let kalman_estimates = archive.kalman_estimates_table.take().unwrap_or_default();

    let rows = group_by_frame(
        &data_association,
        archive.iter_data2d_distorted()?,
        &kalman_estimates,
        tz,
    )?;

    let mut table = Table::new(rows.into_values());
    writeln!(out, "{}", table.with(Style::modern()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, TimeZone, Utc};

    use braid_types::{CamNum, FlydraFloatTimestampLocal, SyncFno};

    fn utc(h: u32, m: u32, s: u32, ms: u32) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 1, 2, h, m, s).unwrap()
            + chrono::Duration::milliseconds(ms.into())
    }

    fn data2d(
        camn: u8,
        frame: i64,
        trigger: Option<chrono::DateTime<Utc>>,
        received: chrono::DateTime<Utc>,
        x: f64,
    ) -> Data2dDistortedRow {
        Data2dDistortedRow {
            camn: CamNum(camn),
            frame,
            timestamp: trigger.as_ref().map(FlydraFloatTimestampLocal::from_dt),
            cam_received_timestamp: FlydraFloatTimestampLocal::from_dt(&received),
            device_timestamp: None,
            block_id: None,
            x,
            y: 2.0,
            area: 1.0,
            slope: 0.0,
            eccentricity: 0.0,
            frame_pt_idx: 0,
            cur_val: 0,
            mean_val: 0.0,
            sumsqf_val: 0.0,
        }
    }

    fn data_assoc(obj_id: u32, frame: u64, cam_num: u8) -> DataAssocRow {
        DataAssocRow {
            obj_id,
            frame: SyncFno(frame),
            cam_num: CamNum(cam_num),
            pt_idx: 0,
        }
    }

    fn kalman(obj_id: u32, frame: u64) -> KalmanEstimatesRow {
        KalmanEstimatesRow {
            obj_id,
            frame: SyncFno(frame),
            timestamp: None,
            x: 0.1,
            y: 0.2,
            z: 0.3,
            xvel: 0.0,
            yvel: 0.0,
            zvel: 0.0,
            P00: 0.0,
            P01: 0.0,
            P02: 0.0,
            P11: 0.0,
            P12: 0.0,
            P22: 0.0,
            P33: 0.0,
            P44: 0.0,
            P55: 0.0,
        }
    }

    /// Order in which `needles` first appear in `haystack`.
    fn positions(haystack: &str, needles: &[&str]) -> Vec<usize> {
        needles
            .iter()
            .map(|n| haystack.find(n).unwrap_or_else(|| panic!("{n:?} missing")))
            .collect()
    }

    fn is_increasing(v: &[usize]) -> bool {
        v.windows(2).all(|w| w[0] < w[1])
    }

    #[test]
    fn latency_is_received_minus_trigger() {
        let trigger = utc(3, 4, 5, 500);
        let row = data2d(
            0,
            1,
            Some(trigger),
            trigger + chrono::Duration::milliseconds(19),
            1.0,
        );
        let d = DisplayData2dDistortedRow::new(row, &Utc);
        assert_eq!(d.cam_latency, Some(19));
        assert_eq!(display_time(&d.timestamp), "03:04:05.500");
    }

    #[test]
    fn time_of_day_uses_given_timezone() {
        let trigger = utc(23, 30, 0, 0);
        let plus2 = FixedOffset::east_opt(2 * 3600).unwrap();
        let row = data2d(
            0,
            1,
            Some(trigger),
            trigger + chrono::Duration::milliseconds(5),
            1.0,
        );
        let d = DisplayData2dDistortedRow::new(row, &plus2);
        assert_eq!(display_time(&d.timestamp), "01:30:00");
        // The latency does not depend on the timezone.
        assert_eq!(d.cam_latency, Some(5));
    }

    #[test]
    fn missing_trigger_timestamp_gives_empty_cells() {
        let row = data2d(0, 1, None, utc(3, 4, 5, 0), 1.0);
        let d = DisplayData2dDistortedRow::new(row, &Utc);
        assert_eq!(display_time(&d.timestamp), "");
        assert_eq!(display_option(&d.cam_latency), "");
    }

    #[test]
    fn display_option_shows_value() {
        assert_eq!(display_option(&Some(-3)), "-3");
        assert_eq!(display_option::<i64>(&None), "");
    }

    #[test]
    fn empty_cells_are_empty_strings() {
        assert_eq!(display_data2d_distorted_rows(&[]), "");
        assert_eq!(display_data_association(&[]), "");
        assert_eq!(display_kalman_estimates(&[]), "");
    }

    #[test]
    fn data2d_rows_sorted_by_camn() {
        let t = utc(3, 4, 5, 0);
        let rows: Vec<_> = [(3, 30.0), (0, 0.5), (2, 20.0)]
            .into_iter()
            .map(|(camn, x)| DisplayData2dDistortedRow::new(data2d(camn, 1, None, t, x), &Utc))
            .collect();
        let s = display_data2d_distorted_rows(&rows);
        assert!(
            is_increasing(&positions(&s, &["0.5", "20.0", "30.0"])),
            "{s}"
        );
    }

    #[test]
    fn data_association_sorted_by_cam_num() {
        let rows: Vec<DisplayDataAssocRow> = [(7, 3), (8, 0), (9, 2)]
            .into_iter()
            .map(|(obj_id, cam_num)| (&data_assoc(obj_id, 1, cam_num)).into())
            .collect();
        let s = display_data_association(&rows);
        // obj_ids in order of increasing cam_num
        assert!(is_increasing(&positions(&s, &["8", "9", "7"])), "{s}");
    }

    #[test]
    fn kalman_estimates_sorted_by_obj_id() {
        let rows: Vec<DisplayKalmanEstimates> = [33, 11, 22]
            .into_iter()
            .map(|obj_id| (&kalman(obj_id, 1)).into())
            .collect();
        let s = display_kalman_estimates(&rows);
        assert!(is_increasing(&positions(&s, &["11", "22", "33"])), "{s}");
    }

    #[test]
    fn grouping_by_frame_skips_nan_detections() {
        let t = utc(3, 4, 5, 0);
        let data2d_rows = vec![
            Ok::<_, std::io::Error>(data2d(0, 10, None, t, 1.0)),
            Ok(data2d(1, 10, None, t, f64::NAN)),
            // A frame with only an empty detection gets no row at all.
            Ok(data2d(0, 11, None, t, f64::NAN)),
            Ok(data2d(1, 12, None, t, 2.0)),
        ];
        let rows = group_by_frame(
            &[data_assoc(5, 12, 1)],
            data2d_rows,
            &[kalman(5, 12), kalman(5, 13)],
            &Utc,
        )
        .unwrap();

        assert_eq!(rows.keys().copied().collect::<Vec<_>>(), [10, 12, 13]);
        assert!(rows.iter().all(|(frame, row)| row.frame == *frame));
        assert_eq!(rows[&10].data2d_distorted_rows.len(), 1);
        assert_eq!(rows[&10].data2d_distorted_rows[0].camn, 0);
        assert_eq!(rows[&12].data2d_distorted_rows.len(), 1);
        assert_eq!(rows[&12].data_association.len(), 1);
        assert_eq!(rows[&12].kalman_estimates.len(), 1);
        assert!(rows[&13].data2d_distorted_rows.is_empty());
        assert_eq!(rows[&13].kalman_estimates.len(), 1);
    }

    #[test]
    fn negative_frame_is_an_error() {
        let t = utc(3, 4, 5, 0);
        let data2d_rows = vec![Ok::<_, std::io::Error>(data2d(0, -1, None, t, 1.0))];
        let err = group_by_frame(&[], data2d_rows, &[], &Utc)
            .err()
            .expect("expected an error");
        assert!(
            err.to_string().contains("negative frame number -1"),
            "{err}"
        );
    }

    #[test]
    fn row_read_errors_are_propagated() {
        let data2d_rows = vec![Err(std::io::Error::other("bad row"))];
        let err = group_by_frame(&[], data2d_rows, &[], &Utc)
            .err()
            .expect("expected an error");
        assert!(err.to_string().contains("bad row"), "{err}");
    }
}
