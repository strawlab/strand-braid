#![cfg_attr(
    feature = "backtrace",
    feature(error_generic_member_access, provide_any)
)]

use tracing::{debug, error, info, trace};
use tracing_futures::Instrument;

use mini_arenas::MiniArenaImage;
use serde::{Deserialize, Serialize};

use std::{
    collections::{BTreeMap, BTreeSet},
    f64,
    sync::{Arc, Mutex},
};

use hdrhistogram::{
    serialization::{interval_log, V2DeflateSerializer},
    Counter, Histogram,
};

use libflate::finish::AutoFinishUnchecked;
use libflate::gzip::Encoder;

use nalgebra::core::dimension::{U1, U2, U3, U6};
use nalgebra::{OMatrix, OVector, Point3, Vector6};

use nalgebra::allocator::Allocator;
use nalgebra::core::dimension::DimMin;
use nalgebra::{DefaultAllocator, RealField};

#[allow(unused_imports)]
use mvg::{DistortedPixel, PointWorldFrame, PointWorldFrameWithSumReprojError};

pub use braidz_types::BraidMetadata;

use flydra_types::{
    CamInfoRow, CamNum, ConnectedCameraSyncState, FlydraFloatTimestampLocal, HostClock,
    KalmanEstimatesRow, RosCamName, SyncFno, TextlogRow, TrackingParams, TriggerClockInfoRow,
    Triggerbox, RECONSTRUCT_LATENCY_HLOG_FNAME, REPROJECTION_DIST_HLOG_FNAME,
};
pub use flydra_types::{Data2dDistortedRow, Data2dDistortedRowF32};

use withkey::WithKey;

mod connected_camera_manager;
pub use connected_camera_manager::{ConnectedCamCallback, ConnectedCamerasManager};

mod write_data;
use write_data::writer_task_main;
pub use write_data::BraidMetadataBuilder;

mod bundled_data;
mod contiguous_stream;
mod frame_bundler;

mod new_object_test_2d;
mod new_object_test_3d;

mod flat_2d;
mod tracking_core;

mod mini_arenas;

mod model_server;
pub use crate::model_server::{new_model_server, ModelServer, SendKalmanEstimatesRow, SendType};

use crate::contiguous_stream::make_contiguous;
use crate::frame_bundler::bundle_frames;
pub use crate::frame_bundler::StreamItem;

pub type MyFloat = flydra_types::MyFloat; // todo: remove that this is public

mod error;
pub use error::{file_error, wrap_error, Error};

pub type Result<M> = std::result::Result<M, Error>;

// The first trigger pulse is labelled with this pulsenumber. Due to the
// behavior of the triggerbox, the first pulse physically leaving the device
// already has pulsenumber 2.
pub const TRIGGERBOX_FIRST_PULSE: u64 = 2;

pub(crate) fn generate_observation_model<R>(
    cam: &flydra_mvg::MultiCamera<R>,
    state: &Vector6<R>,
    ekf_observation_covariance_pixels: f64,
) -> Result<CameraObservationModel<R>>
where
    R: RealField + Copy + Default + serde::Serialize,
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
    R: RealField + Copy + Default + serde::Serialize,
{
    cam: flydra_mvg::MultiCamera<R>,
    observation_matrix: OMatrix<R, U2, U6>,
    observation_matrix_transpose: OMatrix<R, U6, U2>,
    observation_noise_covariance: OMatrix<R, U2, U2>,
}

impl<R> CameraObservationModel<R>
where
    R: RealField + Copy + Default + serde::Serialize,
{
    fn new(
        cam: flydra_mvg::MultiCamera<R>,
        a: OMatrix<R, U2, U3>,
        ekf_observation_covariance_pixels: f64,
    ) -> Self {
        let observation_matrix = {
            let mut o = OMatrix::<R, U2, U6>::zeros();
            o.fixed_columns_mut::<3>(0).copy_from(&a);
            o
        };
        let observation_matrix_transpose = observation_matrix.transpose();

        let r = nalgebra::convert(ekf_observation_covariance_pixels);
        let zero = nalgebra::convert(0.0);
        let observation_noise_covariance = OMatrix::<R, U2, U2>::new(r, zero, zero, r);
        Self {
            cam,
            observation_matrix,
            observation_matrix_transpose,
            observation_noise_covariance,
        }
    }
}

