use strand_dynamic_frame::DynamicFrame;
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
    CI2Error { msg: String },
    #[error("feature not present")]
    FeatureNotPresent(),
    #[error("BackendError({0})")]
    BackendError(#[from] anyhow::Error),
    #[error("io error: {source}")]
    IoError {
        #[from]
        source: std::io::Error,
    },
    #[error("utf8 error: {source}")]
    Utf8Error {
        #[from]
        source: std::str::Utf8Error,
    },
    #[error("try from int error: {source}")]
    TryFromIntError {
        #[from]
        source: std::num::TryFromIntError,
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
        }
    }
}

impl From<String> for Error {
    fn from(msg: String) -> Error {
        Error::CI2Error { msg }
    }
}

// ---------------------------
// CameraModule

/// A module for opening cameras (e.g. pylon).
pub trait CameraModule: Send {
    type CameraType: Camera;
    type Guard;

    // TODO: have full_name and friendly_name?
    fn name(&self) -> &str;
    fn camera_infos(&self) -> Result<Vec<Box<dyn CameraInfo>>>;
    fn camera(&mut self, name: &str) -> Result<Self::CameraType>;

    /// The file extension for node map settings.
    ///
    /// The strings used in [Camera::node_map_load] and [Camera::node_map_save]
    /// would typically be stored in files with this extension.
    fn settings_file_extension(&self) -> &str;
}

#[derive(Clone)]
pub struct DynamicFrameWithInfo {
    /// The image frame acquired from the camera.
    pub image: DynamicFrame,
    /// Frame timing information acquired by the host.
    pub host_timing: HostTimingInfo,
    /// Backend-specific information about the frame.
    ///
    /// This may contain camera backend-specific timing information, which is
    /// presumably better than that available using host-only information.
    /// However, this is not guaranteed to be present.
    pub backend_data: Option<Box<dyn BackendData>>,
}

pub trait BackendData: dyn_clone::DynClone + Send + AsAny {}

// see https://users.rust-lang.org/t/calling-any-downcast-ref-requires-static/52071
pub trait AsAny {
    fn as_any(&self) -> &dyn std::any::Any;
}
impl<T: std::any::Any> AsAny for T {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// implement Clone for BackendData
dyn_clone::clone_trait_object!(BackendData);

impl DynamicFrameWithInfo {
    pub fn width(&self) -> u32 {
        self.image.width()
    }
    pub fn height(&self) -> u32 {
        self.image.height()
    }
    pub fn pixel_format(&self) -> formats::PixFmt {
        self.image.pixel_format()
    }
}

/// Timing information acquired on the host computer.
///
/// This can be considered the "least common denominator" of frame timing
/// information, as it will always be present but is not necessarily as accurate
/// as desired.
#[derive(Debug, Clone)]
pub struct HostTimingInfo {
    /// The frame number as counted by the host.
    ///
    /// This can deviate from the "real" frame number if the frames were
    /// dropped, as might happen if the computer was busy with a different task.
    pub fno: usize,
    /// The timestamp of the frame when it was acquired by the host.
    ///
    /// This will be at least slightly delayed from the "real" frame timestamp
    /// by transmission delays. Furthermore, if the computer was busy during
    /// acquisition there may be additional, highly variable, delays.
    pub datetime: chrono::DateTime<chrono::Utc>,
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

pub trait Camera: CameraInfo + Send {
    // ----- start: weakly typed but easier to implement API -----

    // fn feature_access_query(&self, name: &str) -> Result<AccessQueryResult>;
    fn command_execute(&self, name: &str, verify: bool) -> Result<()>;
    fn feature_bool(&self, name: &str) -> Result<bool>;
    fn feature_bool_set(&self, name: &str, value: bool) -> Result<()>;
    fn feature_enum(&self, name: &str) -> Result<String>;
    fn feature_enum_set(&self, name: &str, value: &str) -> Result<()>;
    fn feature_float(&self, name: &str) -> Result<f64>;
    fn feature_float_set(&self, name: &str, value: f64) -> Result<()>;
    fn feature_int(&self, name: &str) -> Result<i64>;
    fn feature_int_set(&self, name: &str, value: i64) -> Result<()>;

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
    fn next_frame(&mut self) -> Result<DynamicFrameWithInfo>;
}
