use formats::{ImageData, PixFmt, PixelFormat, Stride};
use machine_vision_formats as formats;

use timestamped_frame::{ExtraTimeData, HostTimeData};

use crate::{convert_to_dynamic, match_all_dynamic_fmts, new_basic_frame, BasicExtra, BasicFrame};

macro_rules! new_basic_frame_copy {
    ($x:expr) => {{
        let extra = Box::new(BasicExtra {
            host_timestamp: $x.extra().host_timestamp(),
            host_framenumber: $x.extra().host_framenumber(),
        });
        BasicFrame {
            width: $x.width(),
            height: $x.height(),
            stride: $x.stride() as u32,
            image_data: $x.image_data().to_vec(),
            extra,
            pixel_format: std::marker::PhantomData,
        }
    }};
}

macro_rules! new_basic_frame_move {
    ($x:expr) => {{
        let extra = Box::new(BasicExtra {
            host_timestamp: $x.extra().host_timestamp(),
            host_framenumber: $x.extra().host_framenumber(),
        });
        let width = $x.width();
        let height = $x.height();
        let stride = $x.stride() as u32;
        let image_data: Vec<u8> = $x.into();
        BasicFrame {
            width,
            height,
            stride,
            image_data,
            extra,
            pixel_format: std::marker::PhantomData,
        }
    }};
}

macro_rules! convert_to_dynamic2 {
    ($format_type:ty, $x:expr) => {{
        let pixfmt = formats::pixel_format::pixfmt::<$format_type>().unwrap();
        match pixfmt {
            PixFmt::Mono8 => DynamicFrame::Mono8($x),
            PixFmt::Mono32f => DynamicFrame::Mono32f($x),
            PixFmt::RGB8 => DynamicFrame::RGB8($x),
            PixFmt::BayerRG8 => DynamicFrame::BayerRG8($x),
            PixFmt::BayerRG32f => DynamicFrame::BayerRG32f($x),
            PixFmt::BayerGB8 => DynamicFrame::BayerGB8($x),
            PixFmt::BayerGB32f => DynamicFrame::BayerGB32f($x),
            PixFmt::BayerGR8 => DynamicFrame::BayerGR8($x),
            PixFmt::BayerGR32f => DynamicFrame::BayerGR32f($x),
            PixFmt::BayerBG8 => DynamicFrame::BayerBG8($x),
            PixFmt::BayerBG32f => DynamicFrame::BayerBG32f($x),
            PixFmt::YUV422 => DynamicFrame::YUV422($x),
            _ => {
                panic!("unsupported type {}", pixfmt);
            }
        }
    }};
}

/// An image whose pixel format is known only at runtime.
///
/// When reading an image from disk, for example, its pixel format is not known
/// in advance. This enum represents the different possible formats as different
/// variants.
///
/// Note that we do not implement ImageData<FMT> trait because the pixel format
/// (parameterized by FMT) is not known at compile-time for DynamicFrame.
#[allow(non_camel_case_types)]
#[derive(Clone)]
pub enum DynamicFrame {
    Mono8(BasicFrame<formats::pixel_format::Mono8>),
    Mono32f(BasicFrame<formats::pixel_format::Mono32f>),
    RGB8(BasicFrame<formats::pixel_format::RGB8>),
    BayerRG8(BasicFrame<formats::pixel_format::BayerRG8>),
    BayerRG32f(BasicFrame<formats::pixel_format::BayerRG32f>),
    BayerGB8(BasicFrame<formats::pixel_format::BayerGB8>),
    BayerGB32f(BasicFrame<formats::pixel_format::BayerGB32f>),
    BayerGR8(BasicFrame<formats::pixel_format::BayerGR8>),
    BayerGR32f(BasicFrame<formats::pixel_format::BayerGR32f>),
    BayerBG8(BasicFrame<formats::pixel_format::BayerBG8>),
    BayerBG32f(BasicFrame<formats::pixel_format::BayerBG32f>),
    YUV444(BasicFrame<formats::pixel_format::YUV444>),
    YUV422(BasicFrame<formats::pixel_format::YUV422>),
    NV12(BasicFrame<formats::pixel_format::NV12>),
}

