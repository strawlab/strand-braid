#![cfg_attr(feature = "backtrace", feature(backtrace))]

extern crate machine_vision_formats as formats;

#[cfg(feature = "backtrace")]
use std::backtrace::Backtrace;

use anyhow::Context;
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use std::convert::TryInto;
use std::sync::Arc;

use basic_frame::DynamicFrame;
use ci2::{AcquisitionMode, AutoMode, TriggerMode, TriggerSelector};
use machine_vision_formats::{ImageBuffer, ImageBufferRef};
use pylon_cxx::{HasProperties, NodeMap};
use timestamped_frame::HostTimeData;

trait ExtendedError<T> {
    fn map_pylon_err(self) -> ci2::Result<T>;
}

impl<T> ExtendedError<T> for std::result::Result<T, pylon_cxx::PylonError> {
    fn map_pylon_err(self) -> ci2::Result<T> {
        self.map_err(|e| ci2::Error::BackendError(e.into()))
    }
}

pub type Result<M> = std::result::Result<M, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Pylon error: {source}")]
    PylonError {
        #[from]
        source: pylon_cxx::PylonError,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("int parse error: {source}")]
    IntParseError {
        #[from]
        source: std::num::ParseIntError,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("other error: {msg}")]
    OtherError {
        msg: String,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
}

impl From<Error> for ci2::Error {
    fn from(orig: Error) -> ci2::Error {
        ci2::Error::BackendError(orig.into())
    }
}

pub struct WrappedModule {
    #[allow(dead_code)]
    pylon_auto_init: pylon_cxx::PylonAutoInit,
}

fn to_name(info: &pylon_cxx::DeviceInfo) -> String {
    // TODO: make ci2 cameras have full_name and friendly_name attributes?
    // &info.property_value("FullName").unwrap()
    let serial = &info.property_value("SerialNumber").unwrap();
    let vendor = &info.property_value("VendorName").unwrap();
    format!("{}-{}", vendor, serial)
}

pub fn new_module() -> ci2::Result<WrappedModule> {
    Ok(WrappedModule {
        pylon_auto_init: pylon_cxx::PylonAutoInit::new(),
    })
}

impl ci2::CameraModule for WrappedModule {
    type CameraType = WrappedCamera;

    fn name(&self) -> &str {
        "pyloncxx"
    }
    fn camera_infos(&self) -> ci2::Result<Vec<Box<dyn ci2::CameraInfo>>> {
        let pylon_infos = pylon_cxx::TlFactory::instance()
            .enumerate_devices()
            .map_pylon_err()
            .context("enumerate_devices")?;
        let infos = pylon_infos
            .into_iter()
            .map(|info| {
                let serial = info.property_value("SerialNumber").unwrap();
                let model = info.property_value("ModelName").unwrap();
                let vendor = info.property_value("VendorName").unwrap();
                let name = to_name(&info);
                let pci = Box::new(PylonCameraInfo {
                    name,
                    serial,
                    model,
                    vendor,
                });
                let ci: Box<dyn ci2::CameraInfo> = pci; // explicitly perform type erasure
                ci
            })
            .collect();
        Ok(infos)
    }
    fn camera(&mut self, name: &str) -> ci2::Result<Self::CameraType> {
        WrappedCamera::new(name)
    }
}

/// Raw data and associated metadata from an acquired frame.
#[derive(Clone)]
pub struct Frame {
    /// number of pixels in an image row
    width: u32,
    /// number of pixels in an image column
    height: u32,
    /// number of bytes in an image row
    stride: u32,
    image_data: Vec<u8>,                           // raw image data
    host_timestamp: chrono::DateTime<chrono::Utc>, // timestamp from host computer
    host_framenumber: usize,                       // framenumber from host computer
    pixel_format: formats::PixFmt,                 // format of the data
    pub block_id: u64,                             // framenumber from the camera driver
    pub device_timestamp: u64,                     // timestamp from the camera driver
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "Frame {{ width: {}, height: {}, block_id: {}, device_timestamp: {} }}",
            self.width, self.height, self.block_id, self.device_timestamp
        )
    }
}

