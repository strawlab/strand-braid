#[macro_use]
extern crate log;
extern crate parking_lot;

extern crate libflycapture2_sys;

extern crate chrono;
extern crate ci2;
extern crate flycap2;
extern crate machine_vision_formats as formats;
extern crate timestamped_frame;

use libflycapture2_sys as ffi;

use ci2::Camera;

use parking_lot::Mutex;
use std::sync::Arc;

use flycap2::{get_guid_for_index, get_lowest_pixel_format, get_num_cameras, FlycapContext};
use formats::PixelFormat;

struct InnerModule {
    cams: Vec<FlycapContext>,
}

pub struct WrappedModule {
    inner: Arc<Mutex<InnerModule>>,
}

pub fn new_module() -> ci2::Result<WrappedModule> {
    WrappedModule::new()
}

trait ResultExt<T> {
    fn ci2err(self) -> std::result::Result<T, ci2::Error>;
}

impl<T> ResultExt<T> for flycap2::Result<T> {
    fn ci2err(self) -> std::result::Result<T, ci2::Error> {
        self.map_err(|orig: flycap2::Error| {
            let msg = format!("flycap2 error {:?}", orig);
            ci2::Error::from(msg)
        })
    }
}

impl WrappedModule {
    fn new() -> ci2::Result<Self> {
        let n_cams = get_num_cameras().ci2err()?;
        info!("{} camera(s) found", n_cams);

        let mut cams = Vec::with_capacity(n_cams);

        for i in 0..n_cams {
            let guid = get_guid_for_index(i).ci2err()?;
            info!("cam {}: {:?}", i, guid);
            let camera = FlycapContext::new(guid).ci2err()?;

            let mut format7_info = ffi::_fc2Format7Info::default();
            format7_info.mode = ffi::_fc2Mode::FC2_MODE_0;
            let (format7_info, is_supported) = camera.get_format7_info(format7_info).ci2err()?;
            info!(
                "  format7_info={:?} is_supported={}",
                format7_info, is_supported
            );

            let mut settings = ffi::_fc2Format7ImageSettings::default();
            settings.mode = format7_info.mode;
            settings.width = format7_info.maxWidth;
            settings.height = format7_info.maxHeight;
            let pixel_formats = format7_info.pixelFormatBitField;
            settings.pixelFormat = get_lowest_pixel_format(pixel_formats);

            let (settings, is_supported, packet_info) =
                camera.validate_format7_settings(settings.into()).ci2err()?;

            info!(
                "  settings={:?} is_supported={}, packet_info={:?}",
                settings, is_supported, packet_info
            );

            let packet_info: ffi::_fc2Format7PacketInfo = packet_info.into();
            let packet_size = packet_info.recommendedBytesPerPacket;
            camera
                .set_format7_configuration_packet(settings, packet_size)
                .ci2err()?;
            cams.push(camera);
        }

        Ok(WrappedModule {
            inner: Arc::new(Mutex::new(InnerModule { cams: cams })),
        })
    }
}

impl ci2::CameraModule for WrappedModule {
    type FrameType = Frame;
    type CameraType = WrappedCamera;

    fn name(&self) -> &str {
        "flycap2"
    }
    fn camera_infos(&self) -> ci2::Result<Vec<Box<dyn ci2::CameraInfo>>> {
        let guids: Vec<flycap2::GUID> = {
            let modinner = self.inner.lock();
            modinner.cams.iter().map(|cam| cam.guid().clone()).collect()
        };

        guids
            .into_iter()
            .map(|guid| {
                match WrappedCamera::new(self.inner.clone(), &guid) {
                    Ok(wc) => {
                        let bwc = Box::new(wc);
                        let ci: Box<dyn ci2::CameraInfo> = bwc; // explicitly perform type erasure
                        Ok(ci)
                    }
                    Err(e) => Err(e),
                }
            })
            .collect()
    }
    fn camera(&mut self, name: &str) -> ci2::Result<Self::CameraType> {
        let guid = flycap2::GUID::from_str(&name).ci2err()?;
        let camera = FlycapContext::new(guid).ci2err()?;

        Ok(WrappedCamera::new(self.inner.clone(), &guid)?)
    }
}

pub struct WrappedCamera {
    modinner: Arc<Mutex<InnerModule>>,
    idx: usize,
    guid: flycap2::GUID,
    name: String,
    model: String,
    vendor: String,
    fno: usize,
}

fn _test_camera_is_send() {
    // Compile-time test to ensure WrappedCamera implements Send trait.
    fn implements<T: Send>() {}
    implements::<WrappedCamera>();
}

