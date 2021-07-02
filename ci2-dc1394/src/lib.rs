#[macro_use]
extern crate log;

use machine_vision_formats as formats;

use libdc1394_sys as ffi;
use parking_lot::Mutex;
use std::sync::Arc;

use basic_frame::{BasicExtra, DynamicFrame};

use std::os::unix::io::{AsRawFd, RawFd};

fn select_err_to_ci2_err(orig: ::select::Error) -> ci2::Error {
    match orig {
        ::select::Error::Errno(code) => std::io::Error::from_raw_os_error(code).into(),
        ::select::Error::Timeout => ci2::Error::Timeout,
    }
}

trait ExtendedError<T> {
    fn map_dc1394_err(self) -> ci2::Result<T>;
}

impl<T> ExtendedError<T> for std::result::Result<T, dc1394::Error> {
    fn map_dc1394_err(self) -> ci2::Result<T> {
        self.map_err(|e| ci2::Error::BackendError(e.into()))
    }
}

macro_rules! bail {
    ($e: expr) => {
        return Err(ci2::Error::from(format!($e)));
    };
    ($fmt:expr, $($arg:tt)+) => {
        return Err(ci2::Error::from(format!($fmt, $($arg)+)));
    };
}

struct InnerModule {
    _ctx: dc1394::DC1394,
    cams: Vec<dc1394::Camera>,
}

pub struct WrappedModule {
    inner: Arc<Mutex<InnerModule>>,
}

pub struct DC1394CamInfo {
    name: String,
    serial: String,
    model: String,
    vendor: String,
}

impl ci2::CameraInfo for DC1394CamInfo {
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

macro_rules! check_filter {
    ($filter:expr, $expected:pat) => {{
        match $filter {
            $expected => {}
            _ => bail!("unexpected filter {:?}", $filter),
        }
    }};
}

fn to_name(cam: &dc1394::Camera) -> String {
    format!("{}-{}", cam.vendor().unwrap(), cam.guid())
}

fn to_serial(cam: &dc1394::Camera) -> String {
    format!("{}", cam.guid())
}

pub fn new_module() -> ci2::Result<WrappedModule> {
    let ctx = dc1394::DC1394::new().map_dc1394_err()?;
    let list_tmp = ctx.get_camera_list().map_dc1394_err()?;
    let list = list_tmp.as_slice();
    info!("{} cameras found", list.len());
    let mut cams = Vec::with_capacity(list.len());
    for cam_id in list.iter() {
        info!("  {:?}", cam_id);
        let cam = dc1394::Camera::new(&ctx, &cam_id.guid).map_dc1394_err()?;
        cam.set_video_mode(ffi::dc1394video_mode_t::DC1394_VIDEO_MODE_FORMAT7_0)
            .map_dc1394_err()?;
        let (w, h) = cam.max_image_size().map_dc1394_err()?;
        cam.set_roi(0, 0, w, h).map_dc1394_err()?;
        cams.push(cam);
    }
    Ok(WrappedModule {
        inner: Arc::new(Mutex::new(InnerModule {
            _ctx: ctx,
            cams: cams,
        })),
    })
}

impl ci2::CameraModule for WrappedModule {
    type CameraType = WrappedCamera;

