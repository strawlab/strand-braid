#![cfg_attr(feature = "backtrace", feature(backtrace))]

use log::{debug, error, info, trace};

use serde::{Deserialize, Serialize};

use std::{
    collections::{BTreeMap, BTreeSet},
    f64,
    io::{Read, Seek, Write},
    sync::Arc,
};

use hdrhistogram::{
    serialization::{interval_log, V2DeflateSerializer},
    Counter, Histogram,
};

use libflate::finish::AutoFinishUnchecked;
use libflate::gzip::Encoder;

use nalgebra::core::dimension::{U2, U3, U6};
use nalgebra::{MatrixMN, MatrixN, Point3, Vector2, Vector6, VectorN};

use nalgebra::allocator::Allocator;
use nalgebra::core::dimension::DimMin;
use nalgebra::{DefaultAllocator, RealField};

use adskalman::ObservationModelLinear;
#[allow(unused_imports)]
use mvg::{DistortedPixel, PointWorldFrame, PointWorldFrameWithSumReprojError};

pub use braidz_types::BraidMetadata;

use crossbeam_ok::CrossbeamOk;
use flydra_types::{
    CamInfoRow, CamNum, ConnectedCameraSyncState, FlydraFloatTimestampLocal, HostClock,
    KalmanEstimatesRow, RosCamName, SyncFno, TextlogRow, TriggerClockInfoRow, Triggerbox,
};
pub use flydra_types::{Data2dDistortedRow, Data2dDistortedRowF32};

use withkey::WithKey;

mod connected_camera_manager;
pub use connected_camera_manager::{ConnectedCamCallback, ConnectedCamerasManager};

mod bundled_data;
mod contiguous_stream;
mod frame_bundler;

pub use flydra_types::{
    BRAID_METADATA_YML_FNAME, BRAID_SCHEMA, CALIBRATION_XML_FNAME, CAM_INFO_CSV_FNAME,
    DATA2D_DISTORTED_CSV_FNAME, DATA_ASSOCIATE_FNAME, EXPERIMENT_INFO, IMAGES_DIRNAME,
    KALMAN_ESTIMATES_FNAME, README_WITH_EXT, TEXTLOG, TRIGGER_CLOCK_INFO,
};
use flydra_types::{RECONSTRUCT_LATENCY_LOG_FNAME, REPROJECTION_DIST_LOG_FNAME};

#[cfg(feature = "full-3d")]
mod new_object_test;

#[cfg(feature = "flat-3d")]
mod new_object_test_2d;
#[cfg(feature = "flat-3d")]
use new_object_test_2d as new_object_test;

mod tracking_core;

mod zip_dir;

mod offline_kalmanize;
pub use crate::offline_kalmanize::{kalmanize, KalmanizeOptions};

mod model_server;
pub use crate::model_server::{GetsUpdates, ModelServer, SendKalmanEstimatesRow, SendType};

use crate::contiguous_stream::make_contiguous;
use crate::frame_bundler::bundle_frames;
pub use crate::frame_bundler::StreamItem;

pub type MyFloat = flydra_types::MyFloat;

pub type Result<M> = std::result::Result<M, Error>;

#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
}

impl Error {
    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }
}

impl std::error::Error for Error {}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug)]
pub enum ErrorKind {
    Failure(failure::Error),
    FlydraTypes(flydra_types::FlydraTypesError),
    Mvg(mvg::MvgError),
    Io(std::io::Error),
    Csv(csv::Error),
    GetTimezone(iana_time_zone::GetTimezoneError),
    SerdeJson(serde_json::Error),
    SerdeYaml(serde_yaml::Error),
    FuturesSendError(futures::channel::mpsc::SendError),
    TomlSerError(toml::ser::Error),
    TomlDeError(toml::de::Error),
    InvalidHypothesisTestingParameters,
    ZipDir(zip_or_dir::Error),
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Error {
        Error { kind }
    }
}

impl From<flydra_types::FlydraTypesError> for Error {
    fn from(orig: flydra_types::FlydraTypesError) -> Error {
        Error {
            kind: ErrorKind::FlydraTypes(orig),
        }
    }
}

impl From<failure::Error> for Error {
    fn from(orig: failure::Error) -> Error {
        Error {
            kind: ErrorKind::Failure(orig),
        }
    }
}