impl timestamped_frame::HostTimeData for Frame {
    fn host_timestamp(&self) -> chrono::DateTime<chrono::Utc> {
        self.host_timestamp
    }
    fn host_framenumber(&self) -> usize {
        self.host_framenumber
    }
}

impl<F> formats::ImageData<F> for Frame {
    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.height
    }
    fn buffer_ref(&self) -> ImageBufferRef<'_, F> {
        ImageBufferRef::new(&self.image_data)
    }
    fn buffer(self) -> ImageBuffer<F> {
        ImageBuffer::new(self.image_data)
    }
}

impl formats::Stride for Frame {
    fn stride(&self) -> usize {
        self.stride as usize
    }
}

impl From<Frame> for Vec<u8> {
    fn from(orig: Frame) -> Vec<u8> {
        orig.image_data
    }
}

impl From<Box<Frame>> for Vec<u8> {
    fn from(orig: Box<Frame>) -> Vec<u8> {
        orig.image_data
    }
}

#[derive(Debug)]
struct PylonCameraInfo {
    name: String,
    serial: String,
    model: String,
    vendor: String,
}

impl ci2::CameraInfo for PylonCameraInfo {
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

#[derive(Clone)]
struct FramecountExtra {
    epoch: u32,
    previous_block_id: u64,
    store_fno: u64,
    last_rollover: u64,
}

#[allow(dead_code)]
#[derive(Clone)]
enum FramecoutingMethod {
    TrustDevice,
    BaslerGigE(FramecountExtra),
    IgnoreDevice(usize),
}

#[derive(Clone)]
pub struct WrappedCamera {
    pylon_auto_init: Arc<Mutex<pylon_cxx::PylonAutoInit>>,
    inner: Arc<Mutex<pylon_cxx::InstantCamera>>,
    framecounting_method: FramecoutingMethod,
    device_info: pylon_cxx::DeviceInfo,
    name: String,
    serial: String,
    model: String,
    vendor: String,
    grab_result: Arc<Mutex<pylon_cxx::GrabResult>>,
    is_sfnc2: bool,
}

fn _test_camera_is_send() {
    // Compile-time test to ensure WrappedCamera implements Send trait.
    fn implements<T: Send>() {}
    implements::<WrappedCamera>();
}

impl WrappedCamera {
    fn new(name: &str) -> ci2::Result<Self> {
        let tl_factory = pylon_cxx::TlFactory::instance();
        let devices = tl_factory
            .enumerate_devices()
            .context("enumerate_devices")?;

        for device_info in devices.into_iter() {
            let this_name = to_name(&device_info);
            if this_name == name {
                let serial = device_info
                    .property_value("SerialNumber")
                    .context("getting serial")?;
                let model = device_info
                    .property_value("ModelName")
                    .context("getting model")?;
                let vendor = device_info
                    .property_value("VendorName")
                    .context("getting vendor")?;
                let device_class = device_info
                    .property_value("DeviceClass")
                    .context("getting device class")?;
                let framecounting_method = if &device_class == "BaslerGigE" {
                    FramecoutingMethod::BaslerGigE(FramecountExtra {
                        epoch: 0,
                        previous_block_id: 0,
                        store_fno: 0,
                        last_rollover: 0,
                    })
                } else {
                    FramecoutingMethod::TrustDevice
                };

                let cam = tl_factory
                    .create_device(&device_info)
                    .context("creating device")?;
                cam.open().context("opening camera")?;

                let is_sfnc2 = match cam
                    .integer_node("DeviceSFNCVersionMajor")
                    .map_pylon_err()?
                    .value()
                {
                    Ok(major) => (major >= 2),
                    Err(_) => (false),
                };

                let grab_result =
                    Arc::new(Mutex::new(pylon_cxx::GrabResult::new().map_pylon_err()?));
                return Ok(Self {
                    pylon_auto_init: Arc::new(Mutex::new(pylon_cxx::PylonAutoInit::new())),
                    inner: Arc::new(Mutex::new(cam)),
                    name: name.to_string(),
                    framecounting_method,
                    device_info,
                    serial,
                    model,
                    vendor,
                    grab_result,
                    is_sfnc2,
                });
            }
        }
        return Err(Error::OtherError {
            msg: format!("requested camera '{}' was not found", name),
            #[cfg(feature = "backtrace")]
            backtrace: std::backtrace::Backtrace::capture(),
        }
        .into());
    }

