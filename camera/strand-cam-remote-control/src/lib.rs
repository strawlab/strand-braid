// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Types for [Strand Camera](https://strawlab.org/strand-cam) remote control and configuration

extern crate serde;
extern crate strand_cam_bui_types;
extern crate strand_cam_enum_iter;
extern crate strand_cam_types;

use serde::{Deserialize, Serialize};
use strand_cam_bui_types::ClockModel;
use strand_cam_enum_iter::EnumIter;

/// Frame rate options for video recording.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, Default)]
pub enum RecordingFrameRate {
    /// 1 frame per second
    Fps1,
    /// 2 frames per second
    Fps2,
    /// 5 frames per second
    Fps5,
    /// 10 frames per second
    Fps10,
    /// 20 frames per second
    Fps20,
    /// 25 frames per second
    Fps25,
    /// 30 frames per second
    Fps30,
    /// 40 frames per second
    Fps40,
    /// 50 frames per second
    Fps50,
    /// 60 frames per second
    Fps60,
    /// 100 frames per second
    Fps100,
    /// No frame rate limit
    #[default]
    Unlimited,
}

impl RecordingFrameRate {
    /// Returns the duration between frames for this frame rate.
    pub fn interval(&self) -> std::time::Duration {
        use RecordingFrameRate::*;
        use std::time::Duration;
        match self {
            Fps1 => Duration::from_millis(1000),
            Fps2 => Duration::from_millis(500),
            Fps5 => Duration::from_millis(200),
            Fps10 => Duration::from_millis(100),
            Fps20 => Duration::from_millis(50),
            Fps25 => Duration::from_millis(40),
            Fps30 => Duration::from_nanos(33333333),
            Fps40 => Duration::from_millis(25),
            Fps50 => Duration::from_millis(20),
            Fps60 => Duration::from_nanos(16666667),
            Fps100 => Duration::from_millis(10),
            Unlimited => Duration::from_millis(0),
        }
    }

    /// Returns frame rate as numerator/denominator, or None for unlimited.
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
            Fps40 => (40, 1),
            Fps50 => (50, 1),
            Fps60 => (60, 1),
            Fps100 => (100, 1),
            Unlimited => {
                return None;
            }
        })
    }
}

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
            Fps40 => "40 fps",
            Fps50 => "50 fps",
            Fps60 => "60 fps",
            Fps100 => "100 fps",
            Unlimited => "unlimited",
        };
        write!(fmt, "{s}")
    }
}

impl EnumIter for RecordingFrameRate {
    fn variants() -> Vec<Self> {
        use RecordingFrameRate::*;
        vec![
            Fps1, Fps2, Fps5, Fps10, Fps20, Fps25, Fps30, Fps40, Fps50, Fps60, Fps100, Unlimited,
        ]
    }
}

/// H.264 codec options for MP4 encoding.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum Mp4Codec {
    /// Encode data with Nvidia's NVENC.
    H264NvEnc(NvidiaH264Options),
    /// Encode data with OpenH264.
    H264OpenH264(OpenH264Options),
    /// Encode data with LessAVC.
    H264LessAvc,
    /// Data is already encoded as a raw H264 stream.
    H264RawStream,
}

/// Options for OpenH264 encoder.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Default)]
pub struct OpenH264Options {
    /// Enable OpenH264 debug messages
    pub debug: bool,
    /// Encoding preset configuration
    pub preset: OpenH264Preset,
}

impl OpenH264Options {
    /// Returns debug flag.
    pub fn debug(&self) -> bool {
        self.debug
    }
    /// Returns whether frame skipping should be enabled.
    pub fn enable_skip_frame(&self) -> bool {
        match self.preset {
            OpenH264Preset::AllFrames => false,
            OpenH264Preset::SkipFramesBitrate(_) => true,
        }
    }
    /// Returns the rate control mode to use.
    pub fn rate_control_mode(&self) -> OpenH264RateControlMode {
        match self.preset {
            OpenH264Preset::AllFrames => OpenH264RateControlMode::Off,
            OpenH264Preset::SkipFramesBitrate(_) => OpenH264RateControlMode::Bitrate,
        }
    }
    /// Returns the target bitrate in bits per second.
    pub fn bitrate_bps(&self) -> u32 {
        match self.preset {
            OpenH264Preset::AllFrames => 0,
            OpenH264Preset::SkipFramesBitrate(bitrate) => bitrate,
        }
    }
}

/// Encoding presets for OpenH264.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub enum OpenH264Preset {
    /// Encode all frames without skipping
    AllFrames,
    /// Skip frames to achieve target bitrate
    SkipFramesBitrate(u32),
}