impl<R> adskalman::ObservationModel<R, U6, U2> for CameraObservationModel<R>
where
    DefaultAllocator: Allocator<R, U6, U6>,
    DefaultAllocator: Allocator<R, U6>,
    DefaultAllocator: Allocator<R, U2, U6>,
    DefaultAllocator: Allocator<R, U6, U2>,
    DefaultAllocator: Allocator<R, U2, U2>,
    DefaultAllocator: Allocator<R, U2>,
    DefaultAllocator: Allocator<(usize, usize), U2>,
    U2: DimMin<U2, Output = U2>,
    R: RealField + Copy + Default + serde::Serialize,
{
    fn H(&self) -> &OMatrix<R, U2, U6> {
        &self.observation_matrix
    }
    fn HT(&self) -> &OMatrix<R, U6, U2> {
        &self.observation_matrix_transpose
    }
    fn R(&self) -> &OMatrix<R, U2, U2> {
        &self.observation_noise_covariance
    }
    fn predict_observation(&self, state: &OVector<R, U6>) -> OVector<R, U2> {
        // TODO: update to handle water here. See tag "laksdfjasl".
        let pt = to_world_point(state);
        let undistored = self.cam.project_3d_to_pixel(&pt);
        OMatrix::<R, U1, U2>::new(undistored.coords[0], undistored.coords[1]).transpose()
        // This doesn't compile for some reason:
        // OMatrix::<R, U2, U1>::new(undistored.coords[0], undistored.coords[1])
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
    /// the actual detected point
    pub pt: flydra_types::FlydraRawUdpPoint,
}

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
                return other.timestamp.is_none();
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

fn to_world_point<R: RealField + Copy>(vec6: &OVector<R, U6>) -> PointWorldFrame<R> {
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
    /// timestamp from the camera
    pub device_timestamp: Option<std::num::NonZeroU64>,
    /// frame number from the camera
    pub block_id: Option<std::num::NonZeroU64>,
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
        device_timestamp: Option<std::num::NonZeroU64>,
        block_id: Option<std::num::NonZeroU64>,
    ) -> Self {
        let time_delta = Self::make_time_delta(synced_frame, trigger_timestamp.clone());
        let tdpt = TimeDataPassthrough::new(synced_frame, &trigger_timestamp);
        Self {
            cam_name,
            cam_num,
            synced_frame,
            trigger_timestamp,
            cam_received_timestamp,
            device_timestamp,
            block_id,
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

impl FrameDataAndPoints {
    fn into_save(self, save_empty_data2d: bool) -> Vec<Data2dDistortedRowF32> {
        let frame_data = &self.frame_data;
        let pts_to_save: Vec<Data2dDistortedRowF32> = self
            .points
            .iter()
            .map(|orig| convert_to_save(frame_data, orig))
            .collect();

        let data2d_distorted: Vec<Data2dDistortedRowF32> = if !pts_to_save.is_empty() {
            pts_to_save
        } else if save_empty_data2d {
            let empty_data = vec![convert_empty_to_save(frame_data)];
            empty_data
        } else {
            vec![]
        };
        data2d_distorted
    }
}

fn safe_u8(val: usize) -> u8 {
    assert!(val <= u8::max_value() as usize, "value out of range");
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
        device_timestamp: frame_data.device_timestamp,
        block_id: frame_data.block_id,
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
        device_timestamp: frame_data.device_timestamp,
        block_id: frame_data.block_id,
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
/// <https://web.archive.org/web/20070331175701/http://mail.python.org/pipermail/python-list/2001-January/067815.html>
///
/// This is also called the power set:
/// <http://en.wikipedia.org/wiki/Power_set>
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

#[derive(Debug)]
pub struct KalmanEstimateRecord {
    pub record: KalmanEstimatesRow,
    pub data_assoc_rows: Vec<DataAssocRow>,
    pub mean_reproj_dist_100x: Option<u64>,
}

#[derive(Debug)]
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
}

/// Acts like a `csv::Writer` but buffers and orders by frame.
///
/// This is done to allow consumers of the kalman estimates data to iterate
/// through the saved rows assuming that they are ordered. This assumption
/// is easy to implicitly make, so we make it true by doing this.
struct OrderingWriter {
    wtr: csv::Writer<Box<dyn std::io::Write + Send>>,
    buffer: BTreeMap<u64, Vec<KalmanEstimatesRow>>,
}

fn _test_ordering_writer_is_send() {
    // Compile-time test to ensure OrderingWriter implements Send trait.
    fn implements<T: Send>() {}
    implements::<OrderingWriter>();
}

impl OrderingWriter {
    fn new(wtr: csv::Writer<Box<dyn std::io::Write + Send>>) -> Self {
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
            let entry = &mut self.buffer.entry(key).or_insert_with(Vec::new);
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
        let old_buffer = std::mem::take(&mut self.buffer);
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

#[derive(Default)]
struct HistogramWritingState {
    current_store: Option<StartedHistogram<u64>>,
    histograms: Vec<IntervalHistogram<u64>>,
}

fn save_hlog(
    output_dirname: &std::path::Path,
    fname: &str,
    histograms: &mut [IntervalHistogram<u64>],
    file_start_time: std::time::SystemTime,
) {
    // Write the reconstruction latency histograms to disk.
    let mut log_path = output_dirname.to_path_buf();
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
    hist.histogram.record(value)?;

    *hist_store = Some(hist);

    if let Some(accum_dur) = accum_dur {
        if accum_dur.as_secs() >= 60 {
            finish_histogram(hist_store, file_start_time, histograms, now_system)?;
        }
    }
    Ok(())
}

#[derive(Debug)]
pub struct StartSavingCsvConfig {
    pub out_dir: std::path::PathBuf,
    pub local: Option<chrono::DateTime<chrono::Local>>,
    pub git_rev: String,
    pub fps: Option<f32>,
    pub per_cam_data: BTreeMap<RosCamName, flydra_types::PerCamSaveData>,
    pub print_stats: bool,
    pub save_performance_histograms: bool,
}

/// A struct which implements `std::marker::Send` to control coord processing.
///
/// Do not prefer to keep this. Rather, clone the inner `braidz_write_tx`.
pub struct CoordProcessorControl {
    braidz_write_tx: tokio::sync::mpsc::Sender<SaveToDiskMsg>,
}

impl CoordProcessorControl {
    #[inline]
    pub fn new(braidz_write_tx: tokio::sync::mpsc::Sender<SaveToDiskMsg>) -> Self {
        Self { braidz_write_tx }
    }

    pub async fn start_saving_data(&self, cfg: StartSavingCsvConfig) {
        self.braidz_write_tx
            .send(SaveToDiskMsg::StartSavingCsv(cfg))
            .await
            .unwrap();
    }

    pub async fn stop_saving_data(&self) -> Result<()> {
        self.braidz_write_tx
            .send(SaveToDiskMsg::StopSavingCsv)
            .await?;
        Ok(())
    }

    pub async fn append_textlog_message(&self, msg: TextlogRow) {
        self.braidz_write_tx
            .send(SaveToDiskMsg::Textlog(msg))
            .await
            .unwrap();
    }

    pub async fn append_trigger_clock_info_message(&self, msg: TriggerClockInfoRow) {
        self.braidz_write_tx
            .send(SaveToDiskMsg::TriggerClockInfo(msg))
            .await
            .unwrap();
    }

    pub async fn set_experiment_uuid(&self, uuid: String) {
        self.braidz_write_tx
            .send(SaveToDiskMsg::SetExperimentUuid(uuid))
            .await
            .unwrap();
    }
}

#[derive(Debug)]
pub struct CoordProcessorConfig {
    pub tracking_params: TrackingParams,
    pub save_empty_data2d: bool,
    pub ignore_latency: bool,
    pub mini_arena_debug_image_dir: Option<std::path::PathBuf>,
}

// TODO note: currently, clones of `braidz_write_tx` keep the writing task alive
// (and thus prevent it from being dropped and saving files). We should consider
// refactoring this so that mostly only Weak<Sender<_>> copies of `braidz_write_tx`
// are kept and thus that the sender will drop when needed. The alternative (or
// addition) is to have a message which will close the writer's files, as is
// done with `SaveToDiskMsg::StopSavingCsv`.
#[derive(Debug)]
pub struct CoordProcessor {
    pub cam_manager: ConnectedCamerasManager,
    pub recon: Option<flydra_mvg::FlydraMultiCameraSystem<MyFloat>>, // TODO? keep reference
    pub braidz_write_tx: tokio::sync::mpsc::Sender<SaveToDiskMsg>,
    pub writer_join_handle: tokio::task::JoinHandle<Result<()>>,
    model_servers: Vec<tokio::sync::mpsc::Sender<(SendType, TimeDataPassthrough)>>,
    tracking_params: Arc<TrackingParams>,
    /// Images of the "mini arenas" in use.
    ///
    /// One per camera when we have calibrations to do tracking. Empty
    /// otherwise.
    mini_arena_images: std::collections::BTreeMap<String, MiniArenaImage>,
    /// A vector of model collections, one per "mini arena".
    ///
    /// This is behind `Option<>` for reasons I do not remember.
    model_collections: Option<
        Vec<crate::tracking_core::ModelCollection<crate::tracking_core::CollectionFrameDone>>,
    >,
    next_obj_id: Arc<Mutex<u32>>,
}

impl CoordProcessor {
    #[tracing::instrument]
    pub fn new(
        cfg: CoordProcessorConfig,
        handle: tokio::runtime::Handle,
        cam_manager: ConnectedCamerasManager,
        recon: Option<flydra_mvg::FlydraMultiCameraSystem<MyFloat>>,
        metadata_builder: BraidMetadataBuilder,
        valve: stream_cancel::Valve,
    ) -> Result<Self> {
        let CoordProcessorConfig {
            tracking_params,
            save_empty_data2d,
            ignore_latency,
            mini_arena_debug_image_dir,
        } = cfg;

        trace!("CoordProcessor using {:?}", recon);

        let recon2 = recon.clone();

        info!("using TrackingParams {:?}", tracking_params);

        let mini_arena_images = mini_arenas::build_mini_arena_images(
            recon.as_ref(),
            &tracking_params.mini_arena_config,
            mini_arena_debug_image_dir
                .as_ref()
                .map(std::path::PathBuf::as_path),
        )?;

        let tracking_params: Arc<TrackingParams> = Arc::from(tracking_params);
        let tracking_params2 = tracking_params.clone();
        let cam_manager2 = cam_manager.clone();

        let (braidz_write_tx, braidz_write_rx) = tokio::sync::mpsc::channel(10);

        let braidz_write_rx = tokio_stream::wrappers::ReceiverStream::new(braidz_write_rx);
        let braidz_write_rx = valve.wrap(braidz_write_rx);

        let writer_future = writer_task_main(
            braidz_write_rx,
            cam_manager2,
            recon2,
            tracking_params2,
            save_empty_data2d,
            metadata_builder,
            ignore_latency,
        );
        let writer_join_handle = handle.spawn(writer_future);

        Ok(Self {
            cam_manager,
            recon,
            braidz_write_tx,
            writer_join_handle,
            tracking_params,
            model_servers: vec![],
            model_collections: None,
            mini_arena_images,
            next_obj_id: Arc::new(Mutex::new(0)),
        })
    }

    fn new_model_collections(
        &self,
        recon: &flydra_mvg::FlydraMultiCameraSystem<MyFloat>,
        fps: f32,
    ) -> Vec<crate::tracking_core::ModelCollection<crate::tracking_core::CollectionFrameDone>> {
        self.tracking_params
            .mini_arena_config
            .iter_locators()
            .map(|mini_arena_loc| {
                let mini_arena_idx =
                    mini_arenas::MiniArenaIndex::new(mini_arena_loc.idx().unwrap());
                crate::tracking_core::initialize_model_collection(
                    self.tracking_params.clone(),
                    recon.clone(),
                    fps,
                    self.cam_manager.clone(),
                    mini_arena_idx,
                )
            })
            .collect()
    }

    pub fn get_braidz_write_tx(&self) -> tokio::sync::mpsc::Sender<SaveToDiskMsg> {
        self.braidz_write_tx.clone()
    }

    pub fn add_listener(
        &mut self,
        model_server: tokio::sync::mpsc::Sender<(SendType, TimeDataPassthrough)>,
    ) {
        self.model_servers.push(model_server);
    }

    /// Consume the CoordProcessor and the input stream.
    ///
    /// Returns a future that completes when done. The vast majority of the
    /// runtime is done without returning from this function. It is async,
    /// though, and yields many times throughout this execution.
    ///
    /// Upon completion, returns a [tokio::task::JoinHandle] from a spawned
    /// writing task. To ensure data is completely saved, this should be driven
    /// to completion before ending the process.
    #[tracing::instrument]
    pub async fn consume_stream<S>(
        mut self,
        frame_data_rx: S,
        expected_framerate: Option<f32>,
    ) -> tokio::task::JoinHandle<Result<()>>
    where
        S: 'static + Send + futures::stream::Stream<Item = StreamItem> + std::fmt::Debug + Unpin,
    {
        let mut prev_frame = SyncFno(0);
        use futures::stream::StreamExt;

        // As first step, save raw incoming data. The raw data is saved by
        // cloning each packet and sending this to the writing task. A new
        // `Stream<Item = StreamItem>` is returned which simply moves the items
        // from the original stream.
        let stream1 = Box::pin(frame_data_rx.then(|si: StreamItem| async {
            match &si {
                StreamItem::EOF => {}
                StreamItem::Packet(fdp) => {
                    if fdp.frame_data.synced_frame.0 == u64::MAX {
                        // We have seen a bug after making a contiguous stream
                        // (see below) in which the frame number is `u64::MAX`.
                        // This checks if this obviously wrong frame number is
                        // introduced after the present location or before. In
                        // any case, if we are getting frame numbers like this,
                        // clearly we cannot track anymore, so panicing here
                        // only raises the issue slightly earlier.
                        panic!("Impossible frame number with frame data {:?}", fdp);
                    }

                    self.braidz_write_tx
                        .send(SaveToDiskMsg::Data2dDistorted(fdp.clone()))
                        .await
                        .unwrap();
                }
            }
            si
        }));

        // This clones the `Arc` but the inner camera manager remains not
        // cloned.
        let ccm = self.cam_manager.clone();

        info!("Starting model collection and frame bundler.");

        // Start the model collection.

        if let Some(ref recon) = self.recon {
            let fps = expected_framerate.expect("expected_framerate must be set");
            self.model_collections = Some(self.new_model_collections(recon, fps));
            let dummy_time = TimeDataPassthrough {
                frame: SyncFno(0),
                timestamp: None,
            };
            // send calibration here
            let mut flydra_xml_new: Vec<u8> = Vec::new();
            recon
                .to_flydra_xml(&mut flydra_xml_new)
                .expect("to_flydra_xml");
            let flydra_xml_str = std::str::from_utf8(&flydra_xml_new).unwrap();

            for ms in self.model_servers.iter() {
                ms.send((
                    SendType::CalibrationFlydraXml(flydra_xml_str.to_string()),
                    dummy_time.clone(),
                ))
                .await
                .expect("send calibration");
            }
        }

        // Start the frame bundler.

        // This function takes a stream and returns a stream. In the returned
        // stream, it has bundled the camera-by-camera data into all-cam data.
        // Note that this can drop data that is out-of-order, which is why we
        // must save the incoming data before here.
        let bundled =
            bundle_frames(stream1, ccm.clone()).instrument(tracing::info_span!("bundle_frames"));

        // Ensure that there are no skipped frames.
        let mut contiguous_stream =
            make_contiguous(bundled).instrument(tracing::info_span!("contiguous"));

        let mut mini_arena_assignment_debug = std::env::var_os("DEBUG_MINI_ARENAS")
            .map(|fname| mini_arenas::MiniArenaAssignmentDebug::new(&fname).unwrap());

        // In this inner loop, we handle each incoming datum. We spend the vast majority
        // of the runtime in this loop.
        while let Some(bundle) = contiguous_stream.next().await {
            assert!(
                bundle.frame() >= prev_frame,
                "Frame number decreasing? The previously received frame was {}, but now have {}",
                prev_frame,
                bundle.frame()
            );
            prev_frame = bundle.frame();

            // Undistort incoming points and assign to mini arenas.
            let undistorted = if let Some(recon) = &self.recon {
                bundle.undistort_and_split_to_mini_arenas(
                    recon,
                    &self.mini_arena_images,
                    &self.tracking_params.mini_arena_config,
                )
            } else {
                continue;
            };

            if let Some(dbg) = mini_arena_assignment_debug.as_mut() {
                dbg.write_frame(&undistorted).unwrap();
            }

            if let Some(mcs) = &self.model_collections {
                debug_assert_eq!(undistorted.per_mini_arena.len(), mcs.len());
            }

            // TODO: split processing across arenas into multiple threads.
            if let Some(model_collections) = self.model_collections.take() {
                // Across all arenas, predict motion (Kalman prediction step).
                let model_collections = model_collections
                    .into_iter()
                    .map(|mc| mc.predict_motion())
                    .collect::<Vec<_>>();

                let tdpt = &undistorted.tdpt;

                // ---------------------------------
                // ---------------------------------
                // ---------------------------------

                // Across all arenas, compute likelihood of each observation.
                let model_collections = model_collections
                    .into_iter()
                    .zip(undistorted.per_mini_arena.iter())
                    .map(|(mc, arena_bundle)| mc.compute_observation_likes(tdpt, arena_bundle))
                    .collect::<Vec<_>>();

                // Across all arenas, perform data association
                let model_collections_and_unused_observations = model_collections
                    .into_iter()
                    .zip(undistorted.per_mini_arena.into_iter())
                    .map(|(mc, arena_bundle)| {
                        mc.solve_data_association_and_update(tdpt, arena_bundle)
                    })
                    .collect::<Vec<_>>();

                // ---------------------------------
                // ---------------------------------
                // ---------------------------------

                // create new and delete old objects
                let mut model_collections = Vec::new();
                for (mc, unused) in model_collections_and_unused_observations.into_iter() {
                    let (mc, send_msgs, save_msgs) = mc.births_and_deaths(
                        tdpt,
                        unused,
                        || self.next_obj_id_func(),
                        // &self.model_servers,
                    );
                    model_collections.push(mc);
                    for msg in save_msgs.into_iter() {
                        self.braidz_write_tx.send(msg).await.unwrap();
                    }
                    for ms in self.model_servers.iter() {
                        for msg in send_msgs.iter() {
                            ms.send(msg.clone()).await.unwrap();
                        }
                    }
                }

                self.model_collections = Some(model_collections);
            }
        }
        debug!("consume_stream future done");

        self.writer_join_handle
    }

    fn next_obj_id_func(&self) -> u32 {
        let mut guard = self.next_obj_id.lock().unwrap();
        let val: u32 = *guard;
        *guard += 1;
        val
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
        device_timestamp: None,
        block_id: None,
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
        for row in rdr.into_deserialize() {
            let row: Data2dDistortedRow = row.unwrap();
            count += 1;
            assert!(row.x.is_nan());
            assert!(row.y.is_nan());
            assert!(!row.area.is_nan());
            assert_eq!(row.area, 1.0);
        }
        assert_eq!(count, 1);
    }
}
