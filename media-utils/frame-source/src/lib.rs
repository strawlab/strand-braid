// Copyright 2022-2023 Andrew D. Straw.
use std::path::PathBuf;

use anyhow::Result;

use basic_frame::DynamicFrame;

pub mod pv_tiff_stack;
use pv_tiff_stack::TiffImage;
pub mod fmf_source;
pub mod mp4_source;
pub mod strand_cam_mkv_source;

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
    fn frame0_time(&self) -> Option<chrono::DateTime<chrono::FixedOffset>>;
    /// Set source to skip the first N frames.
    ///
    /// Note that this resets frame0_time accordingly.
    fn skip_n_frames(&mut self, n_frames: usize) -> Result<()>;
    /// Scan over the input images and estimate the luminance range
    ///
    /// Returns Ok<(min, max)> when successful.
    fn estimate_luminance_range(&mut self) -> Result<(u16, u16)>;
    /// Get an iterator over all frames.
    fn iter(&mut self) -> Box<dyn Iterator<Item = Result<FrameData>> + '_>;
}

/// A single frame of data, including `image` and `timestamp` fields.
#[derive(PartialEq, Debug)]
pub struct FrameData {
    /// The timestamp, measured as the duration elapsed since the track onset
    /// until the exposure started.
    ///
    /// This is often called "PTS" (presentation time stamp).
    timestamp: std::time::Duration,
    image: ImageData,
    buf_len: usize,
    /// The number of the frame in the source
    ///
    /// Starts with 0
    idx: usize,
}

impl FrameData {
    /// Get the timestamp, measured as the duration elapsed since the track onset
    /// until the exposure started.
    ///
    /// This is often called "PTS" (presentation time stamp).
    pub fn timestamp(&self) -> std::time::Duration {
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
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
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
    AnnexB(Vec<u8>),
    Avcc(Vec<u8>),
}

#[derive(Clone, PartialEq)]
pub struct EncodedH264 {
    pub data: H264EncodingVariant,
    pub has_precision_timestamp: bool,
}

/// Create a [FrameDataSource] from a path.
///
/// The `do_decode_h264` argument specifies that an H264 source will be decoded
/// (e.g. to extract individual images).
pub fn from_path<P: AsRef<std::path::Path>>(
    input: P,
    do_decode_h264: bool,
) -> Result<Box<dyn FrameDataSource>> {
    let input_path = PathBuf::from(input.as_ref());
    let is_file = std::fs::metadata(input.as_ref())?.is_file();
    if is_file {
        if let Some(extension) = input_path.extension() {
            match extension.to_str() {
                Some("mkv") => {
                    let mkv_video = strand_cam_mkv_source::from_path(&input, do_decode_h264)?;
                    return Ok(Box::new(mkv_video));
                }
                Some("mp4") => {
                    let mp4_video = mp4_source::from_path(&input, do_decode_h264)?;
                    return Ok(Box::new(mp4_video));
                }
                _ => {}
            }
        }
        let fname_lower = input_path.to_string_lossy().to_lowercase();
        if fname_lower.ends_with(".fmf") || fname_lower.ends_with(".fmf.gz") {
            let fmf_video = fmf_source::from_path(&input)?;
            return Ok(Box::new(fmf_video));
        }
        anyhow::bail!(
            "input {} is a file, but the extension was not recognized.",
            input.as_ref().display()
        );
    } else {
        let dirname = input_path;

        if !std::fs::metadata(&dirname)?.is_dir() {
            anyhow::bail!("Attempting to open \"{}\" as directory with TIFF stack failed because it is not a directory.", dirname.display());
        }
        let pattern = dirname.join("*.tif");
        let stack = pv_tiff_stack::from_path_pattern(pattern.to_str().unwrap())?;
        Ok(Box::new(stack))
    }
}
