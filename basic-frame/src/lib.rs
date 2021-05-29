use machine_vision_formats::{
    ImageBuffer, ImageBufferMutRef, ImageBufferRef, ImageData, ImageMutData, Stride,
};
use timestamped_frame::{ExtraTimeData, FrameTrait, HostTimeData, ImageStrideTime};

mod dynamic_frame;
pub use dynamic_frame::DynamicFrame;

/// Convert a BasicFrame into another BasicFrame with a new pixel_format.
#[macro_export]
macro_rules! new_basic_frame {
    ($x:expr) => {{
        BasicFrame {
            width: $x.width,
            height: $x.height,
            stride: $x.stride,
            image_data: $x.image_data,
            extra: $x.extra,
            pixel_format: std::marker::PhantomData,
        }
    }};
}

/// Return an DynamicFrame variant according to $pixfmt.
#[macro_export]
macro_rules! convert_to_dynamic {
    ($pixfmt:expr, $x:expr) => {{
        match $pixfmt {
            PixFmt::Mono8 => DynamicFrame::Mono8(new_basic_frame!($x)),
            PixFmt::Mono32f => DynamicFrame::Mono32f(new_basic_frame!($x)),
            PixFmt::RGB8 => DynamicFrame::RGB8(new_basic_frame!($x)),
            PixFmt::BayerRG8 => DynamicFrame::BayerRG8(new_basic_frame!($x)),
            PixFmt::BayerRG32f => DynamicFrame::BayerRG32f(new_basic_frame!($x)),
            PixFmt::BayerGB8 => DynamicFrame::BayerGB8(new_basic_frame!($x)),
            PixFmt::BayerGB32f => DynamicFrame::BayerGB32f(new_basic_frame!($x)),
            PixFmt::BayerGR8 => DynamicFrame::BayerGR8(new_basic_frame!($x)),
            PixFmt::BayerGR32f => DynamicFrame::BayerGR32f(new_basic_frame!($x)),
            PixFmt::BayerBG8 => DynamicFrame::BayerBG8(new_basic_frame!($x)),
            PixFmt::BayerBG32f => DynamicFrame::BayerBG32f(new_basic_frame!($x)),
            PixFmt::YUV422 => DynamicFrame::YUV422(new_basic_frame!($x)),

            PixFmt::NV12 => DynamicFrame::NV12(new_basic_frame!($x)),
            PixFmt::YUV444 => DynamicFrame::YUV444(new_basic_frame!($x)),
            _ => {
                panic!("unsupported pixel format {}", $pixfmt);
            }
        }
    }};
}

#[macro_export]
macro_rules! match_all_dynamic_fmts {
    ($self:expr, $x:ident, $block:expr) => {
        match $self {
            DynamicFrame::Mono8($x) => $block,
            DynamicFrame::Mono32f($x) => $block,
            DynamicFrame::RGB8($x) => $block,
            DynamicFrame::BayerRG8($x) => $block,
            DynamicFrame::BayerRG32f($x) => $block,
            DynamicFrame::BayerGB8($x) => $block,
            DynamicFrame::BayerGB32f($x) => $block,
            DynamicFrame::BayerGR8($x) => $block,
            DynamicFrame::BayerGR32f($x) => $block,
            DynamicFrame::BayerBG8($x) => $block,
            DynamicFrame::BayerBG32f($x) => $block,
            DynamicFrame::YUV444($x) => $block,
            DynamicFrame::YUV422($x) => $block,
            DynamicFrame::NV12($x) => $block,
        }
    };
}

/// Image data with a statically typed, strided pixel format.
#[derive(Clone)]
pub struct BasicFrame<F> {
    /// width in pixels
    pub width: u32,
    /// height in pixels
    pub height: u32,
    /// number of bytes in an image row
    pub stride: u32,
    /// raw image data
    pub image_data: Vec<u8>,
    /// pixel format
    pub pixel_format: std::marker::PhantomData<F>,
    /// Additional data, including timestamp information.
    pub extra: Box<dyn HostTimeData>,
}