impl WrappedCamera {
    fn new(modinner: Arc<Mutex<InnerModule>>, guid: &flycap2::GUID) -> ci2::Result<WrappedCamera> {
        let mut found_idx = None;

        let name = String::from(guid);

        {
            for (idx, cam) in modinner.lock().cams.iter().enumerate() {
                if cam.guid() == guid {
                    found_idx = Some(idx);
                }
            }
        }

        match found_idx {
            None => {
                let guid_str = String::from(guid);
                Err(ci2::Error::from(format!("camera {} not found", name)))
            }
            Some(idx) => {
                let info = {
                    let modi = modinner.lock();
                    modi.cams[idx].get_camera_info().ci2err()?
                };

                let val_cstr = unsafe { std::ffi::CStr::from_ptr(info.modelName.as_ptr()) };
                let v = val_cstr.to_str()?;
                let model = v.to_string();

                let val_cstr = unsafe { std::ffi::CStr::from_ptr(info.vendorName.as_ptr()) };
                let v = val_cstr.to_str()?;
                let vendor = v.to_string();

                Ok(WrappedCamera {
                    modinner: modinner,
                    idx,
                    guid: guid.clone(),
                    name,
                    model,
                    vendor,
                    fno: 0,
                })
            }
        }
    }
}

impl ci2::CameraInfo for WrappedCamera {
    fn name(&self) -> &str {
        &self.name
    }
    fn serial(&self) -> &str {
        &self.name
    }
    fn model(&self) -> &str {
        &self.model
    }
    fn vendor(&self) -> &str {
        &self.vendor
    }
}

impl ci2::Camera for WrappedCamera {
    type FrameType = Frame;

    fn width(&self) -> ci2::Result<u32> {
        unimplemented!();
    }
    fn height(&self) -> ci2::Result<u32> {
        unimplemented!();
    }
    fn pixel_format(&self) -> ci2::Result<formats::PixelFormat> {
        unimplemented!();
    }
    fn possible_pixel_formats(&self) -> ci2::Result<Vec<formats::PixelFormat>> {
        unimplemented!();
    }
    fn set_pixel_format(&mut self, pixel_format: formats::PixelFormat) -> ci2::Result<()> {
        unimplemented!();
    }
    fn exposure_time(&self) -> ci2::Result<f64> {
        unimplemented!();
    }
    fn exposure_time_range(&self) -> ci2::Result<(f64, f64)> {
        unimplemented!();
    }
    fn set_exposure_time(&mut self, value: f64) -> ci2::Result<()> {
        unimplemented!();
    }

    fn gain(&self) -> ci2::Result<f64> {
        unimplemented!();
    }
    fn gain_range(&self) -> ci2::Result<(f64, f64)> {
        unimplemented!();
    }
    fn set_gain(&mut self, value: f64) -> ci2::Result<()> {
        unimplemented!();
    }

    fn exposure_auto(&self) -> ci2::Result<ci2::AutoMode> {
        unimplemented!();
    }
    fn set_exposure_auto(&mut self, value: ci2::AutoMode) -> ci2::Result<()> {
        unimplemented!();
    }

    fn gain_auto(&self) -> ci2::Result<ci2::AutoMode> {
        unimplemented!();
    }
    fn set_gain_auto(&mut self, value: ci2::AutoMode) -> ci2::Result<()> {
        unimplemented!();
    }

    fn trigger_mode(&self) -> ci2::Result<ci2::TriggerMode> {
        unimplemented!();
    }
    fn set_trigger_mode(&mut self, value: ci2::TriggerMode) -> ci2::Result<()> {
        unimplemented!();
    }

    fn trigger_selector(&self) -> ci2::Result<ci2::TriggerSelector> {
        unimplemented!();
    }
    fn set_trigger_selector(&mut self, value: ci2::TriggerSelector) -> ci2::Result<()> {
        unimplemented!();
    }

    fn acquisition_frame_rate_enable(&self) -> ci2::Result<bool> {
        unimplemented!();
    }
    fn set_acquisition_frame_rate_enable(&mut self, value: bool) -> ci2::Result<()> {
        unimplemented!();
    }

    fn acquisition_frame_rate(&self) -> ci2::Result<f64> {
        unimplemented!();
    }
    fn acquisition_frame_rate_range(&self) -> ci2::Result<(f64, f64)> {
        unimplemented!();
    }
    fn set_acquisition_frame_rate(&mut self, value: f64) -> ci2::Result<()> {
        unimplemented!();
    }