impl Default for OpenH264Preset {
    fn default() -> Self {
        Self::SkipFramesBitrate(5000)
    }
}

/// Rate control modes for OpenH264 encoder.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, Copy)]
pub enum OpenH264RateControlMode {
    /// Quality mode.
    Quality,
    /// Bitrate mode.
    Bitrate,
    /// No bitrate control, only using buffer status, adjust the video quality.
    Bufferbased,
    /// Rate control based timestamp.
    Timestamp,
    /// Rate control off mode.
    Off,
}

/// Options for NVIDIA H.264 encoder.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, Default)]
pub struct NvidiaH264Options {
    /// The bitrate (used in association with the framerate).
    pub bitrate: Option<u32>,
    /// The device number of the CUDA device to use.
    pub cuda_device: i32,
}

/// Configuration for MP4 recording
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Mp4RecordingConfig {
    pub codec: Mp4Codec,
    /// Limits the recording to a maximum frame rate.
    pub max_framerate: RecordingFrameRate,
    pub h264_metadata: Option<H264Metadata>,
}

/// Configuration for an ffmpeg-based recording
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Default)]
pub struct FfmpegRecordingConfig {
    pub codec_args: FfmpegCodecArgs,
    /// Limits the recording to a maximum frame rate.
    pub max_framerate: RecordingFrameRate,
    pub h264_metadata: Option<H264Metadata>,
}

/// Specify recording method and configuration
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum RecordingConfig {
    /// Record using MP4 writer
    Mp4(Mp4RecordingConfig),
    /// Record via y4m pipe to ffmpeg
    Ffmpeg(FfmpegRecordingConfig),
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self::Ffmpeg(Default::default())
    }
}

impl RecordingConfig {
    /// Returns the maximum frame rate for this recording configuration.
    pub fn max_framerate(&self) -> &RecordingFrameRate {
        use RecordingConfig::*;
        match self {
            Mp4(c) => &c.max_framerate,
            Ffmpeg(c) => &c.max_framerate,
        }
    }
}

/// Universal identifier for our H264 metadata.
///
/// Generated with `uuid -v3 ns:URL https://strawlab.org/h264-metadata/`
pub const H264_METADATA_UUID: [u8; 16] = [
    // 0ba99cc7-f607-3851-b35e-8c7d8c04da0a
    0x0B, 0xA9, 0x9C, 0xC7, 0xF6, 0x07, 0x08, 0x51, 0x33, 0x5E, 0x8C, 0x7D, 0x8C, 0x04, 0xDA, 0x0A,
];
/// Version for our H264 metadata.
pub const H264_METADATA_VERSION: &str = "https://strawlab.org/h264-metadata/v1/";

/// Metadata to embed in H.264 streams.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct H264Metadata {
    /// Version of this structure
    ///
    /// Should be equal to H264_METADATA_VERSION.
    /// This field must always be serialized first.
    pub version: String,

    /// Application name that created the stream
    pub writing_app: String,

    /// Stream creation timestamp
    pub creation_time: chrono::DateTime<chrono::FixedOffset>,

    /// Optional camera name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera_name: Option<String>,

    /// Optional gamma correction value
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gamma: Option<f32>,
}

impl H264Metadata {
    pub fn new(writing_app: &str, creation_time: chrono::DateTime<chrono::FixedOffset>) -> Self {
        Self {
            version: H264_METADATA_VERSION.to_string(),
            writing_app: writing_app.to_string(),
            creation_time,
            camera_name: None,
            gamma: None,
        }
    }
}

/// CSV recording configuration.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum CsvSaveConfig {
    /// Do not save CSV
    NotSaving,
    /// Save CSV with optional framerate limit
    Saving(Option<f32>),
}

/// AprilTag family types for detection.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, Default)]
pub enum TagFamily {
    /// 36h11 tag family (default)
    #[default]
    Family36h11,
    /// Standard 41h12 tag family
    FamilyStandard41h12,
    /// 16h5 tag family
    Family16h5,
    /// 25h9 tag family
    Family25h9,
    /// Circle 21h7 tag family
    FamilyCircle21h7,
    /// Circle 49h12 tag family
    FamilyCircle49h12,
    /// Custom 48h12 tag family
    FamilyCustom48h12,
    /// Standard 52h13 tag family
    FamilyStandard52h13,
}

