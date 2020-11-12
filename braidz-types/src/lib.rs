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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BraidzSummary {
    pub filename: String,
    pub filesize: u64,
    pub metadata: BraidMetadata,
    pub expected_fps: f64,
    pub calibration_info: Option<CalibrationInfo>,
    pub data2d_summary: Option<Data2dSummary>,
    pub kalman_estimates_summary: Option<KalmanEstimatesSummary>,
    pub reconstruct_latency_usec_summary: Option<HistogramSummary>,
    pub reprojection_distance_100x_pixels_summary: Option<HistogramSummary>,
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
}
