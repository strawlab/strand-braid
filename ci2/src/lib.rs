#![cfg_attr(
    feature = "backtrace",
    feature(error_generic_member_access)
)]

#[cfg(feature = "backtrace")]
use std::backtrace::Backtrace;

use basic_frame::DynamicFrame;
pub use ci2_types::{AcquisitionMode, AutoMode, TriggerMode, TriggerSelector};
use machine_vision_formats as formats;

// TODO add binning support

// ---------------------------
// errors

pub type Result<M> = std::result::Result<M, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("SingleFrameError({0})")]
    SingleFrameError(String),
    #[error("Timeout")]
    Timeout,
    #[error("CI2Error({msg})")]
    CI2Error {
        msg: String,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("feature not present")]
    FeatureNotPresent(#[cfg(feature = "backtrace")] Backtrace),
    #[error("BackendError({0})")]
    BackendError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        anyhow::Error,
    ),
    #[error("io error: {source}")]
    IoError {
        #[from]
        source: std::io::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("utf8 error: {source}")]
    Utf8Error {
        #[from]
        source: std::str::Utf8Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("try from int error: {source}")]
    TryFromIntError {
        #[from]
        source: std::num::TryFromIntError,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
}

fn _test_error_is_send() {
    // Compile-time test to ensure Error implements Send trait.
    fn implements<T: Send>() {}
    implements::<Error>();
}

impl<'a> From<&'a str> for Error {
    fn from(orig: &'a str) -> Error {
        Error::CI2Error {
            msg: orig.to_string(),
            #[cfg(feature = "backtrace")]
            backtrace: Backtrace::capture(),
        }
    }
}

impl From<String> for Error {
    fn from(msg: String) -> Error {
        Error::CI2Error {
            msg,
            #[cfg(feature = "backtrace")]
            backtrace: Backtrace::capture(),
        }
    }
}

// ---------------------------
// CameraModule

/// A module for opening cameras (e.g. pylon).
pub trait CameraModule {
    type CameraType: Camera;

    // TODO: have full_name and friendly_name?
    fn name(&self) -> &str;
    fn camera_infos(&self) -> Result<Vec<Box<dyn CameraInfo>>>;
    fn camera(&mut self, name: &str) -> Result<Self::CameraType>;

    /// The file extension for node map settings.
    ///
    /// The strings used in [Camera::node_map_load] and [Camera::node_map_save]
    /// would typically be stored in files with this extension.
    fn settings_file_extension(&self) -> &str;

    fn frame_info_extractor(&self) -> &'static dyn ExtractFrameInfo;
}

pub struct FrameInfo {
    pub device_timestamp: Option<std::num::NonZeroU64>,
    pub frame_id: Option<std::num::NonZeroU64>,
    pub host_framenumber: usize,
    pub host_timestamp: chrono::DateTime<chrono::Utc>,
}

pub trait ExtractFrameInfo: Sync + Send {
    fn extract_frame_info(&self, _frame: &DynamicFrame) -> FrameInfo;
}

// ---------------------------
// CameraInfo

pub trait CameraInfo {
    fn name(&self) -> &str;
    fn serial(&self) -> &str;
    fn model(&self) -> &str;
    fn vendor(&self) -> &str;
}

// ---------------------------
// Camera

pub trait Camera: CameraInfo {
    // ----- start: weakly typed but easier to implement API -----

    // fn feature_access_query(&self, name: &str) -> Result<AccessQueryResult>;
    fn feature_enum_set(&self, name: &str, value: &str) -> Result<()>;
    fn feature_float(&self, name: &str) -> Result<f64>;

    // ----- end: weakly typed but easier to implement API -----

    /// Load camera settings from an implementation-dependent settings string.
    ///
    /// This would typically be read from a file with extension given by
    /// [CameraModule::settings_file_extension].
    fn node_map_load(&self, settings: &str) -> Result<()>;
    /// Read camera settings to an implementation-dependent settings string.
    ///
    /// This would typically be saved to a file with extension given by
    /// [CameraModule::settings_file_extension].
    fn node_map_save(&self) -> Result<String>;

