// Copyright 2022-2023 Andrew D. Straw.
#![deny(unsafe_code)]
use std::io::Write;

use machine_vision_formats::{ImageStride, PixelFormat};

use basic_frame::{match_all_dynamic_fmts, DynamicFrame};

use less_avc::ycbcr_image::*;

/// An H.264 encoding error.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("LessAVC error: {source}")]
    LessAvcError {
        #[from]
        source: less_avc::Error,

    },
    #[error("convert image error: {source}")]
    ConvertImageError {
        #[from]
        source: convert_image::Error,

    },
    #[error("y4m writer error: {0}")]
    Y4mError(#[from] y4m_writer::Error),
}

type Result<T> = std::result::Result<T, Error>;

fn convert_to_y4m<FRAME, FMT>(frame: &FRAME) -> Result<y4m_writer::Y4MFrame>
where
    FRAME: ImageStride<FMT>,
    FMT: PixelFormat,
{
    let out_colorspace = y4m::Colorspace::C420paldv;
    let forced_block_size = Some(16);
    let y4m = y4m_writer::encode_y4m_frame(frame, out_colorspace, forced_block_size)?;
    Ok(y4m)
}

fn gen_y4m_ref(y4m: &y4m_writer::Y4MFrame) -> Result<YCbCrImage<'_>> {
    let width = y4m.width.try_into().unwrap();
    let height = y4m.height.try_into().unwrap();

    let y_plane = DataPlane {
        data: y4m.y_plane_data(),
        stride: y4m.y_stride(),
        bit_depth: less_avc::BitDepth::Depth8,
    };

    let planes = if y4m.is_known_mono_only() {
        Planes::Mono(y_plane)
    } else {
        let u_plane = DataPlane {
            data: y4m.u_plane_data(),
            stride: y4m.u_stride(),
            bit_depth: less_avc::BitDepth::Depth8,
        };
        let v_plane = DataPlane {
            data: y4m.v_plane_data(),
            stride: y4m.v_stride(),
            bit_depth: less_avc::BitDepth::Depth8,
        };
        Planes::YCbCr((y_plane, u_plane, v_plane))
    };
    let im = YCbCrImage {
        planes,
        width,
        height,
    };

    Ok(im)
}

#[derive(Default)]
pub struct WrappedLessEncoder {
    inner: Option<less_avc::LessEncoder>,
}

impl WrappedLessEncoder {
    pub fn encode_to_nal_units<FRAME, FMT>(&mut self, frame: &FRAME) -> Result<Vec<Vec<u8>>>
    where
        FRAME: ImageStride<FMT>,
        FMT: PixelFormat,
    {
        let y4m = convert_to_y4m(frame)?;
        let y4m_ref = gen_y4m_ref(&y4m)?;

        let (nals, encoder) = match self.inner.take() {
            None => {
                let (nal_units, encoder) = less_avc::LessEncoder::new(&y4m_ref)?;
                let nals: Vec<Vec<u8>> = nal_units
                    .into_iter()
                    .map(|nal_unit| nal_unit.to_nal_unit())
                    .collect();
                (nals, encoder)
            }
            Some(mut encoder) => {
                let nal_unit = encoder.encode(&y4m_ref)?;
                (vec![nal_unit.to_nal_unit()], encoder)
            }
        };

        self.inner = Some(encoder);

        Ok(nals)
    }

    pub fn encode_dynamic_to_nal_units(
        &mut self,
        frame: &basic_frame::DynamicFrame,
    ) -> Result<Vec<Vec<u8>>> {
        basic_frame::match_all_dynamic_fmts!(frame, f, { self.encode_to_nal_units(f) })
    }
}

pub struct H264WriterWrapper<W> {
    inner: less_avc::H264Writer<W>,
}

impl<W: Write> H264WriterWrapper<W> {
    pub fn new(wtr: W) -> Result<Self> {
        Ok(Self {
            inner: less_avc::H264Writer::new(wtr)?,
        })
    }

    pub fn write_dynamic(&mut self, frame: &DynamicFrame) -> Result<()> {
        match_all_dynamic_fmts!(frame, x, {
            self.write(x)?;
        });
        Ok(())
    }

    pub fn write<IM, FMT>(&mut self, frame: &IM) -> Result<()>
    where
        IM: ImageStride<FMT>,
        FMT: PixelFormat,
    {
        let y4m = convert_to_y4m(frame)?;
        let y4m_ref = gen_y4m_ref(&y4m)?;
        self.inner.write(&y4m_ref)?;
        Ok(())
    }
}
