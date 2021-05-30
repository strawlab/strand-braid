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
    #[error("CI2Error({0})")]
    CI2Error(String),
    #[error("feature not present")]
    FeatureNotPresent,
    #[error("BackendError({0})")]
    BackendError(#[from] anyhow::Error),

    #[error("{0}")]
    IoError(#[from] std::io::Error),
    #[error("{0}")]
    Utf8Error(#[from] std::str::Utf8Error),
    #[error("{0}")]
    TryFromIntError(#[from] std::num::TryFromIntError),
}

fn _test_error_is_send() {
    // Compile-time test to ensure Error implements Send trait.
    fn implements<T: Send>() {}
    implements::<Error>();
}

impl<'a> From<&'a str> for Error {
    fn from(orig: &'a str) -> Error {
        Error::CI2Error(orig.to_string())
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
// FrameROI

/// A region of interest within a sensor.
#[derive(Debug, Clone)]
pub struct FrameROI {
    /// the column offset of the current frame relative to sensor
    pub xmin: u32,
    /// the row offset of the current frame relative to sensor
    pub ymin: u32,
    /// number of columns in the image
    pub width: u32,
    /// number of rows in the image
    pub height: u32,
}

// ---------------------------
// Camera

pub trait Camera: CameraInfo {
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

    // Acquisition ----------------------------
    fn acquisition_start(&mut self) -> Result<()>;
    fn acquisition_stop(&mut self) -> Result<()>;

    /// synchronous (blocking) frame acquisition
    fn next_frame(&mut self) -> Result<DynamicFrame>;
}