impl DynamicFrame {
    pub fn new(
        width: u32,
        height: u32,
        stride: u32,
        extra: Box<dyn HostTimeData>,
        image_data: Vec<u8>,
        pixel_format: PixFmt,
    ) -> DynamicFrame {
        // First create a variant with likely the wrong type...
        let wrong_type = DynamicFrame::Mono8(BasicFrame {
            width,
            height,
            stride,
            extra,
            image_data,
            pixel_format: std::marker::PhantomData,
        });
        // ...then convert it to the right type.
        wrong_type.force_pixel_format(pixel_format)
    }
    pub fn copy_from<FMT: PixelFormat>(
        frame: &dyn timestamped_frame::ImageStrideTime<FMT>,
    ) -> Self {
        convert_to_dynamic2!(FMT, new_basic_frame_copy!(frame))
    }

    // TODO: actually implement the From trait
    pub fn from<FRAME, FMT>(frame: FRAME) -> Self
    where
        FRAME: timestamped_frame::ImageStrideTime<FMT> + Into<Vec<u8>>,
        FMT: PixelFormat,
    {
        convert_to_dynamic2!(FMT, new_basic_frame_move!(frame))
    }
}

impl DynamicFrame {
    /// Return the image to a `BasicFrame` of the given pixel format.
    ///
    /// This is done by moving the data. No copy is made.
    ///
    /// If the image is a different pixel format than requested, None will be
    /// returned.
    pub fn into_basic<FMT>(self) -> Option<BasicFrame<FMT>>
    where
        FMT: PixelFormat,
    {
        let pixfmt = formats::pixel_format::pixfmt::<FMT>().unwrap();
        if pixfmt == self.pixel_format() {
            let width = self.width();
            let height = self.height();
            let stride = self.stride() as u32;
            let (image_data, extra) = self.into_data_extra();
            Some(BasicFrame {
                width,
                height,
                stride,
                extra,
                image_data,
                pixel_format: std::marker::PhantomData,
            })
        } else {
            None
        }
    }
    pub fn pixel_format(&self) -> PixFmt {
        use DynamicFrame::*;
        match self {
            Mono8(_) => PixFmt::Mono8,
            Mono32f(_) => PixFmt::Mono32f,
            RGB8(_) => PixFmt::RGB8,
            BayerRG8(_) => PixFmt::BayerRG8,
            BayerRG32f(_) => PixFmt::BayerRG32f,
            BayerGB8(_) => PixFmt::BayerGB8,
            BayerGB32f(_) => PixFmt::BayerGB32f,
            BayerGR8(_) => PixFmt::BayerGR8,
            BayerGR32f(_) => PixFmt::BayerGR32f,
            BayerBG8(_) => PixFmt::BayerBG8,
            BayerBG32f(_) => PixFmt::BayerBG32f,
            YUV444(_) => PixFmt::YUV444,
            YUV422(_) => PixFmt::YUV422,
            NV12(_) => PixFmt::NV12,
        }
    }
    /// Force the frame into a new pixel format without altering the data.
    pub fn force_pixel_format(self, pixfmt: PixFmt) -> DynamicFrame {
        match_all_dynamic_fmts!(self, x, { convert_to_dynamic!(pixfmt, x) })
    }
    pub fn width(&self) -> u32 {
        match_all_dynamic_fmts!(self, x, { x.width() })
    }
    pub fn height(&self) -> u32 {
        match_all_dynamic_fmts!(self, x, { x.height() })
    }
    /// Get a view of the image data.
    ///
    /// Note that this discards any type information about the pixel format.
    pub fn image_data_without_format(&self) -> &[u8] {
        match_all_dynamic_fmts!(self, x, &x.image_data)
    }
    pub fn into_data_extra(self) -> (Vec<u8>, Box<dyn HostTimeData>) {
        match_all_dynamic_fmts!(self, x, { (x.image_data, x.extra) })
    }
}

impl ExtraTimeData for DynamicFrame {
    fn extra<'a>(&'a self) -> &'a dyn HostTimeData {
        match_all_dynamic_fmts!(self, x, { x.extra.as_ref() })
    }
}

impl From<DynamicFrame> for Vec<u8> {
    fn from(orig: DynamicFrame) -> Self {
        match_all_dynamic_fmts!(orig, x, { x.image_data })
    }
}

impl Stride for DynamicFrame {
    fn stride(&self) -> usize {
        match_all_dynamic_fmts!(self, x, { x.stride() })
    }
}
