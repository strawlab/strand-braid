#![recursion_limit = "128"]
#![cfg_attr(feature = "backtrace", feature(backtrace))]

#[macro_use]
extern crate log;

#[cfg(feature = "backtrace")]
use std::backtrace::Backtrace;

use borrow_fastimage::BorrowedFrame;
use futures::{channel::mpsc, stream::StreamExt};

use machine_vision_formats as formats;
use serde::Serialize;

use chrono::{DateTime, Utc};
#[cfg(feature = "debug-images")]
use std::cell::RefCell;
use std::fs::File;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs, UdpSocket};

use ci2_remote_control::CamArg;
use fastimage::{
    ipp_ctypes, ripp, AlgorithmHint, Chan1, CompareOp, FastImage, FastImageData, FastImageRegion,
    FastImageSize, FastImageView, MomentState, MutableFastImage, MutableFastImageView,
};
use rust_cam_bui_types::ClockModel;

use formats::{pixel_format::Mono32f, ImageBuffer, ImageBufferRef, Stride};
use timestamped_frame::{ExtraTimeData, HostTimeData};

use basic_frame::DynamicFrame;
use flydra_types::{
    get_start_ts, FlydraFloatTimestampLocal, FlydraRawUdpPacket, FlydraRawUdpPoint,
    ImageProcessingSteps, MainbrainBuiLocation, RawCamName, RealtimePointsDestAddr, RosCamName,
};
use ufmf::UFMFWriter;

use http_video_streaming_types::Shape;
pub use image_tracker_types::{ContrastPolarity, ImPtDetectCfg};

#[macro_use]
mod macros;

mod borrow_fastimage;
use crate::borrow_fastimage::borrow_fi;

mod background_model;
use crate::background_model::{BackgroundModel, NUM_BG_START_IMAGES};

mod errors;
pub use crate::errors::*;

#[cfg(feature = "debug-images")]
thread_local!(
    static RT_IMAGE_VIEWER_SENDER: RefCell<rt_image_viewer::RtImageViewerSender> =
        RefCell::new(rt_image_viewer::RtImageViewerSender::new().unwrap())
);

