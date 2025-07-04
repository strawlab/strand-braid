// Copyright 2022-2023 Andrew D. Straw.
use std::{
    io::{BufReader, Read, Seek},
    path::Path,
};

use mkv_strand_reader::ParsedStrandCamMkv;

#[cfg(feature = "openh264")]
use machine_vision_formats::owned::OImage;

use super::*;

#[derive(thiserror::Error, Debug)]
pub enum StrandMkvSourceError {
    #[error("cannot skip frames without decoding H264")]
    CannotSkipWithoutDecodingH264,
    #[error("could not decode single frame with openh264")]
    CouldNotDecodeSingleFrameWithOpenH264,
    #[error("Uncompressed MKV with fourcc '{0}' unsupported")]
    UnsupportedFourcc(String),
    #[error("Unsupported codec '{0}'")]
    UnsupportedCodec(String),
    #[error("Support for {timestamp_source:?} timestamp source not (yet) implemented for Strand Cam MKV files.")]
    UnimplTsSource { timestamp_source: TimestampSource },
    #[error("unexpected image data")]
    UnexpectedImageData,
}

// NAL unit start for b"MISPmicrosectime":
const PRECISION_TIME_NALU_START: &[u8] = &[
    0x00, 0x00, 0x00, 0x01, 0x06, 0x05, 28, b'M', b'I', b'S', b'P', b'm', b'i', b'c', b'r', b'o',
    b's', b'e', b'c', b't', b'i', b'm', b'e',
];

/// An MKV file saved by Strand Camera.
///
/// Note that this is not a general purpose MKV file converter but is specific
/// to MKV files which have been saved by Strand Camera.
pub struct StrandCamMkvSource<R: Read + Seek> {
    rdr: R,
    pub parsed: ParsedStrandCamMkv,
    src_format: Format,
    is_uncompressed: bool,
    h264_decoder_state: Option<crate::opt_openh264_decoder::DecoderType>,
    keyframes_cache: Option<Vec<usize>>,
}

impl<R: Read + Seek> FrameDataSource for StrandCamMkvSource<R> {
    fn width(&self) -> u32 {
        self.parsed.width
    }
    fn height(&self) -> u32 {
        self.parsed.height
    }
    fn camera_name(&self) -> Option<&str> {
        self.parsed
            .metadata
            .camera_name
            .as_ref()
            .map(|x| x.as_ref())
    }
    fn gamma(&self) -> Option<f32> {
        self.parsed.metadata.gamma
    }
    fn frame0_time(&self) -> Option<chrono::DateTime<chrono::FixedOffset>> {
        Some(self.parsed.metadata.creation_time)
    }
    fn average_framerate(&self) -> Option<f64> {
        None
    }
    fn skip_n_frames(&mut self, n_frames: usize) -> Result<()> {
        if n_frames > 0 && self.src_format == Format::H264 {
            if self.keyframes_cache.is_none() {
                self.read_keyframes();
            }
            let keyframes = self.keyframes_cache.as_ref().unwrap();

            let decoder = match self.h264_decoder_state.as_mut() {
                Some(decoder) => decoder,
                None => {
                    return Err(StrandMkvSourceError::CannotSkipWithoutDecodingH264.into());
                }
            };

            let mut best_keyframe = keyframes[0];
            let target_frame = n_frames + 1; // we skip N so want N+1.
            for keyframe in keyframes.iter() {
                if *keyframe <= target_frame {
                    best_keyframe = *keyframe;
                }
            }

            for (idx, bd) in self.parsed.block_data.iter().take(n_frames).enumerate() {
                // always decode keyframe and subsequent before target
                if idx < best_keyframe {
                    // always decode first frame with SPS and PPS
                    if idx != 0 {
                        // skip decoding this frame
                        continue;
                    }
                }
                self.rdr.seek(std::io::SeekFrom::Start(bd.start_idx))?;
                let mut h264_raw_buf = vec![0u8; bd.size];
                self.rdr.read_exact(&mut h264_raw_buf)?;

                if let Some(decoded_yuv) = decoder.decode(&h264_raw_buf)? {
                    decoded_yuv
                } else {
                    return Err(StrandMkvSourceError::CouldNotDecodeSingleFrameWithOpenH264.into());
                };
            }

            self.keyframes_cache = None; // reset this.
        }

        let block_data = self.parsed.block_data.split_off(n_frames);

        let timeshift = block_data[0].pts;
        self.parsed.block_data = block_data
            .into_iter()
            .map(|mut el| {
                el.pts -= timeshift;
                el
            })
            .collect();
        self.parsed.metadata.creation_time += chrono::Duration::from_std(timeshift).unwrap();
        self.keyframes_cache = None;
        Ok(())
    }
    fn estimate_luminance_range(&mut self) -> Result<(u16, u16)> {
        Err(Error::UnsupportedForEsimatingLuminangeRange)
    }
    fn iter<'a>(&'a mut self) -> Box<dyn Iterator<Item = Result<FrameData>> + 'a> {
        Box::new(StrandCamMkvSourceIter {
            parent: self,
            idx: 0,
        })
    }
    fn timestamp_source(&self) -> &str {
        "MKV creation time + PTS"
    }
    fn has_timestamps(&self) -> bool {
        true
    }
}

