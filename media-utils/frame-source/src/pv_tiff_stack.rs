// Copyright 2022-2023 Andrew D. Straw.
use std::{io::Cursor, path::Path};

use color_eyre::eyre::{self as anyhow, WrapErr};

use super::*;

const IJIJINFO_KEY: u16 = 50839;
const MAGICSTR: &str = "IJIJinfo";

pub fn from_path_pattern(pattern: &str) -> Result<PvTiffStack> {
    PvTiffStack::new(pattern)
}

#[derive(Clone, PartialEq)]
pub struct TiffImage {
    // Maybe TODO: make this reference to the data source? (don't force a clone
    // of it).
    pub buf: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub metadata: TiffMetadata,
}

#[derive(Clone, PartialEq)]
pub struct TiffMetadata {
    /// all metadata
    ///
    /// The metadata was parsed from the `.tiff` file EXIF data.
    json: serde_json::Value,
    /// raw `PVCAM-FMD-FrameNr` value
    framenumber: usize,
    /// converted from `PVCAM-FMD-TimestampBofPs`
    ///
    /// This truly is a timestamp in the sense that it is an absolute time from
    /// when the camera turned on. However, the type used to best represent this
    /// is `std::time::Duration` because there is no reference to external
    /// clocks.
    pub timestamp: std::time::Duration,
    /// raw `PVCAM-FMD-BitDepth` value
    pub bit_depth: u8,
}

/// A stack of TIFF images
///
/// Note that this is not a generic TIFF stack reader but rather one saved by
/// Micromanager using a Photometrics camera.
pub struct PvTiffStack {
    paths: Vec<std::path::PathBuf>,
    /// Absolute time of frame0.
    frame0_time: chrono::DateTime<chrono::FixedOffset>,
    /// Offset of the metadata timestamp
    ///
    /// This the value of the metadata timestamp for the frame 0. All other
    /// timestamps are offset by this value so the timestamp of frame 0 is 0.
    frame0_timestamp_offset: std::time::Duration,
    width: u32,
    height: u32,
    tiff_image0: TiffImage,
}

impl FrameDataSource for PvTiffStack {
    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.height
    }
    fn iter(&mut self) -> Box<dyn Iterator<Item = Result<FrameData>> + '_> {
        Box::new(ImageStackIter::new(self))
    }
    fn skip_n_frames(&mut self, n_frames: usize) -> Result<()> {
        let paths = self.paths.split_off(n_frames);
        let new_self = Self::new_from_paths(paths)?;
        *self = new_self;
        Ok(())
    }
    fn frame0_time(&self) -> Option<chrono::DateTime<chrono::FixedOffset>> {
        Some(self.frame0_time)
    }
    fn estimate_luminance_range(&mut self) -> Result<(u16, u16)> {
        // take 5 images or all of them, whatever is less.
        let n_images = self.paths.len().min(5);
        let step_size = self.paths.len() / n_images;
        let (mut low, mut high) = self.tiff_image0.luminance_range()?;
        for this_frame in self.iter().step_by(step_size) {
            let image = match this_frame?.image {
                ImageData::Tiff(image) => image,
                _ => {
                    anyhow::bail!("expected tiff image");
                }
            };
            let (this_low, this_high) = image.luminance_range()?;
            if this_low < low {
                low = this_low;
            }
            if this_high > high {
                high = this_high;
            }
        }
        Ok((low, high))
    }
    fn timestamp_source(&self) -> &str {
        "PVCAM-FMD-TimestampBofPs"
    }
    fn has_timestamps(&self) -> bool {
        true
    }
}

impl PvTiffStack {
    fn new(pattern: &str) -> Result<Self> {
        let mut paths = vec![];
        for path in glob::glob_with(
            pattern,
            glob::MatchOptions {
                case_sensitive: false,
                require_literal_separator: true,
                require_literal_leading_dot: true,
            },
        )? {
            paths.push(path?);
        }
        if paths.is_empty() {
            anyhow::bail!("no files in \"{}\"", pattern);
        }
        Self::new_from_paths(paths)
    }
    fn new_from_paths(mut paths: Vec<PathBuf>) -> Result<Self> {
        paths.sort();

        let file0_buf = read_file(&paths[0])?;
        let frame0_metadata = extract_tiff_metadata(&file0_buf).with_context(|| {
            format!(
                "while extracting TIFF metadata from file {}",
                paths[0].display()
            )
        })?;

        // For the initial time, use the `ReceivedTime` metadata. Corrected by
        // subtracting exposure duration. I guess there remains some error due
        // to transfer time and CPU scheduling.
        let mut frame0_time = frame0_metadata.json["ReceivedTime"]
            .as_str()
            .ok_or_else(json_parse_err)?
            .parse()?;

        let pvcam_fmd_exposure_str = frame0_metadata.json["UserData"]["PVCAM-FMD-ExposureTimePs"]
            ["scalar"]
            .as_str()
            .ok_or_else(json_parse_err)?;
        let exposure_duration =
            chrono::Duration::from_std(parse_picosecs(pvcam_fmd_exposure_str)?)?;
        frame0_time -= exposure_duration;

        let frame0_timestamp_offset = frame0_metadata.timestamp;
        let tiff_image0 = read_tiff_image(&file0_buf, frame0_metadata)?;
        let width = tiff_image0.width;
        let height = tiff_image0.height;

        Ok(Self {
            paths,
            frame0_time,
            frame0_timestamp_offset,
            width,
            height,
            tiff_image0,
        })
    }
    pub fn len(&self) -> usize {
        self.paths.len()
    }
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}