impl EnumIter for TagFamily {
    fn variants() -> Vec<Self> {
        use TagFamily::*;
        vec![
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

        write!(f, "{fam}")
    }
}

/// Bitrate selection options for video encoding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum BitrateSelection {
    /// Bitrate 500
    Bitrate500,
    /// Bitrate 1000 (default)
    #[default]
    Bitrate1000,
    /// Bitrate 2000
    Bitrate2000,
    /// Bitrate 3000
    Bitrate3000,
    /// Bitrate 4000
    Bitrate4000,
    /// Bitrate 5000
    Bitrate5000,
    /// Bitrate 10000
    Bitrate10000,
    /// No bitrate limit
    BitrateUnlimited,
}

impl std::fmt::Display for BitrateSelection {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        use BitrateSelection::*;
        match self {
            Bitrate500 => write!(f, "500"),
            Bitrate1000 => write!(f, "1000"),
            Bitrate2000 => write!(f, "2000"),
            Bitrate3000 => write!(f, "3000"),
            Bitrate4000 => write!(f, "4000"),
            Bitrate5000 => write!(f, "5000"),
            Bitrate10000 => write!(f, "10000"),
            BitrateUnlimited => write!(f, "Unlimited"),
        }
    }
}

impl strand_cam_enum_iter::EnumIter for BitrateSelection {
    fn variants() -> Vec<Self> {
        vec![
            BitrateSelection::Bitrate500,
            BitrateSelection::Bitrate1000,
            BitrateSelection::Bitrate2000,
            BitrateSelection::Bitrate3000,
            BitrateSelection::Bitrate4000,
            BitrateSelection::Bitrate5000,
            BitrateSelection::Bitrate10000,
            BitrateSelection::BitrateUnlimited,
        ]
    }
}

/// Type alias for optional ffmpeg codec argument lists.
type FfmpegCodecArgList = Option<Vec<(String, String)>>;

/// The default output pixel format. 4:2:0 chroma subsampling is the most widely
/// decodable choice; in particular the built-in OpenH264 decoder only handles
/// 4:2:0, so anything we might want to decode later must be encoded this way.
/// Without forcing this, encoders like libx264 pick a format matching the input
/// (e.g. `yuv444p` for RGB input), which OpenH264 cannot decode.
const DEFAULT_OUTPUT_PIXFMT: &str = "yuv420p";

fn default_output_pixfmt() -> Option<String> {
    Some(DEFAULT_OUTPUT_PIXFMT.to_string())
}

/// Codec-specific arguments for ffmpeg.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct FfmpegCodecArgs {
    /// Device-specific arguments
    pub device_args: FfmpegCodecArgList,
    /// Arguments before codec specification
    pub pre_codec_args: FfmpegCodecArgList,
    /// Codec name
    pub codec: Option<String>,
    /// Arguments after codec specification
    pub post_codec_args: FfmpegCodecArgList,
    /// Output pixel format passed to ffmpeg as `-pix_fmt`. Defaults to
    /// [`DEFAULT_OUTPUT_PIXFMT`] (`yuv420p`) so encoded video is decodable by
    /// OpenH264. Set to `None` to let ffmpeg (or a `-vf`/`-pix_fmt` in the other
    /// arg lists) decide, e.g. for hardware encoders whose filter chain already
    /// fixes the format.
    #[serde(default = "default_output_pixfmt")]
    pub pixfmt: Option<String>,
    /// Maximum number of B-frames passed to ffmpeg as `-bf`. `None`, the
    /// default, lets the encoder (or a `-bf` in the other arg lists) decide.
    pub max_bframes: Option<u32>,
}

impl Default for FfmpegCodecArgs {
    fn default() -> Self {
        Self {
            device_args: None,
            pre_codec_args: None,
            codec: None,
            post_codec_args: None,
            pixfmt: default_output_pixfmt(),
            max_bframes: None,
        }
    }
}

impl std::fmt::Display for FfmpegCodecArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        fn arg_fmt(args: Option<&Vec<(String, String)>>) -> String {
            if let Some(args) = args {
                args.iter()
                    .map(|(a1, a2)| format!("{a1} {a2}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                "".into()
            }
        }
        let pre = arg_fmt(self.pre_codec_args.as_ref());
        let codec = self
            .codec
            .as_ref()
            .map(|c| format!("-c:v {c}"))
            .unwrap_or_default();
        let pixfmt = self
            .pixfmt
            .as_ref()
            .map(|p| format!("-pix_fmt {p}"))
            .unwrap_or_default();
        let bframes = self
            .max_bframes
            .map(|n| format!("-bf {n}"))
            .unwrap_or_default();
        let post = arg_fmt(self.post_codec_args.as_ref());
        write!(f, "ffmpeg {pre} {codec} {pixfmt} {bframes} {post}")
    }
}

