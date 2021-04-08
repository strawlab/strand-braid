#![cfg_attr(feature = "backtrace", feature(backtrace))]

#[cfg(feature = "backtrace")]
use std::backtrace::Backtrace;

use thiserror::Error;

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

mod write_data;
use write_data::writer_thread_main;

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

#[derive(Error, Debug)]
pub enum Error {
    #[error("{source}")]
    FlydraTypes {
        #[from]
        source: flydra_types::FlydraTypesError,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    Mvg {
        #[from]
        source: mvg::MvgError,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    Io {
        #[from]
        source: std::io::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    Csv {
        #[from]
        source: csv::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    GetTimezone {
        #[from]
        source: iana_time_zone::GetTimezoneError,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    SerdeJson {
        #[from]
        source: serde_json::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    SerdeYaml {
        #[from]
        source: serde_yaml::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    FuturesSendError {
        #[from]
        source: futures::channel::mpsc::SendError,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    TomlSerError {
        #[from]
        source: toml::ser::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    TomlDeError {
        #[from]
        source: toml::de::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("invalid hypothesis testing parameters")]
    InvalidHypothesisTestingParameters,
    #[error("insufficient data to calculate FPS")]
    InsufficientDataToCalculateFps,
    #[error("{source}")]
    ZipDir {
        #[from]
        source: zip_or_dir::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("Error opening {filename}: {source}")]
    FileError {
        what: &'static str,
        filename: String,
        source: Box<dyn std::error::Error + Sync + Send>,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    WrappedError {
        source: Box<dyn std::error::Error + Sync + Send>,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
}

pub fn file_error<E>(what: &'static str, filename: String, source: E) -> Error
where
    E: 'static + std::error::Error + Sync + Send,
{
    Error::FileError {
        what,
        filename,
        source: Box::new(source),
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace::capture(),
    }
}

pub fn wrap_error<E>(source: E) -> Error
where
    E: 'static + std::error::Error + Sync + Send,
{
    Error::WrappedError {
        source: Box::new(source),
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace::capture(),
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
pub fn pick_csvgz_or_csv(csv_path: &std::path::Path) -> Result<Box<dyn std::io::Read>> {
    let gz_fname = std::path::PathBuf::from(csv_path).with_extension("csv.gz");

    if csv_path.exists() {
        std::fs::File::open(&csv_path)
            .map(|fd| {
                let rdr: Box<dyn std::io::Read> = Box::new(fd); // type erasure
                rdr
            })
            .map_err(|e| file_error("opening", format!("opening {}", csv_path.display()), e))
    } else {
        // This gives us an error corresponding to a non-existing .gz file.
        let gz_fd = std::fs::File::open(&gz_fname)
            .map_err(|e| file_error("opening", format!("opening {}", gz_fname.display()), e))?;
        let decoder = libflate::gzip::Decoder::new(gz_fd)?;
        Ok(Box::new(decoder))
    }
}

/// Pick the `.csv` file (if it exists) as first choice, else pick `.csv.gz`.
///
/// Note, use caution if using `csv_fname` after this, as it may be the original
/// (`.csv`) or new (`.csv.gz`).
fn pick_csvgz_or_csv2<'a, R: Read + Seek>(
    csv_fname: &'a mut zip_or_dir::PathLike<R>,
) -> Result<Box<dyn Read + 'a>> {
    if csv_fname.exists() {
        Ok(Box::new(csv_fname.open()?))
    } else {
        csv_fname.set_extension("csv.gz");

        let displayname = format!("{}", csv_fname.display());

        let gz_fd = csv_fname
            .open()
            .map_err(|e| file_error("opening", displayname, e))?;
        Ok(Box::new(libflate::gzip::Decoder::new(gz_fd)?))
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

pub struct StartSavingCsvConfig {
    pub out_dir: std::path::PathBuf,
    pub local: Option<chrono::DateTime<chrono::Local>>,
    pub git_rev: String,
    pub fps: Option<f32>,
    pub images: ImageDictType,
    pub print_stats: bool,
    pub save_performance_histograms: bool,
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

        use std::error::Error;
        let mut source_err = err.source();

        while let Some(source) = source_err {
            writeln!(stderr, "Source: {}", source).expect("unable to write error to stderr");
            source_err = source.source();
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