// fn _test_basic_frame_is_send<F: Send>() {
//     // Compile-time test to ensure BasicFrame implements Send trait.
//     fn implements<T: Send>() {}
//     implements::<BasicFrame<F>>();
// }

fn _test_basic_frame_is_frame_trait<F>() {
    // Compile-time test to ensure BasicFrame implements FrameTrait trait.
    fn implements<T: FrameTrait<F>, F>() {}
    implements::<BasicFrame<F>, F>();
}

// fn _test_basic_frame_0<F>() {
//     fn implements<T: Into<Vec<u8>>>() {}
//     implements::<BasicFrame<F>>();
// }

// fn _test_basic_frame_1<F>() {
//     fn implements<T: ImageStrideTime<F>, F>() {}
//     implements::<BasicFrame<F>, F>();
// }

impl<F> BasicFrame<F> {
    pub fn copy_from(frame: &dyn ImageStrideTime<F>) -> BasicFrame<F> {
        let width = frame.width();
        let height = frame.height();
        let stride = frame.stride() as u32;
        let host_timestamp = frame.extra().host_timestamp();
        let host_framenumber = frame.extra().host_framenumber();
        let extra = Box::new(BasicExtra {
            host_timestamp,
            host_framenumber,
        });

        let image_data = frame.image_data().to_vec(); // copy data

        Self {
            width,
            height,
            stride,
            image_data,
            extra,
            pixel_format: std::marker::PhantomData,
        }
    }
}

impl<F> ExtraTimeData for BasicFrame<F> {
    fn extra(&self) -> &dyn HostTimeData {
        self.extra.as_ref()
    }
}

#[derive(Clone, Debug)]
pub struct BasicExtra {
    /// timestamp from host computer
    pub host_timestamp: chrono::DateTime<chrono::Utc>,
    /// framenumber from host computer
    pub host_framenumber: usize,
}

impl HostTimeData for BasicExtra {
    fn host_timestamp(&self) -> chrono::DateTime<chrono::Utc> {
        self.host_timestamp
    }
    fn host_framenumber(&self) -> usize {
        self.host_framenumber
    }
}

impl<F> ImageData<F> for BasicFrame<F> {
    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.height
    }
    fn buffer_ref(&self) -> ImageBufferRef<'_, F> {
        ImageBufferRef::new(&self.image_data)
    }
    fn buffer(self) -> ImageBuffer<F> {
        ImageBuffer::new(self.image_data)
    }
}

impl<F> ImageMutData<F> for BasicFrame<F> {
    fn buffer_mut_ref(&mut self) -> ImageBufferMutRef<'_, F> {
        ImageBufferMutRef::new(&mut self.image_data)
    }
}

impl<F> Stride for BasicFrame<F> {
    fn stride(&self) -> usize {
        self.stride as usize
    }
}

impl<F> From<BasicFrame<F>> for Vec<u8> {
    fn from(orig: BasicFrame<F>) -> Vec<u8> {
        orig.image_data
    }
}

impl<F> From<Box<BasicFrame<F>>> for Vec<u8> {
    fn from(orig: Box<BasicFrame<F>>) -> Vec<u8> {
        orig.image_data
    }
}

// impl<FRAME, FMT, EXTRA> From<Box<FRAME>> for BasicFrame<FMT, EXTRA>
// where
//     FRAME: FrameTrait<FMT, EXTRA>,
//     EXTRA: HostTimeData,
//     Vec<u8>: From<Box<FRAME>>,
// {
//     fn from(frame: Box<FRAME>) -> BasicFrame<FMT, EXTRA> {
//         assert_eq!(machine_vision_formats::pixel_format::pixfmt::<FMT>().unwrap(), frame.pix_fmt());
//         let width = frame.width();
//         let height = frame.height();
//         let stride = frame.stride() as u32;
//         let (image_data, extra) = frame.into_data_extra();

//         BasicFrame {
//             width,
//             height,
//             stride,
//             image_data,
//             pixel_format: std::marker::PhantomData,
//             extra,
//         }
//     }
// }