    fn exposure_time_param_name(&self) -> &'static str {
        if self.is_sfnc2 {
            "ExposureTime"
        } else {
            "ExposureTimeAbs"
        }
    }

    fn acquisition_frame_rate_name(&self) -> &'static str {
        if self.is_sfnc2 {
            "AcquisitionFrameRate"
        } else {
            "AcquisitionFrameRateAbs"
        }
    }
}

impl ci2::CameraInfo for WrappedCamera {
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

impl ci2::Camera for WrappedCamera {
    // ----- start: weakly typed but easier to implement API -----

    // fn feature_access_query(&self, name: &str) -> ci2::Result<ci2::AccessQueryResult> {
    //     todo!();
    // }

    fn feature_enum_set(&self, name: &str, value: &str) -> ci2::Result<()> {
        let camera = self.inner.lock();
        let mut node = camera.enum_node(name).map_pylon_err()?;
        node.set_value(value).map_pylon_err()
    }

    // ----- end: weakly typed but easier to implement API -----

    /// Return the sensor width in pixels
    fn width(&self) -> ci2::Result<u32> {
        Ok(self
            .inner
            .lock()
            .integer_node("Width")
            .map_pylon_err()?
            .value()
            .map_pylon_err()?
            .try_into()?)
    }
    /// Return the sensor height in pixels
    fn height(&self) -> ci2::Result<u32> {
        Ok(self
            .inner
            .lock()
            .integer_node("Height")
            .map_pylon_err()?
            .value()
            .map_pylon_err()?
            .try_into()?)
    }

    // Settings: PixFmt ----------------------------
    fn pixel_format(&self) -> ci2::Result<formats::PixFmt> {
        let camera = self.inner.lock();
        let pixel_format_node = camera.enum_node("PixelFormat").map_pylon_err()?;
        convert_to_pixel_format(pixel_format_node.value().map_pylon_err()?.as_ref())
    }
    fn possible_pixel_formats(&self) -> ci2::Result<Vec<formats::PixFmt>> {
        let camera = self.inner.lock();
        let pixel_format_node = camera.enum_node("PixelFormat").map_pylon_err()?;
        // This version returns only the formats we know, silently dropping the unknowns.
        Ok(pixel_format_node
            .settable_values()
            .map_pylon_err()?
            .iter()
            .filter_map(|string_val| convert_to_pixel_format(string_val).ok())
            .collect::<Vec<formats::PixFmt>>())
        // This version returns only the formats we know, returning an error if an unknown is found.
        // Ok(pixel_format_node
        //     .settable_values()
        //     .map_pylon_err()?
        //     .iter()
        //     .map(|string_val| convert_to_pixel_format(string_val))
        //     .collect::<ci2::Result<Vec<formats::PixFmt>>>()?)
    }
    fn set_pixel_format(&mut self, pixel_format: formats::PixFmt) -> ci2::Result<()> {
        let s = convert_pixel_format(pixel_format)?;
        let camera = self.inner.lock();
        let mut pixel_format_node = camera.enum_node("PixelFormat").map_pylon_err()?;
        pixel_format_node.set_value(s).map_pylon_err()
    }

    // Settings: Exposure Time ----------------------------
    /// value given in microseconds
    fn exposure_time(&self) -> ci2::Result<f64> {
        let camera = self.inner.lock();
        let node = camera
            .float_node(self.exposure_time_param_name())
            .map_pylon_err()?;
        node.value().map_pylon_err()
    }
    /// value given in microseconds
    fn exposure_time_range(&self) -> ci2::Result<(f64, f64)> {
        let camera = self.inner.lock();
        let node = camera
            .float_node(self.exposure_time_param_name())
            .map_pylon_err()?;
        Ok((node.min().map_pylon_err()?, node.max().map_pylon_err()?))
    }
    /// value given in microseconds
    fn set_exposure_time(&mut self, value: f64) -> ci2::Result<()> {
        self.inner
            .lock()
            .float_node(self.exposure_time_param_name())
            .map_pylon_err()?
            .set_value(value)
            .map_pylon_err()
    }

