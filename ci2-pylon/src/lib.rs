extern crate failure;
#[macro_use]
extern crate failure_derive;
#[macro_use]
extern crate log;
extern crate parking_lot;

extern crate ci2;
extern crate pylon;
extern crate machine_vision_formats as formats;
extern crate timestamped_frame;
extern crate chrono;

use std::sync::Arc;

use parking_lot::Mutex;

trait ExtendedError<T> {
    fn map_pylon_err(self) -> ci2::Result<T>;
}

impl<T> ExtendedError<T> for std::result::Result<T, pylon::Error> {
    fn map_pylon_err(self) -> ci2::Result<T> {
        self.map_err(|e| ci2::Error::BackendError(failure::Error::from(e)))
    }
}

macro_rules! bail {
    ($e: expr) => {
        return Err(Error::OtherError(format!($e)).into());
    };
    ($fmt:expr, $($arg:tt)+) => {
        return Err(Error::OtherError(format!($fmt, $($arg)+)).into());
    };
}

pub type Result<M> = std::result::Result<M,Error>;

#[derive(Fail, Debug)]
pub enum Error {
    #[fail(display = "{}", _0)]
    PylonError(#[cause] pylon::Error),
    #[fail(display = "{}", _0)]
    IntParseError(#[cause] std::num::ParseIntError),
    #[fail(display = "OtherError {}", _0)]
    OtherError(String),
}

impl From<pylon::Error> for Error {
    fn from(o: pylon::Error) -> Self {
        Error::PylonError(o)
    }
}

impl From<Error> for ci2::Error {
    fn from(orig: Error) -> ci2::Error {
        ci2::Error::BackendError(orig.into())
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
    image_data: Vec<u8>, // raw image data
    host_timestamp: chrono::DateTime<chrono::Utc>, // timestamp from host computer
    host_framenumber: usize, // framenumber from host computer
    pixel_format: formats::PixelFormat, // format of the data
    pub block_id: u64, // framenumber from the camera driver
    pub device_timestamp: u64, // timestamp from the camera driver
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Frame {{ width: {}, height: {}, block_id: {}, device_timestamp: {} }}",
            self.width, self.height, self.block_id, self.device_timestamp)
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

use pylon::{AccessMode, GrabStatus, HasProperties, HasNodeMap};

struct InnerModule {
    _pylon: pylon::Pylon,
    tl_factory: pylon::TLFactory,
}

pub struct WrappedModule {
    inner: Arc<Mutex<InnerModule>>,
}

fn to_name(info: &pylon::DeviceInfo) -> String {
    // TODO: make ci2 cameras have full_name and friendly_name attributes?
    // &info.property_value("FullName").unwrap()
    let serial = &info.property_value("SerialNumber").unwrap();
    let vendor = &info.property_value("VendorName").unwrap();
    format!("{}-{}", vendor, serial)
}

pub fn new_module() -> ci2::Result<WrappedModule> {
    let module = pylon::Pylon::new().map_pylon_err()?;
    let tl_factory = module.tl_factory().map_pylon_err()?;
    // TODO: This will be the grab thread. Shall we (optionally?) boost priority?
    Ok(WrappedModule {
                    inner: Arc::new(Mutex::new(InnerModule {
                                                    _pylon: module,
                                                    tl_factory: tl_factory,
                                                })),
                })
}

impl ci2::CameraModule for WrappedModule {
    type FrameType = Frame;
    type CameraType = WrappedCamera;

    fn name(&self) -> &str {
        "pylon"
    }
    fn camera_infos(&self) -> ci2::Result<Vec<Box<dyn ci2::CameraInfo>>> {
        let ctx = self.inner.lock();
        let pylon_infos = ctx.tl_factory
            .enumerate_devices()
            .map_pylon_err()?;
        let infos = pylon_infos
            .into_iter()
            .map(|info| {
                let serial = info.property_value("SerialNumber").unwrap().to_string();
                let model = info.property_value("ModelName").unwrap().to_string();
                let vendor = info.property_value("VendorName").unwrap().to_string();
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
        Ok(WrappedCamera::new(self.inner.clone(), name)?)
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
pub struct WrappedCamera {
    inner: Arc<Mutex<WrappedCameraInner>>,
    name: String,
    serial: String,
    model: String,
    vendor: String,
    is_sfnc2: bool,
}

fn _test_camera_is_send() {
    // Compile-time test to ensure WrappedCamera implements Send trait.
    fn implements<T: Send>() {}
    implements::<WrappedCamera>();
}

struct FramecountExtra {
    epoch: u32,
    previous_block_id: u64,
    store_fno: u64,
    last_rollover: u64,
}

#[allow(dead_code)]
enum FramecoutingMethod {
    TrustDevice,
    BaslerGigE(FramecountExtra),
    IgnoreDevice(usize),
}

struct WrappedCameraInner {
    _modinner: Arc<Mutex<InnerModule>>,
    device: pylon::Device,
    framecounting_method: FramecoutingMethod,
    stream_grabber: Option<pylon::StreamGrabber>,
    wait_object: Option<pylon::WaitObject>,
    name: String,
}

impl WrappedCamera {
    fn new(modinner: Arc<Mutex<InnerModule>>, name: &str) -> Result<WrappedCamera> {
        let ctx = modinner.lock();

        let infos = ctx.tl_factory.enumerate_devices()?;
        let mut found_info = None;

        for this_info in infos.into_iter() {
            let this_name = to_name(&this_info);
            if this_name == name {
                found_info = Some(this_info);
            }
        }

        if let Some(info) = found_info {
            let mut device = ctx.tl_factory.create_device(&info)?;
            device.open(vec![AccessMode::Control, AccessMode::Stream])?;

            let serial = info.property_value("SerialNumber")?.to_string();
            let model = device.string_value("DeviceModelName")?;
            let vendor = device.string_value("DeviceVendorName")?;
            let device_class = info.property_value("DeviceClass").unwrap().to_string();
            let framecounting_method = if &device_class == "BaslerGigE" {
                debug!("  device_class BaslerGigE");
                FramecoutingMethod::BaslerGigE(FramecountExtra{
                    epoch: 0,
                    previous_block_id: 0,
                    store_fno: 0,
                    last_rollover: 0,
                    })
            } else {
                debug!("  device_class not BaslerGigE");
                FramecoutingMethod::TrustDevice
            };
            // let pixfmt = device.enumeration_value("PixelFormat")?;
            // let pixel_format = pixfmt_to_encoding(&pixfmt)?;
            info!("  opened device {}", name);

            let is_sfnc2 = match device.integer_value("DeviceSFNCVersionMajor") {
                Ok(major) => (major >= 2),
                Err(_) => (false),
            };

            match std::env::var("RUSTCAM_PYLON_PACKET_SIZE") {
                Ok(v) => {
                    match v.parse::<i64>() {
                        Ok(packet_size) => {
                            device.set_integer_value("GevSCPSPacketSize", packet_size)?;
                        }
                        Err(e) => {
                            bail!("could not parse to packet_size: {:?}", e);
                        }
                    }
                }
                Err(std::env::VarError::NotPresent) => {}
                Err(std::env::VarError::NotUnicode(_)) => {
                    bail!("received not unicode env var");
                }
            };

            match std::env::var("RUSTCAM_PYLON_PIXEL_FORMAT") {
                Ok(v) => {
                    device.set_enumeration_value("PixelFormat", &v)?;
                }
                Err(std::env::VarError::NotPresent) => {}
                Err(std::env::VarError::NotUnicode(_)) => {
                    bail!("received not unicode env var");
                }
            };

            Ok(WrappedCamera{
                inner: Arc::new(Mutex::new(WrappedCameraInner {
                   _modinner: modinner.clone(),
                   device: device,
                   framecounting_method,
                   stream_grabber: None,
                   wait_object: None,
                   name: name.to_string(),
                })),
                serial,
                model,
                vendor,
                name: name.to_string(),
                is_sfnc2,
                })

        } else {
            bail!("No device matching name {:?} found", name)
        }
    }

    fn gain_raw_to_db(&self, raw: i64) -> ci2::Result<f64> {
        // TODO check name of camera model with "Gain Properties" table
        // in Basler Product Documentation to ensure this is correct for
        // this particular camera model.
        Ok(0.0359*raw as f64)
    }

    fn gain_db_to_raw(&self, db: f64) -> ci2::Result<i64> {
        // TODO check name of camera model with "Gain Properties" table
        // in Basler Product Documentation to ensure this is correct for
        // this particular camera model.
        Ok((db/0.0359) as i64)
    }

}

impl WrappedCameraInner {
    fn my_start_streaming(&mut self) -> Result<()> {
        let ref mut device = self.device;

        match std::env::var("RUSTCAM_BIN_PIXELS") {
            Ok(v) => {
                match v.parse::<i64>() {
                    Ok(bins) => {
                        device.set_integer_value("BinningHorizontal", bins)?;
                        device.set_integer_value("BinningVertical", bins)?;
                        info!("using {} pixel binning", bins);
                    }
                    Err(e) => {
                        return Err(Error::IntParseError(e));
                    }
                }
            }
            Err(std::env::VarError::NotPresent) => {}
            Err(std::env::VarError::NotUnicode(_)) => {
                bail!("received not unicode env var");
            }
        };

        // // Do not use jumbo frames
        // if device.feature_is_writable("GevSCPSPacketSize")? {
        //     device.set_integer_value( "GevSCPSPacketSize", 1500 )?;
        // }

        let payload_size = device.integer_value("PayloadSize")?;

        //  Image grabbing is done using a stream grabber.
        //  A device may be able to provide different streams. A separate stream grabber must
        //  be used for each stream. In this sample, we create a stream grabber for the default
        //  stream, i.e., the first stream ( index == 0 ).
        let n_streams = device.num_stream_grabber_channels()?;

        if n_streams < 1 {
            bail!("The transport layer doesn't support image streams");
        }

        let mut stream_grabber = device.stream_grabber(0)?;
        match std::env::var("RUSTCAM_PYLON_ENABLE_RESEND") {
            Ok(v) => {
                match v.parse::<i64>() {
                    Ok(enable_resend) => {
                        let enable_resend: bool = enable_resend != 0;
                        stream_grabber.set_boolean_value("EnableResend", enable_resend)?;
                        info!("set EnableResend: {:?}", enable_resend);

                    }
                    Err(e) => {
                        bail!("could not parse to enable_resend: {:?}", e);
                    }
                }
            }
            Err(std::env::VarError::NotPresent) => {}
            Err(std::env::VarError::NotUnicode(_)) => {
                bail!("received not unicode env var");
            }
        };


        stream_grabber.open()?;

        // Get a handle for the stream grabber's wait object. The wait object
        // allows waiting for buffers to be filled with grabbed data.
        let wait_object = stream_grabber.get_wait_object()?;

        let num_buffers = 10;

        stream_grabber
            .set_integer_value("MaxNumBuffer", num_buffers)?;
        stream_grabber
            .set_integer_value("MaxBufferSize", payload_size)?;
        stream_grabber.prepare_grab()?;

        let mut buf_handles = Vec::with_capacity(num_buffers as usize);
        for _ in 0..num_buffers {
            let buf = pylon::Buffer::new(vec![0; payload_size as usize]);
            let handle = stream_grabber.register_buffer(buf)?; // push buffer in, get handle out
            buf_handles.push(handle);
        }

        for handle in buf_handles.into_iter() {
            stream_grabber.queue_buffer(handle)?; // pass ownership into stream grabber
        }

        device.execute_command("AcquisitionStart")?;
        self.stream_grabber = Some(stream_grabber);
        self.wait_object = Some(wait_object);
        Ok(())
    }

    fn my_stop_streaming(&mut self) -> Result<()> {
        if let Some(ref mut stream_grabber) = self.stream_grabber {
            let ref mut device = self.device;

            device.execute_command("AcquisitionStop")?;
            stream_grabber.cancel_grab()?;

            loop {
                let grab_result_opt = stream_grabber.retrieve_result()?;
                if grab_result_opt.is_none() {
                    break;
                }
            }

            loop {
                let handle = stream_grabber.pop_buffer();
                if handle.is_none() {
                    break;
                }
            }

            stream_grabber.finish_grab()?;
            stream_grabber.close()?;
        }
        self.stream_grabber = None;
        Ok(())
    }

    /// synchronous frame acquisition
    /// timeout with duration of zero for non-blocking behavior.
    /// timeout with duration None to block.
    fn my_frame(&mut self, timeout_ms: Option<u32>) -> ci2::Result<Frame> {
        let frame;

        enum FrameEnum {
            HasFrame(Frame),
            HasError((u32,String)),
        }

        if let Some(ref mut stream_grabber) = self.stream_grabber {
            if let Some(ref mut wait_object) = self.wait_object {

                let timeout_ms = match timeout_ms {
                    Some(v) => v,
                    None => 0xFFFFFFFF, // from `unsigned int waitForever` in pylon/WaitObject.h
                };

                let is_ready = wait_object.wait(timeout_ms as u64).map_pylon_err()?;
                let now = chrono::Utc::now(); // earliest possible timestamp
                if !is_ready {
                    return Err(ci2::Error::Timeout);
                }

                let grab_result_opt = stream_grabber.retrieve_result().map_pylon_err()?;
                let grab_result = match grab_result_opt {
                    Some(gr) => gr,
                    None => bail!("failed to retrieve a grab result"),
                };

                match grab_result.status() {
                    GrabStatus::Grabbed => {
                        let im = grab_result.image().map_pylon_err()?;
                        let stride = im.stride().map_pylon_err()? as u32;
                        let pix_type = im.pixel_type().map_pylon_err()?;
                        let pixel_format = convert_pix_type(pix_type)?;

                        let data = im.data_view().to_vec(); // copy data

                        let width = im.width().map_pylon_err()? as u32;
                        let height = im.height().map_pylon_err()? as u32;

                        let device_timestamp = grab_result.time_stamp();

                        let block_id = grab_result.block_id().map_pylon_err()?;
                        let fno: usize = match self.framecounting_method {
                            FramecoutingMethod::BaslerGigE(ref mut i) => {
                                // Basler GigE cameras wrap after 65535 block
                                if block_id < 30000 && i.previous_block_id > 30000 {
                                    // check nothing crazy is going on
                                    if (i.store_fno - i.last_rollover) < 30000 {
                                        return Err(ci2::Error::CI2Error(format!(
                                            "Cannot recover frame count with \
                                            Basler GigE camera {}. Did many \
                                            frames get dropped?", self.name)));
                                    }
                                    i.epoch += 1;
                                    i.last_rollover = i.store_fno;
                                }
                                i.store_fno += 1;
                                let fno = (i.epoch as usize * 65535) + block_id as usize;
                                i.previous_block_id = block_id;
                                fno
                            },
                            FramecoutingMethod::TrustDevice => {
                                block_id as usize
                            },
                            FramecoutingMethod::IgnoreDevice(ref mut store_fno) => {
                                let fno: usize = *store_fno;
                                *store_fno += 1;
                                fno
                            },
                        };

                        let r = Frame {
                            width,
                            height,
                            stride,
                            image_data: data,
                            device_timestamp: device_timestamp,
                            block_id,
                            host_timestamp: now,
                            host_framenumber: fno,
                            pixel_format: pixel_format,
                        };

                        frame = FrameEnum::HasFrame(r);

                    }
                    GrabStatus::Failed => {
                        // log error but continue
                        error!("Failed grab: 0x{:x} {}",
                            grab_result.error_code(), grab_result.error_description());
                        frame = FrameEnum::HasError((grab_result.error_code(), grab_result.error_description()));
                    }
                    GrabStatus::_UndefinedGrabStatus | GrabStatus::Idle | GrabStatus::Queued | GrabStatus::Canceled => {
                        bail!("unmatched grab result {:?}", grab_result.status());
                    }
                }

                stream_grabber.queue_buffer(grab_result.handle()).map_pylon_err()?; // pass ownership into grabber

            } else {
                bail!("Expected wait object. Have you started streaming?")
            }
        } else {
            bail!("expected stream grabber")
        }
        match frame {
            FrameEnum::HasFrame(f) => Ok(f),
            FrameEnum::HasError((err_code, err_descr)) => Err(ci2::Error::SingleFrameError(format!("pylon error code {}: {}", err_code, err_descr))),
        }
    }
}

macro_rules! get_mut_device {
    ($s: ident) => {
        $s.inner.lock().device
    }
}

pub fn convert_pix_type(pylon_type: pylon::PixelType) -> ci2::Result<formats::PixelFormat> {
    use pylon::PixelType::*;
    use formats::PixelFormat;
    use ci2::Error::CI2Error;
    let r = match pylon_type {
        Mono8 => PixelFormat::MONO8,
        YUV422packed => PixelFormat::YUV422,

        BayerGR8 => PixelFormat::BayerGR8,
        BayerRG8 => PixelFormat::BayerRG8,
        BayerGB8 => PixelFormat::BayerGB8,
        BayerBG8 => PixelFormat::BayerBG8,
        RGB8packed => PixelFormat::RGB8,

        pt => {return Err(CI2Error(format!("{:?}", pt)));},
    };
    Ok(r)
}

pub fn convert_pixel_format(pixel_format: formats::PixelFormat) -> ci2::Result<&'static str> {
    use formats::PixelFormat::*;
    use ci2::Error::CI2Error;
    let pixfmt = match pixel_format {
        MONO8 => "Mono8",
        YUV422 => "YUV422packed",

        BayerGR8 => "BayerGR8",
        BayerRG8 => "BayerRG8",
        BayerBG8 => "BayerBG8",
        BayerGB8 => "BayerGB8",

        RGB8 => "RGB8packed",
        // TODO: more than 8 bit encodings, YUV, and so on.
        e => {return Err(CI2Error(format!("{:?}", e)));},
    };
    Ok(pixfmt)
}

pub fn convert_to_pixel_format(orig: &str) -> ci2::Result<formats::PixelFormat> {
    use formats::PixelFormat::*;
    use ci2::Error::CI2Error;
    let pixfmt = match orig {
        "EnumEntry_PixelFormat_Mono8" => MONO8,
        "EnumEntry_PixelFormat_Mono16" => MONO16,
        // "EnumEntry_PixelFormat_BGRA8Packed" => ,
        // "EnumEntry_PixelFormat_BGR8Packed" => ,
        "EnumEntry_PixelFormat_RGB8Packed" => RGB8,
        // "EnumEntry_PixelFormat_RGB16Packed" => ,
        "EnumEntry_PixelFormat_BayerGR8" => BayerGR8,
        "EnumEntry_PixelFormat_BayerRG8" => BayerRG8,
        "EnumEntry_PixelFormat_BayerGB8" => BayerGB8,
        "EnumEntry_PixelFormat_BayerBG8" => BayerBG8,
        // // TODO: more than 8 bit encodings, YUV, and so on.
        e => {return Err(CI2Error(format!("Unknown pixel format: \"{}\"", e)));},
    };
    Ok(pixfmt)
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

fn pixfmt_to_encoding(pixfmt: &str) -> Result<formats::PixelFormat> {
    let p: &str = &pixfmt;
    let pixel_format = match p {
        "Mono8" => formats::PixelFormat::MONO8,
        "BayerGR8" => formats::PixelFormat::BayerGR8,
        "BayerRG8" => formats::PixelFormat::BayerRG8,
        "BayerBG8" => formats::PixelFormat::BayerBG8,
        "BayerGB8" => formats::PixelFormat::BayerGB8,
        "YUV422Packed" => formats::PixelFormat::YUV422,
        // TODO: more than 8 bit encodings, YUV, and so on.
        f => bail!("unknown or unimplemented pixel format {:?}",f),
    };
    Ok(pixel_format)
}

impl ci2::Camera for WrappedCamera {
    type FrameType = Frame;

    fn width(&self) -> ci2::Result<u32> {
        let ref device = get_mut_device!(self);
        let width = device.integer_value("Width").map_pylon_err()? as u32;
        Ok(width)
    }

    fn height(&self) -> ci2::Result<u32> {
        let ref device = get_mut_device!(self);
        let height = device.integer_value("Height").map_pylon_err()? as u32;
        Ok(height)
    }

    fn pixel_format(&self) -> ci2::Result<formats::PixelFormat> {
        let ref device = get_mut_device!(self);
        let pixfmt = device
            .enumeration_value("PixelFormat")
            .map_pylon_err()?;
        let pixel_format = pixfmt_to_encoding(&pixfmt)?;
        Ok(pixel_format)
    }

    fn possible_pixel_formats(&self) -> ci2::Result<Vec<formats::PixelFormat>> {
        let ref device = get_mut_device!(self);
        let formats = device
            .get_enumeration_entries("PixelFormat")
            .map_pylon_err()?;
        let mut result = Vec::new();
        for format in formats.iter() {
            match convert_to_pixel_format(format) {
                Ok(pixfmt) => result.push(pixfmt),
                Err(_) => {
                    info!("ignoring unsupported pixel format: {}", format);
                },
            }
        }
        Ok(result)
    }

    fn set_pixel_format(&mut self, pixel_format: formats::PixelFormat) -> ci2::Result<()> {
        let pixfmt = convert_pixel_format(pixel_format)?;
        let ref mut device = get_mut_device!(self);
        device
            .set_enumeration_value("PixelFormat", pixfmt)
            .map_pylon_err()
    }

    fn exposure_time(&self) -> ci2::Result<f64> {
        let ref device = get_mut_device!(self);
        if self.is_sfnc2 {
            device
                .float_value("ExposureTime")
                .map_pylon_err()
        } else {
            device
                .float_value("ExposureTimeAbs")
                .map_pylon_err()
        }
    }
    fn exposure_time_range(&self) -> ci2::Result<(f64, f64)> {
        let ref device = get_mut_device!(self);
        if self.is_sfnc2 {
            device
                .float_range("ExposureTime")
                .map_pylon_err()
        } else {
            device
                .float_range("ExposureTimeAbs")
                .map_pylon_err()
        }
    }
    fn set_exposure_time(&mut self, value: f64) -> ci2::Result<()> {
        let ref mut device = get_mut_device!(self);
        if self.is_sfnc2 {
            device
                .set_float_value("ExposureTime", value)
                .map_pylon_err()
        } else {
            device
                .set_float_value("ExposureTimeAbs", value)
                .map_pylon_err()
        }
    }

    fn gain(&self) -> ci2::Result<f64> {
        let ref device = get_mut_device!(self);
        if self.is_sfnc2 {
            device.float_value("Gain").map_pylon_err()
        } else {
            let gain_raw = device.integer_value("GainRaw").map_pylon_err()?;
            let gain_db = self.gain_raw_to_db(gain_raw)?;
            debug!("got gain raw {}, converted to db {}", gain_raw, gain_db);
            Ok(gain_db as f64)
        }
    }
    fn gain_range(&self) -> ci2::Result<(f64, f64)> {
        let ref device = get_mut_device!(self);
        if self.is_sfnc2 {
            device.float_range("Gain").map_pylon_err()
        } else {
            let (gain_min,gain_max) = device.integer_range("GainRaw").map_pylon_err()?;
            let gain_min_db = self.gain_raw_to_db(gain_min)?;
            let gain_max_db = self.gain_raw_to_db(gain_max)?;
            Ok((gain_min_db, gain_max_db))
        }
    }
    fn set_gain(&mut self, value: f64) -> ci2::Result<()> {
        let ref mut device = get_mut_device!(self);
        if self.is_sfnc2 {
            device
                .set_float_value("Gain", value)
                .map_pylon_err()
        } else {
            let gain_raw_int = self.gain_db_to_raw(value)?;
            debug!("got gain db {}, converted to raw {}", value, gain_raw_int);
            device
                .set_integer_value("GainRaw", gain_raw_int)
                .map_pylon_err()
        }
    }

    fn exposure_auto(&self) -> ci2::Result<ci2::AutoMode> {
        let ref device = get_mut_device!(self);
        let val = device
            .enumeration_value("ExposureAuto")
            .map_pylon_err()?;
        match val.as_ref() {
            "Off" => Ok(ci2::AutoMode::Off),
            "Once" => Ok(ci2::AutoMode::Once),
            "Continuous" => Ok(ci2::AutoMode::Continuous),
            s => bail!("unexpected ExposureAuto enum string: {}", s),
        }
    }
    fn set_exposure_auto(&mut self, value: ci2::AutoMode) -> ci2::Result<()> {
        let sval = match value {
            ci2::AutoMode::Off => "Off",
            ci2::AutoMode::Once => "Once",
            ci2::AutoMode::Continuous => "Continuous",
        };
        let ref mut device = get_mut_device!(self);
        device
            .set_enumeration_value("ExposureAuto", sval)
            .map_pylon_err()
    }

    fn gain_auto(&self) -> ci2::Result<ci2::AutoMode> {
        let ref device = get_mut_device!(self);
        let val = device
            .enumeration_value("GainAuto")
            .map_pylon_err()?;
        match val.as_ref() {
            "Off" => Ok(ci2::AutoMode::Off),
            "Once" => Ok(ci2::AutoMode::Once),
            "Continuous" => Ok(ci2::AutoMode::Continuous),
            s => bail!("unexpected GainAuto enum string: {}", s),
        }
    }
    fn set_gain_auto(&mut self, value: ci2::AutoMode) -> ci2::Result<()> {
        let sval = match value {
            ci2::AutoMode::Off => "Off",
            ci2::AutoMode::Once => "Once",
            ci2::AutoMode::Continuous => "Continuous",
        };
        let ref mut device = get_mut_device!(self);
        device
            .set_enumeration_value("GainAuto", sval)
            .map_pylon_err()
    }

    fn trigger_mode(&self) -> ci2::Result<ci2::TriggerMode> {
        let ref device = get_mut_device!(self);
        let val = device
            .enumeration_value("TriggerMode")
            .map_pylon_err()?;
        match val.as_ref() {
            "Off" => Ok(ci2::TriggerMode::Off),
            "On" => Ok(ci2::TriggerMode::On),
            s => bail!("unexpected TriggerMode enum string: {}", s),
        }
    }
    fn set_trigger_mode(&mut self, value: ci2::TriggerMode) -> ci2::Result<()> {
        let sval = match value {
            ci2::TriggerMode::Off => "Off",
            ci2::TriggerMode::On => "On",
        };
        let ref mut device = get_mut_device!(self);
        device
            .set_enumeration_value("TriggerMode", sval)
            .map_pylon_err()
    }

    fn acquisition_frame_rate_enable(&self) -> ci2::Result<bool> {
        let ref device = get_mut_device!(self);
        device
            .boolean_value("AcquisitionFrameRateEnable")
            .map_pylon_err()
    }
    fn set_acquisition_frame_rate_enable(&mut self, value: bool) -> ci2::Result<()> {
        let ref mut device = get_mut_device!(self);
        device
            .set_boolean_value("AcquisitionFrameRateEnable", value)
            .map_pylon_err()
    }

    fn acquisition_frame_rate(&self) -> ci2::Result<f64> {
        let ref device = get_mut_device!(self);
        if self.is_sfnc2 {
            device
                .float_value("AcquisitionFrameRate")
                .map_pylon_err()
        } else {
            device
                .float_value("AcquisitionFrameRateAbs")
                .map_pylon_err()
        }
    }
    fn acquisition_frame_rate_range(&self) -> ci2::Result<(f64, f64)> {
        let ref device = get_mut_device!(self);
        if self.is_sfnc2 {
            device
                .float_range("AcquisitionFrameRate")
                .map_pylon_err()
        } else {
            device
                .float_range("AcquisitionFrameRateAbs")
                .map_pylon_err()
        }
    }
    fn set_acquisition_frame_rate(&mut self, value: f64) -> ci2::Result<()> {
        let ref mut device = get_mut_device!(self);
        if self.is_sfnc2 {
            device
                .set_float_value("AcquisitionFrameRate", value)
                .map_pylon_err()
        } else {
            device
                .set_float_value("AcquisitionFrameRateAbs", value)
                .map_pylon_err()
        }
    }

    fn trigger_selector(&self) -> ci2::Result<ci2::TriggerSelector> {
        let ref device = get_mut_device!(self);
        let val = device
            .enumeration_value("TriggerSelector")
            .map_pylon_err()?;
        match val.as_ref() {
            "AcquisitionStart" => Ok(ci2::TriggerSelector::AcquisitionStart),
            "FrameBurstStart" => Ok(ci2::TriggerSelector::FrameBurstStart),
            "FrameStart" => Ok(ci2::TriggerSelector::FrameStart),
            s => bail!("unexpected TriggerSelector enum string: {}", s),
        }
    }
    fn set_trigger_selector(&mut self, value: ci2::TriggerSelector) -> ci2::Result<()> {
        let sval = match value {
            ci2::TriggerSelector::AcquisitionStart => "AcquisitionStart",
            ci2::TriggerSelector::FrameBurstStart => "FrameBurstStart",
            ci2::TriggerSelector::FrameStart => "FrameStart",
            ci2::TriggerSelector::ExposureActive => "ExposureActive",
            s => {
                return Err(ci2::Error::CI2Error(format!(
                    "unexpected TriggerSelector enum: {:?}",
                    s
                )));
            }
        };
        let ref mut device = get_mut_device!(self);
        device
            .set_enumeration_value("TriggerSelector", sval)
            .map_pylon_err()
    }

    fn acquisition_mode(&self) -> ci2::Result<ci2::AcquisitionMode> {
        let ref device = get_mut_device!(self);
        let val = device
            .enumeration_value("AcquisitionMode")
            .map_pylon_err()?;
        match val.as_ref() {
            "Continuous" => Ok(ci2::AcquisitionMode::Continuous),
            "SingleFrame" => Ok(ci2::AcquisitionMode::SingleFrame),
            "MultiFrame" => Ok(ci2::AcquisitionMode::MultiFrame),
            s => bail!("unexpected AcquisitionMode enum string: {}", s),
        }
    }
    fn set_acquisition_mode(&mut self, value: ci2::AcquisitionMode) -> ci2::Result<()> {
        let sval = match value {
            ci2::AcquisitionMode::Continuous => "Continuous",
            ci2::AcquisitionMode::SingleFrame => "SingleFrame",
            ci2::AcquisitionMode::MultiFrame => "MultiFrame",
        };
        let ref mut device = get_mut_device!(self);
        device
            .set_enumeration_value("AcquisitionMode", sval)
            .map_pylon_err()
    }

    fn acquisition_start(&mut self) -> ci2::Result<()> {
        Ok(self.inner.lock().my_start_streaming()?)
    }
    fn acquisition_stop(&mut self) -> ci2::Result<()> {
        Ok(self.inner.lock().my_stop_streaming()?)
    }

    fn next_frame(&mut self) -> ci2::Result<Frame>{
        self.inner.lock().my_frame(None)
    }

}