struct ImageStackIter<'a> {
    idx: usize,
    frame0_timestamp_offset: std::time::Duration,
    inner: std::slice::Iter<'a, std::path::PathBuf>,
}

impl<'a> ImageStackIter<'a> {
    fn new(parent: &'a PvTiffStack) -> Self {
        Self {
            idx: 0,
            frame0_timestamp_offset: parent.frame0_timestamp_offset,
            inner: parent.paths.iter(),
        }
    }
}

impl<'a> Iterator for ImageStackIter<'a> {
    type Item = Result<FrameData>;
    fn next(&mut self) -> Option<Self::Item> {
        let result = self
            .inner
            .next()
            .map(|path| path_to_tiff(path, self.frame0_timestamp_offset, self.idx));
        self.idx += 1;
        result
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl TiffImage {
    fn luminance_range(&self) -> Result<(u16, u16)> {
        // Ideally we should use `tiff-decoder` crate and `read_tiff_image` here
        // but that converts to 8 bit already, so it would be useless.
        let mut decoder = tiff::decoder::Decoder::new(Cursor::new(&self.buf))?;

        let buf = decoder.read_image()?;
        let color = decoder.colortype()?;

        let (width, _height) = decoder.dimensions()?;
        let width: usize = width.try_into()?;

        // TODO do we need to worry about the byte order? The decoder can return
        // byte order information, but for now I am assuming it decodes into the
        // native machine byte order.

        let mut min = 255;
        let mut max = 0;
        match (color, buf) {
            (tiff::ColorType::Gray(16), tiff::decoder::DecodingResult::U16(vals)) => {
                for row in vals.chunks_exact(width) {
                    for val_u16 in row {
                        if *val_u16 < min {
                            min = *val_u16;
                        }
                        if *val_u16 > max {
                            max = *val_u16;
                        }
                    }
                }
            }
            _ => {
                anyhow::bail!("unsupported tiff type for estimating luminance range");
            }
        }
        Ok((min, max))
    }
}

fn read_tiff_image(buf: &[u8], metadata: TiffMetadata) -> Result<TiffImage> {
    let mut decoder = tiff::decoder::Decoder::new(Cursor::new(buf))?;
    let (width, height) = decoder.dimensions()?;
    Ok(TiffImage {
        buf: buf.to_vec(),
        width,
        height,
        metadata,
    })
}

fn extract_tiff_metadata(buf: &[u8]) -> Result<TiffMetadata> {
    let mut rdr = Cursor::new(buf);
    let exifreader = exif::Reader::new();
    let exif = exifreader
        .read_from_container(&mut rdr)
        .context("reading EXIF data")?;

    let imagej_metadata = exif.get_field(
        exif::Tag(exif::Context::Tiff, IJIJINFO_KEY),
        exif::In::PRIMARY,
    );
    let bytes = if let Some(imagej_metadata) = imagej_metadata {
        if let exif::Value::Byte(bytes) = &imagej_metadata.value {
            bytes
        } else {
            anyhow::bail!("imagej data expected to be bytes");
        }
    } else {
        anyhow::bail!("failed to read metadata");
    };
    let mystr = String::from_utf8(bytes.clone())?.replace(['\x00', '\x01'], "");
    if !mystr.starts_with(MAGICSTR) {
        anyhow::bail!("exif metadata does not start with expected magic string");
    }
    let mystr = &mystr[MAGICSTR.len()..];
    let json: serde_json::Value = serde_json::from_str(mystr)?;
    // dbg!(&json);

    let framenumber = json["UserData"]["PVCAM-FMD-FrameNr"]["scalar"]
        .as_str()
        .ok_or_else(json_parse_err)?
        .parse()?;
    let pvcam_fmd_timestamp_str = json["UserData"]["PVCAM-FMD-TimestampBofPs"]["scalar"]
        .as_str()
        .ok_or_else(json_parse_err)?;
    let timestamp = parse_picosecs(pvcam_fmd_timestamp_str)?;

    let bit_depth = json["UserData"]["PVCAM-FMD-BitDepth"]["scalar"]
        .as_str()
        .ok_or_else(json_parse_err)?
        .parse()?;
    Ok(TiffMetadata {
        json,
        framenumber,
        timestamp,
        bit_depth,
    })
}

fn path_to_tiff(
    path: &std::path::PathBuf,
    frame0_timestamp_offset: std::time::Duration,
    assign_idx: usize,
) -> Result<FrameData> {
    let buf = read_file(path)?;
    let buf_len = buf.len();
    let mut metadata = extract_tiff_metadata(&buf)?;
    metadata.timestamp -= frame0_timestamp_offset;
    let timestamp = Timestamp::Duration(metadata.timestamp);
    Ok(FrameData {
        image: ImageData::Tiff(read_tiff_image(&buf, metadata)?),
        timestamp,
        buf_len,
        idx: assign_idx,
    })
}

fn json_parse_err() -> anyhow::Error {
    anyhow::anyhow!("json parse error")
}

fn read_file<P: AsRef<Path>>(path: P) -> Result<Vec<u8>> {
    std::fs::read(path.as_ref()).with_context(|| format!("Reading {}", path.as_ref().display()))
}

fn parse_picosecs(picosecs_str: &str) -> Result<std::time::Duration> {
    let elapsed_picosecs: u128 = picosecs_str.parse()?;
    let elapsed_msecs: f64 = elapsed_picosecs as f64 / 1_000_000_000f64;
    Ok(std::time::Duration::from_secs_f64(elapsed_msecs / 1000.0))
}
