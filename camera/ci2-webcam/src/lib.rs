// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A [ci2] camera backend for consumer webcams (UVC and similar).
//!
//! This backend is intended as a development convenience so that Strand Camera
//! can be exercised without high-end machine-vision hardware. It is *not* the
//! default backend (Pylon is) and deliberately implements only the subset of
//! the [ci2::Camera] trait that maps cleanly onto a webcam.
//!
//! Frame capture is provided by the [`nokhwa`] crate. The backend is named
//! `webcam` rather than `nokhwa` so the underlying capture library can be
//! swapped later without changing the user-facing backend name.
//!
//! # Supported vs. unsupported features
//!
//! Webcams expose almost none of the controls that machine-vision cameras do.
//! Operations that have no webcam analogue (hardware triggering, exposure time
//! in microseconds, gain in dB, node-map save/load, frame-rate limiting, the
//! generic GenICam feature accessors) return [`ci2::Error::FeatureNotPresent`].
//! Strand Camera's startup path tolerates this error for the values it reads.
//!
//! # Pixel formats
//!
//! Each frame from the webcam (commonly YUYV or MJPEG) is decoded on the host.
//! Both [`PixFmt::RGB8`] and [`PixFmt::Mono8`] are offered; RGB8 is the default.

extern crate machine_vision_formats as formats;

use std::sync::OnceLock;

use nokhwa::{
    Camera,
    pixel_format::{LumaFormat, RgbFormat},
    utils::{CameraIndex, RequestedFormat, RequestedFormatType},
};

use ci2::{
    AcquisitionMode, AutoMode, DynamicFrameWithInfo, HostTimingInfo, TriggerMode, TriggerSelector,
};
use formats::PixFmt;
use strand_dynamic_frame::DynamicFrameOwned;
use tracing::info;

/// Map a [`nokhwa::NokhwaError`] into a [`ci2::Error`].
fn nokhwa_err(e: nokhwa::NokhwaError) -> ci2::Error {
    ci2::Error::BackendError(anyhow::Error::new(e))
}

/// `nokhwa` requires a one-time initialization. On most platforms the callback
/// fires immediately; on macOS it gates on the camera permission prompt. We
/// block until it completes the first time and skip it on subsequent calls.
fn ensure_nokhwa_initialized() -> ci2::Result<()> {
    static GRANTED: OnceLock<bool> = OnceLock::new();

    let granted = *GRANTED.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        nokhwa::nokhwa_initialize(move |granted| {
            let _ = tx.send(granted);
        });
        rx.recv().unwrap_or(false)
    });

    if granted {
        Ok(())
    } else {
        Err(ci2::Error::from(
            "nokhwa initialization failed or camera access was not granted",
        ))
    }
}

/// Enumerate the webcams visible to the native backend.
fn enumerate() -> ci2::Result<Vec<nokhwa::utils::CameraInfo>> {
    ensure_nokhwa_initialized()?;
    let backend = nokhwa::native_api_backend()
        .ok_or_else(|| ci2::Error::from("no native nokhwa backend available"))?;
    nokhwa::query(backend).map_err(nokhwa_err)
}

/// A name we can round-trip through [`ci2::CameraModule::camera`].
///
/// Webcams have no reliable serial number, so the human-readable device name is
/// used as the primary identifier, matching how [`nokhwa`] presents devices.
fn device_name(info: &nokhwa::utils::CameraInfo) -> String {
    info.human_name()
}

pub struct WrappedModule {}

pub fn new_module() -> ci2::Result<WrappedModule> {
    Ok(WrappedModule {})
}

/// The webcam backend keeps no global SDK state that needs tearing down, so the
/// guard is a no-op. It exists to match the shape of the other ci2 backends.
pub struct WebcamTerminateGuard {}

pub fn make_singleton_guard(
    _module: &dyn ci2::CameraModule<CameraType = WrappedCamera, Guard = WebcamTerminateGuard>,
) -> ci2::Result<WebcamTerminateGuard> {
    Ok(WebcamTerminateGuard {})
}

impl<'a> ci2::CameraModule for &'a WrappedModule {
    type CameraType = WrappedCamera;
    type Guard = WebcamTerminateGuard;

