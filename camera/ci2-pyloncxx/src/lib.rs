extern crate machine_vision_formats as formats;

use anyhow::Context;
use std::sync::{Arc, Mutex};

use ci2::{
    AcquisitionMode, AutoMode, DynamicFrameWithInfo, HostTimingInfo, TriggerMode, TriggerSelector,
};
use pylon_cxx::HasProperties;
use strand_dynamic_frame::DynamicFrameOwned;

trait ExtendedError<T> {
    fn map_pylon_err(self) -> ci2::Result<T>;
}

impl<T> ExtendedError<T> for std::result::Result<T, pylon_cxx::PylonError> {
    fn map_pylon_err(self) -> ci2::Result<T> {
        self.map_err(|pylon_error| ci2::Error::BackendError(anyhow::Error::new(pylon_error)))
    }
}

pub type Result<M> = std::result::Result<M, Error>;

const BAD_FNO: usize = usize::MAX;

mod feature_cache;
use feature_cache::*;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Pylon error: {source}")]
    PylonError {
        #[from]
        source: pylon_cxx::PylonError,
    },
    #[error("int parse error: {source}")]
    IntParseError {
        #[from]
        source: std::num::ParseIntError,
    },
    #[error("other error: {msg}")]
    OtherError { msg: String },
}

impl From<Error> for ci2::Error {
    fn from(orig: Error) -> ci2::Error {
        ci2::Error::BackendError(orig.into())
    }
}

pub struct WrappedModule {
    pylon_auto_init: pylon_cxx::Pylon,
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
        pylon_auto_init: pylon_cxx::Pylon::new(),
    })
}

pub struct PylonTerminateGuard {
    already_dropped: bool,
}

impl Drop for PylonTerminateGuard {
    fn drop(&mut self) {
        if !self.already_dropped {
            unsafe {
                pylon_cxx::terminate(true);
            }
            self.already_dropped = true;
        }
    }
}

pub fn make_singleton_guard(
    _pylon_module: &dyn ci2::CameraModule<CameraType = WrappedCamera, Guard = PylonTerminateGuard>,
) -> ci2::Result<PylonTerminateGuard> {
    Ok(PylonTerminateGuard {
        already_dropped: false,
    })
}

impl<'a> ci2::CameraModule for &'a WrappedModule {
    type CameraType = WrappedCamera<'a>;
    type Guard = PylonTerminateGuard;