    fn name(&self) -> &str {
        "dc1394"
    }
    fn camera_infos(&self) -> ci2::Result<Vec<Box<dyn ci2::CameraInfo>>> {
        let infos = self
            .inner
            .lock()
            .cams
            .iter()
            .map(|cam| {
                let ci = Box::new(DC1394CamInfo {
                    name: to_name(&cam),
                    serial: to_serial(&cam),
                    model: cam.model().unwrap(),
                    vendor: cam.vendor().unwrap(),
                });
                let ci2: Box<dyn ci2::CameraInfo> = ci; // type erasure
                ci2
            })
            .collect();
        Ok(infos)
    }
    fn camera(&mut self, name: &str) -> ci2::Result<Self::CameraType> {
        Ok(WrappedCamera::new(self.inner.clone(), name).map_dc1394_err()?)
    }
}

fn get_coding(
    coding: ffi::dc1394color_coding_t::Type,
    filter: ffi::dc1394color_filter_t::Type,
    _order: ffi::dc1394byte_order_t::Type,
) -> ci2::Result<formats::PixFmt> {
    let result = match coding {
        ffi::dc1394color_coding_t::DC1394_COLOR_CODING_MONO8 => formats::PixFmt::Mono8,
        ffi::dc1394color_coding_t::DC1394_COLOR_CODING_RAW8 => match filter {
            ffi::dc1394color_filter_t::DC1394_COLOR_FILTER_RGGB => formats::PixFmt::BayerRG8,
            ffi::dc1394color_filter_t::DC1394_COLOR_FILTER_GBRG => formats::PixFmt::BayerGB8,
            ffi::dc1394color_filter_t::DC1394_COLOR_FILTER_GRBG => formats::PixFmt::BayerGR8,
            ffi::dc1394color_filter_t::DC1394_COLOR_FILTER_BGGR => formats::PixFmt::BayerBG8,
            filter => panic!("unimplemented conversion for filter {:?}", filter),
        },
        // ffi::dc1394color_coding_t::DC1394_COLOR_CODING_YUV411 => formats::PixFmt::YUV411,
        ffi::dc1394color_coding_t::DC1394_COLOR_CODING_YUV422 => formats::PixFmt::YUV422,
        // ffi::dc1394color_coding_t::DC1394_COLOR_CODING_YUV444 => formats::PixFmt::YUV444,
        ffi::dc1394color_coding_t::DC1394_COLOR_CODING_RGB8 => formats::PixFmt::RGB8,
        ffi::dc1394color_coding_t::DC1394_COLOR_CODING_RAW16 => {
            let e = "unimplemented conversion for DC1394_COLOR_CODING_RAW16".to_string();
            return Err(ci2::Error::from(e));
        }
        coding => {
            let e = format!("unimplemented conversion for coding {:?}", coding);
            return Err(ci2::Error::from(e));
        }
    };
    Ok(result)
}

struct InnerCam {
    modinner: Arc<Mutex<InnerModule>>,
    idx: usize,
    fno: usize,
    started: bool,
}

#[derive(Clone)]
pub struct WrappedCamera {
    caminner: Arc<Mutex<InnerCam>>,
    name: String,
    serial: String,
    model: String,
    vendor: String,
}

fn _test_camera_is_send() {
    // Compile-time test to ensure WrappedCamera implements Send trait.
    fn implements<T: Send>() {}
    implements::<WrappedCamera>();
}

impl Drop for InnerCam {
    fn drop(&mut self) {
        if self.started {
            trace!("stopping acquisition in {}:{} drop()", file!(), line!());
            match self.acquisition_stop() {
                Ok(()) => {}
                Err(e) => warn!("Error while stopping camera in drop(): {}", e),
            }
        }
    }
}

impl AsRawFd for InnerCam {
    fn as_raw_fd(&self) -> RawFd {
        let modinner = self.modinner.lock();
        let i = self.idx;
        modinner.cams[i].as_raw_fd()
    }
}

impl InnerCam {
    fn new(modinner: Arc<Mutex<InnerModule>>, idx: usize) -> Self {
        Self {
            modinner: modinner,
            idx: idx,
            fno: 0,
            started: false,
        }
    }
    fn width(&self) -> ci2::Result<u32> {
        let modinner = self.modinner.lock();
        let i = self.idx;
        let (w, _h) = modinner.cams[i].image_size().map_dc1394_err()?;
        Ok(w)
    }
    fn height(&self) -> ci2::Result<u32> {
        let modinner = self.modinner.lock();
        let i = self.idx;
        let (_w, h) = modinner.cams[i].image_size().map_dc1394_err()?;
        Ok(h)
    }
    fn pixel_format(&self) -> ci2::Result<formats::PixFmt> {
        let modinner = self.modinner.lock();
        let coding = modinner.cams[self.idx].color_coding().map_dc1394_err()?;
        let filter = modinner.cams[self.idx].color_filter().map_dc1394_err()?;

        // the next two are made up to fit get_coding() signature
        let fake_byte_order = ffi::dc1394byte_order_t::DC1394_BYTE_ORDER_UYVY;

        get_coding(coding, filter, fake_byte_order)
    }
    fn possible_pixel_formats(&self) -> ci2::Result<Vec<formats::PixFmt>> {
        let modinner = self.modinner.lock();
        let codings = modinner.cams[self.idx]
            .possible_color_codings()
            .map_dc1394_err()?;
        let filter = modinner.cams[self.idx].color_filter().map_dc1394_err()?;

        // the next two are made up to fit get_coding() signature
        let fake_byte_order = ffi::dc1394byte_order_t::DC1394_BYTE_ORDER_UYVY;

        let mut encodings = Vec::new();
        for coding in codings.into_iter() {
            match get_coding(coding, filter, fake_byte_order) {
                Ok(fmt) => encodings.push(fmt),
                Err(e) => warn!("convering pixel format: {:?}", e),
            }
        }

        Ok(encodings)
    }
    fn set_pixel_format(&mut self, pixel_format: formats::PixFmt) -> ci2::Result<()> {
        let modinner = self.modinner.lock();
        let filter = modinner.cams[self.idx].color_filter().map_dc1394_err()?;
        let coding = match pixel_format {
            formats::PixFmt::Mono8 => ffi::dc1394color_coding_t::DC1394_COLOR_CODING_MONO8,
            formats::PixFmt::BayerRG8 => {
                check_filter!(filter, ffi::dc1394color_filter_t::DC1394_COLOR_FILTER_RGGB);
                ffi::dc1394color_coding_t::DC1394_COLOR_CODING_RAW8
            }
            formats::PixFmt::BayerGB8 => {
                check_filter!(filter, ffi::dc1394color_filter_t::DC1394_COLOR_FILTER_GBRG);
                ffi::dc1394color_coding_t::DC1394_COLOR_CODING_RAW8
            }
            formats::PixFmt::BayerGR8 => {
                check_filter!(filter, ffi::dc1394color_filter_t::DC1394_COLOR_FILTER_GRBG);
                ffi::dc1394color_coding_t::DC1394_COLOR_CODING_RAW8
            }
            formats::PixFmt::BayerBG8 => {
                check_filter!(filter, ffi::dc1394color_filter_t::DC1394_COLOR_FILTER_BGGR);
                ffi::dc1394color_coding_t::DC1394_COLOR_CODING_RAW8
            }
            _ => unimplemented!(),
        };
        let modinner = self.modinner.lock();
        modinner.cams[self.idx]
            .set_color_coding(coding)
            .map_dc1394_err()
    }
    fn exposure_time(&self) -> ci2::Result<f64> {
        let modinner = self.modinner.lock();
        let i = self.idx;
        modinner.cams[i].exposure_time().map_dc1394_err()
    }
    fn exposure_time_range(&self) -> ci2::Result<(f64, f64)> {
        let modinner = self.modinner.lock();
        let i = self.idx;
        modinner.cams[i].exposure_time_range().map_dc1394_err()
    }
    fn set_exposure_time(&mut self, value: f64) -> ci2::Result<()> {
        let mut modinner = self.modinner.lock();
        let i = self.idx;
        modinner.cams[i].set_exposure_time(value).map_dc1394_err()
    }

