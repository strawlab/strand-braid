use tracing::{debug, error, info, warn};

#[cfg(not(any(feature = "do_not_use_ipp", feature = "use_ipp")))]
compile_error!("Need either feature 'do_not_use_ipp' or 'use_ipp' enabled.");

#[cfg(all(feature = "do_not_use_ipp", feature = "use_ipp"))]
compile_error!("Need only one of feature 'do_not_use_ipp' or 'use_ipp' enabled, not both.");

#[cfg(feature = "do_not_use_ipp")]
use fastfreeimage as fastim_mod;

#[cfg(feature = "use_ipp")]
use fastimage as fastim_mod;

use borrow_fastimage::BorrowedFrame;
use tokio::sync::mpsc;

use machine_vision_formats as formats;
use serde::Serialize;

use chrono::{DateTime, Utc};
use std::fs::File;

use fastim_mod::{
    ipp_ctypes, ripp, AlgorithmHint, CompareOp, FastImage, FastImageData, FastImageRegion,
    FastImageSize, FastImageView, MomentState, MutableFastImage, MutableFastImageView,
};

use braid_types::{FlydraFloatTimestampLocal, FlydraRawUdpPacket, FlydraRawUdpPoint, RawCamName};
use strand_dynamic_frame::DynamicFrame;
use ufmf::UFMFWriter;

pub use flydra_feature_detector_types::{ContrastPolarity, ImPtDetectCfg};
use strand_http_video_streaming_types::Shape;

mod borrow_fastimage;
use crate::borrow_fastimage::borrow_fi;

mod background_model;
use crate::background_model::BackgroundModel;

mod errors;
pub use crate::errors::*;

const NUM_BG_START_IMAGES: usize = 20;

fn eigen_2x2_real(a: f64, b: f64, c: f64, d: f64) -> Result<(f64, f64, f64, f64)> {
    if c == 0.0 {
        return Err(Error::DivideByZero);
    }
    let inside = a * a + 4.0 * b * c - 2.0 * a * d + d * d;
    let inside = f64::sqrt(inside);
    let eval_a = 0.5 * (a + d - inside);
    let eval_b = 0.5 * (a + d + inside);
    let evec_a1 = (-a + d + inside) / (-2.0 * c);
    let evec_b1 = (-a + d - inside) / (-2.0 * c);
    Ok((eval_a, evec_a1, eval_b, evec_b1))
}

fn compute_slope(moments: &MomentState) -> Result<(f64, f64)> {
    let uu11 = moments.central(1, 1, 0)?;
    let uu20 = moments.central(2, 0, 0)?;
    let uu02 = moments.central(0, 2, 0)?;
    let (eval_a, evec_a1, eval_b, evec_b1) = eigen_2x2_real(uu20, uu11, uu11, uu02)?;

    let rise = 1.0;
    let (run, eccentricity) = if eval_a > eval_b {
        (evec_a1, eval_a / eval_b)
    } else {
        (evec_b1, eval_b / eval_a)
    };
    let slope = rise / run;
    Ok((slope, eccentricity))
}

#[allow(dead_code)]
#[derive(Serialize)]
enum ImageTrackerState {
    RosInfoRunLoop,
    RosInfoRosFrame,
    RosInfoPeriodicImage,
    RosInfoCamInfo,
    RosInfoPeriodic,
    SendingRosImage,
    TakeNewBG,
    ProcessFrameStart(usize),
    AcquireDuration(f64),
    ProcessFrameTiming(Vec<(f64, u32)>),
    ProcessFrameEnd(usize),
}

#[derive(Debug, PartialEq)]
struct PointInfo {
    inner: braid_types::FlydraRawUdpPoint,
    index_x: ipp_ctypes::c_int,
    index_y: ipp_ctypes::c_int,
    max_value: u8,
}

impl PointInfo {
    fn to_ufmf_region(&self, size: u16) -> ufmf::RectFromCenter {
        ufmf::RectFromCenter::from_xy_wh(
            self.inner.x0_abs as u16,
            self.inner.y0_abs as u16,
            size,
            size,
        )
    }
}

struct TrackingState {
    background: BackgroundModel,
    moments: MomentState,
    absdiff_im: FastImageData<u8>,
    cmpdiff_im: FastImageData<u8>,
    frames_since_background_update: u32,
}