fn eigen_2x2_real(a: f64, b: f64, c: f64, d: f64) -> Result<(f64, f64, f64, f64)> {
    if c == 0.0 {
        return Err(Error::DivideByZero(
            #[cfg(feature = "backtrace")]
            Backtrace::capture(),
        ));
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
    inner: flydra_types::FlydraRawUdpPoint,
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
    absdiff_im: FastImageData<Chan1, u8>,
    cmpdiff_im: FastImageData<Chan1, u8>,
    frames_since_background_update: u32,
}

impl TrackingState {
    /// Allocate new TrackingState
    fn new<S>(
        raw_im_full: &S,
        running_mean: FastImageData<Chan1, f32>,
        mean_squared_im: FastImageData<Chan1, f32>,
        cfg: &ImPtDetectCfg,
        pixel_format: formats::PixFmt,
        complete_stamp: (chrono::DateTime<chrono::Utc>, usize),
    ) -> Result<Self>
    where
        S: FastImage<C = Chan1, D = u8>,
    {
        let (w, h) = (running_mean.width(), running_mean.height());

        let background = BackgroundModel::new(
            raw_im_full,
            running_mean,
            mean_squared_im,
            &cfg,
            pixel_format,
            complete_stamp,
        )?;

        Ok(Self {
            moments: MomentState::new(AlgorithmHint::Fast)?,
            background,
            absdiff_im: FastImageData::<Chan1, u8>::new(w, h, 0)?,
            cmpdiff_im: FastImageData::<Chan1, u8>::new(w, h, 0)?,
            frames_since_background_update: 0,
        })
    }

    fn do_work<S1, S2>(
        &mut self,
        // corrected_framenumber: usize,
        raw_im_full: &S1,
        cfg: &ImPtDetectCfg,
        maybe_mask_image: Option<&S2>,
        q1: &std::time::Instant,
        sample_vec: &mut Vec<(f64, u32)>,
    ) -> Result<Vec<PointInfo>>
    where
        S1: FastImage<D = u8, C = Chan1>,
        S2: FastImage<D = u8, C = Chan1>,
    {
        // let q1 = std::time::Instant::now();
        sample_vec.push((dur_to_f64(q1.elapsed()), line!() + 20000));

        let mut all_points_found = Vec::new();

        // Create ROI views of the entire frame. At the moment, this is a low cost noop. However,
        // in the future we may want to divide a high-resolution image into multiple smaller tiles
        // and process those independently. Therefore, we keep these views in the code.
        let raw_im_small = FastImageView::view_region(raw_im_full, &self.background.current_roi);
        let mean_im_roi_view =
            FastImageView::view_region(&self.background.mean_im, &self.background.current_roi);

        let mut absdiff_im_roi_view =
            MutableFastImageView::view_region(&mut self.absdiff_im, &self.background.current_roi);

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
        // let qe1 = dur_to_f64(q1.elapsed());
        sample_vec.push((dur_to_f64(q1.elapsed()), line!() + 20000));

        image_debug!(&absdiff_im_roi_view, "absdiff_im_roi_view");

        // mask unused part of absdiff_im to 0
        if let Some(mask_image) = maybe_mask_image {
            ripp::set_8u_c1mr(
                0,
                &mut absdiff_im_roi_view,
                self.background.current_roi.size(),
                mask_image,
            )?;
        }

        // let qe2 = dur_to_f64(q1.elapsed());
        sample_vec.push((dur_to_f64(q1.elapsed()), line!() + 20000));

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
        // let qe3 = dur_to_f64(q1.elapsed());
        sample_vec.push((dur_to_f64(q1.elapsed()), line!() + 20000));

        let origin = fastimage::Point::new(0, 0);

        let mut cmpdiff_im_roi_view =
            MutableFastImageView::view_region(&mut self.cmpdiff_im, &self.background.current_roi);

        // let mut qe4 = Vec::new();
        // let mut qe5 = Vec::new();
        // let mut qe6 = Vec::new();
        // let mut qe7 = Vec::new();
        // let mut qe8 = Vec::new();
        // let mut qe9 = Vec::new();
        let mut n_found_points = 0;
        while n_found_points < cfg.max_num_points {
            // qe4.push( dur_to_f64(q1.elapsed()) );
            sample_vec.push((dur_to_f64(q1.elapsed()), line!() + 20000));
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
                    // qe5.push( dur_to_f64(q1.elapsed()) );
                    sample_vec.push((dur_to_f64(q1.elapsed()), line!() + 20000));
                    (max_abs_diff, max_loc)
                } else {
                    ripp::max_indx_8u_c1r(&absdiff_im_roi_view, self.background.current_roi.size())?
                }
            };

            if cfg.use_cmp {
                if max_std_diff == 0 {
                    break; // no valid point found
                }
            } else {
                if max_abs_diff < cfg.diff_threshold {
                    break; // no valid point found
                }
            };

            // qe6.push( dur_to_f64(q1.elapsed()) );
            sample_vec.push((dur_to_f64(q1.elapsed()), line!() + 20000));

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
            let roi2_sz = FastImageSize::new(right2 - left2 + 1, top2 - bottom2 + 1);

            let roi2 = FastImageRegion::new(fastimage::Point::new(left2, bottom2), roi2_sz);
            {
                let mut absdiff_im_roi2_view =
                    MutableFastImageView::view_region(&mut absdiff_im_roi_view, &roi2);

                // (to reduce moment arm:) if pixel < self.clear_fraction*max(pixel): pixel=0
                let clear_despeckle_thresh = (cfg.clear_fraction * max_abs_diff as f32) as u8;
                let clear_despeckle_thresh =
                    std::cmp::max(clear_despeckle_thresh, cfg.despeckle_threshold);

                // Set anything less than clear_despeckle_thresh to zero
                ripp::threshold_val_8u_c1ir(
                    &mut absdiff_im_roi2_view,
                    &roi2_sz,
                    clear_despeckle_thresh,
                    0,
                    CompareOp::Less,
                )?;

                // qe7.push( dur_to_f64(q1.elapsed()) );
                sample_vec.push((dur_to_f64(q1.elapsed()), line!() + 20000));

                {
                    ripp::moments_8u_c1r(&absdiff_im_roi2_view, &roi2_sz, &mut self.moments)?;
                    let mu00 = self.moments.spatial(0, 0, 0, &origin)?;
                    // qe8.push( dur_to_f64(q1.elapsed()) );
                    sample_vec.push((dur_to_f64(q1.elapsed()), line!() + 20000));

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

                        // qe9.push( dur_to_f64(q1.elapsed()) );
                        sample_vec.push((dur_to_f64(q1.elapsed()), line!() + 20000));

                        all_points_found.push(PointInfo {
                            inner: flydra_types::FlydraRawUdpPoint {
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

                ripp::set_8u_c1r(0, &mut absdiff_im_roi2_view, &roi2_sz)?;
            }
        }
        // trace!("frame {}: {:.1} {:.1} {:.1} qe4 {:?} qe5 {:?} qe6 {:?} qe7 {:?} qe8 {:?} qe9 {:?}",
        //     corrected_framenumber, qe1, qe2, qe3, qe4, qe5, qe6, qe7, qe8, qe9 );
        Ok(all_points_found)
    }
}

enum BackgroundAcquisitionState {
    Initialization,
    StartupMode(StartupState),
    ClearToValue(f32),
    NormalUpdates(TrackingState),
    TemporaryHold,
}

struct StartupState {
    n_frames: usize,
    running_mean: FastImageData<Chan1, f32>,
    mean_squared_im: FastImageData<Chan1, f32>, // "running_sumsq" in realtime_image_analysis
}

pub enum DatagramSocket {
    Udp(UdpSocket),
    #[cfg(feature = "flydra-uds")]
    Uds(unix_socket::UnixDatagram),
}

impl std::fmt::Debug for DatagramSocket {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            DatagramSocket::Udp(s) => writeln!(fmt, "DatagramSocket::Udp({:?})", s),
            #[cfg(feature = "flydra-uds")]
            DatagramSocket::Uds(s) => writeln!(fmt, "DatagramSocket::Uds({:?})", s),
        }
    }
}

macro_rules! do_send {
    ($sock:expr, $data:expr) => {{
        match $sock.send(&$data) {
            Ok(sz) => {
                if sz != $data.len() {
                    return Err(Error::IncompleteSend(
                        #[cfg(feature = "backtrace")]
                        Backtrace::capture(),
                    ));
                }
            }
            Err(err) => {
                if std::io::ErrorKind::WouldBlock == err.kind() {
                    warn!("dropping socket data");
                } else {
                    error!("error sending socket data: {:?}", err);
                    return Err(err.into());
                }
            }
        }
    }};
}

impl DatagramSocket {
    fn send_complete(&self, x: &[u8]) -> Result<()> {
        use DatagramSocket::*;
        match self {
            Udp(s) => do_send!(s, x),
            #[cfg(feature = "flydra-uds")]
            Uds(s) => do_send!(s, x),
        }
        Ok(())
    }
}

#[inline]
fn to_f64(dtl: DateTime<Utc>) -> f64 {
    datetime_conversion::datetime_to_f64(&dtl)
}

fn save_bg_data(
    ufmf_writer: &mut ufmf::UFMFWriter<std::fs::File>,
    state_background: &background_model::BackgroundModel,
) -> Result<()> {
    let (ts, fno) = state_background.complete_stamp;
    let mean: BorrowedFrame<Mono32f> = borrow_fi(&state_background.mean_background, ts, fno)?;
    let sumsq: BorrowedFrame<Mono32f> = borrow_fi(&state_background.mean_squared_im, ts, fno)?;
    ufmf_writer.add_keyframe(b"mean", &mean)?;
    ufmf_writer.add_keyframe(b"sumsq", &sumsq)?;
    Ok(())
}

#[allow(dead_code)]
pub struct FlyTracker {
    ros_cam_name: RosCamName,
    cfg: ImPtDetectCfg,
    expected_framerate: Option<f32>,
    roi_sz: FastImageSize,
    #[allow(dead_code)]
    last_sent_raw_image_time: std::time::Instant,
    mask_image: Option<FastImageData<Chan1, u8>>,
    background_update_state: BackgroundAcquisitionState, // command from UI "take a new bg image"
    coord_socket: Option<DatagramSocket>,
    clock_model: Option<ClockModel>,
    frame_offset: Option<u64>,
    hack_binning: Option<u8>,
    acquisition_histogram: AcquisitionHistogram,
    #[cfg(feature = "debug-images")]
    debug_thread_cjh: (thread_control::Control, std::thread::JoinHandle<()>),
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
    ros_cam_name: RosCamName,
    start: std::time::Instant,
    msec_bins: Vec<u32>,
    longest_frame: u64,
    longest_time: f64,
}

impl AcquisitionHistogram {
    fn new(ros_cam_name: &RosCamName) -> Self {
        Self {
            ros_cam_name: ros_cam_name.clone(),
            start: std::time::Instant::now(),
            msec_bins: vec![0; NUM_MSEC_BINS],
            longest_frame: 0,
            longest_time: 0.0,
        }
    }
    fn push_new_sample(&mut self, duration_secs: f64, frameno: u64) {
        if duration_secs.is_nan() {
            return;
        }
        let msecs = duration_secs * 1000.0;
        if msecs < 0.0 {
            if msecs < -5.0 {
                // A little bit of deviation is expected occasionally due to
                // noise in fitting the time measurements, so do not log warning
                // unless it exceeds 5 msec.
                error!(
                    "{} frame {} acquisition duration negative? ({} msecs)",
                    self.ros_cam_name.as_str(),
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
        self.msec_bins.iter().fold(0, |acc, el| acc + el)
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
                self.ros_cam_name.as_str(),
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

fn open_destination_addr(camdata_addr: Option<RealtimePointsDestAddr>) -> Result<Option<DatagramSocket>> {
    Ok(match camdata_addr {
        None => None,
        Some(ref dest_addr) => {
            info!("Sending detected coordinates to: {:?}", dest_addr);
            let mut result = None;
            let timeout = std::time::Duration::new(0, 1);

            match dest_addr {
                #[cfg(feature = "flydra-uds")]
                &RealtimePointsDestAddr::UnixDomainSocket(ref uds) => {
                    let socket = unix_socket::UnixDatagram::unbound()?;
                    socket.set_write_timeout(Some(timeout))?;
                    info!("UDS connecting to {:?}", uds.filename);
                    socket.connect(&uds.filename)?;
                    result = Some(DatagramSocket::Uds(socket));
                }
                #[cfg(not(feature = "flydra-uds"))]
                &RealtimePointsDestAddr::UnixDomainSocket(ref _uds) => {
                    return Err(Error::UnixDomainSocketsNotSupported(
                        #[cfg(feature = "backtrace")]
                        Backtrace::capture(),
                    ));
                }
                &RealtimePointsDestAddr::IpAddr(ref dest_ip_addr) => {
                    let dest = format!("{}:{}", dest_ip_addr.ip(), dest_ip_addr.port());
                    for dest_addr in dest.to_socket_addrs()? {
                        // Let OS choose what port to use.
                        let mut src_addr = dest_addr.clone();
                        src_addr.set_port(0);
                        if !dest_addr.ip().is_loopback() {
                            // Let OS choose what IP to use, but preserve V4 or V6.
                            match src_addr {
                                SocketAddr::V4(_) => {
                                    src_addr.set_ip(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)));
                                }
                                SocketAddr::V6(_) => {
                                    src_addr.set_ip(IpAddr::V6(Ipv6Addr::new(
                                        0, 0, 0, 0, 0, 0, 0, 0,
                                    )));
                                }
                            }
                        }

                        let sock = UdpSocket::bind(src_addr)?;
                        sock.set_write_timeout(Some(timeout))?;
                        debug!("UDP connecting to {}", dest);
                        sock.connect(&dest)?;
                        result = Some(DatagramSocket::Udp(sock));
                        break;
                    }
                }
            }
            result
        }
    })
}

impl FlyTracker {
    #[allow(unused_variables)]
    pub fn new(
        handle: &tokio::runtime::Handle,
        orig_cam_name: &RawCamName,
        w: u32,
        h: u32,
        cfg: ImPtDetectCfg,
        cam_args_tx: Option<mpsc::Sender<CamArg>>,
        version_str: String,
        frame_offset: Option<u64>,
        http_camserver_info: flydra_types::CamHttpServerInfo,
        ros_periodic_update_interval: std::time::Duration,
        #[cfg(feature = "debug-images")] debug_addr: std::net::SocketAddr,
        api_http_address: Option<MainbrainBuiLocation>,
        camdata_addr: Option<RealtimePointsDestAddr>,
        transmit_current_image_rx: mpsc::Receiver<Vec<u8>>,
        valve: stream_cancel::Valve,
        #[cfg(feature = "debug-images")] debug_image_server_shutdown_rx: Option<
            tokio::sync::oneshot::Receiver<()>,
        >,
    ) -> Result<Self> {
        #[cfg(feature = "debug-images")]
        let debug_thread_cjh = rt_image_viewer::initialize_rt_image_viewer(
            valve,
            debug_image_server_shutdown_rx,
            b"secret",
            &debug_addr,
        )
        .expect("starting debug image viewer");

        let mut hack_binning = None;

        match std::env::var("RUSTCAM_BIN_PIXELS") {
            Ok(v) => match v.parse::<u8>() {
                Ok(bins) => {
                    hack_binning = Some(bins);
                }
                Err(e) => {
                    return Err(Error::OtherError {
                        msg: format!("could not parse to bins: {:?}", e),
                        #[cfg(feature = "backtrace")]
                        backtrace: std::backtrace::Backtrace::capture(),
                    });
                }
            },
            Err(std::env::VarError::NotPresent) => {}
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err(Error::OtherError {
                    msg: format!("received not unicode env var"),
                    #[cfg(feature = "backtrace")]
                    backtrace: std::backtrace::Backtrace::capture(),
                });
            }
        };

        let ros_cam_name = orig_cam_name.to_ros();

        if let Some(api_http_address) = api_http_address {
            debug!(
                "opening connection to mainbrain api http server {}",
                api_http_address.0.guess_base_url_with_token()
            );

            let ros_cam_name = orig_cam_name.to_ros();

            let fut = register_node_and_update_image(
                api_http_address,
                orig_cam_name.clone(),
                http_camserver_info,
                ros_cam_name,
                transmit_current_image_rx,
            );

            let orig_cam_name = orig_cam_name.clone();
            let f2 = async move {
                let result = fut.await;
                info!(
                    "background image handler for camera '{}' is done.",
                    orig_cam_name.as_str()
                );
                match result {
                    Ok(()) => {}
                    Err(e) => {
                        error!("error: {} ({}:{})", e, file!(), line!());
                    }
                }
            };

            handle.spawn(Box::pin(f2));
        }

        debug!("sending tracked points to {:?}", camdata_addr);
        let coord_socket = open_destination_addr(camdata_addr)?;

        let acquisition_histogram = AcquisitionHistogram::new(&ros_cam_name);

        let mut result = Self {
            ros_cam_name,
            cfg,
            expected_framerate: None,
            roi_sz: FastImageSize::new(w as ipp_ctypes::c_int, h as ipp_ctypes::c_int),
            mask_image: None,
            last_sent_raw_image_time: std::time::Instant::now(),
            background_update_state: BackgroundAcquisitionState::Initialization,
            coord_socket,
            clock_model: None,
            frame_offset,
            hack_binning,
            acquisition_histogram,
            #[cfg(feature = "debug-images")]
            debug_thread_cjh,
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
        self.mask_image = Some(compute_mask_image(&self.roi_sz, &self.cfg.valid_region)?);
        Ok(())
    }

    pub fn set_frame_offset(&mut self, value: u64) -> () {
        debug!("set_frame_offset");
        self.frame_offset = Some(value);
    }

    pub fn set_clock_model(&mut self, cm: Option<ClockModel>) -> () {
        debug!("set_clock_model {:?}", cm);
        self.clock_model = cm;
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
    /// If `self.coord_socket` is set, send the detected features using it.
    pub fn process_new_frame(
        &mut self,
        frame: &DynamicFrame,
        ufmf_state: UfmfState,
        device_timestamp: Option<std::num::NonZeroU64>,
        block_id: Option<std::num::NonZeroU64>,
    ) -> Result<(FlydraRawUdpPacket, UfmfState)> {
        let pixel_format = frame.pixel_format();
        let mut saved_bg_image = None;
        let process_new_frame_start = Utc::now();
        let q1 = std::time::Instant::now();
        let mut sample_vec = Vec::new();
        let acquire_stamp = FlydraFloatTimestampLocal::from_dt(&frame.extra().host_timestamp());
        let opt_trigger_stamp = get_start_ts(
            self.clock_model.as_ref(),
            self.frame_offset,
            frame.extra().host_framenumber() as u64,
        );
        let acquire_duration = match opt_trigger_stamp {
            Some(ref trigger_stamp) => {
                // If available, the time from trigger pulse to the first code outside
                // the camera driver.
                acquire_stamp.as_f64() - trigger_stamp.as_f64()
            }
            None => std::f64::NAN,
        };

        self.acquisition_histogram
            .push_new_sample(acquire_duration, frame.extra().host_framenumber() as u64);

        if self.acquisition_histogram.is_old() {
            self.acquisition_histogram.show_stats();
            self.acquisition_histogram = AcquisitionHistogram::new(&self.ros_cam_name);
        }

        sample_vec.push((dur_to_f64(q1.elapsed()), line!()));

        let preprocess_stamp = to_f64(process_new_frame_start);
        // let preprocess_duration = preprocess_stamp - acquire_stamp;

        let mut do_save_ufmf_bg = false;
        let mut new_ufmf_state = match ufmf_state {
            UfmfState::Starting(dest) => {
                let path = std::path::Path::new(&dest);
                info!("saving UFMF to path {}", path.display());
                let f = std::fs::File::create(&path)?;
                let ufmf_writer = UFMFWriter::new(
                    f,
                    cast::u16(frame.width())?,
                    cast::u16(frame.height())?,
                    frame.pixel_format(),
                    Some(frame),
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

        let raw_im_full = FastImageView::view_raw(
            frame.image_data_without_format(),
            frame.stride() as ipp_ctypes::c_int,
            frame.width() as ipp_ctypes::c_int,
            frame.height() as ipp_ctypes::c_int,
        );
        sample_vec.push((dur_to_f64(q1.elapsed()), line!()));
        image_debug!(&raw_im_full, "raw_im_full");
        sample_vec.push((dur_to_f64(q1.elapsed()), line!()));

        if *raw_im_full.size() != self.roi_sz {
            return Err(Error::ImageSizeChanged(
                #[cfg(feature = "backtrace")]
                Backtrace::capture(),
            ));
        }

        // move state into local variable so we can move it into next state
        let current_update_state = std::mem::replace(
            &mut self.background_update_state,
            BackgroundAcquisitionState::TemporaryHold,
        );

        // Create empty packet for results on this frame, add found points later.
        let mut packet = FlydraRawUdpPacket {
            cam_name: self.ros_cam_name.as_str().to_string(),
            timestamp: opt_trigger_stamp.clone(),
            cam_received_time: acquire_stamp,
            device_timestamp,
            block_id,
            framenumber: frame.extra().host_framenumber() as i32,
            n_frames_skipped: 0, // FIXME TODO XXX FIX THIS, should be n_frames_skipped
            done_camnode_processing: 0.0,
            preprocess_stamp,
            image_processing_steps: ImageProcessingSteps::empty(),
            points: vec![],
        };

        sample_vec.push((dur_to_f64(q1.elapsed()), line!()));
        let (results, next_background_update_state) = match current_update_state {
            BackgroundAcquisitionState::TemporaryHold => {
                panic!("unreachable");
            }
            BackgroundAcquisitionState::Initialization => {
                let running_mean = FastImageData::<Chan1, f32>::copy_from_8u32f_c1(&raw_im_full)?;

                let mut mean_squared_im =
                    FastImageData::<Chan1, f32>::copy_from_8u32f_c1(&raw_im_full)?;
                ripp::sqr_32f_c1ir(&mut mean_squared_im, &self.roi_sz)?;

                let startup_state = StartupState {
                    n_frames: 1,
                    running_mean,
                    mean_squared_im,
                };
                packet.image_processing_steps |= ImageProcessingSteps::BGINIT;
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
                    &self.roi_sz,
                    1.0 / NUM_BG_START_IMAGES as f32,
                )?;

                let mut this_squared = FastImageData::copy_from_8u32f_c1(&raw_im_full)?;
                ripp::sqr_32f_c1ir(&mut this_squared, &self.roi_sz)?;
                ripp::add_weighted_32f_c1ir(
                    &this_squared,
                    &mut startup_state.mean_squared_im,
                    &self.roi_sz,
                    1.0 / NUM_BG_START_IMAGES as f32,
                )?;

                startup_state.n_frames += 1;
                packet.image_processing_steps |= ImageProcessingSteps::BGSTARTUP;
                let complete_stamp = (
                    frame.extra().host_timestamp(),
                    frame.extra().host_framenumber(),
                );

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
                let running_mean = FastImageData::<Chan1, f32>::new(
                    raw_im_full.width(),
                    raw_im_full.height(),
                    value,
                )?;

                let mut mean_squared_im =
                    FastImageData::<Chan1, f32>::copy_from_32f_c1(&running_mean)?;
                ripp::sqr_32f_c1ir(&mut mean_squared_im, &self.roi_sz)?;

                let complete_stamp = (
                    frame.extra().host_timestamp(),
                    frame.extra().host_framenumber(),
                );

                let state = TrackingState::new(
                    &raw_im_full,
                    running_mean,
                    mean_squared_im,
                    &self.cfg,
                    pixel_format,
                    complete_stamp,
                )?;
                debug!("cleared background model to value {}", value);
                packet.image_processing_steps |= ImageProcessingSteps::BGCLEARED;
                (packet, BackgroundAcquisitionState::NormalUpdates(state))
            }
            BackgroundAcquisitionState::NormalUpdates(mut state) => {
                sample_vec.push((dur_to_f64(q1.elapsed()), line!()));
                let got_new_bg_data = state.background.poll_complete_updates();
                sample_vec.push((dur_to_f64(q1.elapsed()), line!()));

                if state.frames_since_background_update >= self.cfg.bg_update_interval {
                    sample_vec.push((dur_to_f64(q1.elapsed()), line!()));
                    if self.cfg.do_update_background_model {
                        sample_vec.push((dur_to_f64(q1.elapsed()), line!()));
                        packet.image_processing_steps |= ImageProcessingSteps::BGUPDATE;
                        // defer processing bg images until after this frame data sent
                        saved_bg_image = Some(frame);
                        sample_vec.push((dur_to_f64(q1.elapsed()), line!()));
                    }
                    state.frames_since_background_update = 0;
                } else {
                    state.frames_since_background_update += 1;
                }
                sample_vec.push((dur_to_f64(q1.elapsed()), line!()));
                // The following can take 40+ msec? e.g. 2018-08-29T08:41:19.582785551Z
                let points = if let Some(ref mask_image) = self.mask_image {
                    state.do_work(
                        //corrected_frame,
                        &raw_im_full,
                        &self.cfg,
                        Some(mask_image),
                        &q1,
                        &mut sample_vec,
                    )?
                } else {
                    state.do_work::<_, FastImageData<Chan1, u8>>(
                        // corrected_frame,
                        &raw_im_full,
                        &self.cfg,
                        None,
                        &q1,
                        &mut sample_vec,
                    )?
                };
                sample_vec.push((dur_to_f64(q1.elapsed()), line!()));

                let radius = self.cfg.feature_window_size;
                let point_data: Vec<_> = points
                    .iter()
                    .map(|p| p.to_ufmf_region(radius * 2))
                    .collect();
                if let UfmfState::Saving(ref mut ufmf_writer) = new_ufmf_state {
                    ufmf_writer.add_frame(&frame, &point_data)?;
                    if do_save_ufmf_bg || got_new_bg_data {
                        save_bg_data(ufmf_writer, &state.background)?;
                    }
                }
                packet.image_processing_steps |= ImageProcessingSteps::BGNORMAL;

                let inner_points: Vec<FlydraRawUdpPoint> =
                    points.iter().map(|pt| pt.inner.clone()).collect();

                let utc_now = Utc::now();

                packet.points = inner_points;
                packet.done_camnode_processing = to_f64(utc_now);

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

                sample_vec.push((dur_to_f64(q1.elapsed()), line!()));

                if let Some(ref coord_socket) = self.coord_socket {
                    // Send the data to the mainbrain
                    let data: Vec<u8> = serde_cbor::ser::to_vec_packed_sd(&packet)?;
                    coord_socket.send_complete(&data)?;
                }
                sample_vec.push((dur_to_f64(q1.elapsed()), line!()));

                (packet, BackgroundAcquisitionState::NormalUpdates(state))
            }
        };
        self.background_update_state = next_background_update_state;

        sample_vec.push((dur_to_f64(q1.elapsed()), line!()));

        if let Some(frame) = saved_bg_image {
            if let BackgroundAcquisitionState::NormalUpdates(ref mut state) =
                self.background_update_state
            {
                state
                    .background
                    .start_bg_update(frame, &self.cfg, &q1, &mut sample_vec)?;
                sample_vec.push((dur_to_f64(q1.elapsed()), line!()));
            } else {
                panic!("unreachable");
            }
        }

        Ok((results, new_ufmf_state))
    }
}

async fn register_node_and_update_image(
    api_http_address: flydra_types::MainbrainBuiLocation,
    orig_cam_name: flydra_types::RawCamName,
    http_camserver_info: flydra_types::CamHttpServerInfo,
    ros_cam_name: RosCamName,
    mut transmit_current_image_rx: mpsc::Receiver<Vec<u8>>,
) -> Result<()> {
    let mut mainbrain_session =
        braid_http_session::mainbrain_future_session(api_http_address).await?;
    mainbrain_session
        .register_flydra_camnode(orig_cam_name, http_camserver_info, ros_cam_name.clone())
        .await?;
    while let Some(image_png_vecu8) = transmit_current_image_rx.next().await {
        mainbrain_session
            .update_image(ros_cam_name.clone(), image_png_vecu8)
            .await?;
    }
    info!(
        "done listening for background images from {}",
        ros_cam_name.as_str()
    );
    Ok(())
}

pub fn compute_mask_image(
    roi_sz: &FastImageSize,
    shape: &Shape,
) -> Result<FastImageData<Chan1, u8>> {
    // mask_image
    let mask_value = 255;
    let use_value = 0;

    let mut mask_image =
        FastImageData::<Chan1, u8>::new(roi_sz.width(), roi_sz.height(), use_value)?;
    let width = mask_image.width() as usize;

    match shape {
        &Shape::Everything => {
            // all pixels valid
        }
        &Shape::Circle(ref valid) => {
            let r2 = (valid.radius as ipp_ctypes::c_int).pow(2);
            for i in 0..mask_image.height() as ipp_ctypes::c_int {
                let dy2 = (i - valid.center_y as ipp_ctypes::c_int).pow(2);
                let row_slice = mask_image.row_slice_mut(i as usize);

                for j in 0..width {
                    let dx2 = (j as ipp_ctypes::c_int - valid.center_x as ipp_ctypes::c_int).pow(2);

                    let this_r2 = dx2 + dy2;
                    if this_r2 >= r2 {
                        row_slice[j] = mask_value;
                    };
                }
            }
        }
        &Shape::Polygon(ref shape) => {
            let shape = ncollide_geom::mask_from_points(&shape.points);
            let m = nalgebra::geometry::Isometry::identity();
            for row in 0..mask_image.height() {
                let row_slice = mask_image.row_slice_mut(row as usize);
                for col in 0..width {
                    let cur_pos = nalgebra::geometry::Point2::new(col as f64, row as f64);
                    use ncollide2d::query::point_query::PointQuery;
                    if shape.distance_to_point(&m, &cur_pos, true) >= 1.0 {
                        // outside polygon
                        row_slice[col] = mask_value;
                    }
                }
            }
        }
    }

    Ok(mask_image)
}

pub(crate) fn dur_to_f64(duration: std::time::Duration) -> f64 {
    (duration.as_secs() as f64 + duration.subsec_nanos() as f64 * 1e-9) * 1000.0
}