    fn name(self: &&'a WrappedModule) -> &'static str {
        "pyloncxx"
    }
    fn camera_infos(self: &&'a WrappedModule) -> ci2::Result<Vec<Box<dyn ci2::CameraInfo>>> {
        let pylon_infos = pylon_cxx::TlFactory::instance(&self.pylon_auto_init)
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
    fn camera(self: &mut &'a WrappedModule, name: &str) -> ci2::Result<Self::CameraType> {
        WrappedCamera::new(&self.pylon_auto_init, name)
    }
    fn settings_file_extension(&self) -> &str {
        // See https://www.baslerweb.com/en/sales-support/knowledge-base/frequently-asked-questions/saving-camera-features-or-user-sets-as-file-on-hard-disk/588482/
        "pfs" // Pylon Feature Stream
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
pub struct WrappedCamera<'a> {
    inner: Arc<Mutex<pylon_cxx::InstantCamera<'a>>>,
    store_fno: usize,
    name: String,
    serial: String,
    model: String,
    vendor: String,
    grab_result: Arc<Mutex<pylon_cxx::GrabResult>>,
    is_sfnc2: bool,
    pfs_cache: Arc<Mutex<PfsCache>>,
}

fn _test_camera_is_send() {
    // Compile-time test to ensure WrappedCamera implements Send trait.
    fn implements<T: Send>() {}
    implements::<WrappedCamera>();
}

impl<'a> WrappedCamera<'a> {
    fn new(lib: &'a pylon_cxx::Pylon, name: &str) -> ci2::Result<Self> {
        let max_u64_as_usize: usize = u64::MAX.try_into().unwrap();
        assert_eq!(max_u64_as_usize, BAD_FNO);

        let tl_factory = pylon_cxx::TlFactory::instance(lib);
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
                let store_fno = 0;

                let cam = tl_factory
                    .create_device(&device_info)
                    .context("creating device")?;
                cam.open().context("opening camera")?;

                let is_sfnc2 = match cam
                    .node_map()
                    .map_pylon_err()?
                    .integer_node("DeviceSFNCVersionMajor")
                    .map_pylon_err()?
                    .value()
                {
                    Ok(major) => major >= 2,
                    Err(_) => false,
                };

                let set_max_transfer_size = match std::env::var_os("DISABLE_SET_MAX_TRANSFER_SIZE")
                {
                    Some(v) => &v == "0",
                    None => true,
                };

                if set_max_transfer_size {
                    // Set stream grabber MaxTransferSize. This is a
                    // Basler-specific quirk and so to avoid introducing a
                    // Basler-specific API, we do this always (unless the user
                    // sets the environment variable to disable it).

                    let mut node = cam
                        .stream_grabber_node_map()
                        .map_pylon_err()?
                        .integer_node("MaxTransferSize")
                        .map_pylon_err()?;

                    if let Ok(max_size) = node.max() {
                        // If this node exists, we want to set it. If we cannot
                        // open the node (because, e.g. the stream grabber is
                        // for GigE not USB3), don't bother.
                        node.set_value(max_size).map_pylon_err()?;
                        tracing::debug!(
                            "For camera {}, set stream grabber MaxTransferSize to {}",
                            name,
                            max_size
                        );

                        #[cfg(target_os = "linux")]
                        {
                            // This seems to be a USB camera, let's also check /sys/module/usbcore/parameters/usbfs_memory_mb
                            let fname = "/sys/module/usbcore/parameters/usbfs_memory_mb";
                            match std::fs::read_to_string(&fname) {
                                Ok(usbfs_memory_mb) => {
                                    let usbfs_memory_mb: i64 =
                                        usbfs_memory_mb.trim().parse().unwrap();
                                    let desired_mb = 1000;
                                    if usbfs_memory_mb < desired_mb {
                                        tracing::warn!("You seem to be using a USB3 camera on linux but the file \"{}\" \
                                        is set to only {}. For best performance, consider setting it to {}. \
                                        For more information, see \
                                        https://web.archive.org/web/20230318224225/https://www.baslerweb.com/en/sales-support/knowledge-base/frequently-asked-questions/how-can-i-set-the-usbfs-on-linux-or-linux-for-arm-to-prevent-image-losses-with-pylon-and-usb-cameras/29826/.",
                                        fname, usbfs_memory_mb, desired_mb);
                                    } else {
                                        tracing::debug!(
                                            "File \"{}\" indicates a value of {}.",
                                            fname,
                                            usbfs_memory_mb
                                        );
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("Could not read {} to check USB subsystem memory due to error: {}", fname, e);
                                }
                            }

                            // While we are at it, let's check max number of open file descriptors.
                            // one greater than the maximum file descriptor number that can be opened by this process.
                            match rlimit::Resource::NOFILE.get() {
                                Ok((soft, _hard)) => {
                                    let desired = 4096;
                                    if soft < desired {
                                        tracing::warn!("You seem to be using linux but you have only {} file descriptors available. \
                                        For best performance, set this to at least {}. See https://github.com/basler/pypylon/issues/80#issuecomment-461727225 \
                                        for more information. Hint: use 'ulimit -n 4096' to update.",
                                        soft, desired);
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("Could not check max number of open file descriptors due to error: {}", e);
                                }
                            }
                        }
                    }
                }

                let pfs_cache = {
                    let node_map = cam.node_map().map_pylon_err()?;
                    let settings = node_map.save_to_string().map_pylon_err()?;
                    PfsCache::new_from_string(settings)?
                };
                let pfs_cache = Arc::new(Mutex::new(pfs_cache));

                let grab_result =
                    Arc::new(Mutex::new(pylon_cxx::GrabResult::new().map_pylon_err()?));
                return Ok(Self {
                    // pylon_auto_init: Arc::new(Mutex::new(pylon_cxx::Pylon::new())),
                    inner: Arc::new(Mutex::new(cam)),
                    name: name.to_string(),
                    store_fno,
                    serial,
                    model,
                    vendor,
                    grab_result,
                    is_sfnc2,
                    pfs_cache,
                });
            }
        }
        Err(Error::OtherError {
            msg: format!("requested camera '{}' was not found", name),
        }
        .into())
    }

    fn exposure_time_param_name(&self) -> &'static str {
        if self.is_sfnc2 {
            "ExposureTime"
        } else {
            "ExposureTimeRaw"
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

impl<'a> ci2::CameraInfo for WrappedCamera<'a> {
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

impl<'a> ci2::Camera for WrappedCamera<'a> {
    // ----- start: weakly typed but easier to implement API -----

    // fn feature_access_query(&self, name: &str) -> ci2::Result<ci2::AccessQueryResult> {
    //     todo!();
    // }

    fn command_execute(&self, name: &str, verify: bool) -> ci2::Result<()> {
        let camera = self.inner.lock().unwrap();
        camera
            .node_map()
            .map_pylon_err()?
            .command_node(name)
            .map_pylon_err()?
            .execute(verify)
            .map_pylon_err()
    }

    fn feature_bool(&self, name: &str) -> ci2::Result<bool> {
        let camera = self.inner.lock().unwrap();
        camera
            .node_map()
            .map_pylon_err()?
            .boolean_node(name)
            .map_pylon_err()?
            .value()
            .map_pylon_err()
    }

    fn feature_bool_set(&self, name: &str, value: bool) -> ci2::Result<()> {
        let camera = self.inner.lock().unwrap();
        camera
            .node_map()
            .map_pylon_err()?
            .boolean_node(name)
            .map_pylon_err()?
            .set_value(value)
            .map_pylon_err()
    }

    fn feature_enum(&self, name: &str) -> ci2::Result<String> {
        let camera = self.inner.lock().unwrap();
        let node = camera
            .node_map()
            .map_pylon_err()?
            .enum_node(name)
            .map_pylon_err()?;
        node.value().map_pylon_err()
    }

    fn feature_enum_set(&self, name: &str, value: &str) -> ci2::Result<()> {
        let camera = self.inner.lock().unwrap();
        let mut node = camera
            .node_map()
            .map_pylon_err()?
            .enum_node(name)
            .map_pylon_err()?;
        node.set_value_pfs(&mut self.pfs_cache.lock().unwrap(), value)
            .map_pylon_err()
    }

    fn feature_float(&self, name: &str) -> ci2::Result<f64> {
        let camera = self.inner.lock().unwrap();
        camera
            .node_map()
            .map_pylon_err()?
            .float_node(name)
            .map_pylon_err()?
            .value()
            .map_pylon_err()
    }

    fn feature_float_set(&self, name: &str, value: f64) -> ci2::Result<()> {
        let camera = self.inner.lock().unwrap();
        camera
            .node_map()
            .map_pylon_err()?
            .float_node(name)
            .map_pylon_err()?
            .set_value(value)
            .map_pylon_err()
    }

    fn feature_int(&self, name: &str) -> ci2::Result<i64> {
        let camera = self.inner.lock().unwrap();
        camera
            .node_map()
            .map_pylon_err()?
            .integer_node(name)
            .map_pylon_err()?
            .value()
            .map_pylon_err()
    }

    fn feature_int_set(&self, name: &str, value: i64) -> ci2::Result<()> {
        let camera = self.inner.lock().unwrap();
        camera
            .node_map()
            .map_pylon_err()?
            .integer_node(name)
            .map_pylon_err()?
            .set_value(value)
            .map_pylon_err()
    }

    // ----- end: weakly typed but easier to implement API -----

    fn node_map_load(&self, settings: &str) -> ci2::Result<()> {
        // It seems that sometimes the Pylon PFS (Pylon Feature Stream) files
        // may have CRLF line endings but loading from a string only works with
        // LF line endings. So here we convert line endings to LF only.
        let settings_lf_only = settings.lines().collect::<Vec<_>>().join("\n");

        let camera = self.inner.lock().unwrap();
        camera
            .node_map()
            .map_pylon_err()?
            .load_from_string(settings_lf_only, true)
            .map_pylon_err()?;
        Ok(())
    }

    fn node_map_save(&self) -> ci2::Result<String> {
        // Ideally we would simply call camera.node_map().map_pylon_err()?.save_to_string() here,
        // but this requires stopping the camera. Instead we cache the node
        // values.
        Ok(self.pfs_cache.lock().unwrap().to_header_string())
    }

    /// Return the sensor width in pixels
    fn width(&self) -> ci2::Result<u32> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .node_map()
            .map_pylon_err()?
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
            .unwrap()
            .node_map()
            .map_pylon_err()?
            .integer_node("Height")
            .map_pylon_err()?
            .value()
            .map_pylon_err()?
            .try_into()?)
    }

    // Settings: PixFmt ----------------------------
    fn pixel_format(&self) -> ci2::Result<formats::PixFmt> {
        let camera = self.inner.lock().unwrap();
        let pixel_format_node = camera
            .node_map()
            .map_pylon_err()?
            .enum_node("PixelFormat")
            .map_pylon_err()?;
        convert_to_pixel_format(pixel_format_node.value().map_pylon_err()?.as_ref())
    }
    fn possible_pixel_formats(&self) -> ci2::Result<Vec<formats::PixFmt>> {
        let camera = self.inner.lock().unwrap();
        let pixel_format_node = camera
            .node_map()
            .map_pylon_err()?
            .enum_node("PixelFormat")
            .map_pylon_err()?;
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
        let camera = self.inner.lock().unwrap();
        let mut pixel_format_node = camera
            .node_map()
            .map_pylon_err()?
            .enum_node("PixelFormat")
            .map_pylon_err()?;
        pixel_format_node
            .set_value_pfs(&mut self.pfs_cache.lock().unwrap(), s)
            .map_pylon_err()
    }

    // Settings: Exposure Time ----------------------------
    /// value given in microseconds

    fn exposure_time(&self) -> ci2::Result<f64> {
        let camera = self.inner.lock().unwrap();
        let name = self.exposure_time_param_name();
        if self.is_sfnc2 {
            camera
                .node_map()
                .map_pylon_err()?
                .float_node(name)
                .map_pylon_err()?
                .value()
                .map_pylon_err()
        } else {
            camera
                .node_map()
                .map_pylon_err()?
                .integer_node(name)
                .map_pylon_err()?
                .value()
                .map_pylon_err()
                .map(|x| x as f64)
        }
    }

    /// value given in microseconds
    fn exposure_time_range(&self) -> ci2::Result<(f64, f64)> {
        let camera = self.inner.lock().unwrap();
        let name = self.exposure_time_param_name();
        if self.is_sfnc2 {
            let node = camera
                .node_map()
                .map_pylon_err()?
                .float_node(name)
                .map_pylon_err()?;
            Ok((node.min().map_pylon_err()?, node.max().map_pylon_err()?))
        } else {
            let node = camera
                .node_map()
                .map_pylon_err()?
                .integer_node(name)
                .map_pylon_err()?;
            Ok((
                node.min().map_pylon_err()? as f64,
                node.max().map_pylon_err()? as f64,
            ))
        }
    }

    /// value given in microseconds
    fn set_exposure_time(&mut self, value: f64) -> ci2::Result<()> {
        let camera = self.inner.lock().unwrap();
        let name = self.exposure_time_param_name();
        if self.is_sfnc2 {
            camera
                .node_map()
                .map_pylon_err()?
                .float_node(name)
                .map_pylon_err()?
                .set_value_pfs(&mut self.pfs_cache.lock().unwrap(), value)
                .map_pylon_err()
        } else {
            camera
                .node_map()
                .map_pylon_err()?
                .integer_node(name)
                .map_pylon_err()?
                .set_value_pfs(&mut self.pfs_cache.lock().unwrap(), value.round() as i64)
                .map_pylon_err()
        }
    }

    // Settings: Exposure Time Auto Mode ----------------------------
    fn exposure_auto(&self) -> ci2::Result<AutoMode> {
        let camera = self.inner.lock().unwrap();
        let val = camera
            .node_map()
            .map_pylon_err()?
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
            .unwrap()
            .node_map()
            .map_pylon_err()?
            .enum_node("ExposureAuto")
            .map_pylon_err()?
            .set_value_pfs(&mut self.pfs_cache.lock().unwrap(), sval)
            .map_pylon_err()
    }

    // Settings: Gain ----------------------------
    /// value given in dB
    fn gain(&self) -> ci2::Result<f64> {
        let camera = self.inner.lock().unwrap();
        if self.is_sfnc2 {
            camera
                .node_map()
                .map_pylon_err()?
                .float_node("Gain")
                .map_pylon_err()?
                .value()
                .map_pylon_err()
        } else {
            let gain_raw = camera
                .node_map()
                .map_pylon_err()?
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
        let camera = self.inner.lock().unwrap();
        if self.is_sfnc2 {
            let gain_node = camera
                .node_map()
                .map_pylon_err()?
                .float_node("Gain")
                .map_pylon_err()?;
            Ok((
                gain_node.min().map_pylon_err()?,
                gain_node.max().map_pylon_err()?,
            ))
        } else {
            let gain_node = camera
                .node_map()
                .map_pylon_err()?
                .integer_node("GainRaw")
                .map_pylon_err()?;

            let gain_min = gain_node.min().map_pylon_err()?;
            let gain_max = gain_node.max().map_pylon_err()?;

            let gain_min_db = gain_raw_to_db(gain_min)?;
            let gain_max_db = gain_raw_to_db(gain_max)?;
            Ok((gain_min_db, gain_max_db))
        }
    }

    /// value given in dB
    fn set_gain(&mut self, gain_db: f64) -> ci2::Result<()> {
        let camera = self.inner.lock().unwrap();
        if self.is_sfnc2 {
            camera
                .node_map()
                .map_pylon_err()?
                .float_node("Gain")
                .map_pylon_err()?
                .set_value_pfs(&mut self.pfs_cache.lock().unwrap(), gain_db)
                .map_pylon_err()?;
        } else {
            let gain_raw = gain_db_to_raw(gain_db)?;
            camera
                .node_map()
                .map_pylon_err()?
                .integer_node("GainRaw")
                .map_pylon_err()?
                .set_value_pfs(&mut self.pfs_cache.lock().unwrap(), gain_raw)
                .map_pylon_err()?;
        }
        Ok(())
    }

    // Settings: Gain Auto Mode ----------------------------
    fn gain_auto(&self) -> ci2::Result<AutoMode> {
        let camera = self.inner.lock().unwrap();
        let val = camera
            .node_map()
            .map_pylon_err()?
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
            .unwrap()
            .node_map()
            .map_pylon_err()?
            .enum_node("GainAuto")
            .map_pylon_err()?
            .set_value_pfs(&mut self.pfs_cache.lock().unwrap(), sval)
            .map_pylon_err()
    }

    // Settings: TriggerMode ----------------------------
    fn trigger_mode(&self) -> ci2::Result<TriggerMode> {
        let camera = self.inner.lock().unwrap();
        let val = camera
            .node_map()
            .map_pylon_err()?
            .enum_node("TriggerMode")
            .map_pylon_err()?
            .value()
            .map_pylon_err()?;
        match val.as_ref() {
            "Off" => Ok(ci2::TriggerMode::Off),
            "On" => Ok(ci2::TriggerMode::On),
            s => Err(ci2::Error::from(format!(
                "unexpected TriggerMode enum string: {}",
                s
            ))),
        }
    }
    fn set_trigger_mode(&mut self, value: TriggerMode) -> ci2::Result<()> {
        let sval = match value {
            ci2::TriggerMode::Off => "Off",
            ci2::TriggerMode::On => "On",
        };
        self.inner
            .lock()
            .unwrap()
            .node_map()
            .map_pylon_err()?
            .enum_node("TriggerMode")
            .map_pylon_err()?
            .set_value_pfs(&mut self.pfs_cache.lock().unwrap(), sval)
            .map_pylon_err()
    }

    // Settings: AcquisitionFrameRateEnable ----------------------------
    fn acquisition_frame_rate_enable(&self) -> ci2::Result<bool> {
        self.inner
            .lock()
            .unwrap()
            .node_map()
            .map_pylon_err()?
            .boolean_node("AcquisitionFrameRateEnable")
            .map_pylon_err()?
            .value()
            .map_pylon_err()
    }
    fn set_acquisition_frame_rate_enable(&mut self, value: bool) -> ci2::Result<()> {
        self.inner
            .lock()
            .unwrap()
            .node_map()
            .map_pylon_err()?
            .boolean_node("AcquisitionFrameRateEnable")
            .map_pylon_err()?
            .set_value_pfs(&mut self.pfs_cache.lock().unwrap(), value)
            .map_pylon_err()
    }

    // Settings: AcquisitionFrameRate ----------------------------
    fn acquisition_frame_rate(&self) -> ci2::Result<f64> {
        let camera = self.inner.lock().unwrap();
        let node = camera
            .node_map()
            .map_pylon_err()?
            .float_node(self.acquisition_frame_rate_name())
            .map_pylon_err()?;
        node.value().map_pylon_err()
    }
    fn acquisition_frame_rate_range(&self) -> ci2::Result<(f64, f64)> {
        let camera = self.inner.lock().unwrap();
        let node = camera
            .node_map()
            .map_pylon_err()?
            .float_node(self.acquisition_frame_rate_name())
            .map_pylon_err()?;
        Ok((node.min().map_pylon_err()?, node.max().map_pylon_err()?))
    }
    fn set_acquisition_frame_rate(&mut self, value: f64) -> ci2::Result<()> {
        self.inner
            .lock()
            .unwrap()
            .node_map()
            .map_pylon_err()?
            .float_node(self.acquisition_frame_rate_name())
            .map_pylon_err()?
            .set_value_pfs(&mut self.pfs_cache.lock().unwrap(), value)
            .map_pylon_err()
    }

    // Settings: TriggerSelector ----------------------------
    fn trigger_selector(&self) -> ci2::Result<TriggerSelector> {
        let camera = self.inner.lock().unwrap();
        let val = camera
            .node_map()
            .map_pylon_err()?
            .enum_node("TriggerSelector")
            .map_pylon_err()?
            .value()
            .map_pylon_err()?;
        match val.as_ref() {
            "AcquisitionStart" => Ok(ci2::TriggerSelector::AcquisitionStart),
            "FrameBurstStart" => Ok(ci2::TriggerSelector::FrameBurstStart),
            "FrameStart" => Ok(ci2::TriggerSelector::FrameStart),
            "ExposureActive" => Ok(ci2::TriggerSelector::ExposureActive),
            s => Err(ci2::Error::from(format!(
                "unexpected TriggerSelector enum string: {}",
                s
            ))),
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
        let camera = self.inner.lock().unwrap();
        camera
            .node_map()
            .map_pylon_err()?
            .enum_node("TriggerSelector")
            .map_pylon_err()?
            .set_value_pfs(&mut self.pfs_cache.lock().unwrap(), sval)
            .map_pylon_err()
    }

    // Settings: AcquisitionMode ----------------------------
    fn acquisition_mode(&self) -> ci2::Result<AcquisitionMode> {
        let mode = self
            .inner
            .lock()
            .unwrap()
            .node_map()
            .map_pylon_err()?
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
            .unwrap()
            .node_map()
            .map_pylon_err()?
            .enum_node("AcquisitionMode")
            .map_pylon_err()?
            .set_value_pfs(&mut self.pfs_cache.lock().unwrap(), sval)
            .map_pylon_err()
    }

    // Acquisition ----------------------------
    fn acquisition_start(&mut self) -> ci2::Result<()> {
        self.inner
            .lock()
            .unwrap()
            .start_grabbing(&pylon_cxx::GrabOptions::default())
            .map_pylon_err()?;
        Ok(())
    }
    fn acquisition_stop(&mut self) -> ci2::Result<()> {
        self.inner.lock().unwrap().stop_grabbing().map_pylon_err()?;
        Ok(())
    }

    /// synchronous (blocking) frame acquisition
    fn next_frame(&mut self) -> ci2::Result<DynamicFrameWithInfo> {
        let pixel_format = self.pixel_format()?;

        let mut gr = self.grab_result.lock().unwrap();
        let cam = self.inner.lock().unwrap();

        // Wait for an image and then retrieve it. A timeout of 99999 ms is used.
        cam.retrieve_result(99999, &mut gr, pylon_cxx::TimeoutHandling::ThrowException)
            .map_pylon_err()?;

        let now = chrono::Utc::now(); // earliest possible timestamp

        // Image grabbed successfully?
        if gr.grab_succeeded().map_pylon_err()? {
            let buffer = gr.buffer().map_pylon_err()?;
            let block_id = gr.block_id().map_pylon_err()?;

            let fno: usize = self.store_fno;
            self.store_fno += 1;

            let width = gr.width().map_pylon_err()?;
            let height = gr.height().map_pylon_err()?;
            let stride = gr.stride().map_pylon_err()?;
            let image_data = buffer.to_vec();
            let device_timestamp = gr.time_stamp().map_pylon_err()?;

            let backend_data = if !(device_timestamp == 0 && block_id == u64::MAX) {
                Some(Box::new(ci2_pylon_types::PylonExtra {
                    block_id,
                    device_timestamp,
                }) as Box<dyn ci2::BackendData>)
            } else {
                // This happens when the Basler driver emulates a camera. Don't
                // propagate these bad values further.
                None
            };

            let host_timing = HostTimingInfo { fno, datetime: now };
            let image =
                DynamicFrameOwned::from_buf(width, height, stride, image_data, pixel_format)
                    .unwrap();

            Ok(DynamicFrameWithInfo {
                image,
                host_timing,
                backend_data,
            })

        // println!("Gray value of first pixel: {}\n", image_buffer[0]);
        } else {
            self.store_fno += 1;

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
        s => Err(ci2::Error::from(format!(
            "unexpected AutoMode enum string: {}",
            s
        ))),
    }
}

fn mode_to_str(value: AutoMode) -> &'static str {
    match value {
        ci2::AutoMode::Off => "Off",
        ci2::AutoMode::Once => "Once",
        ci2::AutoMode::Continuous => "Continuous",
    }
}