impl std::fmt::Debug for TrackingState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrackingState")
            .field(
                "frames_since_background_update",
                &self.frames_since_background_update,
            )
            .finish_non_exhaustive()
    }
}

impl TrackingState {
    /// Allocate new TrackingState
    fn new<S>(
        raw_im_full: &S,
        running_mean: FastImageData<f32>,
        mean_squared_im: FastImageData<f32>,
        cfg: &ImPtDetectCfg,
        pixel_format: formats::PixFmt,
        complete_stamp: chrono::DateTime<chrono::Utc>,
    ) -> Result<Self>
    where
        S: FastImage<D = u8>,
    {
        let (w, h) = (running_mean.width(), running_mean.height());

        let background = BackgroundModel::new(
            raw_im_full,
            running_mean,
            mean_squared_im,
            cfg,
            pixel_format,
            complete_stamp,
        )?;

        Ok(Self {
            moments: MomentState::new(AlgorithmHint::Fast)?,
            background,
            absdiff_im: FastImageData::<u8>::new(w, h, 0)?,
            cmpdiff_im: FastImageData::<u8>::new(w, h, 0)?,
            frames_since_background_update: 0,
        })
    }

    fn do_work<S1, S2>(
        &mut self,
        raw_im_full: &S1,
        cfg: &ImPtDetectCfg,
        maybe_mask_image: Option<&S2>,
    ) -> Result<Vec<PointInfo>>
    where
        S1: FastImage<D = u8>,
        S2: FastImage<D = u8>,
    {
        let mut all_points_found = Vec::new();

        // Create ROI views of the entire frame. At the moment, this is a low cost noop. However,
        // in the future we may want to divide a high-resolution image into multiple smaller tiles
        // and process those independently. Therefore, we keep these views in the code.
        let raw_im_small = FastImageView::view_region(raw_im_full, &self.background.current_roi)?;
        let mean_im_roi_view =
            FastImageView::view_region(&self.background.mean_im, &self.background.current_roi)?;

        let mut absdiff_im_roi_view =
            MutableFastImageView::view_region(&mut self.absdiff_im, &self.background.current_roi)?;

        // find difference from mean
        match cfg.polarity {
            ContrastPolarity::DetectLight => {
                // absdiff_im = raw_im_small - mean_im
                ripp::sub_8u_c1rsfs(
                    &mean_im_roi_view,
                    &raw_im_small,
                    &mut absdiff_im_roi_view,
                    self.background.current_roi.size(),
                    0,
                )?;
            }
            ContrastPolarity::DetectDark => {
                // absdiff_im = mean_im - raw_im_small
                ripp::sub_8u_c1rsfs(
                    &raw_im_small,
                    &mean_im_roi_view,
                    &mut absdiff_im_roi_view,
                    self.background.current_roi.size(),
                    0,
                )?;
            }
            ContrastPolarity::DetectAbsDiff => {
                // absdiff_im = |mean_im - raw_im_small|
                ripp::abs_diff_8u_c1r(
                    &raw_im_small,
                    &mean_im_roi_view,
                    &mut absdiff_im_roi_view,
                    self.background.current_roi.size(),
                )?;
            }
        }

        // mask unused part of absdiff_im to 0
        if let Some(mask_image) = maybe_mask_image {
            ripp::set_8u_c1mr(
                0,
                &mut absdiff_im_roi_view,
                self.background.current_roi.size(),
                mask_image,
            )?;
        }

        if cfg.use_cmp {
            // clip the minimum comparison value to diff_threshold
            ripp::threshold_val_8u_c1ir(
                &mut self.background.cmp_im,
                self.background.current_roi.size(),
                cfg.diff_threshold,
                cfg.diff_threshold,
                CompareOp::Less,
            )?;
        }

        let origin = fastim_mod::Point::new(0, 0);

        let mut cmpdiff_im_roi_view =
            MutableFastImageView::view_region(&mut self.cmpdiff_im, &self.background.current_roi)?;

        let mut n_found_points = 0;
        while n_found_points < cfg.max_num_points {
            let mut max_std_diff = 0;

            let (max_abs_diff, max_loc) = {
                // find max pixel
                if cfg.use_cmp {
                    // cmpdiff_im = absdiff_im - cmp_im (saturates 8u)
                    ripp::sub_8u_c1rsfs(
                        &self.background.cmp_im,
                        &absdiff_im_roi_view,
                        &mut cmpdiff_im_roi_view,
                        self.background.current_roi.size(),
                        0,
                    )?;

                    let (max_std_diff2, max_loc) = ripp::max_indx_8u_c1r(
                        &cmpdiff_im_roi_view,
                        self.background.current_roi.size(),
                    )?;
                    max_std_diff = max_std_diff2;
                    // value at maximum difference from std
                    let max_abs_diff = absdiff_im_roi_view
                        .pixel_slice(max_loc.y() as usize, max_loc.x() as usize)[0];
                    (max_abs_diff, max_loc)
                } else {
                    ripp::max_indx_8u_c1r(&absdiff_im_roi_view, self.background.current_roi.size())?
                }
            };

            if cfg.use_cmp {
                if max_std_diff == 0 {
                    break; // no valid point found
                }
            } else if max_abs_diff < cfg.diff_threshold {
                break; // no valid point found
            };

            // TODO: absdiff_im_roi2_view is a view into absdiff_im_roi_view, eliminate
            // global coords here.
            let left2 = max_loc.x() - cfg.feature_window_size as ipp_ctypes::c_int
                + self.background.current_roi.left();
            let right2 = max_loc.x()
                + cfg.feature_window_size as ipp_ctypes::c_int
                + self.background.current_roi.left();
            let bottom2 = max_loc.y() - cfg.feature_window_size as ipp_ctypes::c_int
                + self.background.current_roi.bottom();
            let top2 = max_loc.y()
                + cfg.feature_window_size as ipp_ctypes::c_int
                + self.background.current_roi.bottom();

            let left2 = std::cmp::max(left2, self.background.current_roi.left());
            let right2 = std::cmp::min(right2, self.background.current_roi.right());
            let bottom2 = std::cmp::max(bottom2, self.background.current_roi.bottom());
            let top2 = std::cmp::min(top2, self.background.current_roi.top());
            let roi2_sz = FastImageSize::new(right2 - left2, top2 - bottom2);

            let roi2 = FastImageRegion::new(fastim_mod::Point::new(left2, bottom2), roi2_sz);
            {
                let mut absdiff_im_roi2_view =
                    MutableFastImageView::view_region(&mut absdiff_im_roi_view, &roi2)?;

                // (to reduce moment arm:) if pixel < self.clear_fraction*max(pixel): pixel=0
                let clear_despeckle_thresh = (cfg.clear_fraction * max_abs_diff as f32) as u8;
                let clear_despeckle_thresh =
                    std::cmp::max(clear_despeckle_thresh, cfg.despeckle_threshold);

                // Set anything less than clear_despeckle_thresh to zero
                ripp::threshold_val_8u_c1ir(
                    &mut absdiff_im_roi2_view,
                    roi2_sz,
                    clear_despeckle_thresh,
                    0,
                    CompareOp::Less,
                )?;

                {
                    ripp::moments_8u_c1r(&absdiff_im_roi2_view, roi2_sz, &mut self.moments)?;
                    let mu00 = self.moments.spatial(0, 0, 0, &origin)?;

                    if mu00 == 0.0 {
                        break; // no valid point found
                    } else {
                        let area = mu00;

                        let mu10 = self.moments.spatial(1, 0, 0, &origin)?;
                        let mu01 = self.moments.spatial(0, 1, 0, &origin)?;

                        let x0 = mu10 / mu00;
                        let y0 = mu01 / mu00;
                        let maybe_slope_eccentricty = compute_slope(&self.moments).ok();

                        // set x0 and y0 relative to whole frame
                        let x0_abs = x0 + left2 as f64;
                        let y0_abs = y0 + bottom2 as f64;

                        let index_x = max_loc.x();
                        let index_y = max_loc.y();
                        let cur_val =
                            raw_im_full.pixel_slice(max_loc.y() as usize, max_loc.x() as usize)[0];
                        let mean_val = self
                            .background
                            .mean_background
                            .pixel_slice(max_loc.y() as usize, max_loc.x() as usize)[0]
                            as f64;
                        let sumsqf_val = self
                            .background
                            .mean_squared_im
                            .pixel_slice(max_loc.y() as usize, max_loc.x() as usize)[0]
                            as f64;

                        all_points_found.push(PointInfo {
                            inner: braid_types::FlydraRawUdpPoint {
                                x0_abs,
                                y0_abs,
                                area,
                                maybe_slope_eccentricty,
                                cur_val,
                                mean_val,
                                sumsqf_val,
                            },
                            index_x,
                            index_y,
                            max_value: max_abs_diff,
                        });
                        n_found_points += 1;
                    };
                }

                ripp::set_8u_c1r(0, &mut absdiff_im_roi2_view, roi2_sz)?;
            }
        }
        Ok(all_points_found)
    }
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
enum BackgroundAcquisitionState {
    Initialization,
    StartupMode(StartupState),
    ClearToValue(f32),
    NormalUpdates(TrackingState),
    TemporaryHold,
}

struct StartupState {
    n_frames: usize,
    running_mean: FastImageData<f32>,
    mean_squared_im: FastImageData<f32>, // "running_sumsq" in realtime_image_analysis
}

impl std::fmt::Debug for StartupState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StartupState")
            .field("n_frames", &self.n_frames)
            .finish_non_exhaustive()
    }
}