    // Settings: Exposure Time Auto Mode ----------------------------
    fn exposure_auto(&self) -> ci2::Result<AutoMode> {
        let camera = self.inner.lock();
        let val = camera
            .enum_node("ExposureAuto")
            .map_pylon_err()?
            .value()
            .map_pylon_err()?;
        str_to_auto_mode(val.as_ref())
    }
    fn set_exposure_auto(&mut self, value: AutoMode) -> ci2::Result<()> {
        let sval = mode_to_str(value);
        self.inner
            .lock()
            .enum_node("ExposureAuto")
            .map_pylon_err()?
            .set_value(sval)
            .map_pylon_err()
    }

    // Settings: Gain ----------------------------
    /// value given in dB
    fn gain(&self) -> ci2::Result<f64> {
        let camera = self.inner.lock();
        if self.is_sfnc2 {
            camera
                .float_node("Gain")
                .map_pylon_err()?
                .value()
                .map_pylon_err()
        } else {
            let gain_raw = camera
                .integer_node("GainRaw")
                .map_pylon_err()?
                .value()
                .map_pylon_err()?;

            let gain_db = gain_raw_to_db(gain_raw)?;
            // debug!("got gain raw {}, converted to db {}", gain_raw, gain_db);
            Ok(gain_db as f64)
        }
    }
    /// value given in dB
    fn gain_range(&self) -> ci2::Result<(f64, f64)> {
        let camera = self.inner.lock();
        if self.is_sfnc2 {
            let gain_node = camera.float_node("Gain").map_pylon_err()?;
            Ok((
                gain_node.min().map_pylon_err()?,
                gain_node.max().map_pylon_err()?,
            ))
        } else {
            let gain_node = camera.integer_node("GainRaw").map_pylon_err()?;

            let gain_min = gain_node.min().map_pylon_err()?;
            let gain_max = gain_node.max().map_pylon_err()?;

            let gain_min_db = gain_raw_to_db(gain_min)?;
            let gain_max_db = gain_raw_to_db(gain_max)?;
            Ok((gain_min_db, gain_max_db))
        }
    }

    /// value given in dB
    fn set_gain(&mut self, gain_db: f64) -> ci2::Result<()> {
        let camera = self.inner.lock();
        if self.is_sfnc2 {
            camera
                .float_node("Gain")
                .map_pylon_err()?
                .set_value(gain_db)
                .map_pylon_err()?;
        } else {
            let gain_raw = gain_db_to_raw(gain_db)?;
            camera
                .integer_node("GainRaw")
                .map_pylon_err()?
                .set_value(gain_raw)
                .map_pylon_err()?;
        }
        Ok(())
    }

    // Settings: Gain Auto Mode ----------------------------
    fn gain_auto(&self) -> ci2::Result<AutoMode> {
        let camera = self.inner.lock();
        let val = camera
            .enum_node("GainAuto")
            .map_pylon_err()?
            .value()
            .map_pylon_err()?;
        str_to_auto_mode(val.as_ref())
    }

    fn set_gain_auto(&mut self, value: AutoMode) -> ci2::Result<()> {
        let sval = mode_to_str(value);
        self.inner
            .lock()
            .enum_node("GainAuto")
            .map_pylon_err()?
            .set_value(sval)
            .map_pylon_err()
    }

    // Settings: TriggerMode ----------------------------
    fn trigger_mode(&self) -> ci2::Result<TriggerMode> {
        let camera = self.inner.lock();
        let val = camera
            .enum_node("TriggerMode")
            .map_pylon_err()?
            .value()
            .map_pylon_err()?;
        match val.as_ref() {
            "Off" => Ok(ci2::TriggerMode::Off),
            "On" => Ok(ci2::TriggerMode::On),
            s => {
                return Err(ci2::Error::from(format!(
                    "unexpected TriggerMode enum string: {}",
                    s
                )));
            }
        }
    }
    fn set_trigger_mode(&mut self, value: TriggerMode) -> ci2::Result<()> {
        let sval = match value {
            ci2::TriggerMode::Off => "Off",
            ci2::TriggerMode::On => "On",
        };
        self.inner
            .lock()
            .enum_node("TriggerMode")
            .map_pylon_err()?
            .set_value(sval)
            .map_pylon_err()
    }