    fn gain(&self) -> ci2::Result<f64> {
        let modinner = self.modinner.lock();
        let i = self.idx;
        modinner.cams[i].gain().map_dc1394_err()
    }
    fn gain_range(&self) -> ci2::Result<(f64, f64)> {
        let modinner = self.modinner.lock();
        let i = self.idx;
        modinner.cams[i].gain_range().map_dc1394_err()
    }
    fn set_gain(&mut self, value: f64) -> ci2::Result<()> {
        let mut modinner = self.modinner.lock();
        let i = self.idx;
        modinner.cams[i].set_gain(value).map_dc1394_err()
    }

    fn exposure_auto(&self) -> ci2::Result<ci2::AutoMode> {
        let modinner = self.modinner.lock();
        let i = self.idx;
        let value = modinner.cams[i].exposure_auto().map_dc1394_err()?;
        let result = match value {
            dc1394::ExposureAuto::Off => ci2::AutoMode::Off,
            dc1394::ExposureAuto::Once => ci2::AutoMode::Once,
            dc1394::ExposureAuto::Continuous => ci2::AutoMode::Continuous,
        };
        Ok(result)
    }

    fn set_exposure_auto(&mut self, value: ci2::AutoMode) -> ci2::Result<()> {
        let mut modinner = self.modinner.lock();
        let i = self.idx;
        let v2 = match value {
            ci2::AutoMode::Off => dc1394::ExposureAuto::Off,
            ci2::AutoMode::Once => dc1394::ExposureAuto::Once,
            ci2::AutoMode::Continuous => dc1394::ExposureAuto::Continuous,
        };
        modinner.cams[i].set_exposure_auto(v2).map_dc1394_err()?;
        Ok(())
    }