fn save_bg_data(
    ufmf_writer: &mut ufmf::UFMFWriter<std::fs::File>,
    state_background: &background_model::BackgroundModel,
) -> Result<()> {
    use formats::pixel_format::Mono32f;
    let ts = state_background.complete_stamp;
    let mean: BorrowedFrame<Mono32f> = borrow_fi(&state_background.mean_background)?;
    let sumsq: BorrowedFrame<Mono32f> = borrow_fi(&state_background.mean_squared_im)?;
    ufmf_writer.add_keyframe(b"mean", &mean, ts)?;
    ufmf_writer.add_keyframe(b"sumsq", &sumsq, ts)?;
    Ok(())
}

/// Implementation of low-latency feature detector.
///
/// Maintains compatibility with old flydra camera node.
///
/// Most work is done in [Self::process_new_frame].
pub struct FlydraFeatureDetector {
    raw_cam_name: RawCamName,
    cfg: ImPtDetectCfg,
    roi_sz: FastImageSize,
    #[allow(dead_code)]
    last_sent_raw_image_time: std::time::Instant,
    mask_image: Option<FastImageData<u8>>,
    background_update_state: BackgroundAcquisitionState, // command from UI "take a new bg image"
    acquisition_histogram: AcquisitionHistogram,
    acquisition_duration_allowed_imprecision_msec: Option<f64>,

