extern crate machine_vision_formats as formats;

use tracing::error;

use formats::PixFmt;

pub static MONO8_VERTEX_SRC: &'static str = include_str!("mono8_vertex.glsl");
pub static MONO8_FRAGMENT_SRC: &'static str = include_str!("mono8_fragment.glsl");

pub static RGB8_VERTEX_SRC: &'static str = include_str!("rgb8_vertex.glsl");
pub static RGB8_FRAGMENT_SRC: &'static str = include_str!("rgb8_fragment.glsl");

pub static DEMOSAIC_VERTEX_SRC: &'static str = include_str!("demosaic_vertex.glsl");
pub static DEMOSAIC_FRAGMENT_SRC: &'static str = include_str!("demosaic_fragment.glsl");

pub struct DemosaicInfo {
    pub source_size: [f32; 4],
    pub first_red: [f32; 2],
}

// Uniform Type
pub enum UniformType {
    Mono8,
    Bayer(DemosaicInfo),
    Rgb8,
}

/// Internal format of the OpenGL texture expected
#[derive(Debug)]
pub enum InternalFormat {
    Raw8,
    Rgb8,
}

pub fn get_programs(
    width: u32,
    height: u32,
    pixel_format: PixFmt,
) -> (UniformType, &'static str, &'static str, InternalFormat) {
    let w = width as f32;
    let h = height as f32;
    let source_size = [w, h, 1.0 / w, 1.0 / h];
    let (uni_ty, vert_src, frag_src, ifmt) = match pixel_format {
        PixFmt::Mono8 => (
            UniformType::Mono8,
            MONO8_VERTEX_SRC,
            MONO8_FRAGMENT_SRC,
            InternalFormat::Raw8,
        ),
        PixFmt::BayerBG8 => {
            let di = DemosaicInfo {
                source_size: source_size,
                first_red: [0.0, 0.0],
            };
            (
                UniformType::Bayer(di),
                DEMOSAIC_VERTEX_SRC,
                DEMOSAIC_FRAGMENT_SRC,
                InternalFormat::Raw8,
            )
        }
        PixFmt::BayerRG8 => {
            let di = DemosaicInfo {
                source_size: source_size,
                first_red: [1.0, 1.0],
            };
            (
                UniformType::Bayer(di),
                DEMOSAIC_VERTEX_SRC,
                DEMOSAIC_FRAGMENT_SRC,
                InternalFormat::Raw8,
            )
        }

        PixFmt::BayerGR8 => {
            let di = DemosaicInfo {
                source_size: source_size,
                first_red: [0.0, 1.0],
            };
            (
                UniformType::Bayer(di),
                DEMOSAIC_VERTEX_SRC,
                DEMOSAIC_FRAGMENT_SRC,
                InternalFormat::Raw8,
            )
        }
        PixFmt::BayerGB8 => {
            let di = DemosaicInfo {
                source_size: source_size,
                first_red: [1.0, 0.0],
            };
            (
                UniformType::Bayer(di),
                DEMOSAIC_VERTEX_SRC,
                DEMOSAIC_FRAGMENT_SRC,
                InternalFormat::Raw8,
            )
        }
        PixFmt::RGB8 => (
            UniformType::Rgb8,
            RGB8_VERTEX_SRC,
            RGB8_FRAGMENT_SRC,
            InternalFormat::Rgb8,
        ),
        ref e => {
            error!("do not know how to decode {:?}, using MONO8 decoder", e);
            (
                UniformType::Mono8,
                MONO8_VERTEX_SRC,
                MONO8_FRAGMENT_SRC,
                InternalFormat::Raw8,
            )
        }
    };
    (uni_ty, vert_src, frag_src, ifmt)
}
