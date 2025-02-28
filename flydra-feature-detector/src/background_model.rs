use crate::{errors::Error, fastim_mod, ipp_ctypes, Result};

use tracing::{debug, error};

use chrono::{DateTime, Utc};
use flydra_feature_detector_types::ImPtDetectCfg;
use machine_vision_formats::{self as formats, ImageData, Stride};

use basic_frame::DynamicFrame;

use fastim_mod::{
    ripp, Chan1, CompareOp, FastImage, FastImageData, FastImageRegion, FastImageView, RoundMode,
};

type ToWorker = (DynamicFrame, DateTime<Utc>, ImPtDetectCfg);
type FromWorker = (
    FastImageData<Chan1, f32>,
    FastImageData<Chan1, f32>,
    FastImageData<Chan1, u8>,
    FastImageData<Chan1, u8>,
    FastImageRegion,
    chrono::DateTime<chrono::Utc>,
);

pub(crate) struct BackgroundModel {
    pub(crate) mean_background: FastImageData<Chan1, f32>,
    pub(crate) mean_im: FastImageData<Chan1, u8>,
    pub(crate) mean_squared_im: FastImageData<Chan1, f32>,
    pub(crate) cmp_im: FastImageData<Chan1, u8>,
    pub(crate) current_roi: FastImageRegion,
    // pub(crate) complete_stamp: (chrono::DateTime<chrono::Utc>, usize),
    pub(crate) complete_stamp: chrono::DateTime<chrono::Utc>,
    tx_to_worker: std::sync::mpsc::SyncSender<ToWorker>,
    rx_from_worker: std::sync::mpsc::Receiver<FromWorker>,
}

impl std::fmt::Debug for BackgroundModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackgroundModel").finish_non_exhaustive()
    }
}

impl BackgroundModel {
    /// Allocate new BackgroundModel
    pub(crate) fn new<S>(
        raw_im_full: &S,
        running_mean: FastImageData<Chan1, f32>,
        mean_squared_im: FastImageData<Chan1, f32>,
        cfg: &ImPtDetectCfg,
        pixel_format: formats::PixFmt,
        complete_stamp: chrono::DateTime<chrono::Utc>,
    ) -> Result<Self>
    where
        S: FastImage<C = Chan1, D = u8>,
    {
        let mean_im = FastImageData::copy_from_32f8u_c1(&running_mean, RoundMode::Near)?;
        let (w, h) = (mean_im.width(), mean_im.height());
        let current_roi = FastImageRegion::new(
            fastim_mod::Point::new(0, 0),
            fastim_mod::FastImageSize::new(w, h),
        );

        let mut worker = BackgroundModelWorker {
            mean_background: running_mean,
            mean_squared_im,
            mean_im,
            cmp_im: FastImageData::<Chan1, u8>::new(w, h, 0)?,
            current_roi: current_roi.clone(),
        };

        worker.do_bg_update(raw_im_full, cfg)?;
        let running_mean = FastImageData::copy_from_32f_c1(&worker.mean_background)?;
        let mean_squared_im = FastImageData::copy_from_32f_c1(&worker.mean_squared_im)?;
        let mean_im = FastImageData::copy_from_8u_c1(&worker.mean_im)?;
        let cmp_im = FastImageData::copy_from_8u_c1(&worker.cmp_im)?;

        let (tx_to_worker, rx_from_main) = std::sync::mpsc::sync_channel::<ToWorker>(10);
        let (tx_to_main, rx_from_worker) = std::sync::mpsc::sync_channel::<FromWorker>(10);

        std::thread::Builder::new()
            .name("bg-img-proc".to_string())
            .spawn(move || {
                loop {
                    let x = match rx_from_main.recv() {
                        Ok(x) => x,
                        Err(e) => {
                            // This is normal when taking a new background image.
                            debug!("disconnect {} ({}:{})", e, file!(), line!());
                            break;
                        }
                    };
                    let (frame, ts, cfg) = x;
                    let data = match &frame {
                        DynamicFrame::Mono8(x) => x.image_data(),
                        DynamicFrame::BayerRG8(x) => x.image_data(),
                        DynamicFrame::BayerGB8(x) => x.image_data(),
                        DynamicFrame::BayerGR8(x) => x.image_data(),
                        DynamicFrame::BayerBG8(x) => x.image_data(),
                        other => {
                            panic!("unsupported format: {}", other.pixel_format());
                        }
                    };
                    let raw_im_full = FastImageView::view_raw(
                        data,
                        frame.stride() as ipp_ctypes::c_int,
                        frame.width() as ipp_ctypes::c_int,
                        frame.height() as ipp_ctypes::c_int,
                    )
                    .expect("view full raw image");

                    worker.do_bg_update(&raw_im_full, &cfg).expect("bg update");

                    let running_mean =
                        FastImageData::copy_from_32f_c1(&worker.mean_background).unwrap();
                    let mean_squared_im =
                        FastImageData::copy_from_32f_c1(&worker.mean_squared_im).unwrap();
                    let mean_im = FastImageData::copy_from_8u_c1(&worker.mean_im).unwrap();
                    let cmp_im = FastImageData::copy_from_8u_c1(&worker.cmp_im).unwrap();

                    let roi = worker.current_roi.clone();
                    let msg = (running_mean, mean_squared_im, mean_im, cmp_im, roi, ts);
                    match tx_to_main.try_send(msg) {
                        Ok(()) => {}
                        Err(std::sync::mpsc::TrySendError::Full(_msg)) => {
                            error!("updated background image dropped because pipe full");
                        }
                        Err(std::sync::mpsc::TrySendError::Disconnected(_msg)) => break,
                    }
                }
            })?;

        let _f32_encoding = {
            use crate::formats::PixFmt::*;

            match pixel_format {
                Mono8 => Mono32f,
                BayerRG8 => BayerRG32f,
                BayerBG8 => BayerBG32f,
                BayerGB8 => BayerGB32f,
                BayerGR8 => BayerGR32f,
                pixel_format => {
                    return Err(Error::UnsupportedPixelFormat { fmt: pixel_format });
                }
            }
        };

        let result = Self {
            mean_background: running_mean,
            mean_squared_im,
            mean_im,
            cmp_im,
            current_roi,
            tx_to_worker,
            rx_from_worker,
            complete_stamp,
        };
        Ok(result)
    }