    transmit_feature_detect_settings_tx:
        Option<mpsc::Sender<flydra_feature_detector_types::ImPtDetectCfg>>,
}

#[derive(Debug)]
pub enum UfmfState {
    Starting(String),
    Saving(UFMFWriter<File>),
    Stopped,
}

const NUM_MSEC_BINS: usize = 100;
const WARN_THRESH_MSEC: usize = 60;

struct AcquisitionHistogram {
    raw_cam_name: RawCamName,
    start: std::time::Instant,
    msec_bins: Vec<u32>,
    longest_frame: u64,
    longest_time: f64,
    acquisition_duration_allowed_imprecision_msec: Option<f64>,
}

impl AcquisitionHistogram {
    fn new(
        raw_cam_name: &RawCamName,
        acquisition_duration_allowed_imprecision_msec: Option<f64>,
    ) -> Self {
        Self {
            raw_cam_name: raw_cam_name.clone(),
            start: std::time::Instant::now(),
            msec_bins: vec![0; NUM_MSEC_BINS],
            longest_frame: 0,
            longest_time: 0.0,
            acquisition_duration_allowed_imprecision_msec,
        }
    }
    fn push_new_sample(&mut self, duration_secs: f64, frameno: u64) {
        if duration_secs.is_nan() {
            return;
        }
        let msecs = duration_secs * 1000.0;
        if msecs < 0.0 {
            if let Some(acquisition_duration_allowed_imprecision_msec) =
                self.acquisition_duration_allowed_imprecision_msec
                && msecs < acquisition_duration_allowed_imprecision_msec {
                    // A little bit of deviation is expected occasionally due to
                    // noise in fitting the time measurements, so do not log warning
                    // unless it exceeds 5 msec.
                    error!(
                        "{} frame {} acquisition duration negative? ({} msecs)",
                        self.raw_cam_name.as_str(),
                        frameno,
                        msecs
                    );
                }
            return;
        }
        let bin_num = if msecs > NUM_MSEC_BINS as f64 {
            NUM_MSEC_BINS - 1
        } else {
            msecs as usize
        };
        self.msec_bins[bin_num] += 1;
        if duration_secs > self.longest_time {
            self.longest_time = duration_secs;
            self.longest_frame = frameno;
        }
    }
    fn num_valid_samples(&self) -> u32 {
        self.msec_bins.iter().sum()
    }
    fn is_old(&self) -> bool {
        self.start.elapsed() > std::time::Duration::from_secs(10)
    }
    fn show_stats(&self) {
        if self.num_valid_samples() >= 1 {
            // compute mode (argmax)
            let (argmax, _max) = self.msec_bins.iter().enumerate().fold(
                (0, 0),
                |acc: (usize, u32), (idx, count): (usize, &u32)| {
                    if count > &acc.1 {
                        (idx, *count)
                    } else {
                        acc
                    }
                },
            );

            let mut max = 0;
            for (msec, msec_count) in self.msec_bins.iter().enumerate() {
                if msec_count > &0 {
                    max = msec;
                }
            }
            let max_str = if max == NUM_MSEC_BINS - 1 {
                format!("{}+", max)
            } else {
                format!("{}", max)
            };
            let msg = format!(
                "{} acquisition duration statistics: mode: {} msec, max: {} msec (longest: {})",
                self.raw_cam_name.as_str(),
                argmax,
                max_str,
                self.longest_frame
            );
            if max > WARN_THRESH_MSEC {
                warn!("{}", msg);
            } else {
                debug!("{}", msg);
            }
        }
    }
}

