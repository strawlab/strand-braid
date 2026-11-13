// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! core type definitions for braidz files
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use braid_types::{CamNum, TrackingParams};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BraidMetadata {
    // backward-incompatible changes here require a BRAID_SCHEMA bump (see its definition)
    pub schema: u16, // BraidMetadataSchemaTag
    pub git_revision: String,
    /// When the recording started, with the UTC offset of the recording
    /// computer at that time.
    pub original_recording_time: Option<chrono::DateTime<chrono::FixedOffset>>,
    pub save_empty_data2d: bool,
    /// The name of the saving program.
    ///
    /// This is new in schema 3 and the default value
    /// when loading old files is "".
    #[serde(default = "default_saving_program_name")]
    pub saving_program_name: String,
    /// With `PtpSync` triggering, how many seconds the PTP timescale was taken
    /// to run ahead of UTC when the `timestamp` columns were computed from the
    /// cameras' `device_timestamp` values. 37 for a grandmaster on the PTP
    /// (TAI) timescale, 0 for one sending UTC.
    ///
    /// `None` without `PtpSync` triggering, and in files saved before this
    /// field existed; `PtpSync` recordings among those took the offset as 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ptp_utc_offset_secs: Option<i32>,
}

fn default_saving_program_name() -> String {
    "".to_string()
}

/// A summary of a braidz file (or braid directory).
///
/// Even for a many-gigabyte braidz file, this is expected to allocate
/// megabytes, but not gigabytes, of memory and will contain a summary of the
/// data such as filename, calibration, images, reconstruction quality metrics,
/// and so on. It will not load the entire braidz file to memory.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BraidzSummary {
    /// The filename of the braidz file or braid directory.
    pub filename: String,
    /// Number of bytes in a braidz file. (This is meaningless for braid directories.)
    pub filesize: u64,
    pub metadata: BraidMetadata,
    pub cam_info: CamInfo,
    pub expected_fps: f64,
    pub calibration_info: Option<CalibrationSummary>,
    pub data2d_summary: Option<Data2dSummary>,
    pub kalman_estimates_summary: Option<KalmanEstimatesSummary>,
    pub reconstruct_latency_usec_summary: Option<HistogramSummary>,
    pub reprojection_distance_100x_pixels_summary: Option<HistogramSummary>,
}

/// A summary of a multi-camera calibration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CalibrationSummary {
    /// If `Some(n)`, material with refractive index `n` at z<0.
    pub water: Option<f64>,
    /// All the cameras in this system.
    pub cameras: Vec<CameraSummary>,
}

impl From<CalibrationInfo> for CalibrationSummary {
    fn from(orig: CalibrationInfo) -> Self {
        Self {
            water: orig.water,
            cameras: orig
                .cameras
                .cams_by_name()
                .iter()
                .map(|(name, cam)| CameraSummary::new(name, cam))
                .collect(),
        }
    }
}

/// A summary of a camera calibration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CameraSummary {
    pub name: String,
    pub camera_center: (f64, f64, f64),
    pub fx: f64,
    pub fy: f64,
    pub distortion: Option<Vec<f64>>,
}

impl CameraSummary {
    pub fn new(name: &str, cam: &braid_mvg::Camera<f64>) -> Self {
        let cc = cam.extrinsics().camcenter();
        let fx = cam.intrinsics().fx();
        let fy = cam.intrinsics().fy();
        let d = &cam.intrinsics().distortion;
        let distortion = if d.is_linear() {
            None
        } else {
            Some(d.opencv_vec().iter().map(Clone::clone).collect())
        };
        Self {
            name: name.into(),
            camera_center: (cc[0], cc[1], cc[2]),
            distortion,
            fx,
            fy,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct CamInfo {
    pub camn2camid: BTreeMap<CamNum, String>,
    pub camid2camn: BTreeMap<String, CamNum>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HistogramSummary {
    /// the number of points in the histogram
    pub len: u64,
    pub mean: f64,
    pub min: u64,
    pub max: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CalibrationInfo {
    /// If `Some(n)`, material with refractive index `n` at z<0.
    pub water: Option<f64>,
    /// All the cameras in this system.
    pub cameras: braid_mvg::MultiCameraSystem<f64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Data2dSummary {
    pub num_cameras_with_data: u16,
    pub num_rows: u64,
    pub frame_limits: [u64; 2],
    pub time_limits: [chrono::DateTime<chrono::Utc>; 2],
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KalmanEstimatesSummary {
    pub num_trajectories: u32,
    pub x_limits: [f64; 2],
    pub y_limits: [f64; 2],
    pub z_limits: [f64; 2],
    pub num_rows: u64,
    pub tracking_parameters: TrackingParams,
    /// The sum of total distance in all trajectories.
    pub total_distance: f64,
}

pub fn camera_name_from_filename<P: AsRef<std::path::Path>>(
    full_path: P,
) -> (String, Option<String>) {
    let filename = full_path
        .as_ref()
        .file_name()
        .unwrap()
        .to_os_string()
        .to_str()
        .unwrap()
        .to_string();

    const MOVIE_REGEXP: &str = r"^movie\d{8}_\d{6}(?:.?\d*)_(.*).(?:mp4|mkv|fmf|h264|fmf\.gz)$";
    let movie_re = regex::Regex::new(MOVIE_REGEXP).unwrap();
    let cam_from_filename = movie_re.captures(&filename).map(|caps| {
        // get the raw camera name
        caps.get(1).unwrap().as_str().to_string()
    });
    (filename, cam_from_filename)
}

#[test]
fn test_cam_from_filename() {
    // prior to adding subseconds
    let fname1 = "dir1/movie20211108_084523_Basler-22445994.mp4";
    let (_, cam) = camera_name_from_filename(fname1);
    assert_eq!(cam, Some("Basler-22445994".to_string()));

    // with subseconds
    let fname2 = "movie20240302_144852.000002145_Basler-40454395.mp4";
    let (_, cam) = camera_name_from_filename(fname2);
    assert_eq!(cam, Some("Basler-40454395".to_string()));
}

#[cfg(test)]
mod tests {
    use super::BraidMetadata;

    #[test]
    fn recording_time_keeps_utc_offset() {
        // From a real file recorded at UTC+01:00. The offset must survive a
        // round trip regardless of the timezone of the computer reading it.
        let yaml = "schema: 1
git_revision: f801a2d7ae24e7138a463ea8be9dd6c5d5919254
original_recording_time: \"2020-11-04T17:41:58.039288607+01:00\"
save_empty_data2d: true
";
        let md: BraidMetadata = serde_yaml::from_str(yaml).unwrap();
        let t = md.original_recording_time.unwrap();
        assert_eq!(t.offset().local_minus_utc(), 3600);
        assert_eq!(t.to_rfc3339(), "2020-11-04T17:41:58.039288607+01:00");

        let written = serde_yaml::to_string(&md).unwrap();
        assert!(
            written.contains("original_recording_time: 2020-11-04T17:41:58.039288607+01:00"),
            "{written}"
        );
    }
}
