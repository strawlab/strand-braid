use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use flydra_types::{
    CamInfoRow, CamNum, Data2dDistortedRow, KalmanEstimatesRow, TrackingParams,
};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BraidMetadata {
    // changes to this struct should update BraidMetadataSchemaTag
    pub schema: u16, // BraidMetadataSchemaTag
    pub git_revision: String,
    pub original_recording_time: Option<chrono::DateTime<chrono::Local>>,
    pub save_empty_data2d: bool,
    // pub saving_program_name: String, // TODO: add when we bump BraidMetadataSchemaTag
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
    pub calibration_info: Option<CalibrationInfo>,
    pub data2d_summary: Option<Data2dSummary>,
    pub kalman_estimates_summary: Option<KalmanEstimatesSummary>,
    pub reconstruct_latency_usec_summary: Option<HistogramSummary>,
    pub reprojection_distance_100x_pixels_summary: Option<HistogramSummary>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
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
    pub cameras: mvg::MultiCameraSystem<f64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Data2dSummary {
    pub num_cameras_with_data: u16,
    pub num_rows: u64,
    pub frame_limits: [u64; 2],
    pub time_limits: [chrono::DateTime<chrono::Local>; 2],
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