    fn acquisition_mode(&self) -> ci2::Result<ci2::AcquisitionMode> {
        Ok(ci2::AcquisitionMode::Continuous)
    }
    fn set_acquisition_mode(&mut self, value: ci2::AcquisitionMode) -> ci2::Result<()> {
        if value != ci2::AcquisitionMode::Continuous {
            return Err(ci2::Error::from(format!(
                "unsupported acquisition mode: {:?}",
                value
            )));
        }
        Ok(())
    }

    fn acquisition_start(&mut self) -> ci2::Result<()> {
        let mut modinner = self.modinner.lock();
        let i = self.idx;
        modinner.cams[i].start_capture().ci2err()?;
        info!("      started capture");
        Ok(())
    }
    fn acquisition_stop(&mut self) -> ci2::Result<()> {
        let mut modinner = self.modinner.lock();
        let i = self.idx;
        modinner.cams[i].stop_capture().ci2err()?;
        info!("      stopped capture");
        Ok(())
    }
    fn next_frame(&mut self) -> ci2::Result<Frame> {
        // self.inner.lock().my_frame(None)
        let modinner = self.modinner.lock();
        let i = self.idx;
        let im = modinner.cams[i].retrieve_buffer().ci2err()?;
        self.fno += 1;
        flycap2_to_frame(im, self.fno)
    }
}

fn get_coding(
    format: ffi::fc2PixelFormat,
    bayer_format: ffi::fc2BayerTileFormat,
) -> ci2::Result<PixelFormat> {
    let e = match (format, bayer_format) {
        (ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_MONO8, ffi::_fc2BayerTileFormat::FC2_BT_NONE) => {
            PixelFormat::MONO8
        }
        (ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_411YUV8, ffi::_fc2BayerTileFormat::FC2_BT_NONE) => {
            PixelFormat::YUV411
        }
        (ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_422YUV8, ffi::_fc2BayerTileFormat::FC2_BT_NONE) => {
            PixelFormat::YUV422
        }
        (ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_444YUV8, ffi::_fc2BayerTileFormat::FC2_BT_NONE) => {
            PixelFormat::YUV444
        }
        (ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_RGB8, ffi::_fc2BayerTileFormat::FC2_BT_NONE) => {
            PixelFormat::RGB8
        }
        (ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_RAW8, ffi::_fc2BayerTileFormat::FC2_BT_RGGB) => {
            PixelFormat::BayerRG8
        }
        (ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_RAW8, ffi::_fc2BayerTileFormat::FC2_BT_GRBG) => {
            PixelFormat::BayerGR8
        }
        (ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_RAW8, ffi::_fc2BayerTileFormat::FC2_BT_GBRG) => {
            PixelFormat::BayerGB8
        }
        (ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_RAW8, ffi::_fc2BayerTileFormat::FC2_BT_BGGR) => {
            PixelFormat::BayerBG8
        }

        (f, b) => {
            return Err(ci2::Error::from(format!(
                "unimplemented conversion for {:?} {:?}",
                f, b
            )));
        }
    };
    Ok(e)
}

fn flycap2_to_frame(im: flycap2::Image, host_framenumber: usize) -> ci2::Result<Frame> {
    let device_timestamp = im.get_timestamp().ci2err()?;
    let raw = im.get_raw();
    let now: chrono::DateTime<chrono::Utc> = chrono::Utc::now();

    let data_copy = im.get_data_view().ci2err()?.to_vec(); // copy data

    let frame = Frame {
        width: raw.cols,
        height: raw.rows,
        stride: raw.stride,
        image_data: data_copy,
        host_timestamp: now,
        host_framenumber: host_framenumber,
        pixel_format: get_coding(raw.format, raw.bayerFormat)?,
        device_timestamp,
    };

    Ok(frame)
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
    pixel_format: formats::PixelFormat,            // format of the data
    device_timestamp: ffi::fc2TimeStamp,           // timestamp from the camera driver
}

impl Frame {
    pub fn device_timestamp(&self) -> ffi::fc2TimeStamp {
        self.device_timestamp.clone()
    }
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        // write!(f, "Frame {{ width: {}, height: {}, block_id: {}, device_timestamp: {} }}",
        //     self.width, self.height, self.block_id, self.device_timestamp)
        write!(
            f,
            "Frame {{ width: {}, height: {} }}",
            self.width, self.height
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

impl formats::ImageData for Frame {
    fn image_data(&self) -> &[u8] {
        &self.image_data
    }
    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.height
    }
    fn pixel_format(&self) -> formats::PixelFormat {
        self.pixel_format
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
