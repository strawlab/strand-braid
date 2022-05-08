extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate ci2_types;
extern crate enum_iter;
extern crate rust_cam_bui_types;

use enum_iter::EnumIter;
use rust_cam_bui_types::ClockModel;

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum RecordingFrameRate {
    Fps1,
    Fps2,
    Fps5,
    Fps10,
    Fps20,
    Fps25,
    Fps30,
    Fps100,
    Unlimited,
}

impl RecordingFrameRate {
    pub fn interval(&self) -> std::time::Duration {
        use std::time::Duration;
        use RecordingFrameRate::*;
        match self {
            Fps1 => Duration::from_millis(1000),
            Fps2 => Duration::from_millis(500),
            Fps5 => Duration::from_millis(200),
            Fps10 => Duration::from_millis(100),
            Fps20 => Duration::from_millis(50),
            Fps25 => Duration::from_millis(40),
            Fps30 => Duration::from_nanos(33333333),
            Fps100 => Duration::from_millis(10),
            Unlimited => Duration::from_millis(0),
        }
    }

    pub fn as_numerator_denominator(&self) -> Option<(u32, u32)> {
        use RecordingFrameRate::*;
        Some(match self {
            Fps1 => (1, 1),
            Fps2 => (2, 1),
            Fps5 => (5, 1),
            Fps10 => (10, 1),
            Fps20 => (20, 1),
            Fps25 => (25, 1),
            Fps30 => (30, 1),
            Fps100 => (100, 1),
            Unlimited => {
                return None;
            }
        })
    }
}

impl Default for RecordingFrameRate {
    fn default() -> RecordingFrameRate {
        RecordingFrameRate::Fps20
    }
}

// use Debug to impl Display
impl std::fmt::Display for RecordingFrameRate {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::result::Result<(), std::fmt::Error> {
        use RecordingFrameRate::*;
        let s = match self {
            Fps1 => "1 fps",
            Fps2 => "2 fps",
            Fps5 => "5 fps",
            Fps10 => "10 fps",
            Fps20 => "20 fps",
            Fps25 => "25 fps",
            Fps30 => "30 fps",
            Fps100 => "100 fps",
            Unlimited => "unlimited",
        };
        write!(fmt, "{}", s)
    }
}

impl EnumIter for RecordingFrameRate {
    fn variants() -> &'static [Self] {
        use RecordingFrameRate::*;
        &[
            Fps1, Fps2, Fps5, Fps10, Fps20, Fps25, Fps30, Fps100, Unlimited,
        ]
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum MkvCodec {
    Uncompressed,
    VP8(VP8Options),
    VP9(VP9Options),
    H264(H264Options),
}

impl Default for MkvCodec {
    fn default() -> MkvCodec {
        MkvCodec::VP8(VP8Options::default())
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct VP8Options {
    pub bitrate: u32,
}

impl Default for VP8Options {
    fn default() -> Self {
        Self { bitrate: 1000 }
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct VP9Options {
    pub bitrate: u32,
}

impl Default for VP9Options {
    fn default() -> Self {
        Self { bitrate: 1000 }
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct H264Options {
    /// The bitrate (used in association with the framerate).
    pub bitrate: u32,
    /// The device number of the CUDA device to use.
    pub cuda_device: i32,
}

impl Default for H264Options {
    fn default() -> Self {
        Self {
            bitrate: 1000,
            cuda_device: 0,
        }
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct MkvRecordingConfig {
    pub codec: MkvCodec,
    pub max_framerate: RecordingFrameRate,
    pub writing_application: Option<String>,
    pub save_creation_time: bool,
    pub title: Option<String>,
    /// Automatically trim image width and height by removing right pixels if
    /// needed by encoder.
    pub do_trim_size: bool,
}

impl Default for MkvRecordingConfig {
    fn default() -> Self {
        Self {
            codec: MkvCodec::default(),
            max_framerate: RecordingFrameRate::Fps25,
            writing_application: None,
            save_creation_time: true,
            title: None,
            do_trim_size: true,
        }
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum CsvSaveConfig {
    /// Do not save CSV
    NotSaving,
    /// Save CSV with this as a framerate limit
    Saving(Option<f32>),
}

// April tags

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum TagFamily {
    Family36h11,
    FamilyStandard41h12,
    Family16h5,
    Family25h9,
    FamilyCircle21h7,
    FamilyCircle49h12,
    FamilyCustom48h12,
    FamilyStandard52h13,
}

impl Default for TagFamily {
    fn default() -> Self {
        TagFamily::Family36h11
    }
}

impl EnumIter for TagFamily {
    fn variants() -> &'static [Self] {
        use TagFamily::*;
        &[
            Family36h11,
            FamilyStandard41h12,
            Family16h5,
            Family25h9,
            FamilyCircle21h7,
            FamilyCircle49h12,
            FamilyCustom48h12,
            FamilyStandard52h13,
        ]
    }
}

impl std::fmt::Display for TagFamily {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        use TagFamily::*;
        let fam = match self {
            Family36h11 => "36h11".to_string(),
            FamilyStandard41h12 => "standard-41h12".to_string(),
            Family16h5 => "16h5".to_string(),
            Family25h9 => "25h9".to_string(),
            FamilyCircle21h7 => "circle-21h7".to_string(),
            FamilyCircle49h12 => "circle-49h12".to_string(),
            FamilyCustom48h12 => "custom-48h12".to_string(),
            FamilyStandard52h13 => "standard-52h13".to_string(),
        };

        write!(f, "{}", fam)
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum CamArg {
    /// Ignore future frame processing errors for this duration of seconds from current time.
    ///
    /// If seconds are not given, ignore forever.
    SetIngoreFutureFrameProcessingErrors(Option<i64>),

    SetExposureTime(f64),
    SetExposureAuto(ci2_types::AutoMode),
    SetFrameRateLimitEnabled(bool),
    SetFrameRateLimit(f64),
    SetGain(f64),
    SetGainAuto(ci2_types::AutoMode),
    SetRecordingFps(RecordingFrameRate),
    SetMkvRecordingConfig(MkvRecordingConfig),
    SetMkvRecordingFps(RecordingFrameRate),
    SetIsRecordingMkv(bool),
    SetIsRecordingFmf(bool),
    /// used only with image-tracker crate
    SetIsRecordingUfmf(bool),
    /// used only with image-tracker crate
    SetIsDoingObjDetection(bool),
    /// used only with image-tracker crate
    SetIsSavingObjDetectionCsv(CsvSaveConfig),
    /// used only with image-tracker crate
    SetObjDetectionConfig(String),
    CamArgSetKalmanTrackingConfig(String),
    CamArgSetLedProgramConfig(String),
    SetFrameOffset(u64),
    SetClockModel(Option<ClockModel>),
    SetFormatStr(String),
    ToggleCheckerboardDetection(bool),
    ToggleCheckerboardDebug(bool),
    SetCheckerboardWidth(u32),
    SetCheckerboardHeight(u32),
    ClearCheckerboards,
    PerformCheckerboardCalibration,
    DoQuit,
    PostTrigger(MkvRecordingConfig),
    SetPostTriggerBufferSize(usize),
    ToggleAprilTagFamily(TagFamily),
    ToggleAprilTagDetection(bool),
    SetIsRecordingAprilTagCsv(bool),
    ToggleImOpsDetection(bool),
    SetImOpsDestination(std::net::SocketAddr),
    SetImOpsSource(std::net::IpAddr),
    SetImOpsCenterX(u32),
    SetImOpsCenterY(u32),
    SetImOpsThreshold(u8),
}