    fn name(self: &&'a WrappedModule) -> &'static str {
        "webcam"
    }

    fn camera_infos(self: &&'a WrappedModule) -> ci2::Result<Vec<Box<dyn ci2::CameraInfo>>> {
        let infos = enumerate()?
            .iter()
            .map(|info| {
                let ci: Box<dyn ci2::CameraInfo> = Box::new(WebcamCameraInfo::from_nokhwa(info));
                ci
            })
            .collect();
        Ok(infos)
    }

    fn camera(self: &mut &'a WrappedModule, name: &str) -> ci2::Result<Self::CameraType> {
        WrappedCamera::new(name)
    }

    fn settings_file_extension(&self) -> &str {
        // Webcams have no node map, but a value is required by the trait.
        "txt"
    }
}

#[derive(Debug, Clone)]
struct WebcamCameraInfo {
    name: String,
    serial: String,
    model: String,
    vendor: String,
}

impl WebcamCameraInfo {
    fn from_nokhwa(info: &nokhwa::utils::CameraInfo) -> Self {
        // Webcams do not expose vendor/serial the way GenICam cameras do, so we
        // populate these from the information nokhwa provides.
        Self {
            name: device_name(info),
            serial: index_to_string(info.index()),
            model: info.human_name(),
            vendor: info.description().to_string(),
        }
    }
}

impl ci2::CameraInfo for WebcamCameraInfo {
    fn name(&self) -> &str {
        &self.name
    }
    fn serial(&self) -> &str {
        &self.serial
    }
    fn model(&self) -> &str {
        &self.model
    }
    fn vendor(&self) -> &str {
        &self.vendor
    }
}

fn index_to_string(index: &CameraIndex) -> String {
    match index {
        CameraIndex::Index(i) => i.to_string(),
        CameraIndex::String(s) => s.clone(),
    }
}

pub struct WrappedCamera {
    cam: Camera,
    info: WebcamCameraInfo,
    store_fno: usize,
    /// The pixel format presented to the caller. Frames are decoded to this
    /// format on the host. Only [`PixFmt::RGB8`] and [`PixFmt::Mono8`] are
    /// supported; RGB8 is the default.
    pixel_format: PixFmt,
}

fn _test_camera_is_send() {
    // Compile-time test to ensure WrappedCamera implements Send trait.
    fn implements<T: Send>() {}
    implements::<WrappedCamera>();
}

impl WrappedCamera {
    fn new(name: &str) -> ci2::Result<Self> {
        let devices = enumerate()?;
        if devices.is_empty() {
            return Err(ci2::Error::from("no webcams found"));
        }

        for (i, device) in devices.iter().enumerate() {
            info!("webcam #{i}: {}", device.human_name());
        }

        // Match on the human-readable name first, then fall back to the index
        // string. An empty name selects the first available device.
        let device = if name.is_empty() {
            &devices[0]
        } else {
            devices
                .iter()
                .find(|d| device_name(d) == name || index_to_string(d.index()) == name)
                .ok_or_else(|| ci2::Error::from(format!("could not find webcam \"{name}\"")))?
        };

        let info = WebcamCameraInfo::from_nokhwa(device);

        // Request the highest available frame rate, decoding to RGB on the
        // host. Decoding to mono is also possible from the same stream.
        let requested =
            RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate);
        let cam = Camera::new(device.index().clone(), requested).map_err(nokhwa_err)?;

        info!(
            "opened webcam \"{}\" with format {}",
            info.name,
            cam.camera_format()
        );

        Ok(Self {
            cam,
            info,
            store_fno: 0,
            pixel_format: PixFmt::RGB8,
        })
    }
}

impl ci2::CameraInfo for WrappedCamera {
    fn name(&self) -> &str {
        &self.info.name
    }
    fn serial(&self) -> &str {
        &self.info.serial
    }
    fn model(&self) -> &str {
        &self.info.model
    }
    fn vendor(&self) -> &str {
        &self.info.vendor
    }
}

impl ci2::Camera for WrappedCamera {
    // ----- start: weakly typed but easier to implement API -----
    //
    // Webcams have no GenICam feature tree, so all of these are unsupported.

    fn command_execute(&self, _name: &str, _verify: bool) -> ci2::Result<()> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn feature_bool(&self, _name: &str) -> ci2::Result<bool> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn feature_bool_set(&self, _name: &str, _value: bool) -> ci2::Result<()> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn feature_enum(&self, _name: &str) -> ci2::Result<String> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn feature_enum_set(&self, _name: &str, _value: &str) -> ci2::Result<()> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn feature_float(&self, _name: &str) -> ci2::Result<f64> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn feature_float_set(&self, _name: &str, _value: f64) -> ci2::Result<()> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn feature_int(&self, _name: &str) -> ci2::Result<i64> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn feature_int_set(&self, _name: &str, _value: i64) -> ci2::Result<()> {
        Err(ci2::Error::FeatureNotPresent())
    }

    // ----- end: weakly typed but easier to implement API -----

    fn node_map_load(&self, _settings: &str) -> ci2::Result<()> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn node_map_save(&self) -> ci2::Result<String> {
        Err(ci2::Error::FeatureNotPresent())
    }

    fn width(&self) -> ci2::Result<u32> {
        Ok(self.cam.resolution().width())
    }
    fn height(&self) -> ci2::Result<u32> {
        Ok(self.cam.resolution().height())
    }