    /// Update background model for new image
    pub(crate) fn start_bg_update(
        &mut self,
        frame: &DynamicFrame,
        cfg: &ImPtDetectCfg,
        ts: DateTime<Utc>,
    ) -> Result<()> {
        match self.tx_to_worker.try_send((frame.clone(), ts, cfg.clone())) {
            Ok(()) => {}
            Err(std::sync::mpsc::TrySendError::Full(_msg)) => {
                error!("not updating background image because pipe full");
            }
            Err(std::sync::mpsc::TrySendError::Disconnected(_msg)) => {
                return Err(Error::BackgroundProcessingThreadDisconnected);
            }
        }
        Ok(())
    }

    /// returns if we got new data
    pub(crate) fn poll_complete_updates(&mut self) -> Result<bool> {
        match self.rx_from_worker.try_recv() {
            Ok(msg) => {
                let (running_mean, mean_squared_im, mean_im, cmp_im, roi, ts) = msg;
                self.mean_background = running_mean;
                self.mean_squared_im = mean_squared_im;
                self.mean_im = mean_im;
                self.cmp_im = cmp_im;
                self.current_roi = roi;
                self.complete_stamp = ts;
                Ok(true)
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => Ok(false),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Err(Error::BackgroundProcessingThreadDisconnected)
            }
        }
    }
}

struct BackgroundModelWorker {
    mean_background: FastImageData<Chan1, f32>,
    mean_im: FastImageData<Chan1, u8>,
    mean_squared_im: FastImageData<Chan1, f32>,
    cmp_im: FastImageData<Chan1, u8>,
    current_roi: FastImageRegion,
}

impl BackgroundModelWorker {
    /// Update background model for new image
    fn do_bg_update<S>(&mut self, raw_im_full: &S, cfg: &ImPtDetectCfg) -> Result<()>
    where
        S: FastImage<C = Chan1, D = u8>,
    {
        let (w, h) = (self.current_roi.width(), self.current_roi.height());

        ripp::add_weighted_8u32f_c1ir(
            raw_im_full,
            &mut self.mean_background,
            self.current_roi.size(),
            cfg.alpha,
        )?;
        ripp::convert_32f8u_c1r(
            &self.mean_background,
            &mut self.mean_im,
            self.current_roi.size(),
            RoundMode::Near,
        )?;

        let mut this_squared = FastImageData::copy_from_8u32f_c1(raw_im_full)?;
        ripp::sqr_32f_c1ir(&mut this_squared, self.current_roi.size())?;
        ripp::add_weighted_32f_c1ir(
            &this_squared,
            &mut self.mean_squared_im,
            self.current_roi.size(),
            cfg.alpha,
        )?;

        let mut mean2 = FastImageData::copy_from_32f_c1(&self.mean_background)?;
        ripp::sqr_32f_c1ir(&mut mean2, self.current_roi.size())?;

        // std2 = mean_squared_im - mean2
        let mut std2 = FastImageData::<Chan1, f32>::new(w, h, 0.0)?;
        ripp::sub_32f_c1r(
            &mean2,
            &self.mean_squared_im,
            &mut std2,
            self.current_roi.size(),
        )?;

        // running_stdframe = self.cfg.n_sigma * sqrt(|std2|)
        let mut running_stdframe = FastImageData::<Chan1, f32>::new(w, h, 0.0)?;
        ripp::abs_32f_c1r(&std2, &mut running_stdframe, self.current_roi.size())?;
        ripp::sqrt_32f_c1ir(&mut running_stdframe, self.current_roi.size())?;
        ripp::mul_c_32f_c1ir(cfg.n_sigma, &mut running_stdframe, self.current_roi.size())?;

        // now we do hack, erm, heuristic for bright points, which aren't gaussian.
        let mut noisy_pixels_mask = FastImageData::<Chan1, u8>::new(w, h, 0)?;
        ripp::compare_c_8u_c1r(
            &self.mean_im,
            cfg.bright_non_gaussian_cutoff,
            &mut noisy_pixels_mask,
            self.current_roi.size(),
            CompareOp::Greater,
        )?;

        ripp::convert_32f8u_c1r(
            &running_stdframe,
            &mut self.cmp_im,
            self.current_roi.size(),
            RoundMode::Near,
        )?;
        ripp::set_8u_c1mr(
            cfg.bright_non_gaussian_replacement,
            &mut self.cmp_im,
            self.current_roi.size(),
            &noisy_pixels_mask,
        )?;
        Ok(())
    }
}