    // Settings: AcquisitionFrameRateEnable ----------------------------
    fn acquisition_frame_rate_enable(&self) -> ci2::Result<bool> {
        self.inner
            .lock()
            .boolean_node("AcquisitionFrameRateEnable")
            .map_pylon_err()?
            .value()
            .map_pylon_err()
    }
    fn set_acquisition_frame_rate_enable(&mut self, value: bool) -> ci2::Result<()> {
        self.inner
            .lock()
            .boolean_node("AcquisitionFrameRateEnable")
            .map_pylon_err()?
            .set_value(value)
            .map_pylon_err()
    }

    // Settings: AcquisitionFrameRate ----------------------------
    fn acquisition_frame_rate(&self) -> ci2::Result<f64> {
        let camera = self.inner.lock();
        let node = camera
            .float_node(self.acquisition_frame_rate_name())
            .map_pylon_err()?;
        node.value().map_pylon_err()
    }
    fn acquisition_frame_rate_range(&self) -> ci2::Result<(f64, f64)> {
        let camera = self.inner.lock();
        let node = camera
            .float_node(self.acquisition_frame_rate_name())
            .map_pylon_err()?;
        Ok((node.min().map_pylon_err()?, node.max().map_pylon_err()?))
    }
    fn set_acquisition_frame_rate(&mut self, value: f64) -> ci2::Result<()> {
        self.inner
            .lock()
            .float_node(self.acquisition_frame_rate_name())
            .map_pylon_err()?
            .set_value(value)
            .map_pylon_err()
    }

    // Settings: TriggerSelector ----------------------------
    fn trigger_selector(&self) -> ci2::Result<TriggerSelector> {
        let camera = self.inner.lock();
        let val = camera
            .enum_node("TriggerSelector")
            .map_pylon_err()?
            .value()
            .map_pylon_err()?;
        match val.as_ref() {
            "AcquisitionStart" => Ok(ci2::TriggerSelector::AcquisitionStart),
            "FrameBurstStart" => Ok(ci2::TriggerSelector::FrameBurstStart),
            "FrameStart" => Ok(ci2::TriggerSelector::FrameStart),
            "ExposureActive" => Ok(ci2::TriggerSelector::ExposureActive),
            s => {
                return Err(ci2::Error::from(format!(
                    "unexpected TriggerSelector enum string: {}",
                    s
                )));
            }
        }
    }
    fn set_trigger_selector(&mut self, value: TriggerSelector) -> ci2::Result<()> {
        let sval = match value {
            ci2::TriggerSelector::AcquisitionStart => "AcquisitionStart",
            ci2::TriggerSelector::FrameBurstStart => "FrameBurstStart",
            ci2::TriggerSelector::FrameStart => "FrameStart",
            ci2::TriggerSelector::ExposureActive => "ExposureActive",
            s => {
                return Err(ci2::Error::from(format!(
                    "unexpected TriggerSelector: {:?}",
                    s
                )));
            }
        };
        let camera = self.inner.lock();
        camera
            .enum_node("TriggerSelector")
            .map_pylon_err()?
            .set_value(sval)
            .map_pylon_err()
    }