impl From<mvg::MvgError> for Error {
    fn from(orig: mvg::MvgError) -> Error {
        Error {
            kind: ErrorKind::Mvg(orig),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(orig: std::io::Error) -> Error {
        Error {
            kind: ErrorKind::Io(orig),
        }
    }
}

impl From<csv::Error> for Error {
    fn from(orig: csv::Error) -> Error {
        Error {
            kind: ErrorKind::Csv(orig),
        }
    }
}

impl From<iana_time_zone::GetTimezoneError> for Error {
    fn from(orig: iana_time_zone::GetTimezoneError) -> Error {
        Error {
            kind: ErrorKind::GetTimezone(orig),
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(orig: serde_json::Error) -> Error {
        Error {
            kind: ErrorKind::SerdeJson(orig),
        }
    }
}

impl From<serde_yaml::Error> for Error {
    fn from(orig: serde_yaml::Error) -> Error {
        Error {
            kind: ErrorKind::SerdeYaml(orig),
        }
    }
}

impl From<futures::channel::mpsc::SendError> for Error {
    fn from(orig: futures::channel::mpsc::SendError) -> Error {
        Error {
            kind: ErrorKind::FuturesSendError(orig),
        }
    }
}

impl From<toml::ser::Error> for Error {
    fn from(orig: toml::ser::Error) -> Error {
        Error {
            kind: ErrorKind::TomlSerError(orig),
        }
    }
}

impl From<toml::de::Error> for Error {
    fn from(orig: toml::de::Error) -> Error {
        Error {
            kind: ErrorKind::TomlDeError(orig),
        }
    }
}

impl From<zip_or_dir::Error> for Error {
    fn from(orig: zip_or_dir::Error) -> Error {
        Error {
            kind: ErrorKind::ZipDir(orig),
        }
    }
}

pub(crate) fn generate_observation_model<R>(
    cam: &flydra_mvg::MultiCamera<R>,
    state: &Vector6<R>,
    ekf_observation_covariance_pixels: f32,
) -> Result<CameraObservationModel<R>>
where
    R: RealField + Default + serde::Serialize,
{
    let pt3d: PointWorldFrame<R> = to_world_point(state);
    // Deals with water if needed.
    let mat2x3 = cam.linearize_numerically_at(&pt3d, nalgebra::convert(0.001))?;
    Ok(CameraObservationModel::new(
        cam.clone(),
        mat2x3,
        ekf_observation_covariance_pixels,
    ))
}

// We use a 6 dimensional state vector:
// [x,y,z,xvel,yvel,zvel].
#[derive(Debug)]
struct CameraObservationModel<R>
where
    R: RealField + Default + serde::Serialize,
{
    cam: flydra_mvg::MultiCamera<R>,
    observation_matrix: MatrixMN<R, U2, U6>,
    observation_matrix_transpose: MatrixMN<R, U6, U2>,
    observation_noise_covariance: MatrixN<R, U2>,
}

impl<R> CameraObservationModel<R>
where
    R: RealField + Default + serde::Serialize,
{
    fn new(
        cam: flydra_mvg::MultiCamera<R>,
        a: MatrixMN<R, U2, U3>,
        ekf_observation_covariance_pixels: f32,
    ) -> Self {
        let observation_matrix = {
            let mut o = MatrixMN::<R, U2, U6>::zeros();
            o.fixed_columns_mut::<U3>(0).copy_from(&a);
            o
        };
        let observation_matrix_transpose = observation_matrix.transpose();

        let r = nalgebra::convert(ekf_observation_covariance_pixels as f64);
        let zero = nalgebra::convert(0.0);
        let observation_noise_covariance = MatrixN::<R, U2>::new(r, zero, zero, r);
        Self {
            cam,
            observation_matrix,
            observation_matrix_transpose,
            observation_noise_covariance,
        }
    }
}

impl<R> ObservationModelLinear<R, U6, U2> for CameraObservationModel<R>
where
    DefaultAllocator: Allocator<R, U6, U6>,
    DefaultAllocator: Allocator<R, U6>,
    DefaultAllocator: Allocator<R, U2, U6>,
    DefaultAllocator: Allocator<R, U6, U2>,
    DefaultAllocator: Allocator<R, U2, U2>,
    DefaultAllocator: Allocator<R, U2>,
    DefaultAllocator: Allocator<(usize, usize), U2>,
    U2: DimMin<U2, Output = U2>,
    R: RealField + Default + serde::Serialize,
{
    fn observation_matrix(&self) -> &MatrixMN<R, U2, U6> {
        &self.observation_matrix
    }
    fn observation_matrix_transpose(&self) -> &MatrixMN<R, U6, U2> {
        &self.observation_matrix_transpose
    }
    fn observation_noise_covariance(&self) -> &MatrixN<R, U2> {
        &self.observation_noise_covariance
    }
    fn evaluate(&self, state: &VectorN<R, U6>) -> VectorN<R, U2> {
        // TODO: update to handle water here. See tag "laksdfjasl".
        let pt = to_world_point(&state);
        let undistored = self.cam.project_3d_to_pixel(&pt);
        Vector2::<R>::new(undistored.coords[0], undistored.coords[1])
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DataAssocRow {
    // changes to this struct should update BraidMetadataSchemaTag
    pub obj_id: u32,
    pub frame: SyncFno,
    pub cam_num: CamNum,
    pub pt_idx: u8,
}
impl WithKey<SyncFno> for DataAssocRow {
    fn key(&self) -> SyncFno {
        self.frame
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExperimentInfoRow {
    // changes to this struct should update BraidMetadataSchemaTag
    pub uuid: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NumberedRawUdpPoint {
    /// the original index of the detected point
    pub idx: u8,
    /// the actuall detected point
    pub pt: flydra_types::FlydraRawUdpPoint,
}

#[cfg(feature = "full-3d")]
pub type TrackingParams = flydra_types::TrackingParamsInner3D;

#[cfg(feature = "flat-3d")]
pub type TrackingParams = flydra_types::TrackingParamsInnerFlat3D;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct TrackingParamsSaver {
    tracking_params: flydra_types::TrackingParams,
    git_revision: String,
}

trait TimeDelta {
    type CompareWith;
    fn duration_since(&self, other: &Self::CompareWith, frame_dt: f64) -> f64;
}

#[derive(Clone, Debug, Serialize)]
struct SyncedFrameCount {
    frame: SyncFno,
}

impl TimeDelta for SyncedFrameCount {
    type CompareWith = SyncedFrameCount;
    fn duration_since(&self, other: &SyncedFrameCount, dt: f64) -> f64 {
        let df = self.frame.0 as i64 - other.frame.0 as i64;
        df as f64 * dt
    }
}

impl std::cmp::PartialEq for SyncedFrameCount {
    fn eq(&self, other: &SyncedFrameCount) -> bool {
        self.frame.eq(&other.frame)
    }
}

impl std::cmp::PartialOrd for SyncedFrameCount {
    fn partial_cmp(&self, other: &SyncedFrameCount) -> Option<std::cmp::Ordering> {
        self.frame.partial_cmp(&other.frame)
    }
}

#[derive(Clone, Debug)]
struct TimestampSyncSource {
    stamp: FlydraFloatTimestampLocal<Triggerbox>,
}

impl TimeDelta for TimestampSyncSource {
    type CompareWith = TimestampSyncSource;
    fn duration_since(&self, other: &TimestampSyncSource, _dt: f64) -> f64 {
        self.stamp.as_f64() - other.stamp.as_f64()
    }
}

impl std::cmp::PartialEq for TimestampSyncSource {
    fn eq(&self, other: &TimestampSyncSource) -> bool {
        self.stamp.as_f64().eq(&other.stamp.as_f64())
    }
}

impl std::cmp::PartialOrd for TimestampSyncSource {
    fn partial_cmp(&self, other: &TimestampSyncSource) -> Option<std::cmp::Ordering> {
        self.stamp.as_f64().partial_cmp(&other.stamp.as_f64())
    }
}

#[derive(Debug, Clone)]
pub struct TimeDataPassthrough {
    frame: SyncFno,
    timestamp: Option<FlydraFloatTimestampLocal<Triggerbox>>,
}

impl TimeDataPassthrough {
    #[inline]
    pub fn new(frame: SyncFno, timestamp: &Option<FlydraFloatTimestampLocal<Triggerbox>>) -> Self {
        let timestamp = timestamp.clone();
        Self { frame, timestamp }
    }
    /// The acquisition frame (synchronized, not raw from camera)
    #[inline]
    pub fn synced_frame(&self) -> SyncFno {
        self.frame
    }
    /// The acquisition timestamp (synchronized, not raw from camera)
    ///
    /// If there is no clock model, returns None.
    #[inline]
    pub fn trigger_timestamp(&self) -> Option<FlydraFloatTimestampLocal<Triggerbox>> {
        self.timestamp.clone()
    }
}

impl std::cmp::PartialEq for TimeDataPassthrough {
    fn eq(&self, other: &TimeDataPassthrough) -> bool {
        let result = self.frame.eq(&other.frame);
        if result {
            if self.timestamp.is_none() {
                if other.timestamp.is_none() {
                    return true;
                } else {
                    return false;
                }
            }

            let ts1 = self.timestamp.clone().unwrap();
            let ts2 = other.timestamp.clone().unwrap();

            // Not sure why the timestamps may be slightly out of sync. Perhaps
            // the time model updated in the middle of processing a
            // frame from multiple cameras? In that case, the timestamps
            // could indeed be slightly different from each other from the same
            // frame.
            if (ts1.as_f64() - ts2.as_f64()).abs() > 0.001 {
                error!(
                    "for frame {}: multiple timestamps {} and {} not within 1 ms",
                    self.frame,
                    ts1.as_f64(),
                    ts2.as_f64()
                );
            }
        }
        result
    }
}

fn to_world_point<R: nalgebra::RealField>(vec6: &Vector6<R>) -> PointWorldFrame<R> {
    // TODO could we just borrow a pointer to data instead of copying it?
    PointWorldFrame {
        coords: Point3::new(vec6.x, vec6.y, vec6.z),
    }
}

/// image processing results from a single camera
#[derive(Clone, Debug, PartialEq)]
pub struct FrameData {
    /// camera name as kept by mvg::MultiCamSystem
    pub cam_name: RosCamName,
    /// camera identification number
    pub cam_num: CamNum,
    /// framenumber after synchronization
    pub synced_frame: SyncFno,
    /// time at which hardware trigger fired
    pub trigger_timestamp: Option<FlydraFloatTimestampLocal<Triggerbox>>,
    /// time at which camnode got frame
    pub cam_received_timestamp: FlydraFloatTimestampLocal<HostClock>,
    time_delta: SyncedFrameCount,
    tdpt: TimeDataPassthrough,
}

impl FrameData {
    #[inline]
    pub fn new(
        cam_name: RosCamName,
        cam_num: CamNum,
        synced_frame: SyncFno,
        trigger_timestamp: Option<FlydraFloatTimestampLocal<Triggerbox>>,
        cam_received_timestamp: FlydraFloatTimestampLocal<HostClock>,
    ) -> Self {
        let time_delta = Self::make_time_delta(synced_frame, trigger_timestamp.clone());
        let tdpt = TimeDataPassthrough::new(synced_frame, &trigger_timestamp);
        Self {
            cam_name,
            cam_num,
            synced_frame,
            trigger_timestamp,
            cam_received_timestamp,
            time_delta,
            tdpt,
        }
    }

    #[inline]
    fn make_time_delta(
        synced_frame: SyncFno,
        _trigger_timestamp: Option<FlydraFloatTimestampLocal<Triggerbox>>,
    ) -> SyncedFrameCount {
        SyncedFrameCount {
            frame: synced_frame,
        }
    }
}

/// image processing results from a single camera on a single frame
///
/// This is essentially a fixed up version of the data received
/// from the flydra UDP packet on each frame from each camera.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameDataAndPoints {
    pub frame_data: FrameData,
    pub points: Vec<NumberedRawUdpPoint>,
}

fn safe_u8(val: usize) -> u8 {
    if val > u8::max_value() as usize {
        panic!("value out of range");
    }
    val as u8
}

fn convert_to_save(frame_data: &FrameData, input: &NumberedRawUdpPoint) -> Data2dDistortedRowF32 {
    let (slope, eccentricity) = match input.pt.maybe_slope_eccentricty {
        None => (std::f32::NAN, std::f32::NAN),
        Some((s, e)) => (s as f32, e as f32),
    };

    Data2dDistortedRowF32 {
        camn: frame_data.cam_num,
        frame: frame_data.synced_frame.0 as i64,
        timestamp: frame_data.trigger_timestamp.clone(),
        cam_received_timestamp: frame_data.cam_received_timestamp.clone(),
        x: input.pt.x0_abs as f32,
        y: input.pt.y0_abs as f32,
        area: input.pt.area as f32,
        slope,
        eccentricity,
        frame_pt_idx: input.idx,
        cur_val: input.pt.cur_val,
        mean_val: input.pt.mean_val as f32,
        sumsqf_val: input.pt.sumsqf_val as f32,
    }
}

fn convert_empty_to_save(frame_data: &FrameData) -> Data2dDistortedRowF32 {
    Data2dDistortedRowF32 {
        camn: frame_data.cam_num,
        frame: frame_data.synced_frame.0 as i64,
        timestamp: frame_data.trigger_timestamp.clone(),
        cam_received_timestamp: frame_data.cam_received_timestamp.clone(),
        x: std::f32::NAN,
        y: std::f32::NAN,
        area: std::f32::NAN,
        slope: std::f32::NAN,
        eccentricity: std::f32::NAN,
        frame_pt_idx: 0,
        cur_val: 0,
        mean_val: std::f32::NAN,
        sumsqf_val: std::f32::NAN,
    }
}

/// find all subsets of orig_set
///
/// translated from python version by Alex Martelli:
/// https://web.archive.org/web/20070331175701/http://mail.python.org/pipermail/python-list/2001-January/067815.html
///
/// This is also called the power set:
/// http://en.wikipedia.org/wiki/Power_set
pub fn set_of_subsets<K, V>(orig_set: &BTreeMap<K, V>) -> BTreeSet<BTreeSet<K>>
where
    K: Clone + Ord,
{
    (0..2u32.pow(orig_set.len() as u32))
        .map(|x| {
            orig_set
                .iter()
                .enumerate()
                .filter_map(|(i, (k, _v))| {
                    if x & (1 << i) != 0x00 {
                        Some(k.clone())
                    } else {
                        None
                    }
                })
                .collect()
        })
        .collect()
}

#[test]
fn test_set_of_subsets() {
    let mut orig = BTreeMap::new();
    orig.insert(1, 'a');
    orig.insert(2, 'b');
    orig.insert(3, 'c');

    let result = set_of_subsets(&orig);

    let expected = vec![
        vec![],
        vec![1],
        vec![2],
        vec![3],
        vec![1, 2],
        vec![1, 3],
        vec![2, 3],
        vec![1, 2, 3],
    ];

    assert_eq!(result.len(), expected.len());
    for e in expected.into_iter() {
        assert!(result.contains(&e.into_iter().collect::<BTreeSet<_>>()));
    }
}

pub struct KalmanEstimateRecord {
    pub record: KalmanEstimatesRow,
    pub data_assoc_rows: Vec<DataAssocRow>,
    pub mean_reproj_dist_100x: Option<u64>,
}

pub enum SaveToDiskMsg {
    // birth?
    KalmanEstimate(KalmanEstimateRecord),
    // death?
    Data2dDistorted(FrameDataAndPoints),
    StartSavingCsv(StartSavingCsvConfig),
    StopSavingCsv,
    Textlog(TextlogRow),
    TriggerClockInfo(TriggerClockInfoRow),
    SetExperimentUuid(String),
    QuitNow,
}

/// Load .csv or .csv.gz file
#[deprecated = "use the zip-or-dir crate and pick_csvgz_or_csv2"]
pub fn pick_csvgz_or_csv(
    csv_path: &std::path::Path,
) -> std::result::Result<Box<dyn std::io::Read>, failure::Error> {
    use failure::ResultExt;
    let gz_fname = std::path::PathBuf::from(csv_path).with_extension("csv.gz");

    if csv_path.exists() {
        std::fs::File::open(&csv_path)
            .map(|fd| {
                let rdr: Box<dyn std::io::Read> = Box::new(fd); // type erasure
                rdr
            })
            .context(format!("opening {}", csv_path.display()))
            .map_err(|e| failure::Error::from(e).into())
    } else {
        // This gives us an error corresponding to a non-existing .gz file.
        let gz_fd =
            std::fs::File::open(&gz_fname).context(format!("opening {}", gz_fname.display()))?;
        let decoder = libflate::gzip::Decoder::new(gz_fd).context("decoding .gz".to_string())?;
        Ok(Box::new(decoder))
    }
}

/// Pick the `.csv` file (if it exists) as first choice, else pick `.csv.gz`.
///
/// Note, use caution if using `csv_fname` after this, as it may be the original
/// (`.csv`) or new (`.csv.gz`).
fn pick_csvgz_or_csv2<'a, R: Read + Seek>(
    csv_fname: &'a mut zip_or_dir::PathLike<R>,
) -> std::result::Result<Box<dyn Read + 'a>, failure::Error> {
    if csv_fname.exists() {
        Ok(Box::new(csv_fname.open()?))
    } else {
        use failure::ResultExt;

        csv_fname.set_extension("csv.gz");

        let displayname = format!("{}", csv_fname.display());

        let gz_fd = csv_fname
            .open()
            .context(format!("opening {}", displayname))?;
        Ok(Box::new(
            libflate::gzip::Decoder::new(gz_fd).context("decoding .gz".to_string())?,
        ))
    }
}

/// Acts like a `csv::Writer` but buffers and orders by frame.
///
/// This is done to allow consumers of the kalman estimates data to iterate
/// through the saved rows assuming that they are ordered. This assumption
/// is easy to implicitly make, so we make it true by doing this.
struct OrderingWriter {
    wtr: csv::Writer<Box<dyn std::io::Write>>,
    buffer: BTreeMap<u64, Vec<KalmanEstimatesRow>>,
}

impl OrderingWriter {
    fn new(wtr: csv::Writer<Box<dyn std::io::Write>>) -> Self {
        let buffer = BTreeMap::new();
        Self { wtr, buffer }
    }
    /// Flush the writer to disk. Note this does not drain the buffer.
    fn flush(&mut self) -> std::io::Result<()> {
        self.wtr.flush()
    }
    fn serialize(&mut self, row: KalmanEstimatesRow) -> csv::Result<()> {
        let key = row.frame.0;
        {
            let ref mut entry = self.buffer.entry(key).or_insert_with(|| Vec::new());
            entry.push(row);
        }

        // Buffer up to 1000 frames, then start saving the oldest ones.
        let buffer_size = 1000;
        if self.buffer.len() > buffer_size {
            let n_to_save = self.buffer.len() - buffer_size;
            let mut to_remove: Vec<u64> = Vec::with_capacity(n_to_save);
            {
                for (frame, rows) in self.buffer.iter().take(n_to_save) {
                    for row in rows.iter() {
                        self.wtr.serialize(row)?;
                    }
                    to_remove.push(*frame);
                }
            }
            for frame in to_remove.iter() {
                self.buffer.remove(frame);
            }
        }
        Ok(())
    }
}

impl Drop for OrderingWriter {
    fn drop(&mut self) {
        // get current buffer
        let old_buffer = std::mem::replace(&mut self.buffer, BTreeMap::new());
        // drain buffer
        for (_frame, rows) in old_buffer.into_iter() {
            for row in rows.into_iter() {
                self.wtr.serialize(row).expect("serialzing buffered row");
            }
        }
        // flush writer
        self.wtr.flush().expect("flush writer");
    }
}

struct IntervalHistogram<T: Counter> {
    histogram: Histogram<T>,
    start_timestamp: std::time::Duration,
    duration: std::time::Duration,
}

struct StartedHistogram<T: Counter> {
    histogram: Histogram<T>,
    start_timestamp: std::time::SystemTime,
}

impl<T: Counter> StartedHistogram<T> {
    fn end(
        self,
        file_start_time: &std::time::SystemTime,
        end_timestamp: std::time::SystemTime,
    ) -> std::result::Result<IntervalHistogram<T>, std::time::SystemTimeError> {
        let start_timestamp = self.start_timestamp.duration_since(*file_start_time)?;
        let duration = end_timestamp.duration_since(self.start_timestamp)?;
        Ok(IntervalHistogram {
            histogram: self.histogram,
            start_timestamp,
            duration,
        })
    }
}

struct HistogramWritingState {
    current_store: Option<StartedHistogram<u64>>,
    histograms: Vec<IntervalHistogram<u64>>,
}

impl Default for HistogramWritingState {
    fn default() -> Self {
        Self {
            current_store: None,
            histograms: vec![],
        }
    }
}

struct WritingState {
    output_dirname: std::path::PathBuf,
    /// The readme file in the output directory.
    ///
    /// We keep this file open to establish locking on the open directory.
    ///
    /// In theory, we might prefer an open reference to the directory itself,
    /// but this does not seem possible. So we have a potential slight race
    /// condition when we have our directory but not yet the file handle on
    /// readme.
    readme_fd: Option<std::fs::File>,
    save_empty_data2d: bool,
    // kalman_estimates_wtr: Option<csv::Writer<Box<dyn std::io::Write>>>,
    kalman_estimates_wtr: Option<OrderingWriter>,
    data_assoc_wtr: Option<csv::Writer<Box<dyn std::io::Write>>>,
    data_2d_wtr: csv::Writer<Box<dyn std::io::Write>>,
    textlog_wtr: csv::Writer<Box<dyn std::io::Write>>,
    trigger_clock_info_wtr: csv::Writer<Box<dyn std::io::Write>>,
    experiment_info_wtr: csv::Writer<Box<dyn std::io::Write>>,
    writer_stats: Option<usize>,
    file_start_time: std::time::SystemTime,

    reconstruction_latency_usec: HistogramWritingState,
    reproj_dist_pixels: HistogramWritingState,
}

impl Drop for WritingState {
    fn drop(&mut self) {
        fn dummy_csv() -> csv::Writer<Box<dyn std::io::Write>> {
            let fd = Box::new(Vec::with_capacity(0));
            csv::Writer::from_writer(fd)
        }

        if let Some(count) = self.writer_stats {
            info!("    {} rows of kalman estimates", count);
        }

        // Drop all CSV files, which closes them.
        {
            self.kalman_estimates_wtr.take();
            self.data_assoc_wtr.take();
            // Could equivalently call `.flush()` on the writers?
            self.data_2d_wtr = dummy_csv();
            self.textlog_wtr = dummy_csv();
            self.trigger_clock_info_wtr = dummy_csv();
            self.experiment_info_wtr = dummy_csv();
        }

        // Move out original output name so that a subsequent call to `drop()`
        // doesn't accidentally overwrite our real data.
        let output_dirname =
            std::mem::replace(&mut self.output_dirname, std::path::PathBuf::default());

        let now_system = std::time::SystemTime::now();
        {
            finish_histogram(
                &mut self.reconstruction_latency_usec.current_store,
                self.file_start_time,
                &mut self.reconstruction_latency_usec.histograms,
                now_system,
            )
            .unwrap();

            finish_histogram(
                &mut self.reproj_dist_pixels.current_store,
                self.file_start_time,
                &mut self.reproj_dist_pixels.histograms,
                now_system,
            )
            .unwrap();
        }

        save_hlog(
            &output_dirname,
            RECONSTRUCT_LATENCY_LOG_FNAME,
            &mut self.reconstruction_latency_usec.histograms,
            self.file_start_time,
        );

        save_hlog(
            &output_dirname,
            REPROJECTION_DIST_LOG_FNAME,
            &mut self.reproj_dist_pixels.histograms,
            self.file_start_time,
        );

        {
            // TODO: read all the (forward) kalman estimates and smooth them to
            // an additional file. If we do it here, it is done after the
            // realtime tracking and thus does not interfere with recording
            // data. On the other hand, if we smooth at the end of each
            // trajectory, those smoothing costs are amortized throughout the
            // experiment.

            let replace_extension = match output_dirname.extension() {
                Some(ext) => ext == "braid",
                None => false,
            };

            // compute the name of the zip file.
            let output_zipfile: std::path::PathBuf = if replace_extension {
                output_dirname.with_extension("braidz")
            } else {
                let mut tmp = output_dirname.clone().into_os_string();
                tmp.push(".braidz");
                tmp.into()
            };

            info!("creating zip file {}", output_zipfile.display());
            // zip the output_dirname directory
            {
                let mut file = std::fs::File::create(&output_zipfile).unwrap();

                let header = "BRAIDZ file. This is a standard ZIP file with a \
                    specific schema. You can view the contents of this \
                    file at https://braidz.strawlab.org/\n";
                file.write_all(header.as_bytes()).unwrap();

                let walkdir = walkdir::WalkDir::new(&output_dirname);

                // Reorder the results to save the README_WITH_EXT file first
                // so that the first bytes of the file have it. This is why we
                // special-case the file here.
                let mut readme_entry: Option<walkdir::DirEntry> = None;
                let files1: Vec<walkdir::DirEntry> =
                    walkdir.into_iter().filter_map(|e| e.ok()).collect();
                let mut files = Vec::new();
                for entry in files1.into_iter() {
                    if entry.file_name() == README_WITH_EXT {
                        readme_entry = Some(entry);
                    } else {
                        files.push(entry);
                    }
                }
                if let Some(entry) = readme_entry {
                    files.insert(0, entry);
                }

                let zipw = zip::ZipWriter::new(file);
                let options = zip::write::FileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored)
                    .unix_permissions(0o755);

                zip_dir::zip_dir(&mut files.into_iter(), &output_dirname, zipw, options)
                    .expect("zip_dir");
            }

            // Release the file so we no longer have exclusive access to the
            // directory. (Until we remove the directory, we have a small race
            // condition where another process could open the directory without
            // obtaining the readme file handle.)
            self.readme_fd = None;

            // Once the original directory is written successfully to a zip
            // file, we remove it.
            info!(
                "done creating zip file, removing {}",
                output_dirname.display()
            );
            std::fs::remove_dir_all(&output_dirname).unwrap();
        }
    }
}

fn save_hlog(
    output_dirname: &std::path::PathBuf,
    fname: &str,
    histograms: &mut Vec<IntervalHistogram<u64>>,
    file_start_time: std::time::SystemTime,
) {
    // Write the reconstruction latency histograms to disk.
    let mut log_path = output_dirname.clone();
    log_path.push(fname);
    log_path.set_extension("hlog");
    let mut fd = std::fs::File::create(&log_path).expect("creating latency log file");

    let mut serializer = V2DeflateSerializer::new();
    // create a writer via a builder
    let mut latency_log_wtr = interval_log::IntervalLogWriterBuilder::new()
        .with_start_time(file_start_time)
        .begin_log_with(&mut fd, &mut serializer)
        .unwrap();

    for h in histograms.iter() {
        latency_log_wtr
            .write_histogram(&h.histogram, h.start_timestamp, h.duration, None)
            .unwrap();
    }
}

impl WritingState {
    fn new(
        cfg: StartSavingCsvConfig,
        cam_info_rows: Vec<CamInfoRow>,
        recon: &Option<flydra_mvg::FlydraMultiCameraSystem<MyFloat>>,
        tracking_params: Arc<TrackingParams>,
        save_empty_data2d: bool,
    ) -> Result<Self> {
        let output_dirname = cfg.out_dir;
        let local = cfg.local;
        let git_revision = cfg.git_rev;
        let fps = cfg.fps;
        let images = cfg.images;

        // Any changes to what is saved should update BraidMetadataSchemaTag.

        // create output dir
        std::fs::create_dir_all(&output_dirname)?;

        // Until we obtain the readme file handle, we have a small race
        // condition where another process could also open this directory.

        let readme_fd = {
            let readme_path = output_dirname.join(README_WITH_EXT);

            let mut fd = std::fs::File::create(&readme_path)?;

            // Start and end it with some newlines so the text is more
            // readable.
            fd.write_all(
                "\n\nThis is data saved by the braid program. \
                See https://strawlab.org/braid for more information.\n\n"
                    .as_bytes(),
            )
            .unwrap();
            Some(fd)
        };

        {
            let braid_metadata_path = output_dirname
                .join(BRAID_METADATA_YML_FNAME)
                .with_extension("yml");

            let metadata = BraidMetadata {
                schema: BRAID_SCHEMA, // BraidMetadataSchemaTag
                git_revision: git_revision.clone(),
                original_recording_time: local,
                save_empty_data2d,
            };
            let metadata_buf = serde_yaml::to_string(&metadata).unwrap();

            let mut fd = std::fs::File::create(&braid_metadata_path)?;
            fd.write_all(&metadata_buf.as_bytes()).unwrap();
        }

        // write images
        {
            let mut image_path = output_dirname.clone();
            image_path.push(IMAGES_DIRNAME);
            std::fs::create_dir_all(&image_path)?;

            for (fname, tup) in images.into_iter() {
                let mut fullpath = image_path.clone();
                fullpath.push(fname);
                let mut fd = std::fs::File::create(&fullpath)?;
                fd.write(&tup)?;
            }
        }

        // write cam info (pairs of CamNum and cam name)
        {
            let mut csv_path = output_dirname.clone();
            csv_path.push(CAM_INFO_CSV_FNAME);
            csv_path.set_extension("csv.gz");
            let fd = std::fs::File::create(&csv_path)?;
            let fd: Box<dyn std::io::Write> = Box::new(AutoFinishUnchecked::new(Encoder::new(fd)?));
            let mut cam_info_wtr = csv::Writer::from_writer(fd);
            for row in cam_info_rows.iter() {
                cam_info_wtr.serialize(row)?;
            }
        }

        // write calibration
        if let Some(ref recon) = recon {
            let mut cal_path = output_dirname.clone();
            cal_path.push(CALIBRATION_XML_FNAME);
            cal_path.set_extension("xml");
            let fd = std::fs::File::create(&cal_path)?;
            recon.to_flydra_xml(fd)?;
        }

        // open textlog and write initial message
        let textlog_wtr = {
            let timestamp = datetime_conversion::datetime_to_f64(&chrono::Local::now());

            let fps = match fps {
                Some(fps) => format!("{}", fps),
                None => "unknown".to_string(),
            };
            let version = "2.0.0";
            let tzname = iana_time_zone::get_timezone()?;
            let message = format!(
                "MainBrain running at {} fps, (\
                flydra_version {}, git_revision {}, time_tzname0 {})",
                fps, version, git_revision, tzname
            );

            let tps = TrackingParamsSaver {
                tracking_params: Arc::make_mut(&mut tracking_params.clone()).clone().into(), // convert to flydra_types::TrackingParams
                git_revision,
            };
            let message2 = serde_json::to_string(&tps)?;

            let textlog: Vec<TextlogRow> = vec![
                TextlogRow {
                    mainbrain_timestamp: timestamp,
                    cam_id: "mainbrain".to_string(),
                    host_timestamp: timestamp,
                    message,
                },
                TextlogRow {
                    mainbrain_timestamp: timestamp,
                    cam_id: "mainbrain".to_string(),
                    host_timestamp: timestamp,
                    message: message2,
                },
            ];

            let mut csv_path = output_dirname.clone();
            csv_path.push(TEXTLOG);
            csv_path.set_extension("csv.gz");
            let fd = std::fs::File::create(&csv_path)?;
            let fd: Box<dyn std::io::Write> = Box::new(AutoFinishUnchecked::new(Encoder::new(fd)?));
            let mut textlog_wtr = csv::Writer::from_writer(fd);
            for row in textlog.iter() {
                textlog_wtr.serialize(row)?;
            }
            textlog_wtr
        };

        // kalman estimates
        let kalman_estimates_wtr = if let Some(ref _recon) = recon {
            let mut csv_path = output_dirname.clone();
            csv_path.push(KALMAN_ESTIMATES_FNAME);
            csv_path.set_extension("csv.gz");
            let fd = std::fs::File::create(&csv_path)?;
            let fd: Box<dyn std::io::Write> = Box::new(AutoFinishUnchecked::new(Encoder::new(fd)?));
            Some(OrderingWriter::new(csv::Writer::from_writer(fd)))
        } else {
            None
        };

        let trigger_clock_info_wtr = {
            let mut csv_path = output_dirname.clone();
            csv_path.push(TRIGGER_CLOCK_INFO);
            csv_path.set_extension("csv.gz");
            let fd = std::fs::File::create(&csv_path)?;
            let fd: Box<dyn std::io::Write> = Box::new(AutoFinishUnchecked::new(Encoder::new(fd)?));
            csv::Writer::from_writer(fd)
        };

        let experiment_info_wtr = {
            let mut csv_path = output_dirname.clone();
            csv_path.push(EXPERIMENT_INFO);
            csv_path.set_extension("csv.gz");
            let fd = std::fs::File::create(&csv_path)?;
            let fd: Box<dyn std::io::Write> = Box::new(AutoFinishUnchecked::new(Encoder::new(fd)?));
            csv::Writer::from_writer(fd)
        };

        let data_assoc_wtr = if let Some(ref _recon) = recon {
            let mut csv_path = output_dirname.clone();
            csv_path.push(DATA_ASSOCIATE_FNAME);
            csv_path.set_extension("csv.gz");
            let fd = std::fs::File::create(&csv_path)?;
            let fd: Box<dyn std::io::Write> = Box::new(AutoFinishUnchecked::new(Encoder::new(fd)?));
            Some(csv::Writer::from_writer(fd))
        } else {
            None
        };

        let data_2d_wtr = {
            let mut csv_path = output_dirname.clone();
            csv_path.push(DATA2D_DISTORTED_CSV_FNAME);
            csv_path.set_extension("csv.gz");
            let fd = std::fs::File::create(&csv_path)?;
            let fd: Box<dyn std::io::Write> = Box::new(AutoFinishUnchecked::new(Encoder::new(fd)?));
            csv::Writer::from_writer(fd)
        };

        let writer_stats = if cfg.print_stats { Some(0) } else { None };

        let file_start_time = if let Some(local) = local {
            local.into()
        } else {
            std::time::SystemTime::now()
        };

        Ok(Self {
            output_dirname,
            readme_fd,
            save_empty_data2d,
            kalman_estimates_wtr,
            data_assoc_wtr,
            data_2d_wtr,
            textlog_wtr,
            trigger_clock_info_wtr,
            experiment_info_wtr,
            writer_stats,
            file_start_time,
            reconstruction_latency_usec: HistogramWritingState::default(),
            reproj_dist_pixels: HistogramWritingState::default(),
        })
    }

    fn save_data_2d_distorted(&mut self, fdp: FrameDataAndPoints) -> Result<()> {
        let frame_data = &fdp.frame_data;
        let pts_to_save: Vec<Data2dDistortedRowF32> = fdp
            .points
            .iter()
            .map(|orig| convert_to_save(&frame_data, &orig))
            .collect();

        let data2d_distorted: Vec<Data2dDistortedRowF32> = if pts_to_save.len() > 0 {
            pts_to_save
        } else {
            if self.save_empty_data2d {
                let empty_data = vec![convert_empty_to_save(&frame_data)];
                empty_data
            } else {
                vec![]
            }
        };

        for row in data2d_distorted.iter() {
            self.data_2d_wtr.serialize(&row)?;
        }
        Ok(())
    }

    fn flush_all(&mut self) -> Result<()> {
        if let Some(ref mut kew) = self.kalman_estimates_wtr {
            kew.flush()?;
        }
        if let Some(ref mut daw) = self.data_assoc_wtr {
            daw.flush()?;
        }
        self.data_2d_wtr.flush()?;
        self.textlog_wtr.flush()?;
        self.trigger_clock_info_wtr.flush()?;
        self.experiment_info_wtr.flush()?;
        Ok(())
    }
}

fn finish_histogram(
    hist_store: &mut Option<StartedHistogram<u64>>,
    file_start_time: std::time::SystemTime,
    histograms: &mut Vec<IntervalHistogram<u64>>,
    now_system: std::time::SystemTime,
) -> std::result::Result<(), hdrhistogram::RecordError> {
    if let Some(hist) = hist_store.take() {
        if let Ok(h) = hist.end(&file_start_time, now_system) {
            histograms.push(h);
        }
    }
    Ok(())
}

fn histogram_record(
    value: u64,
    hist_store: &mut Option<StartedHistogram<u64>>,
    high: u64,
    sigfig: u8,
    file_start_time: std::time::SystemTime,
    histograms: &mut Vec<IntervalHistogram<u64>>,
    now_system: std::time::SystemTime,
) -> std::result::Result<(), hdrhistogram::RecordError> {
    // Create a new histogram if needed, else compute how long we have used this one.
    let (mut hist, accum_dur) = match hist_store.take() {
        None => {
            // Range from 1 usec to 1 minute with 2 significant figures.
            let hist = StartedHistogram {
                histogram: Histogram::<u64>::new_with_bounds(1, high, sigfig).unwrap(),
                start_timestamp: now_system,
            };
            (hist, None)
        }
        Some(hist) => {
            let start = hist.start_timestamp;
            (hist, now_system.duration_since(start).ok())
        }
    };

    // Record the value in the histogram.
    hist.histogram.record(value as u64)?;

    *hist_store = Some(hist);

    if let Some(accum_dur) = accum_dur {
        if accum_dur.as_secs() >= 60 {
            finish_histogram(hist_store, file_start_time, histograms, now_system)?;
        }
    }
    Ok(())
}

fn writer_thread_main(
    save_data_rx: crossbeam_channel::Receiver<SaveToDiskMsg>,
    cam_manager: ConnectedCamerasManager,
    recon: Option<flydra_mvg::FlydraMultiCameraSystem<MyFloat>>,
    tracking_params: Arc<TrackingParams>,
    save_empty_data2d: bool,
    ignore_latency: bool,
) -> Result<()> {
    use crate::SaveToDiskMsg::*;
    use std::time::{Duration, Instant};

    let mut writing_state: Option<WritingState> = None;

    const FLUSH_INTERVAL: u64 = 1;
    let flush_interval = Duration::from_secs(FLUSH_INTERVAL);

    let mut last_flushed = Instant::now();

    // TODO: add a timeout on recv() so that we periodically flush even if we
    // received no message.

    loop {
        match save_data_rx.recv() {
            Ok(msg) => {
                match msg {
                    KalmanEstimate(ke) => {
                        let KalmanEstimateRecord {
                            record,
                            data_assoc_rows,
                            mean_reproj_dist_100x,
                        } = ke;
                        let trigger_timestamp = record.timestamp.clone();

                        // Now actually send the data to the writers.
                        if let Some(ref mut ws) = writing_state {
                            if let Some(ref mut kew) = ws.kalman_estimates_wtr {
                                kew.serialize(record)?;
                                match ws.writer_stats.as_mut() {
                                    Some(count) => *count += 1,
                                    None => {}
                                }
                            }
                            if let Some(ref mut daw) = ws.data_assoc_wtr {
                                for row in data_assoc_rows.iter() {
                                    daw.serialize(row)?;
                                }
                            }

                            if !ignore_latency {
                                // Log reconstruction latency to histogram.
                                if let Some(trigger_timestamp) = trigger_timestamp {
                                    // `trigger_timestamp` is when this frame was acquired.
                                    // It may be None if it cannot be inferred while the
                                    // triggerbox clock model is first initializing.
                                    use chrono::{DateTime, Local};
                                    let then: DateTime<Local> = trigger_timestamp.into();
                                    let now = Local::now();
                                    let elapsed = now.signed_duration_since(then);
                                    let now_system: std::time::SystemTime = now.into();

                                    if let Some(latency_usec) = elapsed.num_microseconds() {
                                        if latency_usec >= 0 {
                                            // The latency should always be positive, but num_microseconds()
                                            // can return negative and we don't want to panic if time goes
                                            // backwards for some reason.
                                            match histogram_record(
                                                latency_usec as u64,
                                                &mut ws.reconstruction_latency_usec.current_store,
                                                1000 * 1000 * 60,
                                                2,
                                                ws.file_start_time,
                                                &mut ws.reconstruction_latency_usec.histograms,
                                                now_system,
                                            ) {
                                                Ok(()) => {}
                                                Err(_) => log::error!(
                                                    "latency value {} out of expected range",
                                                    latency_usec
                                                ),
                                            }
                                        }
                                    }
                                }
                            }

                            {
                                if let Some(mean_reproj_dist_100x) = mean_reproj_dist_100x {
                                    let now_system = std::time::SystemTime::now();

                                    match histogram_record(
                                        mean_reproj_dist_100x,
                                        &mut ws.reproj_dist_pixels.current_store,
                                        1000000,
                                        2,
                                        ws.file_start_time,
                                        &mut ws.reproj_dist_pixels.histograms,
                                        now_system,
                                    ) {
                                        Ok(()) => {}
                                        Err(_) => log::error!(
                                            "mean reprojection 100x distance value {} out of expected range",
                                            mean_reproj_dist_100x
                                        ),
                                    }
                                }
                            }
                        }

                        // simply drop data if no file opened
                    }
                    Data2dDistorted(fdp) => {
                        if let Some(ref mut ws) = writing_state {
                            ws.save_data_2d_distorted(fdp)?;
                        }
                        // simply drop data if no file opened
                    }
                    StartSavingCsv(cfg) => {
                        writing_state = Some(WritingState::new(
                            cfg,
                            cam_manager.sample(),
                            &recon,
                            tracking_params.clone(),
                            save_empty_data2d,
                        )?);
                    }
                    StopSavingCsv => {
                        // This will drop the writers and thus close them.
                        writing_state = None;
                    }
                    SetExperimentUuid(uuid) => {
                        let entry = ExperimentInfoRow { uuid };
                        if let Some(ref mut ws) = writing_state {
                            ws.experiment_info_wtr.serialize(&entry)?;
                        }
                    }
                    Textlog(entry) => {
                        if let Some(ref mut ws) = writing_state {
                            ws.textlog_wtr.serialize(&entry)?;
                        }
                        // simply drop data if no file opened
                    }
                    TriggerClockInfo(entry) => {
                        if let Some(ref mut ws) = writing_state {
                            ws.trigger_clock_info_wtr.serialize(&entry)?;
                        }
                        // simply drop data if no file opened
                    }
                    QuitNow => {
                        // We rely on `writing_state.drop()` to flush and close
                        // everything.
                        break;
                    }
                };
            }
            Err(e) => {
                let _: crossbeam_channel::RecvError = e;
                // sender disconnected. we can quit too.
                break;
            }
        };

        // after processing message, check if we should flush data.
        if last_flushed.elapsed() > flush_interval {
            // flush all writers
            if let Some(ref mut ws) = writing_state {
                ws.flush_all()?;
            }

            last_flushed = Instant::now();
        }
    }
    Ok(())
}

pub struct StartSavingCsvConfig {
    pub out_dir: std::path::PathBuf,
    pub local: Option<chrono::DateTime<chrono::Local>>,
    pub git_rev: String,
    pub fps: Option<f32>,
    pub images: ImageDictType,
    pub print_stats: bool,
}

/// A struct which implements `std::marker::Send` to control coord processing.
#[derive(Clone)]
pub struct CoordProcessorControl {
    save_data_tx: crossbeam_channel::Sender<SaveToDiskMsg>,
}

// TODO: also include a timestamp?
pub type ImageDictType = BTreeMap<String, Vec<u8>>;

impl CoordProcessorControl {
    pub fn new(save_data_tx: crossbeam_channel::Sender<SaveToDiskMsg>) -> Self {
        Self { save_data_tx }
    }

    pub fn start_saving_data(&self, cfg: StartSavingCsvConfig) {
        self.save_data_tx
            .send(SaveToDiskMsg::StartSavingCsv(cfg))
            .cb_ok();
    }

    pub fn stop_saving_data(&self) {
        self.save_data_tx.send(SaveToDiskMsg::StopSavingCsv).cb_ok();
    }

    pub fn append_textlog_message(&self, msg: TextlogRow) {
        self.save_data_tx.send(SaveToDiskMsg::Textlog(msg)).cb_ok();
    }

    pub fn append_trigger_clock_info_message(&self, msg: TriggerClockInfoRow) {
        self.save_data_tx
            .send(SaveToDiskMsg::TriggerClockInfo(msg))
            .cb_ok();
    }

    pub fn set_experiment_uuid(&self, uuid: String) {
        self.save_data_tx
            .send(SaveToDiskMsg::SetExperimentUuid(uuid))
            .cb_ok();
    }
}

pub struct CoordProcessor {
    pub cam_manager: ConnectedCamerasManager,
    pub recon: Option<flydra_mvg::FlydraMultiCameraSystem<MyFloat>>, // TODO? keep reference
    pub save_data_tx: crossbeam_channel::Sender<SaveToDiskMsg>,
    pub writer_thread_handle: Option<std::thread::JoinHandle<()>>,
    model_servers: Vec<Box<dyn GetsUpdates>>,
    tracking_params: Arc<TrackingParams>,
    mc2: Option<crate::tracking_core::ModelCollection<crate::tracking_core::CollectionFrameDone>>,
}

impl Drop for CoordProcessor {
    fn drop(&mut self) {
        self.save_data_tx.send(SaveToDiskMsg::QuitNow).cb_ok();
        let h2 = std::mem::replace(&mut self.writer_thread_handle, None);
        if let Some(h) = h2 {
            h.join().unwrap();
        }
    }
}

impl CoordProcessor {
    pub fn new(
        cam_manager: ConnectedCamerasManager,
        recon: Option<flydra_mvg::FlydraMultiCameraSystem<MyFloat>>,
        tracking_params: TrackingParams,
        save_data_tx: crossbeam_channel::Sender<SaveToDiskMsg>,
        save_data_rx: crossbeam_channel::Receiver<SaveToDiskMsg>,
        save_empty_data2d: bool,
        ignore_latency: bool,
    ) -> Result<Self> {
        trace!("CoordProcessor using {:?}", recon);

        let recon2 = recon.clone();

        info!("using TrackingParams {:?}", tracking_params);

        let tracking_params = Arc::new(tracking_params);
        let tracking_params2 = tracking_params.clone();
        let writer_thread_builder = std::thread::Builder::new().name("writer_thread".to_string());
        let cam_manager2 = cam_manager.clone();
        let writer_thread_handle = Some(writer_thread_builder.spawn(move || {
            run_func(|| {
                writer_thread_main(
                    save_data_rx,
                    cam_manager2,
                    recon2.clone(),
                    tracking_params2,
                    save_empty_data2d,
                    ignore_latency,
                )
            })
        })?);

        Ok(Self {
            cam_manager,
            recon,
            save_data_tx,
            writer_thread_handle,
            tracking_params,
            model_servers: vec![],
            mc2: None,
        })
    }

    fn new_model_collection(
        &self,
        recon: &flydra_mvg::FlydraMultiCameraSystem<MyFloat>,
        fps: f32,
    ) -> crate::tracking_core::ModelCollection<crate::tracking_core::CollectionFrameDone> {
        crate::tracking_core::initialize_model_collection(
            self.tracking_params.clone(),
            recon.clone(),
            fps,
            self.cam_manager.clone(),
            self.save_data_tx.clone(),
        )
    }

    pub fn get_write_controller(&self) -> CoordProcessorControl {
        CoordProcessorControl {
            save_data_tx: self.save_data_tx.clone(),
        }
    }

    pub fn add_listener(&mut self, model_server: Box<dyn GetsUpdates>) {
        self.model_servers.push(model_server);
    }

    /// Consume the CoordProcessor and the input stream.
    ///
    /// Returns a future that completes when done.
    pub async fn consume_stream<S>(
        mut self,
        frame_data_rx: S,
        expected_framerate: Option<f32>,
    ) -> Option<std::thread::JoinHandle<()>>
    where
        S: 'static + Send + futures::stream::Stream<Item = StreamItem> + Unpin,
    {
        let mut prev_frame = SyncFno(0);
        let save_data_tx = self.save_data_tx.clone();

        use futures::stream::StreamExt;

        // Save raw incoming data as first step.
        let stream1 = frame_data_rx.map(move |si: StreamItem| {
            match &si {
                StreamItem::EOF => {}
                StreamItem::Packet(fdp) => {
                    save_data_tx
                        .send(SaveToDiskMsg::Data2dDistorted(fdp.clone()))
                        .cb_ok();
                }
            }
            si
        });

        // Bundle the camera-by-camera data into all-cam data. Note that this
        // can drop data that is out-of-order, which is why we must save the
        // incoming data before here.
        let bundled = bundle_frames(stream1, self.cam_manager.clone());

        // Ensure that there are no skipped frames.
        let mut contiguous_stream = make_contiguous(bundled);

        if let Some(ref recon) = self.recon {
            let fps = expected_framerate.expect("expected_framerate must be set");
            self.mc2 = Some(self.new_model_collection(recon, fps))
        }

        let writer_thread_handle = self.writer_thread_handle.take();

        while let Some(bundle) = contiguous_stream.next().await {
            if bundle.frame() < prev_frame {
                info!("resynchronized cameras, restarting ModelCollection");
                if let Some(ref recon) = self.recon {
                    let fps = expected_framerate.expect("expected_framerate must be set");
                    self.mc2 = Some(self.new_model_collection(recon, fps))
                }
            }
            prev_frame = bundle.frame();

            if let Some(model_collection) = self.mc2.take() {
                // undistort all observations
                let undistorted = bundle.undistort(&model_collection.mcinner.recon);
                // calculate priors (update estimates to current frame)
                let model_collection = model_collection.predict_motion();
                // calculate likelihood of each observation
                let model_collection = model_collection.compute_observation_likes(undistorted);
                // perform data association
                let (model_collection, unused) =
                    model_collection.solve_data_association_and_update();
                // create new and delete old objects
                let model_collection =
                    model_collection.births_and_deaths(unused, &self.model_servers);
                self.mc2 = Some(model_collection);
            }
        }

        debug!("consume_stream future done");
        writer_thread_handle
    }
}

/// run a function returning Result<()> and handle errors.
// see https://github.com/withoutboats/failure/issues/76#issuecomment-347402383
pub fn run_func<F: FnOnce() -> Result<()>>(real_func: F) {
    // Decide which command to run, and run it, and print any errors.
    if let Err(err) = real_func() {
        let mut stderr = std::io::stderr();
        writeln!(stderr, "In {}:{}: Error: {}", file!(), line!(), err)
            .expect("unable to write error to stderr");

        if let ErrorKind::Failure(err) = err.kind {
            writeln!(stderr, "Error: {}", err).expect("unable to write error to stderr");

            for cause in err.iter_causes() {
                writeln!(stderr, "Caused by: {}", cause).expect("unable to write error to stderr");
            }
        }
        std::process::exit(1);
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CamAndDist {
    pub(crate) ros_cam_name: RosCamName,
    /// The reprojection distance of the undistorted pixels.
    pub(crate) reproj_dist: MyFloat,
}

pub(crate) struct HypothesisTestResult {
    pub(crate) coords: Point3<MyFloat>,
    pub(crate) cams_and_reproj_dist: Vec<CamAndDist>,
}

#[test]
fn test_csv_nan() {
    // test https://github.com/BurntSushi/rust-csv/issues/153

    let save_row_data = Data2dDistortedRowF32 {
        camn: CamNum(1),
        frame: 2,
        timestamp: None,
        cam_received_timestamp: FlydraFloatTimestampLocal::from_dt(&chrono::Local::now()),
        x: std::f32::NAN,
        y: std::f32::NAN,
        area: 1.0,
        slope: 2.0,
        eccentricity: 3.0,
        frame_pt_idx: 4,
        cur_val: 5,
        mean_val: 6.0,
        sumsqf_val: 7.0,
    };

    let mut csv_buf = Vec::<u8>::new();

    {
        let mut wtr = csv::Writer::from_writer(&mut csv_buf);
        wtr.serialize(&save_row_data).unwrap();
    }

    println!("{}", std::str::from_utf8(&csv_buf).unwrap());

    {
        let rdr = csv::Reader::from_reader(csv_buf.as_slice());
        let mut count = 0;
        for row in rdr.into_deserialize().into_iter() {
            let row: Data2dDistortedRow = row.unwrap();
            count += 1;
            assert!(row.x.is_nan());
            assert!(row.y.is_nan());
            assert!(!row.area.is_nan());
            assert_eq!(row.area, 1.0);

            break;
        }
        assert_eq!(count, 1);
    }
}
