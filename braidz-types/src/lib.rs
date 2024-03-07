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
    /// The name of the saving program.
    ///
    /// This is new in schema 3 and the default value
    /// when loading old files is "".
    #[serde(default = "default_saving_program_name")]
    pub saving_program_name: String,
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
}

impl CameraSummary {
    pub fn new(name: &str, cam: &mvg::Camera<f64>) -> Self {
        let cc = cam.extrinsics().camcenter();
        Self {
            name: name.into(),
            camera_center: (cc[0], cc[1], cc[2]),
        }
    }
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

    const MOVIE_REGEXP: &str = r"^movie\d{8}_\d{6}.?\d*_(.*).mp4$";
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
