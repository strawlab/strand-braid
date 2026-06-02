use crate::{BackgroundUpdateMode, Result, errors::Error, fastim_mod};

use tracing::{debug, error};

use chrono::{DateTime, Utc};
use flydra_feature_detector_types::ImPtDetectCfg;
use machine_vision_formats as formats;

use strand_dynamic_frame::{DynamicFrame, DynamicFrameOwned};

use fastim_mod::{CompareOp, FastImage, FastImageData, FastImageRegion, RoundMode, ripp};

type ToWorker = (DynamicFrameOwned, DateTime<Utc>, ImPtDetectCfg);
type FromWorker = (
    FastImageData<f32>,
    FastImageData<f32>,
    FastImageData<u8>,
    FastImageData<u8>,
    FastImageRegion,
    chrono::DateTime<chrono::Utc>,
);

pub(crate) struct BackgroundModel {
    pub(crate) mean_background: FastImageData<f32>,
    pub(crate) mean_im: FastImageData<u8>,
    pub(crate) mean_squared_im: FastImageData<f32>,
    pub(crate) cmp_im: FastImageData<u8>,
    /// The `diff_threshold` value with which `cmp_im` has already been clamped
    /// (in place) by `do_work`, or `None` if `cmp_im` is freshly installed and
    /// not yet clamped. Used to skip the (idempotent) per-frame clamp whenever
    /// neither `cmp_im` nor `diff_threshold` has changed.
    pub(crate) cmp_thresh_applied: Option<u8>,
    pub(crate) current_roi: FastImageRegion,
    // pub(crate) complete_stamp: (chrono::DateTime<chrono::Utc>, usize),
    pub(crate) complete_stamp: chrono::DateTime<chrono::Utc>,
    updater: Updater,
}

/// How the (expensive) background model recomputation is scheduled relative to
/// per-frame processing.
enum Updater {
    /// The recomputation runs on a dedicated worker thread. The updated model
    /// is applied by whichever [BackgroundModel::poll_complete_updates] call
    /// happens to win the race for the result, so the frame at which the model
    /// changes depends on thread scheduling and is *not* reproducible.
    Asynchronous {
        tx_to_worker: std::sync::mpsc::SyncSender<ToWorker>,
        rx_from_worker: std::sync::mpsc::Receiver<FromWorker>,
    },
    /// The recomputation runs synchronously inside
    /// [BackgroundModel::start_bg_update] and the result is applied by the very
    /// next [BackgroundModel::poll_complete_updates], so the model always
    /// changes at the same frame for identical input (bit-reproducible). Boxed
    /// because the worker state is much larger than the asynchronous variant's
    /// channel handles.
    Synchronous(Box<SyncUpdater>),
}

