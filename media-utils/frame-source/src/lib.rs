// Copyright 2022-2023 Andrew D. Straw.
use std::path::PathBuf;

use basic_frame::DynamicFrame;

pub mod pv_tiff_stack;
use pv_tiff_stack::TiffImage;
pub mod fmf_source;
mod h264_annexb_splitter;
pub mod h264_source;
pub mod mp4_source;
mod opt_openh264_decoder;
mod srt_reader;
pub mod strand_cam_mkv_source;

mod ntp_timestamp;
#[cfg(test)]
mod test_timestamps;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("SRT parse error")]
    SrtParseError,
    #[error("decoder unexpectedly did not return image data")]
    DecoderDidNotReturnImageData,
    #[error("expected SPS not found")]
    ExpectedSpsNotFound,
    #[error("expected PPS not found")]
    ExpectedPpsNotFound,
    #[error("fmf file with not enough data")]
    FmfWithNotEnoughData,
    #[error("JSON parse error")]
    JsonParseError,
    #[error("expected tiff image")]
    ExpectedTiffImage,
    #[error("no files found with pattern")]
    NoFilesFound,
    #[error("unsupported for estimating luminance range")]
    UnsupportedForEsimatingLuminangeRange,
    #[error("imagej data expected to be bytes")]
    ImageJDataExpectedToBeBytes,
    #[error("failed to read metadata")]
    FailedToReadMetadata,
    #[error("exif metadata does not start with expected magic string")]
    ExifMetadataFailsMagic,
    #[error("Skipping frames with H264 file is not supported.")]
    SkippingFramesNotSupported,
    #[error("Not implemented: {0}")]
    NotImplemented(&'static str),
    #[error("Requested SRT file as timestamp source, but no .srt file path given.")]
    NoSrtPathGiven,
    #[error("H264Error: {0}")]
    H264Error(&'static str),
    #[error("unexpected error reading NAL unit {nal_location_index} SEI: {e:?}")]
    H264Nal {
        nal_location_index: usize,
        e: h264_reader::rbsp::BitReaderError,
    },
    #[error("PPS error {0}")]
    H264Pps(String),
    #[error("H264 timestamp error {0}")]
    H264TimestampError(String),
    #[error("H264 UDU error {0}")]
    UduError(String),
    #[error("unexpected payload length")]
    UnexpectedPayloadLength,
    #[error("unexpected start code emulation prevention byte")]
    UnexpectedStartCodeByte,
    #[error("MP4 source error: {0}")]
    Mp4SourceError(#[from] mp4_source::Mp4SourceError),
    #[error("strand camera MKV source error: {0}")]
    StrandMkvSourceError(#[from] strand_cam_mkv_source::StrandMkvSourceError),
    #[error("srt file given, but not supported for this file type")]
    NoSrtSupportForFileType,
    #[error("input {0} is a file, but the extension was not recognized.")]
    UnknownExtensionForFile(PathBuf),
    #[error("Attempting to open \"{0}\" as directory with TIFF stack failed because it is not a directory.")]
    TiffStackNotDir(PathBuf),
    #[error("{0}")]
    FmfError(#[from] fmf::FMFError),
    #[error("{0}")]
    PatternError(#[from] glob::PatternError),
    #[error("{0}")]
    GlobError(#[from] glob::GlobError),
    #[error("{0}")]
    OutOfRangeError(#[from] chrono::OutOfRangeError),
    #[error("{0}")]
    ChronoParseError(#[from] chrono::ParseError),
    #[error("{0}")]
    TiffError(#[from] tiff::TiffError),
    #[error("{0}")]
    TryFromIntError(#[from] std::num::TryFromIntError),
    #[error("{0}")]
    ParseIntError(#[from] std::num::ParseIntError),
    #[error("{0}")]
    ExifError(#[from] exif::Error),
    #[error("{0}")]
    FromUtf8Error(#[from] std::string::FromUtf8Error),
    #[error("{0}")]
    SerdeJsonError(#[from] serde_json::Error),
    #[error("{0}")]
    MkvStrandError(#[from] mkv_strand_reader::Error),
    #[cfg(feature = "openh264")]
    #[error("OpenH264Error: {0}")]
    OpenH264Error(#[from] openh264::Error),
    #[error("Mp4Error: {0}")]
    Mp4Error(#[from] mp4::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(feature = "openh264")]
pub const COMPILED_WITH_OPENH264: bool = true;
#[cfg(not(feature = "openh264"))]
pub const COMPILED_WITH_OPENH264: bool = false;

/// A source of FrameData
///
/// The `frame0_time` method return value is an `Option` because we want to be
/// able to parse sources without an absolute time for the first frame, such as
/// normal MP4 video files. Similarly, we do not have a `len` method indicating
/// number of frames because some sources (e.g. an .h264 file) do not store how
/// many frames they have but rather must be parsed from beginning to end.
pub trait FrameDataSource {
    /// Get the width of the source images, in pixels.
    fn width(&self) -> u32;
    /// Get the height of the source images, in pixels.
    fn height(&self) -> u32;
    fn camera_name(&self) -> Option<&str> {
        None
    }
    fn gamma(&self) -> Option<f32> {
        None
    }
    /// Get the timestamp of the first frame.
    ///
    /// Note that (in case they can differ), this is the time
    /// of the first frame rather than the creation time
    /// in the metadata.
    fn frame0_time(&self) -> Option<chrono::DateTime<chrono::FixedOffset>>;
    /// Set source to skip the first N frames.
    ///
    /// Note that this resets frame0_time accordingly.
    fn skip_n_frames(&mut self, n_frames: usize) -> Result<()>;
    /// Scan over the input images and estimate the luminance range
    ///
    /// Returns Ok<(min, max)> when successful.
    fn estimate_luminance_range(&mut self) -> Result<(u16, u16)>;
    /// Whether timestamps are available.
    ///
    /// If no timestamp is available, the frame "timestamp" with contain a
    /// fraction of completeness.
    fn has_timestamps(&self) -> bool;
    /// A string describing the source of the timestamp data
    fn timestamp_source(&self) -> &str;
    /// Get an iterator over all frames.
    fn iter<'a>(&'a mut self) -> Box<dyn Iterator<Item = Result<FrameData>> + 'a>;
}

/// A single frame of data, including `image` and `timestamp` fields.
#[derive(PartialEq, Debug)]
pub struct FrameData {
    /// This is often called "PTS" (presentation time stamp).
    timestamp: Timestamp,
    image: ImageData,
    buf_len: usize,
    /// The number of the frame in the source
    ///
    /// Starts with 0
    idx: usize,
}

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum Timestamp {
    /// The timestamp, measured as the duration elapsed since the track onset
    /// until the exposure started.
    Duration(std::time::Duration),
    /// In cases where no time is available, the fraction done.
    Fraction(f32),
}

impl Timestamp {
    pub fn unwrap_duration(&self) -> std::time::Duration {
        match self {
            Timestamp::Duration(d) => *d,
            Timestamp::Fraction(_) => {
                panic!("expected duration");
            }
        }
    }
}

impl FrameData {
    /// Get the timestamp, measured as the duration elapsed since the track onset
    /// until the exposure started.
    ///
    /// This is often called "PTS" (presentation time stamp).
    pub fn timestamp(&self) -> Timestamp {
        self.timestamp
    }
    /// Get the image data
    pub fn image(&self) -> &ImageData {
        &self.image
    }
    /// Get the image data
    pub fn into_image(self) -> ImageData {
        self.image
    }
    /// Get the number of the bytes used in the source.
    pub fn num_bytes(&self) -> usize {
        self.buf_len
    }
    /// Get the number of the frame in the source.
    ///
    /// Starts with 0.
    pub fn idx(&self) -> usize {
        self.idx
    }

    pub fn decoded(&self) -> Option<&DynamicFrame> {
        match &self.image {
            ImageData::Decoded(frame) => Some(frame),
            _ => None,
        }
    }

    pub fn take_decoded(self) -> Option<DynamicFrame> {
        match self.image {
            ImageData::Decoded(frame) => Some(frame),
            _ => None,
        }
    }
}

/// The image data
#[derive(Clone, PartialEq)]
pub enum ImageData {
    Decoded(DynamicFrame),
    Tiff(TiffImage),
    EncodedH264(EncodedH264),
}

impl std::fmt::Debug for ImageData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageData::Decoded(_) => {
                write!(f, "ImageData::Decoded")
            }
            ImageData::Tiff(_) => {
                write!(f, "ImageData::Tiff")
            }
            ImageData::EncodedH264(_) => {
                write!(f, "ImageData::EncodedH264")
            }
        }
    }
}

#[derive(Clone, PartialEq)]
pub enum H264EncodingVariant {
    /// single large buffer with Annex B headers
    AnnexB(Vec<u8>),
    /// single large buffer with AVCC headers
    Avcc(Vec<u8>),
    /// multiple buffers with just NAL unit data
    RawEbsp(Vec<Vec<u8>>),
}

impl std::fmt::Debug for H264EncodingVariant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AnnexB(buf) => write!(f, "H264EncodingVariant::AnnexB({} bytes)", buf.len()),
            Self::Avcc(buf) => write!(f, "H264EncodingVariant::Avcc({} bytes)", buf.len()),
            Self::RawEbsp(bufs) => {
                write!(f, "H264EncodingVariant::RawEbsp({} buffers)", bufs.len())
            }
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct EncodedH264 {
    pub data: H264EncodingVariant,
    pub has_precision_timestamp: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TimestampSource {
    BestGuess,
    FrameInfoRecvTime,
    Mp4Pts,
    MispMicrosectime,
    SrtFile,
}

trait MyAsStr {
    fn as_str(&self) -> &'static str;
}

impl MyAsStr for Option<TimestampSource> {
    fn as_str(&self) -> &'static str {
        use TimestampSource::*;
        match self {
            Some(BestGuess) => "(best guess)",
            Some(FrameInfoRecvTime) => "FrameInfo receive time",
            Some(Mp4Pts) => "MP4 PTS",
            Some(MispMicrosectime) => "MISPmicrosectime",
            Some(SrtFile) => "SRT file",
            None => "(no timestamps)",
        }
    }
}

/// Create a [FrameDataSource] from a path.
///
/// The `do_decode_h264` argument specifies that an H264 source will be decoded
/// (e.g. to extract individual images).
pub fn from_path<P: AsRef<std::path::Path>>(
    input: P,
    do_decode_h264: bool,
) -> Result<Box<dyn FrameDataSource>> {
    from_path_with_srt_timestamp_source(input, do_decode_h264, TimestampSource::BestGuess, None)
}

/// Create a [FrameDataSource] from a path with defined timestamp source
///
/// The `do_decode_h264` argument specifies that an H264 source will be decoded
/// (e.g. to extract individual images).
pub fn from_path_with_timestamp_source<P: AsRef<std::path::Path>>(
    input: P,
    do_decode_h264: bool,
    timestamp_source: TimestampSource,
) -> Result<Box<dyn FrameDataSource>> {
    from_path_with_srt_timestamp_source(input, do_decode_h264, timestamp_source, None)
}

/// Create a [FrameDataSource] from a path with defined timestamp source
///
/// The `do_decode_h264` argument specifies that an H264 source will be decoded
/// (e.g. to extract individual images).
pub fn from_path_with_srt_timestamp_source<P: AsRef<std::path::Path>>(
    input: P,
    do_decode_h264: bool,
    timestamp_source: TimestampSource,
    srt_file_path: Option<PathBuf>,
) -> Result<Box<dyn FrameDataSource>> {
    let input_path = PathBuf::from(input.as_ref());
    let is_file = std::fs::metadata(input.as_ref())?.is_file();
    if is_file {
        if let Some(extension) = input_path.extension() {
            match extension.to_str() {
                Some("mkv") => {
                    if srt_file_path.is_some() {
                        return Err(Error::NoSrtSupportForFileType);
                    }
                    let mkv_video = strand_cam_mkv_source::from_path_with_timestamp_source(
                        &input,
                        do_decode_h264,
                        timestamp_source,
                    )?;
                    return Ok(Box::new(mkv_video));
                }
                Some("mp4") => {
                    let mp4_video = mp4_source::from_path_with_timestamp_source(
                        &input,
                        do_decode_h264,
                        timestamp_source,
                        srt_file_path,
                    )?;
                    return Ok(Box::new(mp4_video));
                }
                Some("h264") => {
                    if srt_file_path.is_some() {
                        return Err(Error::NoSrtSupportForFileType);
                    }
                    let h264_video = h264_source::from_annexb_path_with_timestamp_source(
                        &input,
                        do_decode_h264,
                        timestamp_source,
                        None,
                    )?;
                    return Ok(Box::new(h264_video));
                }
                _ => {}
            }
        }
        let fname_lower = input_path.to_string_lossy().to_lowercase();
        if fname_lower.ends_with(".fmf") || fname_lower.ends_with(".fmf.gz") {
            let fmf_video = fmf_source::from_path(&input)?;
            return Ok(Box::new(fmf_video));
        }
        Err(Error::UnknownExtensionForFile(PathBuf::from(
            input.as_ref(),
        )))
    } else {
        let dirname = input_path;

        if !std::fs::metadata(&dirname)?.is_dir() {
            return Err(Error::TiffStackNotDir(dirname));
        }
        let pattern = dirname.join("*.tif");
        if srt_file_path.is_some() {
            return Err(Error::NoSrtSupportForFileType);
        }
        let stack = pv_tiff_stack::from_path_pattern(pattern.to_str().unwrap())?;
        Ok(Box::new(stack))
    }
}