/// Codec selection for video encoding.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum CodecSelection {
    /// H.264 NVENC hardware encoder
    H264Nvenc,
    /// OpenH264 software encoder
    H264OpenH264,
    /// Custom ffmpeg codec configuration
    Ffmpeg(FfmpegCodecArgs),
}

impl CodecSelection {
    /// Checks if this codec selection requires a specific feature.
    pub fn requires(&self, what: &str) -> bool {
        use CodecSelection::*;
        match self {
            H264Nvenc => what == "nvenc",
            H264OpenH264 => false,
            Ffmpeg(args) => {
                if let Some(codec) = &args.codec {
                    codec.contains(what)
                } else {
                    false
                }
            }
        }
    }
}

impl std::fmt::Display for CodecSelection {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        use CodecSelection::*;
        let x = match self {
            H264Nvenc => "H264 NVENC",
            H264OpenH264 => "OpenH264",
            Ffmpeg(args) => {
                return std::fmt::Display::fmt(args, f);
            }
        };
        write!(f, "{x}")
    }
}

impl strand_cam_enum_iter::EnumIter for CodecSelection {
    fn variants() -> Vec<Self> {
        use CodecSelection::*;
        vec![
            H264Nvenc,
            H264OpenH264,
            // Don't give bare option as it seems less useful than specifying a codec.
            // Keep these in sync with the list in ffmpeg-writer.
            Ffmpeg(FfmpegCodecArgs {
                codec: Some("h264_videotoolbox".to_string()),
                ..Default::default()
            }),
            Ffmpeg(FfmpegCodecArgs {
                codec: Some("h264_nvenc".to_string()),
                ..Default::default()
            }),
            Ffmpeg(FfmpegCodecArgs {
                codec: Some("h264_nvmpi".to_string()),
                ..Default::default()
            }),
            Ffmpeg(FfmpegCodecArgs {
                device_args: Some(vec![("-vaapi_device".into(), "/dev/dri/renderD128".into())]),
                pre_codec_args: Some(vec![("-vf".into(), "format=nv12,hwupload".into())]),
                codec: Some("h264_vaapi".to_string()),
                post_codec_args: Some(vec![("-color_range".into(), "pc".into())]),
                // The `format=nv12,hwupload` filter chain already fixes the
                // format and the encoder works on hardware surfaces; forcing an
                // output `-pix_fmt` here would conflict.
                pixfmt: None,
                ..Default::default()
            }),
            // x264 with defaults
            Ffmpeg(FfmpegCodecArgs {
                codec: Some("libx264".to_string()),
                ..Default::default()
            }),
            // x264 with -crf and -preset
            Ffmpeg(FfmpegCodecArgs {
                codec: Some("libx264".to_string()),
                post_codec_args: Some(vec![
                    ("-crf".into(), "22".into()),
                    ("-preset".into(), "medium".into()),
                ]),
                ..Default::default()
            }),
        ]
    }
}

/// Camera control commands for remote operation.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum CamArg {
    /// Ignore future frame processing errors for this duration of seconds from current time.
    ///
    /// If seconds are not given, ignore forever.
    SetIngoreFutureFrameProcessingErrors(Option<i64>),

    SetExposureTime(f64),
    SetExposureAuto(strand_cam_types::AutoMode),
    SetFrameRateLimitEnabled(bool),
    SetFrameRateLimit(f64),
    SetGain(f64),
    SetGainAuto(strand_cam_types::AutoMode),
    SetRecordingFps(RecordingFrameRate),
    SetMp4Bitrate(BitrateSelection),
    SetMp4Codec(CodecSelection),
    SetMp4CudaDevice(String),
    SetMp4MaxFramerate(RecordingFrameRate),
    SetIsRecordingMp4(bool),
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
    SetTriggerboxClockModel(Option<ClockModel>),
    SetFormatStr(String),
    ToggleCheckerboardDetection(bool),
    ToggleCheckerboardDebug(bool),
    SetCheckerboardWidth(u32),
    SetCheckerboardHeight(u32),
    ClearCheckerboards,
    PerformCheckerboardCalibration,
    DoQuit,
    PostTrigger,
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

    /// Executes a named, backend-defined command via the `ci2::Camera`
    /// trait's `command_execute` method (e.g. `"StartPlayback"` for the
    /// `video-file` backend, which ends a hold on its first frame -- see
    /// `camera/ci2-video-file`). An unrecognized name is a no-op error,
    /// logged and otherwise ignored; most backends don't recognize any
    /// command name at all.
    ExecuteCommand(String),
}