    fn gain_auto(&self) -> ci2::Result<ci2::AutoMode> {
        let modinner = self.modinner.lock();
        let i = self.idx;
        let value = modinner.cams[i].gain_auto().map_dc1394_err()?;
        let result = match value {
            dc1394::GainAuto::Off => ci2::AutoMode::Off,
            dc1394::GainAuto::Once => ci2::AutoMode::Once,
            dc1394::GainAuto::Continuous => ci2::AutoMode::Continuous,
        };
        Ok(result)
    }
    fn set_gain_auto(&mut self, value: ci2::AutoMode) -> ci2::Result<()> {
        let mut modinner = self.modinner.lock();
        let i = self.idx;
        let v2 = match value {
            ci2::AutoMode::Off => dc1394::GainAuto::Off,
            ci2::AutoMode::Once => dc1394::GainAuto::Once,
            ci2::AutoMode::Continuous => dc1394::GainAuto::Continuous,
        };
        modinner.cams[i].set_gain_auto(v2).map_dc1394_err()?;
        Ok(())
    }

    fn trigger_mode(&self) -> ci2::Result<ci2::TriggerMode> {
        let modinner = self.modinner.lock();
        let i = self.idx;
        let value = modinner.cams[i].trigger_mode().map_dc1394_err()?;
        let result = match value {
            dc1394::TriggerMode::Off => ci2::TriggerMode::Off,
            dc1394::TriggerMode::On => ci2::TriggerMode::On,
        };
        Ok(result)
    }
    fn set_trigger_mode(&mut self, value: ci2::TriggerMode) -> ci2::Result<()> {
        let mut modinner = self.modinner.lock();
        let i = self.idx;
        let v2 = match value {
            ci2::TriggerMode::Off => dc1394::TriggerMode::Off,
            ci2::TriggerMode::On => dc1394::TriggerMode::On,
        };
        modinner.cams[i].set_trigger_mode(v2).map_dc1394_err()?;
        Ok(())
    }

    fn trigger_selector(&self) -> ci2::Result<ci2::TriggerSelector> {
        let modinner = self.modinner.lock();
        let i = self.idx;
        let value = modinner.cams[i].trigger_selector().map_dc1394_err()?;
        let result = match value {
            dc1394::TriggerSelector::AcquisitionStart => ci2::TriggerSelector::AcquisitionStart,
            dc1394::TriggerSelector::FrameBurstStart => ci2::TriggerSelector::FrameBurstStart,
            dc1394::TriggerSelector::FrameStart => ci2::TriggerSelector::FrameStart,
        };
        Ok(result)
    }
    fn set_trigger_selector(&mut self, value: ci2::TriggerSelector) -> ci2::Result<()> {
        let mut modinner = self.modinner.lock();
        let i = self.idx;
        let v2 = match value {
            ci2::TriggerSelector::AcquisitionStart => dc1394::TriggerSelector::AcquisitionStart,
            ci2::TriggerSelector::FrameBurstStart => dc1394::TriggerSelector::FrameBurstStart,
            ci2::TriggerSelector::FrameStart => dc1394::TriggerSelector::FrameStart,
            selector => {
                let e = format!("unimplemented trigger selector: {:?}", selector);
                return Err(ci2::Error::from(e));
            }
        };
        modinner.cams[i].set_trigger_selector(v2).map_dc1394_err()?;
        Ok(())
    }

