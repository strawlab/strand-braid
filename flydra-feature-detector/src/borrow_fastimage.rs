use crate::{fastim_mod, Result};

use fastim_mod::FastImage;
use machine_vision_formats::{self as formats, ImageBuffer, ImageBufferRef};

#[derive(Clone)]
pub(crate) struct BorrowedFrame<'a, FMT>
where
    FMT: Clone,
{
    buffer_ref: ImageBufferRef<'a, FMT>,
    width: u32,
    height: u32,
    stride: usize,
}

impl<FMT> formats::Stride for BorrowedFrame<'_, FMT>
where
    FMT: Clone,
{
    fn stride(&self) -> usize {
        self.stride
    }
}

impl<FMT> formats::ImageData<FMT> for BorrowedFrame<'_, FMT>
where
    FMT: Clone,
{
    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.height
    }
    fn buffer_ref(&self) -> ImageBufferRef<'_, FMT> {
        self.buffer_ref.clone()
    }
    fn buffer(self) -> ImageBuffer<FMT> {
        ImageBuffer {
            data: self.buffer_ref.data.to_vec(),
            pixel_format: self.buffer_ref().pixel_format,
        }
    }
}

pub(crate) fn borrow_fi<C, D, FMT>(
    fid: &fastim_mod::FastImageData<C, D>,
) -> Result<BorrowedFrame<'_, FMT>>
where
    C: fastim_mod::ChanTrait,
    D: Copy + num_traits::Zero + PartialEq,
    FMT: Clone,
{
    let stride = fid.stride() as usize;
    let sz = fid.size();
    let width = cast::u32(sz.width())?;
    let height = cast::u32(sz.height())?;

    // fid.data() returns a &[D] but I couldn't find a way to view that as &[u8]
    // without getting a lifetime error. Therefore, I did this approach.
    // (Specifically, I tried https://stackoverflow.com/a/29042896/1633026 made
    // generic over D instead of i32.)
    let data = unsafe {
        let ptr: *const D = fid.raw_ptr();
        std::slice::from_raw_parts(ptr as *const u8, stride * height as usize)
    };
    Ok(BorrowedFrame {
        buffer_ref: ImageBufferRef {
            data,
            pixel_format: std::marker::PhantomData,
        },
        width,
        height,
        stride,
    })
}