    // Settings: AcquisitionMode ----------------------------
    fn acquisition_mode(&self) -> ci2::Result<AcquisitionMode> {
        let mode = self
            .inner
            .lock()
            .enum_node("AcquisitionMode")
            .map_pylon_err()?
            .value()
            .map_pylon_err()?;
        Ok(match mode.as_ref() {
            "Continuous" => ci2::AcquisitionMode::Continuous,
            "SingleFrame" => ci2::AcquisitionMode::SingleFrame,
            "MultiFrame" => ci2::AcquisitionMode::MultiFrame,
            s => {
                return Err(ci2::Error::from(format!(
                    "unexpected AcquisitionMode: {:?}",
                    s
                )));
            }
        })
    }
    fn set_acquisition_mode(&mut self, value: ci2::AcquisitionMode) -> ci2::Result<()> {
        let sval = match value {
            ci2::AcquisitionMode::Continuous => "Continuous",
            ci2::AcquisitionMode::SingleFrame => "SingleFrame",
            ci2::AcquisitionMode::MultiFrame => "MultiFrame",
        };
        self.inner
            .lock()
            .enum_node("AcquisitionMode")
            .map_pylon_err()?
            .set_value(sval)
            .map_pylon_err()
    }

    // Acquisition ----------------------------
    fn acquisition_start(&mut self) -> ci2::Result<()> {
        self.inner
            .lock()
            .start_grabbing(&pylon_cxx::GrabOptions::default())
            .map_pylon_err()?;
        Ok(())
    }
    fn acquisition_stop(&mut self) -> ci2::Result<()> {
        self.inner.lock().stop_grabbing().map_pylon_err()?;
        Ok(())
    }

    /// synchronous (blocking) frame acquisition
    fn next_frame(&mut self) -> ci2::Result<DynamicFrame> {
        let pixel_format = self.pixel_format()?;

        let mut gr = self.grab_result.lock();
        let cam = self.inner.lock();

        // Wait for an image and then retrieve it. A timeout of 99999 ms is used.
        cam.retrieve_result(99999, &mut *gr, pylon_cxx::TimeoutHandling::ThrowException)
            .map_pylon_err()?;

        let now = chrono::Utc::now(); // earliest possible timestamp

        // Image grabbed successfully?
        if gr.grab_succeeded().map_pylon_err()? {
            let buffer = gr.buffer().map_pylon_err()?;
            let block_id = gr.block_id().map_pylon_err()?;

            let fno: usize = match self.framecounting_method {
                FramecoutingMethod::BaslerGigE(ref mut i) => {
                    // Basler GigE cameras wrap after 65535 block
                    if block_id < 30000 && i.previous_block_id > 30000 {
                        // check nothing crazy is going on
                        if (i.store_fno - i.last_rollover) < 30000 {
                            return Err(ci2::Error::from(format!(
                                "Cannot recover frame count with \
                                Basler GigE camera {}. Did many \
                                frames get dropped?",
                                self.name
                            )));
                        }
                        i.epoch += 1;
                        i.last_rollover = i.store_fno;
                    }
                    i.store_fno += 1;
                    let fno = (i.epoch as usize * 65535) + block_id as usize;
                    i.previous_block_id = block_id;
                    fno
                }
                FramecoutingMethod::TrustDevice => block_id as usize,
                FramecoutingMethod::IgnoreDevice(ref mut store_fno) => {
                    let fno: usize = *store_fno;
                    *store_fno += 1;
                    fno
                }
            };

            let width = gr.width().map_pylon_err()?;
            let height = gr.height().map_pylon_err()?;
            let stride = gr.stride().map_pylon_err()?.try_into()?;
            let image_data = buffer.to_vec();
            let device_timestamp = gr.time_stamp().map_pylon_err()?;

            let extra = Box::new(PylonExtra {
                block_id,
                host_timestamp: now,
                host_framenumber: fno,
                device_timestamp,
                pixel_format,
            });
            Ok(DynamicFrame::new(
                width,
                height,
                stride,
                extra,
                image_data,
                pixel_format,
            ))

        // println!("Gray value of first pixel: {}\n", image_buffer[0]);
        } else {
            Err(ci2::Error::SingleFrameError(format!(
                "Pylon Error {}: {}",
                gr.error_code().map_pylon_err()?,
                gr.error_description().map_pylon_err()?
            )))
        }
    }
}

pub fn convert_pixel_format(pixel_format: formats::PixFmt) -> ci2::Result<&'static str> {
    use formats::PixFmt::*;
    let pixfmt = match pixel_format {
        Mono8 => "Mono8",

        // MONO10 => "Mono10",
        // MONO10p => "Mono10p",
        // MONO12 => "Mono12",
        // MONO12p => "Mono12p",
        // MONO16 => "Mono16",
        YUV422 => "YUV422packed",
        RGB8 => "RGB8packed",

        BayerGR8 => "BayerGR8",
        BayerRG8 => "BayerRG8",
        BayerBG8 => "BayerBG8",
        BayerGB8 => "BayerGB8",
        // e => {
        //     return Err(ci2::Error::from(format!("Unknown PixelFormat {:?}", e)));
        // }
        unknown => {
            return Err(ci2::Error::from(format!("Unsuppored PixFmt {}", unknown)));
        }
    };
    Ok(pixfmt)
}