struct StrandCamMkvSourceIter<'a, R: Read + Seek> {
    parent: &'a mut StrandCamMkvSource<R>,
    idx: usize,
}

#[derive(PartialEq)]
enum Format {
    UncompressedMono,
    H264,
}

impl<R: Read + Seek> StrandCamMkvSource<R> {
    fn new<P>(
        rdr: R,
        path: Option<P>,
        do_decode_h264: bool,
        timestamp_source: crate::TimestampSource,
    ) -> Result<Self>
    where
        P: AsRef<std::path::Path>,
    {
        let (parsed, rdr) = mkv_strand_reader::parse_strand_cam_mkv(rdr, false, path)?;
        if let Some(uncompressed_fourcc) = &parsed.uncompressed_fourcc {
            if uncompressed_fourcc.as_str() != "Y800" {
                return Err(
                    StrandMkvSourceError::UnsupportedFourcc(uncompressed_fourcc.clone()).into(),
                );
            }
        }

        let is_uncompressed = parsed.uncompressed_fourcc.is_some();

        let src_format = if is_uncompressed {
            Format::UncompressedMono
        } else if &parsed.codec == "V_MPEG4/ISO/AVC" {
            Format::H264
        } else {
            return Err(StrandMkvSourceError::UnsupportedCodec(parsed.codec).into());
        };

        let h264_decoder_state = if do_decode_h264 {
            Some(crate::opt_openh264_decoder::DecoderType::new()?)
        } else {
            None
        };

        match timestamp_source {
            crate::TimestampSource::BestGuess => {}
            _ => {
                return Err(StrandMkvSourceError::UnimplTsSource { timestamp_source }.into());
            }
        }

        Ok(Self {
            rdr,
            parsed,
            src_format,
            is_uncompressed,
            h264_decoder_state,
            keyframes_cache: None,
        })
    }

    pub fn is_uncompressed(&self) -> bool {
        self.is_uncompressed
    }

    /// Return all indices of keyframes (I frames)
    fn read_keyframes(&mut self) {
        let mut keyframes_cache = vec![];
        for (idx, bd) in self.parsed.block_data.iter().enumerate() {
            if bd.is_keyframe {
                keyframes_cache.push(idx);
            }
        }
        self.keyframes_cache = Some(keyframes_cache);
    }