impl FlydraFeatureDetector {
    /// Create new [FlydraFeatureDetector].
    pub fn new(
        raw_cam_name: &RawCamName,
        w: u32,
        h: u32,
        cfg: ImPtDetectCfg,
        transmit_feature_detect_settings_tx: Option<
            mpsc::Sender<flydra_feature_detector_types::ImPtDetectCfg>,
        >,
        acquisition_duration_allowed_imprecision_msec: Option<f64>,
    ) -> Result<Self> {
        let acquisition_histogram =
            AcquisitionHistogram::new(raw_cam_name, acquisition_duration_allowed_imprecision_msec);

        let mut result = Self {
            raw_cam_name: raw_cam_name.clone(),
            cfg,
            roi_sz: FastImageSize::new(w as ipp_ctypes::c_int, h as ipp_ctypes::c_int),
            mask_image: None,
            last_sent_raw_image_time: std::time::Instant::now(),
            background_update_state: BackgroundAcquisitionState::Initialization,
            acquisition_histogram,
            acquisition_duration_allowed_imprecision_msec,
            transmit_feature_detect_settings_tx,
        };

        result.reload_config()?;
        Ok(result)
    }

    pub fn valid_region(&self) -> Shape {
        self.cfg.valid_region.clone()
    }
    pub fn config(&self) -> ImPtDetectCfg {
        self.cfg.clone()
    }
    pub fn set_config(&mut self, cfg: ImPtDetectCfg) -> Result<()> {
        self.cfg = cfg;
        self.reload_config()
    }

    fn reload_config(&mut self) -> Result<()> {
        // Send updated feature detection parameters
        if let Some(sender) = &mut self.transmit_feature_detect_settings_tx {
            sender.try_send(self.cfg.clone()).unwrap();
        }

        self.mask_image = Some(compute_mask_image(self.roi_sz, &self.cfg.valid_region)?);
        Ok(())
    }

    // command from UI to say "take a new bg image"
    pub fn do_take_current_image_as_background(&mut self) -> Result<()> {
        debug!("taking bg image in camera");
        self.background_update_state = BackgroundAcquisitionState::Initialization;
        Ok(())
    }

    // command from UI to say "set bg image to value"
    pub fn do_clear_background(&mut self, value: f32) -> Result<()> {
        debug!("clearing bg image to {}", value);
        self.background_update_state = BackgroundAcquisitionState::ClearToValue(value);
        Ok(())
    }