    fn acquisition_mode(&self) -> ci2::Result<ci2::AcquisitionMode> {
        Ok(ci2::AcquisitionMode::Continuous)
    }
    fn set_acquisition_mode(&mut self, value: ci2::AcquisitionMode) -> ci2::Result<()> {
        if value != ci2::AcquisitionMode::Continuous {
            bail!("unsupported acquisition mode: {:?}", value);
        }
        Ok(())
    }

    fn acquisition_start(&mut self) -> ci2::Result<()> {
        let num_buffers = 20;
        let mut modinner = self.modinner.lock();
        let i = self.idx;
        let video_mode = modinner.cams[i].video_mode().map_dc1394_err()?;
        modinner.cams[i]
            .capture_setup(num_buffers)
            .map_dc1394_err()?;
        modinner.cams[i]
            .set_transmission(ffi::dc1394switch_t::DC1394_ON)
            .map_dc1394_err()?;
        info!("      started capture (mode: {:?})", video_mode);
        self.started = true;
        Ok(())
    }
    fn acquisition_stop(&mut self) -> ci2::Result<()> {
        let mut modinner = self.modinner.lock();
        let i = self.idx;
        modinner.cams[i]
            .set_transmission(ffi::dc1394switch_t::DC1394_OFF)
            .map_dc1394_err()?;
        info!("      stopped capture");
        self.started = false;
        Ok(())
    }

