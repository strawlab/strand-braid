// Copyright 2022-2023 Andrew D. Straw.
use std::io::Cursor;

use eyre::{self as anyhow, Result};

use frame_source::pv_tiff_stack::TiffImage;
use strand_dynamic_frame::DynamicFrameOwned;

/// Configuration describing how to handle high dynamic range source material.
#[allow(non_camel_case_types)]
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, clap::ValueEnum, Debug)]
pub enum HdrConfig {
    /// Preserve full dynamic range
    Preserve,
    /// Reduce high dynamic range, map to 8 bits
    ///
    /// This preserves full dynamic range and thus simply drops detail
    Downscale_To_8Bit,
    /// Take lowest 8 bits (this is typically for debugging as it creates
    /// banding artifacts)
    Low_8Bits,
    /// Autoscale to 8 bits
    ///
    /// This analyzes the source material and linearly maps the used dynamic
    /// range to fill 8 bits.
    Rescale_Linear_To_8Bits,
}

impl std::fmt::Display for HdrConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        use clap::ValueEnum;
        let tmp = self.to_possible_value().unwrap();
        let s = tmp.get_name();
        write!(f, "{s}")
    }
}

pub struct ValHistogram {
    pub minmax: Option<(u16, u16)>,
}

impl Default for ValHistogram {
    fn default() -> Self {
        Self::new()
    }
}

impl ValHistogram {
    pub fn new() -> Self {
        Self { minmax: None }
    }

    fn update(&mut self, vals: &[u16]) {
        use itertools::Itertools;
        match vals.iter().minmax().into_option() {
            None => {}
            Some((this_min, this_max)) => {
                let minmax = match self.minmax {
                    None => (*this_min, *this_max),
                    Some((prev_min, prev_max)) => (
                        if prev_min < *this_min {
                            prev_min
                        } else {
                            *this_min
                        },
                        if prev_max > *this_max {
                            prev_max
                        } else {
                            *this_max
                        },
                    ),
                };
                self.minmax = Some(minmax);
            }
        }
    }
}

pub fn read_tiff_image(
    tiff_image: &TiffImage,
    hdr_cfg: &HdrConfig,
    hdr_lum_range: Option<(u16, u16)>,
    histogram: &mut ValHistogram,
) -> Result<DynamicFrameOwned> {
    use tiff::decoder::DecodingResult;

    let TiffImage { buf, metadata, .. } = tiff_image;

    let mut decoder = tiff::decoder::Decoder::new(Cursor::new(buf))?;
    let buf = decoder.read_image()?;
    let color = decoder.colortype()?;

    let (width, height) = decoder.dimensions()?;

    if hdr_lum_range.is_some() && hdr_cfg != &HdrConfig::Rescale_Linear_To_8Bits {
        anyhow::bail!("hdr_lum_range is set, but not rescaling to linear 8 bits");
    }

    let image_data = match (color, buf) {
        (tiff::ColorType::Gray(16), DecodingResult::U16(vals)) => {
            histogram.update(&vals);
            match &hdr_cfg {
                HdrConfig::Preserve => {
                    anyhow::bail!(
                        "HDR configuration '{}': export to HDR mp4 not yet implemented. (Hint: use --hdr-config {})",
                        HdrConfig::Preserve, HdrConfig::Downscale_To_8Bit
                    );
                }
                HdrConfig::Downscale_To_8Bit => {
                    let hdr_fn = match metadata.bit_depth {
                        9 => |x: &u16| ((*x >> 1) & 0x00FF) as u8,
                        10 => |x: &u16| ((*x >> 2) & 0x00FF) as u8,
                        11 => |x: &u16| ((*x >> 3) & 0x00FF) as u8,
                        12 => |x: &u16| ((*x >> 4) & 0x00FF) as u8,
                        13 => |x: &u16| ((*x >> 5) & 0x00FF) as u8,
                        14 => |x: &u16| ((*x >> 6) & 0x00FF) as u8,
                        15 => |x: &u16| ((*x >> 7) & 0x00FF) as u8,
                        16 => |x: &u16| ((*x >> 8) & 0x00FF) as u8,
                        _ => {
                            anyhow::bail!("unsupported bit depth {}", metadata.bit_depth);
                        }
                    };
                    vals.iter().map(hdr_fn).collect()
                }
                HdrConfig::Low_8Bits => vals.iter().map(|x: &u16| (*x & 0x00FF) as u8).collect(),
                HdrConfig::Rescale_Linear_To_8Bits => {
                    if let Some((min, max)) = hdr_lum_range {
                        let scale = 255.0 / (max as f64 - min as f64);
                        let clip = |val: f64| {
                            if val > 255.0 {
                                255
                            } else if val < 0.0 {
                                0
                            } else {
                                val as u8
                            }
                        };
                        vals.iter()
                            .map(|x: &u16| clip((*x as f64 - min as f64) * scale))
                            .collect()
                    } else {
                        anyhow::bail!("luminance range needed for autoscaling");
                    }
                }
            }
        }
        (tiff::ColorType::Gray(8), DecodingResult::U8(image_data)) => image_data,
        _ => {
            anyhow::bail!("unsupported tiff type");
        }
    };
    let expected_size = width * height;
    if image_data.len() != expected_size as usize {
        anyhow::bail!("actual image size different than expected");
    }
    let stride = width.try_into().unwrap();

    Ok(DynamicFrameOwned::from_buf(
        width,
        height,
        stride,
        image_data,
        machine_vision_formats::PixFmt::Mono8,
    )
    .unwrap())
}