    /// Detect features of interest and update background model.
    ///
    /// The detected features are returned as a [FlydraRawUdpPacket] in the
    /// returned output tuple.
    ///
    /// A ufmf file can be updated by setting the `ufmf_state` argument to a
    /// value other than [UfmfState::Stopped].
    #[tracing::instrument(level = "debug", skip_all)]
    pub fn process_new_frame(
        &mut self,
        orig_frame: &DynamicFrame<'_>,
        fno: usize,
        timestamp_utc: DateTime<Utc>,
        ufmf_state: UfmfState,
        device_timestamp: Option<u64>,
        block_id: Option<u64>,
        braid_ts: Option<FlydraFloatTimestampLocal<braid_types::Triggerbox>>,
    ) -> Result<(FlydraRawUdpPacket, UfmfState)> {
        let mut saved_bg_image = None;
        let acquire_stamp = FlydraFloatTimestampLocal::from_dt(&timestamp_utc);
        let acquire_duration = match braid_ts {
            Some(ref trigger_stamp) => {
                // If available, the time from trigger pulse to the first code outside
                // the camera driver.
                acquire_stamp.as_f64() - trigger_stamp.as_f64()
            }
            None => f64::NAN,
        };

        self.acquisition_histogram
            .push_new_sample(acquire_duration, fno as u64);

        if self.acquisition_histogram.is_old() {
            self.acquisition_histogram.show_stats();
            self.acquisition_histogram = AcquisitionHistogram::new(
                &self.raw_cam_name,
                self.acquisition_duration_allowed_imprecision_msec,
            );
        }

        let mut do_save_ufmf_bg = false;
        let mut new_ufmf_state = match ufmf_state {
            UfmfState::Starting(dest) => {
                let path = std::path::Path::new(&dest);
                info!("saving UFMF to path {}", path.display());
                let f = std::fs::File::create(path)?;
                let ufmf_writer = UFMFWriter::new(
                    f,
                    cast::u16(orig_frame.width())?,
                    cast::u16(orig_frame.height())?,
                    orig_frame.pixel_format(),
                    Some((orig_frame, timestamp_utc)),
                )?;
                // save current background state when starting ufmf save.
                do_save_ufmf_bg = true;
                UfmfState::Saving(ufmf_writer)
            }
            UfmfState::Saving(ufmf_writer) => UfmfState::Saving(ufmf_writer),
            UfmfState::Stopped => {
                // do nothing
                UfmfState::Stopped
            }
        };

        let frame_ref = orig_frame;
        let frame = frame_ref
            .into_pixel_format::<formats::pixel_format::Mono8>()
            .unwrap();
        let pixel_format =
            machine_vision_formats::pixel_format::pixfmt::<formats::pixel_format::Mono8>().unwrap();

        let raw_im_full = frame;

        if raw_im_full.size() != self.roi_sz {
            return Err(Error::ImageSizeChanged);
        }

        // move state into local variable so we can move it into next state
        let current_update_state = std::mem::replace(
            &mut self.background_update_state,
            BackgroundAcquisitionState::TemporaryHold,
        );

        // Create empty packet for results on this frame, add found points later.
        let mut packet = FlydraRawUdpPacket {
            cam_name: self.raw_cam_name.as_str().to_string(),
            timestamp: braid_ts,
            cam_received_time: acquire_stamp,
            device_timestamp,
            block_id,
            framenumber: fno as i32,
            points: vec![],
        };

        let (results, next_background_update_state) = match current_update_state {
            BackgroundAcquisitionState::TemporaryHold => {
                panic!("unreachable");
            }
            BackgroundAcquisitionState::Initialization => {
                let running_mean = FastImageData::<f32>::copy_from_8u32f_c1(&raw_im_full)?;

                let mut mean_squared_im = FastImageData::<f32>::copy_from_8u32f_c1(&raw_im_full)?;
                ripp::sqr_32f_c1ir(&mut mean_squared_im, self.roi_sz)?;

                let startup_state = StartupState {
                    n_frames: 1,
                    running_mean,
                    mean_squared_im,
                };
                (
                    packet,
                    BackgroundAcquisitionState::StartupMode(startup_state),
                )
            }
            BackgroundAcquisitionState::StartupMode(mut startup_state) => {
                // startup_state: StartupState

                ripp::add_weighted_8u32f_c1ir(
                    &raw_im_full,
                    &mut startup_state.running_mean,
                    self.roi_sz,
                    1.0 / NUM_BG_START_IMAGES as f32,
                )?;

                let mut this_squared = FastImageData::copy_from_8u32f_c1(&raw_im_full)?;
                ripp::sqr_32f_c1ir(&mut this_squared, self.roi_sz)?;
                ripp::add_weighted_32f_c1ir(
                    &this_squared,
                    &mut startup_state.mean_squared_im,
                    self.roi_sz,
                    1.0 / NUM_BG_START_IMAGES as f32,
                )?;

                startup_state.n_frames += 1;
                let complete_stamp = timestamp_utc;

                if startup_state.n_frames >= NUM_BG_START_IMAGES {
                    let state = TrackingState::new(
                        &raw_im_full,
                        startup_state.running_mean,
                        startup_state.mean_squared_im,
                        &self.cfg,
                        pixel_format,
                        complete_stamp,
                    )?;
                    (packet, BackgroundAcquisitionState::NormalUpdates(state))
                } else {
                    (
                        packet,
                        BackgroundAcquisitionState::StartupMode(startup_state),
                    )
                }
            }
            BackgroundAcquisitionState::ClearToValue(value) => {
                let running_mean =
                    FastImageData::<f32>::new(raw_im_full.width(), raw_im_full.height(), value)?;

                let mut mean_squared_im = FastImageData::<f32>::copy_from_32f_c1(&running_mean)?;
                ripp::sqr_32f_c1ir(&mut mean_squared_im, self.roi_sz)?;

                let complete_stamp = timestamp_utc;

                let state = TrackingState::new(
                    &raw_im_full,
                    running_mean,
                    mean_squared_im,
                    &self.cfg,
                    pixel_format,
                    complete_stamp,
                )?;
                debug!("cleared background model to value {}", value);
                (packet, BackgroundAcquisitionState::NormalUpdates(state))
            }
            BackgroundAcquisitionState::NormalUpdates(mut state) => {
                let got_new_bg_data = state.background.poll_complete_updates()?;

                if state.frames_since_background_update >= self.cfg.bg_update_interval {
                    if self.cfg.do_update_background_model {
                        // defer processing bg images until after this frame data sent
                        saved_bg_image = Some(&frame_ref);
                    }
                    state.frames_since_background_update = 0;
                } else {
                    state.frames_since_background_update += 1;
                }
                // The following can take 40+ msec? e.g. 2018-08-29T08:41:19.582785551Z
                let points = state.do_work(&raw_im_full, &self.cfg, self.mask_image.as_ref())?;

                let radius = self.cfg.feature_window_size;
                let point_data: Vec<_> = points
                    .iter()
                    .map(|p| p.to_ufmf_region(radius * 2))
                    .collect();
                if let UfmfState::Saving(ref mut ufmf_writer) = new_ufmf_state {
                    ufmf_writer.add_frame(frame_ref, timestamp_utc, &point_data)?;
                    if do_save_ufmf_bg || got_new_bg_data {
                        save_bg_data(ufmf_writer, &state.background)?;
                    }
                }

                let inner_points: Vec<FlydraRawUdpPoint> =
                    points.iter().map(|pt| pt.inner.clone()).collect();

                packet.points = inner_points;

                // let process_duration = to_f64(utc_now) - preprocess_stamp;
                // trace!("cam {}, frame {}, {} frames since bg update, \
                //     {} points: acquire {:.1} msec, preprocess {:.1} msec, \
                //     process {:.1} msec",
                //         self.ros_cam_name,
                //         corrected_frame,
                //         state.frames_since_background_update,
                //         points.len(),
                //         acquire_duration*1000.0,
                //         preprocess_duration*1000.0,
                //         process_duration*1000.0);

                // let (results, next_background_update_state) =
                (packet, BackgroundAcquisitionState::NormalUpdates(state))
            }
        };
        self.background_update_state = next_background_update_state;

        if let Some(orig_frame) = saved_bg_image {
            if let BackgroundAcquisitionState::NormalUpdates(ref mut state) =
                self.background_update_state
            {
                state
                    .background
                    .start_bg_update(orig_frame, &self.cfg, timestamp_utc)?;
            } else {
                panic!("unreachable");
            }
        }

        Ok((results, new_ufmf_state))
    }
}