    /// synchronous frame acquisition
    /// timeout with duration of zero for non-blocking behavior.
    /// timeout with duration None to block.
    fn my_frame(&mut self, timeout_ms: Option<u32>) -> ci2::Result<DynamicFrame> {
        let dequeue_policy = ffi::dc1394capture_policy_t::DC1394_CAPTURE_POLICY_WAIT;

        let result = {
            let modinner = self.modinner.lock();
            let camera = &modinner.cams[self.idx];
            if let Some(timeout_ms) = timeout_ms {
                let fd = camera.as_raw_fd();
                select::block_or_timeout(fd, timeout_ms).map_err(select_err_to_ci2_err)?;
            }

            let im = camera.capture_dequeue(&dequeue_policy).map_dc1394_err()?;

            // let roi = formats::FrameROI {
            //     xmin: im.position()[0],
            //     ymin: im.position()[1],
            //     width: im.size()[0],
            //     height: im.size()[1],
            //     };

            let width = im.size()[0];
            let height = im.size()[1];
            let stride = im.stride();
            let image_data = im.data_view().to_vec(); // copy data
            let now: chrono::DateTime<chrono::Utc> = chrono::Utc::now();

            let pixel_format =
                get_coding(im.color_coding(), im.color_filter(), im.yuv_byte_order())?;

            let extra = Box::new(BasicExtra {
                host_timestamp: now,
                host_framenumber: self.fno,
            });
            let frame = DynamicFrame::new(width, height, stride, extra, image_data, pixel_format);

            self.fno += 1;
            frame
        };
        Ok(result)
    }
}

impl WrappedCamera {
    fn new(modinner: Arc<Mutex<InnerModule>>, name: &str) -> dc1394::Result<Self> {
        let mut found = None;
        {
            let inner = modinner.lock();
            for i in 0..inner.cams.len() {
                if to_name(&inner.cams[i]) == name {
                    found = Some(i);
                }
            }
        }
        if let Some(i) = found {
            let (name, serial, model, vendor) = {
                let inner = modinner.lock();
                let ref cam = inner.cams[i];
                (to_name(&cam), to_serial(&cam), cam.model()?, cam.vendor()?)
            };

            Ok(Self {
                caminner: Arc::new(Mutex::new(InnerCam::new(modinner, i))),
                name: name,
                serial: serial,
                model: model,
                vendor: vendor,
            })
        } else {
            Err(dc1394::Error::CameraNewFailed)
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
    fn width(&self) -> ci2::Result<u32> {
        self.caminner.lock().width()
    }
    fn height(&self) -> ci2::Result<u32> {
        self.caminner.lock().height()
    }
    fn pixel_format(&self) -> ci2::Result<formats::PixFmt> {
        self.caminner.lock().pixel_format()
    }
    fn possible_pixel_formats(&self) -> ci2::Result<Vec<formats::PixFmt>> {
        self.caminner.lock().possible_pixel_formats()
    }
    fn set_pixel_format(&mut self, pixel_format: formats::PixFmt) -> ci2::Result<()> {
        self.caminner.lock().set_pixel_format(pixel_format)
    }

    fn exposure_time(&self) -> ci2::Result<f64> {
        self.caminner.lock().exposure_time()
    }
    fn exposure_time_range(&self) -> ci2::Result<(f64, f64)> {
        self.caminner.lock().exposure_time_range()
    }
    fn set_exposure_time(&mut self, value: f64) -> ci2::Result<()> {
        self.caminner.lock().set_exposure_time(value)
    }
    fn gain(&self) -> ci2::Result<f64> {
        self.caminner.lock().gain()
    }
    fn gain_range(&self) -> ci2::Result<(f64, f64)> {
        self.caminner.lock().gain_range()
    }
    fn set_gain(&mut self, value: f64) -> ci2::Result<()> {
        self.caminner.lock().set_gain(value)
    }
    fn exposure_auto(&self) -> ci2::Result<ci2::AutoMode> {
        self.caminner.lock().exposure_auto()
    }
    fn set_exposure_auto(&mut self, value: ci2::AutoMode) -> ci2::Result<()> {
        self.caminner.lock().set_exposure_auto(value)
    }
    fn gain_auto(&self) -> ci2::Result<ci2::AutoMode> {
        self.caminner.lock().gain_auto()
    }
    fn set_gain_auto(&mut self, value: ci2::AutoMode) -> ci2::Result<()> {
        self.caminner.lock().set_gain_auto(value)
    }

    fn trigger_mode(&self) -> ci2::Result<ci2::TriggerMode> {
        self.caminner.lock().trigger_mode()
    }
    fn set_trigger_mode(&mut self, value: ci2::TriggerMode) -> ci2::Result<()> {
        self.caminner.lock().set_trigger_mode(value)
    }

    fn acquisition_frame_rate_enable(&self) -> ci2::Result<bool> {
        Ok(false)
    }
    fn set_acquisition_frame_rate_enable(&mut self, value: bool) -> ci2::Result<()> {
        if value != false {
            bail!("unsupported set_acquisition_frame_rate_enable: {:?}", value);
        }
        Ok(())
    }

    fn acquisition_frame_rate(&self) -> ci2::Result<f64> {
        bail!("unimplemented frame rate query");
    }
    fn acquisition_frame_rate_range(&self) -> ci2::Result<(f64, f64)> {
        bail!("unimplemented frame rate range query");
    }
    fn set_acquisition_frame_rate(&mut self, _value: f64) -> ci2::Result<()> {
        bail!("unimplemented frame rate set");
    }

    fn trigger_selector(&self) -> ci2::Result<ci2::TriggerSelector> {
        self.caminner.lock().trigger_selector()
    }
    fn set_trigger_selector(&mut self, value: ci2::TriggerSelector) -> ci2::Result<()> {
        self.caminner.lock().set_trigger_selector(value)
    }

    fn acquisition_mode(&self) -> ci2::Result<ci2::AcquisitionMode> {
        self.caminner.lock().acquisition_mode()
    }
    fn set_acquisition_mode(&mut self, value: ci2::AcquisitionMode) -> ci2::Result<()> {
        self.caminner.lock().set_acquisition_mode(value)
    }

    fn acquisition_start(&mut self) -> ci2::Result<()> {
        self.caminner.lock().acquisition_start()
    }
    fn acquisition_stop(&mut self) -> ci2::Result<()> {
        self.caminner.lock().acquisition_stop()
    }

    fn next_frame(&mut self) -> ci2::Result<DynamicFrame> {
        self.caminner.lock().my_frame(None)
    }
}