struct SyncUpdater {
    worker: BackgroundModelWorker,
    pending: Option<FromWorker>,
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
        running_mean: FastImageData<f32>,
        mean_squared_im: FastImageData<f32>,
        cfg: &ImPtDetectCfg,
        pixel_format: formats::PixFmt,
        complete_stamp: chrono::DateTime<chrono::Utc>,
        mode: BackgroundUpdateMode,
    ) -> Result<Self>
    where
        S: FastImage<D = u8>,
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
            cmp_im: FastImageData::<u8>::new(w, h, 0)?,
            current_roi: current_roi.clone(),
        };

        worker.do_bg_update(raw_im_full, cfg)?;
        let (running_mean, mean_squared_im, mean_im, cmp_im, _roi, _ts) =
            worker.snapshot(complete_stamp)?;

        let updater = match mode {
            BackgroundUpdateMode::Synchronous => Updater::Synchronous(Box::new(SyncUpdater {
                worker,
                pending: None,
            })),
            BackgroundUpdateMode::Asynchronous => {
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
                            let (orig_frame, ts, cfg) = x;
                            let frame_ref = orig_frame.borrow();
                            let frame = frame_ref
                                .into_pixel_format::<formats::pixel_format::Mono8>()
                                .unwrap();

                            let raw_im_full = frame;
                            worker.do_bg_update(&raw_im_full, &cfg).expect("bg update");

                            let msg = worker.snapshot(ts).unwrap();
                            match tx_to_main.try_send(msg) {
                                Ok(()) => {}
                                Err(std::sync::mpsc::TrySendError::Full(_msg)) => {
                                    error!("updated background image dropped because pipe full");
                                }
                                Err(std::sync::mpsc::TrySendError::Disconnected(_msg)) => break,
                            }
                        }
                    })?;

                Updater::Asynchronous {
                    tx_to_worker,
                    rx_from_worker,
                }
            }
        };

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
            cmp_thresh_applied: None,
            current_roi,
            updater,
            complete_stamp,
        };
        Ok(result)
    }

    /// Update background model for new image
    pub(crate) fn start_bg_update(
        &mut self,
        frame: &DynamicFrame<'_>,
        cfg: &ImPtDetectCfg,
        ts: DateTime<Utc>,
    ) -> Result<()> {
        match &mut self.updater {
            Updater::Asynchronous { tx_to_worker, .. } => {
                let frame_copy = frame.copy_to_owned();
                match tx_to_worker.try_send((frame_copy, ts, cfg.clone())) {
                    Ok(()) => {}
                    Err(std::sync::mpsc::TrySendError::Full(_msg)) => {
                        error!("not updating background image because pipe full");
                    }
                    Err(std::sync::mpsc::TrySendError::Disconnected(_msg)) => {
                        return Err(Error::BackgroundProcessingThreadDisconnected);
                    }
                }
            }
            Updater::Synchronous(sync) => {
                // Compute the update inline so it is applied at a deterministic
                // frame boundary (by the next `poll_complete_updates`).
                let frame = frame
                    .into_pixel_format::<formats::pixel_format::Mono8>()
                    .unwrap();
                sync.worker.do_bg_update(&frame, cfg)?;
                sync.pending = Some(sync.worker.snapshot(ts)?);
            }
        }
        Ok(())
    }

    /// returns if we got new data
    pub(crate) fn poll_complete_updates(&mut self) -> Result<bool> {
        let msg = match &mut self.updater {
            Updater::Asynchronous { rx_from_worker, .. } => match rx_from_worker.try_recv() {
                Ok(msg) => Some(msg),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err(Error::BackgroundProcessingThreadDisconnected);
                }
            },
            Updater::Synchronous(sync) => sync.pending.take(),
        };
        match msg {
            Some((running_mean, mean_squared_im, mean_im, cmp_im, roi, ts)) => {
                self.mean_background = running_mean;
                self.mean_squared_im = mean_squared_im;
                self.mean_im = mean_im;
                self.cmp_im = cmp_im;
                // Freshly installed cmp_im has not been clamped yet.
                self.cmp_thresh_applied = None;
                self.current_roi = roi;
                self.complete_stamp = ts;
                Ok(true)
            }
            None => Ok(false),
        }
    }
}

struct BackgroundModelWorker {
    mean_background: FastImageData<f32>,
    mean_im: FastImageData<u8>,
    mean_squared_im: FastImageData<f32>,
    cmp_im: FastImageData<u8>,
    current_roi: FastImageRegion,
}

impl BackgroundModelWorker {
    /// Copy the current model state into a message for the main thread.
    fn snapshot(&self, ts: DateTime<Utc>) -> Result<FromWorker> {
        Ok((
            FastImageData::copy_from_32f_c1(&self.mean_background)?,
            FastImageData::copy_from_32f_c1(&self.mean_squared_im)?,
            FastImageData::copy_from_8u_c1(&self.mean_im)?,
            FastImageData::copy_from_8u_c1(&self.cmp_im)?,
            self.current_roi.clone(),
            ts,
        ))
    }

    /// Update background model for new image
    fn do_bg_update<S>(&mut self, raw_im_full: &S, cfg: &ImPtDetectCfg) -> Result<()>
    where
        S: FastImage<D = u8>,
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
        let mut std2 = FastImageData::<f32>::new(w, h, 0.0)?;
        ripp::sub_32f_c1r(
            &mean2,
            &self.mean_squared_im,
            &mut std2,
            self.current_roi.size(),
        )?;

        // running_stdframe = self.cfg.n_sigma * sqrt(|std2|)
        let mut running_stdframe = FastImageData::<f32>::new(w, h, 0.0)?;
        ripp::abs_32f_c1r(&std2, &mut running_stdframe, self.current_roi.size())?;
        ripp::sqrt_32f_c1ir(&mut running_stdframe, self.current_roi.size())?;
        ripp::mul_c_32f_c1ir(cfg.n_sigma, &mut running_stdframe, self.current_roi.size())?;

        // now we do hack, erm, heuristic for bright points, which aren't gaussian.
        let mut noisy_pixels_mask = FastImageData::<u8>::new(w, h, 0)?;
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