pub fn compute_mask_image(roi_sz: FastImageSize, shape: &Shape) -> Result<FastImageData<u8>> {
    // mask_image
    let mask_value = 255;
    let use_value = 0;

    let mut mask_image = FastImageData::<u8>::new(roi_sz.width(), roi_sz.height(), use_value)?;
    let size = mask_image.size();
    let mask_row_iter = mask_image.valid_row_iter_mut(size)?;

    match shape {
        Shape::Everything => {
            // all pixels valid
        }
        Shape::MultipleCircles(circles) => {
            let mut masks = vec![];
            for circle_params in circles.iter() {
                let this_mask = compute_mask_image(roi_sz, &Shape::Circle(circle_params.clone()))?;
                masks.push(this_mask);
            }
            // slow
            for row in 0..roi_sz.height().try_into().unwrap() {
                for col in 0..roi_sz.width().try_into().unwrap() {
                    let val = masks
                        .iter()
                        .map(|mask| mask.pixel_slice(row, col)[0])
                        .min()
                        .unwrap();
                    mask_image.pixel_slice_mut(row, col)[0] = val;
                }
            }
        }
        Shape::Circle(valid) => {
            let r2 = (valid.radius as ipp_ctypes::c_int).pow(2);
            for (i, mask_row) in mask_row_iter.enumerate() {
                let dy2 = (i as ipp_ctypes::c_int - valid.center_y as ipp_ctypes::c_int).pow(2);
                for (j, row_item) in mask_row.iter_mut().enumerate() {
                    let dx2 = (j as ipp_ctypes::c_int - valid.center_x as ipp_ctypes::c_int).pow(2);

                    let this_r2 = dx2 + dy2;
                    if this_r2 >= r2 {
                        *row_item = mask_value;
                    };
                }
            }
        }
        Shape::Polygon(shape) => {
            let shape = parry_geom::mask_from_points(&shape.points);
            let m = nalgebra::geometry::Isometry::identity();
            for (row, mask_row) in mask_row_iter.enumerate() {
                for (col, row_item) in mask_row.iter_mut().enumerate() {
                    let cur_pos = nalgebra::geometry::Point2::new(col as f64, row as f64);
                    use parry2d_f64::query::PointQuery;
                    if shape.distance_to_point(&m, &cur_pos, true) >= 1.0 {
                        // outside polygon
                        *row_item = mask_value;
                    }
                }
            }
        }
    }

    Ok(mask_image)
}

