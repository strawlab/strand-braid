use crate::*;

use basic_frame::BasicExtra;
use timestamped_frame::ExtraTimeData;

#[derive(Clone)]
pub(crate) struct BorrowedFrame<'a, FMT>
where
    FMT: Clone,
{
    buffer_ref: ImageBufferRef<'a, FMT>,
    width: u32,
    height: u32,
    stride: usize,
    extra: BasicExtra,
}

impl<'a, FMT> formats::Stride for BorrowedFrame<'a, FMT>
where
    FMT: Clone,
{
    fn stride(&self) -> usize {
        self.stride
    }
}

impl<'a, FMT> formats::ImageData<FMT> for BorrowedFrame<'a, FMT>
where
    FMT: Clone,
{
    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.height
    }
    fn buffer_ref(&self) -> ImageBufferRef<FMT> {
        self.buffer_ref.clone()
    }
    fn buffer(self) -> ImageBuffer<FMT> {
        ImageBuffer {
            data: self.buffer_ref.data.to_vec(),
            pixel_format: self.buffer_ref().pixel_format,
        }
    }
}

impl<'a, FMT: Clone> ExtraTimeData for BorrowedFrame<'a, FMT> {
    fn extra<'b>(&'b self) -> &'b dyn HostTimeData {
        &self.extra
    }
}

// impl<'a, FMT> HostTimeData for BorrowedFrame<'a, FMT>
// where
//     FMT: Clone + Send,
// {
//     fn host_timestamp(&self) -> chrono::DateTime<chrono::Utc> {
//         self.host_timestamp
//     }
//     fn host_framenumber(&self) -> usize {
//         self.host_framenumber
//     }
// }

pub(crate) fn borrow_fi<'a, C, D, FMT>(
    fid: &'a fastimage::FastImageData<C, D>,
    host_timestamp: chrono::DateTime<chrono::Utc>,
    host_framenumber: usize,
) -> Result<BorrowedFrame<'a, FMT>>
where
    C: fastimage::ChanTrait,
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
    let extra = BasicExtra {
        host_timestamp,
        host_framenumber,
    };
    Ok(BorrowedFrame {
        buffer_ref: ImageBufferRef {
            data,
            pixel_format: std::marker::PhantomData,
        },
        width,
        height,
        stride,
        extra,
    })
}
