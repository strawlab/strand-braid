extern crate failure;
#[macro_use]
extern crate log;
extern crate parking_lot;

extern crate aravis;
extern crate aravis_sys as sys;
extern crate basic_frame;
extern crate chrono;
extern crate ci2;
extern crate machine_vision_formats as formats;

use parking_lot::Mutex;
use std::sync::Arc;

use basic_frame::BasicFrame;

trait ErrorOption<T> {
    fn none_err(self) -> Result<T, ci2::Error>;
}

impl<T> ErrorOption<T> for Option<T> {
    fn none_err(self) -> Result<T, ci2::Error> {
        match self {
            Some(val) => Ok(val),
            None => Err(ci2::Error::from("unexpected None".to_string())),
        }
    }
}

fn failure_err_to_ci2_err(e: failure::Error) -> ci2::Error {
    ci2::Error::BackendError(failure::Error::from(e))
}

pub struct WrappedModule {}

pub fn new_module() -> ci2::Result<WrappedModule> {
    Ok(WrappedModule {})
}

impl ci2::CameraModule for WrappedModule {
    type FrameType = BasicFrame;
    type CameraType = WrappedCamera;

    fn name(&self) -> &str {
        "aravis"
    }
    fn camera_infos(&self) -> ci2::Result<Vec<Box<ci2::CameraInfo>>> {
        aravis::update_device_list();
        let n_devices = aravis::get_n_devices();

        (0..n_devices)
            .map(|i| {
                let ci = CameraInfo::new(i)?;
                let ci = Box::new(ci) as Box<ci2::CameraInfo>; // type erasure
                Ok(ci)
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(failure_err_to_ci2_err)
    }
    fn camera(&mut self, device_id: &str) -> ci2::Result<Self::CameraType> {
        WrappedCamera::new(device_id)
    }
}

#[derive(Debug, Clone)]
pub struct CameraInfo {
    name: String,
    serial: String,
    model: String,
    vendor: String,
}

impl CameraInfo {
    fn new(index: u32) -> ci2::Result<Self> {
        let device_id = aravis::get_device_id(index).none_err()?;
        let serial = aravis::get_device_serial_nbr(index).none_err()?;
        let model = aravis::get_device_model(index).none_err()?;
        let vendor = aravis::get_device_vendor(index).none_err()?;
        Ok(Self {
            name: device_id,
            serial,
            model,
            vendor,
        })
    }
}

impl ci2::CameraInfo for CameraInfo {
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
pub struct WrappedCamera {
    inner: Arc<Mutex<aravis::Camera>>,
    info: CameraInfo,
    stream: Arc<Mutex<aravis::Stream>>,
    count: usize,
}

// According to the aravis docs, this is true.
unsafe impl Send for WrappedCamera {}

fn _test_camera_is_send() {
    // Compile-time test to ensure WrappedCamera implements Send trait.
    fn implements<T: Send>() {}
    implements::<WrappedCamera>();
}

impl WrappedCamera {
    fn new(device_id: &str) -> ci2::Result<Self> {
        aravis::update_device_list();
        let n_devices = aravis::get_n_devices();

        let mut found = None;
        for i in 0..n_devices {
            let name = aravis::get_device_id(i).none_err()?;
            if name == device_id {
                found = Some(i);
            }
        }

        let idx = match found {
            Some(idx) => idx,
            None => {
                return Err(ci2::Error::from(format!(
                    "device_id {} not found",
                    device_id
                )));
            }
        };

        let info = CameraInfo::new(idx)?;

        use aravis::CameraManualExt;
        let cam = match aravis::Camera::new2(device_id) {
            Some(camera) => camera,
            None => {
                return Err(ci2::Error::from(format!(
                    "camera {} found but not opened",
                    device_id
                )));
            }
        };

        use aravis::CameraExt;
        let payload = cam.get_payload();

        use aravis::StreamManualExt;
        let stream = cam.create_stream_simple().unwrap();
        for _ in 0..50 {
            let buffer = aravis::Buffer::new_allocate(payload as u64);
            stream.push_buffer_fixed(buffer);
        }

        Ok(Self {
            inner: Arc::new(Mutex::new(cam)),
            info,
            stream: Arc::new(Mutex::new(stream)),
            count: 0,
        })
    }
}

impl ci2::CameraInfo for WrappedCamera {
    fn name(&self) -> &str {
        self.info.name()
    }
    fn serial(&self) -> &str {
        self.info.serial()
    }
    fn model(&self) -> &str {
        self.info.model()
    }
    fn vendor(&self) -> &str {
        self.info.vendor()
    }
}

fn convert_fmt(fmt: &str) -> ci2::Result<formats::PixelFormat> {
    match fmt {
        // TODO remove the `as i32` when type of pixel formats fixed
        "Mono8" => Ok(formats::PixelFormat::MONO8),
        "Mono10" => Ok(formats::PixelFormat::MONO10),
        f => {
            return Err(ci2::Error::from(format!(
                "unimplemented pixel format {}",
                f
            )))
        }
    }
}

impl ci2::Camera for WrappedCamera {
    type FrameType = BasicFrame;

    fn width(&self) -> ci2::Result<u32> {
        use aravis::CameraExt;
        let (_xmin, _ymin, width, _height) = self.inner.lock().get_region();
        Ok(width as u32)
    }
    fn height(&self) -> ci2::Result<u32> {
        use aravis::CameraExt;
        let (_xmin, _ymin, _width, height) = self.inner.lock().get_region();
        Ok(height as u32)
    }

    fn pixel_format(&self) -> ci2::Result<formats::PixelFormat> {
        use aravis::CameraExt;
        let fmt = self.inner.lock().get_pixel_format_as_string();
        match fmt {
            Some(fmt) => Ok(convert_fmt(&fmt)?),
            None => Err(ci2::Error::from("failed getting pixel_format".to_string())),
        }
    }
    fn possible_pixel_formats(&self) -> ci2::Result<Vec<formats::PixelFormat>> {
        let mut result = Vec::new();

        use aravis::CameraExt;
        let avail_formats = self.inner.lock().get_available_pixel_formats_as_strings();
        for fmt in avail_formats.iter() {
            match convert_fmt(fmt) {
                Ok(pixfmt) => result.push(pixfmt),
                Err(e) => warn!(
                    "ignoris aravis pixel format {}, \
                    because conversion failed with: {}",
                    fmt, e
                ),
            }
        }
        Ok(result)
    }
    fn set_pixel_format(&mut self, pixel_format: formats::PixelFormat) -> ci2::Result<()> {
        let fmtstr = match pixel_format {
            formats::PixelFormat::MONO8 => "Mono8",
            formats::PixelFormat::MONO10 => "Mono10",
            e => {
                return Err(ci2::Error::from(format!(
                    "unimplemented pixel_format {:?}",
                    e
                )))
            }
        };
        use aravis::CameraExt;
        // self.inner.lock().set_pixel_format(fmt as u32); // TODO remove the `as u32` when type of pixel formats fixed
        self.inner.lock().set_pixel_format_from_string(fmtstr);
        Ok(())
    }

    fn exposure_time(&self) -> ci2::Result<f64> {
        use aravis::CameraExt;
        Ok(self.inner.lock().get_exposure_time())
    }
    fn exposure_time_range(&self) -> ci2::Result<(f64, f64)> {
        use aravis::CameraExt;
        let (low, high) = self.inner.lock().get_exposure_time_bounds();
        Ok((low, high))
    }
    fn set_exposure_time(&mut self, value: f64) -> ci2::Result<()> {
        use aravis::CameraExt;
        self.inner.lock().set_exposure_time(value);
        Ok(())
    }
    fn gain(&self) -> ci2::Result<f64> {
        use aravis::CameraExt;
        Ok(self.inner.lock().get_gain())
    }
    fn gain_range(&self) -> ci2::Result<(f64, f64)> {
        use aravis::CameraExt;
        let (low, high) = self.inner.lock().get_gain_bounds();
        Ok((low, high))
    }
    fn set_gain(&mut self, value: f64) -> ci2::Result<()> {
        use aravis::CameraExt;
        self.inner.lock().set_gain(value);
        Ok(())
    }
    fn exposure_auto(&self) -> ci2::Result<ci2::AutoMode> {
        use aravis::CameraExt;
        let value = self.inner.lock().get_exposure_time_auto();
        Ok(auto_to_ci2(value))
    }
    fn set_exposure_auto(&mut self, value: ci2::AutoMode) -> ci2::Result<()> {
        use aravis::CameraExt;
        self.inner
            .lock()
            .set_exposure_time_auto(auto_to_aravis(value));
        Ok(())
    }
    fn gain_auto(&self) -> ci2::Result<ci2::AutoMode> {
        use aravis::CameraExt;
        let value = self.inner.lock().get_gain_auto();
        Ok(auto_to_ci2(value))
    }
    fn set_gain_auto(&mut self, value: ci2::AutoMode) -> ci2::Result<()> {
        use aravis::CameraExt;
        self.inner.lock().set_gain_auto(auto_to_aravis(value));
        Ok(())
    }

    fn trigger_mode(&self) -> ci2::Result<ci2::TriggerMode> {
        use aravis::{CameraExt, DeviceExt};
        let camera = self.inner.lock();
        let device = camera.get_device().unwrap(); // TODO the memory management here must be wrong
        let value = device.get_string_feature_value("TriggerMode").unwrap();
        let r = match value.as_ref() {
            "On" => ci2::TriggerMode::On,
            "Off" => ci2::TriggerMode::Off,
            v => panic!("unexpected value {}", v),
        };
        Ok(r)
    }
    fn set_trigger_mode(&mut self, value: ci2::TriggerMode) -> ci2::Result<()> {
        use aravis::{CameraExt, DeviceExt};
        let camera = self.inner.lock();
        let device = camera.get_device().unwrap(); // TODO the memory management here must be wrong
        let mode_string = match value {
            ci2::TriggerMode::On => "On",
            ci2::TriggerMode::Off => "Off",
        };
        device.set_string_feature_value("TriggerMode", mode_string);
        Ok(())
    }

    fn trigger_selector(&self) -> ci2::Result<ci2::TriggerSelector> {
        use aravis::{CameraExt, DeviceExt};
        let camera = self.inner.lock();
        let device = camera.get_device().unwrap(); // TODO the memory management here must be wrong
        let value = device.get_string_feature_value("TriggerSelector").unwrap();
        let r = match value.as_ref() {
            "AcquisitionStart" => ci2::TriggerSelector::AcquisitionStart,
            "FrameStart" => ci2::TriggerSelector::FrameStart,
            "FrameBurstStart" => ci2::TriggerSelector::FrameBurstStart,
            v => panic!("unexpected value {}", v),
        };
        Ok(r)
    }
    fn set_trigger_selector(&mut self, value: ci2::TriggerSelector) -> ci2::Result<()> {
        use aravis::{CameraExt, DeviceExt};
        let camera = self.inner.lock();
        let device = camera.get_device().unwrap(); // TODO the memory management here must be wrong
        let sel_string = match value {
            ci2::TriggerSelector::AcquisitionStart => "AcquisitionStart",
            ci2::TriggerSelector::FrameStart => "FrameStart",
            ci2::TriggerSelector::FrameBurstStart => "FrameBurstStart",
        };
        device.set_string_feature_value("TriggerSelector", sel_string);
        Ok(())
    }

    fn acquisition_mode(&self) -> ci2::Result<ci2::AcquisitionMode> {
        use aravis::{CameraExt, DeviceExt};
        let camera = self.inner.lock();
        let device = camera.get_device().unwrap(); // TODO the memory management here must be wrong
        let value = device.get_string_feature_value("AcquisitionMode").unwrap();
        let r = match value.as_ref() {
            "Continuous" => ci2::AcquisitionMode::Continuous,
            "SingleFrame" => ci2::AcquisitionMode::SingleFrame,
            "MultiFrame" => ci2::AcquisitionMode::MultiFrame,
            v => panic!("unexpected value {}", v),
        };
        Ok(r)
    }
    fn set_acquisition_mode(&mut self, value: ci2::AcquisitionMode) -> ci2::Result<()> {
        use aravis::{CameraExt, DeviceExt};
        let camera = self.inner.lock();
        let device = camera.get_device().unwrap(); // TODO the memory management here must be wrong
        let sel_string = match value {
            ci2::AcquisitionMode::Continuous => "Continuous",
            ci2::AcquisitionMode::SingleFrame => "SingleFrame",
            ci2::AcquisitionMode::MultiFrame => "MultiFrame",
        };
        device.set_string_feature_value("AcquisitionMode", sel_string);
        Ok(())
    }

    fn acquisition_start(&mut self) -> ci2::Result<()> {
        use aravis::CameraExt;
        self.inner.lock().start_acquisition();
        Ok(())
    }
    fn acquisition_stop(&mut self) -> ci2::Result<()> {
        use aravis::CameraExt;
        self.inner.lock().stop_acquisition();
        Ok(())
    }

    fn next_frame(&mut self) -> ci2::Result<Self::FrameType> {
        let result = {
            let stream = self.stream.lock();
            use aravis::{StreamExt, StreamManualExt};
            let buffer = stream.pop_buffer().none_err()?;

            use aravis::BufferExt;

            let width = buffer.get_image_width() as u32;

            let stride = width; // TODO fixme
            let image_data = buffer.get_data();
            let host_timestamp = chrono::Utc::now();
            let pixel_format = formats::PixelFormat::MONO8; // TODO fixme
            let host_framenumber = self.count;
            self.count += 1;

            // copy data
            let result = BasicFrame {
                width,
                height: buffer.get_image_height() as u32,
                stride,
                image_data,
                host_timestamp,
                host_framenumber,
                pixel_format,
            };

            stream.push_buffer_fixed(buffer);
            result
        };

        Ok(result)
    }
}

fn auto_to_ci2(value: aravis::Auto) -> ci2::AutoMode {
    match value {
        aravis::Auto::Off => ci2::AutoMode::Off,
        aravis::Auto::Once => ci2::AutoMode::Once,
        aravis::Auto::Continuous => ci2::AutoMode::Continuous,
        aravis::Auto::__Unknown(v) => panic!("unknown Auto mode {}", v),
    }
}

fn auto_to_aravis(value: ci2::AutoMode) -> aravis::Auto {
    match value {
        ci2::AutoMode::Off => aravis::Auto::Off,
        ci2::AutoMode::Once => aravis::Auto::Once,
        ci2::AutoMode::Continuous => aravis::Auto::Continuous,
    }
}