    fn get_frame(&mut self, idx: usize) -> Option<Result<FrameData>> {
        let bd = self.parsed.block_data.get(idx);
        bd?;
        Some(self.get_frame_inner(idx))
    }
    fn get_frame_inner(&mut self, idx: usize) -> Result<FrameData> {
        let bd = &self.parsed.block_data[idx];

        let width = self.parsed.width;
        let height = self.parsed.height;
        let stride = usize::try_from(self.parsed.width).unwrap();

        self.rdr.seek(std::io::SeekFrom::Start(bd.start_idx))?;
        let mut image_data = vec![0u8; bd.size];
        self.rdr.read_exact(&mut image_data)?;

        let pts = bd.pts;

        let image = match self.src_format {
            Format::UncompressedMono => super::ImageData::Decoded(
                DynamicFrameOwned::from_buf(
                    width,
                    height,
                    stride,
                    image_data,
                    machine_vision_formats::PixFmt::Mono8,
                )
                .unwrap(),
            ),
            Format::H264 => {
                // This is a hacky and imperfect way to check if the h264 stream
                // has a timestamp. It is hacky because:
                //  1) it assumes the timestamp NAL unit will be the first NAL
                // unit (or that there will only be one NAL unit). That said, I
                // think this is actually what the MISB standard specifies.
                //  2) it does not really parse the NAL unit structure and
                // assumes, for example, that the start bytes are `[0x00, 0x00,
                // 0x00, 0x01]` whereas `[0x00, 0x00, 0x01]` would also be
                // theoretically valid start bytes. Still, we write the full 4
                // start bytes, so this should be OK.
                if !image_data.starts_with(&[0, 0, 0, 1]) {
                    return Err(StrandMkvSourceError::UnexpectedImageData.into());
                }
                let has_precision_timestamp = image_data.starts_with(PRECISION_TIME_NALU_START);
                if let Some(decoder) = self.h264_decoder_state.as_mut() {
                    let dynamic_frame = if let Some(decoded_yuv) = decoder.decode(&image_data)? {
                        my_decode(decoded_yuv, width, height)?
                    } else {
                        return Err(
                            StrandMkvSourceError::CouldNotDecodeSingleFrameWithOpenH264.into()
                        );
                    };
                    super::ImageData::Decoded(dynamic_frame)
                } else {
                    super::ImageData::EncodedH264(super::EncodedH264 {
                        data: H264EncodingVariant::AnnexB(image_data),
                        has_precision_timestamp,
                    })
                }
            }
        };

        Ok(FrameData {
            timestamp: Timestamp::Duration(pts),
            image,
            buf_len: bd.size,
            idx,
        })
    }
}

#[cfg(not(feature = "openh264"))]
fn my_decode(_decoded_yuv: (), _width: u32, _height: u32) -> Result<DynamicFrameOwned> {
    Err(Error::H264Error("No H264 decoder support at compile time"))
}

#[cfg(feature = "openh264")]
fn my_decode(
    decoded_yuv: openh264::decoder::DecodedYUV<'_>,
    width: u32,
    height: u32,
) -> Result<DynamicFrameOwned> {
    use openh264::formats::YUVSource;
    let dim = decoded_yuv.dimensions();

    let stride = dim.0 * 3;
    let mut image_data = vec![0u8; stride * dim.1];
    decoded_yuv.write_rgb8(&mut image_data);
    Ok(strand_dynamic_frame::DynamicFrameOwned::from_static(
        OImage::<machine_vision_formats::pixel_format::RGB8>::new(
            width, height, stride, image_data,
        )
        .unwrap(),
    ))
}

impl<R: Read + Seek> Iterator for StrandCamMkvSourceIter<'_, R> {
    type Item = Result<FrameData>;
    fn next(&mut self) -> Option<Self::Item> {
        let result = self.parent.get_frame(self.idx);
        self.idx += 1;
        result
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.parent.parsed.block_data.len() - self.idx;
        (remaining, Some(remaining))
    }
}

pub(crate) fn mkv_source_from_path_with_timestamp_source<P: AsRef<Path>>(
    path: P,
    do_decode_h264: bool,
    timestamp_source: crate::TimestampSource,
) -> Result<StrandCamMkvSource<BufReader<std::fs::File>>> {
    let rdr = std::fs::File::open(path.as_ref())?;
    let buf_reader = BufReader::new(rdr);
    StrandCamMkvSource::new(
        buf_reader,
        Some(path.as_ref().to_path_buf()),
        do_decode_h264,
        timestamp_source,
    )
}