pub fn convert_to_pixel_format(orig: &str) -> ci2::Result<formats::PixFmt> {
    use formats::PixFmt::*;
    let pixfmt = match orig {
        "Mono8" => Mono8,
        // "Mono10" => MONO10,
        // "Mono10p" => MONO10p,
        // "Mono12" => MONO12,
        // "Mono12p" => MONO12p,
        // "Mono16" => MONO16,
        "YUV422packed" => YUV422,
        "RGB8Packed" => RGB8,

        "BayerGR8" => BayerGR8,
        "BayerRG8" => BayerRG8,
        "BayerGB8" => BayerGB8,
        "BayerBG8" => BayerBG8,

        e => {
            return Err(ci2::Error::from(format!(
                "Unknown pixel format string: {:?}",
                e
            )));
        }
    };
    Ok(pixfmt)
}

fn gain_raw_to_db(raw: i64) -> ci2::Result<f64> {
    // TODO check name of camera model with "Gain Properties" table
    // in Basler Product Documentation to ensure this is correct for
    // this particular camera model.
    Ok(0.0359 * raw as f64)
}

fn gain_db_to_raw(db: f64) -> ci2::Result<i64> {
    // TODO check name of camera model with "Gain Properties" table
    // in Basler Product Documentation to ensure this is correct for
    // this particular camera model.
    Ok((db / 0.0359) as i64)
}

fn str_to_auto_mode(val: &str) -> ci2::Result<ci2::AutoMode> {
    match val {
        "Off" => Ok(ci2::AutoMode::Off),
        "Once" => Ok(ci2::AutoMode::Once),
        "Continuous" => Ok(ci2::AutoMode::Continuous),
        s => {
            return Err(ci2::Error::from(format!(
                "unexpected AutoMode enum string: {}",
                s
            )));
        }
    }
}

fn mode_to_str(value: AutoMode) -> &'static str {
    match value {
        ci2::AutoMode::Off => "Off",
        ci2::AutoMode::Once => "Once",
        ci2::AutoMode::Continuous => "Continuous",
    }
}

#[derive(Clone, Debug)]
pub struct PylonExtra {
    pub block_id: u64,
    host_timestamp: DateTime<Utc>,
    host_framenumber: usize,
    pub pixel_format: formats::PixFmt,
    pub device_timestamp: u64,
}

impl HostTimeData for PylonExtra {
    fn host_framenumber(&self) -> usize {
        self.host_framenumber
    }
    fn host_timestamp(&self) -> DateTime<Utc> {
        self.host_timestamp
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_pylon_extra() {
        use crate::PylonExtra;
        use timestamped_frame::HostTimeData;

        let pe: Box<dyn HostTimeData> = Box::new(PylonExtra {
            block_id: 123,
            host_timestamp: chrono::Utc::now(),
            host_framenumber: 456,
            pixel_format: formats::PixFmt::Mono8,
            device_timestamp: 789,
        });

        dbg!(&pe);

        let extra: &dyn HostTimeData = pe.as_ref();
        // let extra: &dyn HostTimeData = &pe;

        dbg!(extra.blarg());

        dbg!(extra);

        let extra_any: &dyn std::any::Any = extra.as_any();

        dbg!(extra_any);

        let _extra2 = extra_any.downcast_ref::<PylonExtra>().unwrap();
        // let _extra2 = extra_any.downcast_ref::<Box<PylonExtra>>().unwrap();
    }
}