#[test]
fn test_mask_polygon() -> eyre::Result<()> {
    let roi_sz = FastImageSize::new(12, 8);
    let shape = Shape::Polygon(strand_http_video_streaming_types::PolygonParams {
        points: vec![(1.0, 1.0), (10.0, 1.0), (10.0, 6.0), (1.0, 6.0)],
    });
    let mask = compute_mask_image(roi_sz, &shape)?;
    let expected = {
        let mut full = FastImageData::<u8>::new(12, 8, 255)?;
        for row in 1..7 {
            for col in 1..11 {
                full.pixel_slice_mut(row, col)[0] = 0;
            }
        }
        full
    };
    assert_eq!(mask, expected);
    Ok(())
}

#[test]
fn test_mask_circle() -> eyre::Result<()> {
    let roi_sz = FastImageSize::new(13, 9);
    let shape = Shape::Circle(strand_http_video_streaming_types::CircleParams {
        center_x: 6,
        center_y: 4,
        radius: 5,
    });
    let mask = compute_mask_image(roi_sz, &shape)?;
    let expected = {
        let mut full = FastImageData::<u8>::new(13, 9, 255)?;
        for row in [0, 8] {
            for col in 4..9 {
                full.pixel_slice_mut(row, col)[0] = 0;
            }
        }
        for row in [1, 7] {
            for col in 3..10 {
                full.pixel_slice_mut(row, col)[0] = 0;
            }
        }
        for row in 2..7 {
            for col in 2..11 {
                full.pixel_slice_mut(row, col)[0] = 0;
            }
        }
        full
    };
    assert_eq!(mask, expected);
    Ok(())
}

#[test]
fn test_mask_multiple_circles() -> eyre::Result<()> {
    let roi_sz = FastImageSize::new(8, 3);
    let circles = vec![
        strand_http_video_streaming_types::CircleParams {
            center_x: 2,
            center_y: 1,
            radius: 1,
        },
        strand_http_video_streaming_types::CircleParams {
            center_x: 6,
            center_y: 1,
            radius: 1,
        },
    ];
    let shape = Shape::MultipleCircles(circles);
    let mask = compute_mask_image(roi_sz, &shape)?;
    let expected = {
        let mut full = FastImageData::<u8>::new(8, 3, 255)?;
        let row = 1;
        for col in [2, 6] {
            full.pixel_slice_mut(row, col)[0] = 0;
        }

        full
    };
    assert_eq!(mask, expected);
    Ok(())
}