    /// Return the sensor width in pixels
    fn width(&self) -> Result<u32>;
    /// Return the sensor height in pixels
    fn height(&self) -> Result<u32>;

    // TODO: add this
    // fn stride(&self) -> Result<u32>;

    // Settings: PixFmt ----------------------------
    fn pixel_format(&self) -> Result<formats::PixFmt>;
    fn possible_pixel_formats(&self) -> Result<Vec<formats::PixFmt>>;
    fn set_pixel_format(&mut self, pixel_format: formats::PixFmt) -> Result<()>;

    // Settings: Exposure Time ----------------------------
    /// value given in microseconds
    fn exposure_time(&self) -> Result<f64>;
    /// value given in microseconds
    fn exposure_time_range(&self) -> Result<(f64, f64)>;
    /// value given in microseconds
    fn set_exposure_time(&mut self, _: f64) -> Result<()>;

    // Settings: Exposure Time Auto Mode ----------------------------
    fn exposure_auto(&self) -> Result<AutoMode>;
    fn set_exposure_auto(&mut self, _: AutoMode) -> Result<()>;

    // Settings: Gain ----------------------------
    /// value given in dB
    fn gain(&self) -> Result<f64>;
    /// value given in dB
    fn gain_range(&self) -> Result<(f64, f64)>;
    /// value given in dB
    fn set_gain(&mut self, _: f64) -> Result<()>;

    // Settings: Gain Auto Mode ----------------------------
    fn gain_auto(&self) -> Result<AutoMode>;
    fn set_gain_auto(&mut self, _: AutoMode) -> Result<()>;

    // Settings: TriggerMode ----------------------------
    fn trigger_mode(&self) -> Result<TriggerMode>;
    fn set_trigger_mode(&mut self, _: TriggerMode) -> Result<()>;

    // Settings: AcquisitionFrameRateEnable ----------------------------
    fn acquisition_frame_rate_enable(&self) -> Result<bool>;
    fn set_acquisition_frame_rate_enable(&mut self, value: bool) -> Result<()>;

    // Settings: AcquisitionFrameRate ----------------------------
    fn acquisition_frame_rate(&self) -> Result<f64>;
    fn acquisition_frame_rate_range(&self) -> Result<(f64, f64)>;
    fn set_acquisition_frame_rate(&mut self, value: f64) -> Result<()>;

    // Settings: TriggerSelector ----------------------------
    fn trigger_selector(&self) -> Result<TriggerSelector>;
    fn set_trigger_selector(&mut self, _: TriggerSelector) -> Result<()>;

    // Settings: AcquisitionMode ----------------------------
    fn acquisition_mode(&self) -> Result<AcquisitionMode>;
    fn set_acquisition_mode(&mut self, _: AcquisitionMode) -> Result<()>;

    // Set external triggering ------------------------------
    /// Set the camera to use external triggering using default parameters.
    ///
    /// The default parameters may vary by camera backend will ideally use
    /// a hardware trigger to trigger the start of each frame.
    fn start_default_external_triggering(&mut self) -> Result<()> {
        // This is the generic default implementation which may be overriden by
        // implementors.

        // The trigger selector must be set before the trigger mode.
        self.set_trigger_selector(TriggerSelector::FrameStart)?;
        self.set_trigger_mode(TriggerMode::On)
    }

    fn set_software_frame_rate_limit(&mut self, fps_limit: f64) -> Result<()> {
        // This is the generic default implementation which may be overriden by
        // implementors.
        self.set_acquisition_frame_rate_enable(true)?;
        self.set_acquisition_frame_rate(fps_limit)
    }

    // Acquisition ----------------------------
    fn acquisition_start(&mut self) -> Result<()>;
    fn acquisition_stop(&mut self) -> Result<()>;

    /// synchronous (blocking) frame acquisition
    // TODO: enable the ability to enqueue memory locations for new frame data.
    // This way pre-allocated can be stored to by the library and copies of the
    // data do not have to be made.
    // TODO: specify timeout
    fn next_frame(&mut self) -> Result<DynamicFrame>;
}

// #[derive(Debug, Clone, PartialEq)]
// pub struct AccessQueryResult {
//     pub is_readable: bool,
//     pub is_writeable: bool,
// }