    fn pixel_format(&self) -> ci2::Result<PixFmt> {
        Ok(self.pixel_format)
    }
    fn possible_pixel_formats(&self) -> ci2::Result<Vec<PixFmt>> {
        Ok(vec![PixFmt::RGB8, PixFmt::Mono8])
    }
    fn set_pixel_format(&mut self, pixel_format: PixFmt) -> ci2::Result<()> {
        match pixel_format {
            PixFmt::RGB8 | PixFmt::Mono8 => {
                self.pixel_format = pixel_format;
                Ok(())
            }
            other => Err(ci2::Error::from(format!(
                "webcam backend does not support pixel format {other}"
            ))),
        }
    }

    fn exposure_time(&self) -> ci2::Result<f64> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn exposure_time_range(&self) -> ci2::Result<(f64, f64)> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn set_exposure_time(&mut self, _: f64) -> ci2::Result<()> {
        Err(ci2::Error::FeatureNotPresent())
    }

    fn exposure_auto(&self) -> ci2::Result<AutoMode> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn set_exposure_auto(&mut self, _: AutoMode) -> ci2::Result<()> {
        Err(ci2::Error::FeatureNotPresent())
    }

    fn gain(&self) -> ci2::Result<f64> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn gain_range(&self) -> ci2::Result<(f64, f64)> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn set_gain(&mut self, _: f64) -> ci2::Result<()> {
        Err(ci2::Error::FeatureNotPresent())
    }

    fn gain_auto(&self) -> ci2::Result<AutoMode> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn set_gain_auto(&mut self, _: AutoMode) -> ci2::Result<()> {
        Err(ci2::Error::FeatureNotPresent())
    }

    fn trigger_mode(&self) -> ci2::Result<TriggerMode> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn set_trigger_mode(&mut self, _: TriggerMode) -> ci2::Result<()> {
        Err(ci2::Error::FeatureNotPresent())
    }

    fn acquisition_frame_rate_enable(&self) -> ci2::Result<bool> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn set_acquisition_frame_rate_enable(&mut self, _value: bool) -> ci2::Result<()> {
        Err(ci2::Error::FeatureNotPresent())
    }

    fn acquisition_frame_rate(&self) -> ci2::Result<f64> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn acquisition_frame_rate_range(&self) -> ci2::Result<(f64, f64)> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn set_acquisition_frame_rate(&mut self, _value: f64) -> ci2::Result<()> {
        Err(ci2::Error::FeatureNotPresent())
    }

    fn trigger_selector(&self) -> ci2::Result<TriggerSelector> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn set_trigger_selector(&mut self, _: TriggerSelector) -> ci2::Result<()> {
        Err(ci2::Error::FeatureNotPresent())
    }

    fn acquisition_mode(&self) -> ci2::Result<AcquisitionMode> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn set_acquisition_mode(&mut self, _: AcquisitionMode) -> ci2::Result<()> {
        Err(ci2::Error::FeatureNotPresent())
    }

    fn acquisition_start(&mut self) -> ci2::Result<()> {
        self.store_fno = 0;
        self.cam.open_stream().map_err(nokhwa_err)
    }
    fn acquisition_stop(&mut self) -> ci2::Result<()> {
        self.cam.stop_stream().map_err(nokhwa_err)
    }

    fn next_frame(&mut self) -> ci2::Result<DynamicFrameWithInfo> {
        // `nokhwa`'s `frame` call blocks until the next frame is available.
        let buffer = self.cam.frame().map_err(nokhwa_err)?;
        let datetime = chrono::Utc::now();

        let image = match self.pixel_format {
            PixFmt::Mono8 => {
                let decoded = buffer.decode_image::<LumaFormat>().map_err(nokhwa_err)?;
                let (width, height) = (decoded.width(), decoded.height());
                let stride = width as usize;
                DynamicFrameOwned::from_buf(
                    width,
                    height,
                    stride,
                    decoded.into_raw(),
                    PixFmt::Mono8,
                )
            }
            // RGB8 is the default; any unexpected value would have been
            // rejected by `set_pixel_format`.
            _ => {
                let decoded = buffer.decode_image::<RgbFormat>().map_err(nokhwa_err)?;
                let (width, height) = (decoded.width(), decoded.height());
                let stride = width as usize * 3;
                DynamicFrameOwned::from_buf(width, height, stride, decoded.into_raw(), PixFmt::RGB8)
            }
        }
        .ok_or_else(|| ci2::Error::SingleFrameError("decoded frame had invalid layout".into()))?;

        let fno = self.store_fno;
        self.store_fno += 1;

        Ok(DynamicFrameWithInfo {
            image: std::sync::Arc::new(image),
            host_timing: HostTimingInfo { fno, datetime },
            backend_data: None,
        })
    }
}
