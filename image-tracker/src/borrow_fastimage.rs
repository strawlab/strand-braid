use crate::*;

pub(crate) struct BorrowedFrame<'a> {
    data: &'a [u8],
    width: u32,
    height: u32,
    stride: usize,
    host_timestamp: chrono::DateTime<chrono::Utc>,
    host_framenumber: usize,
    pixel_format: formats::PixelFormat,
}

impl<'a> formats::Stride for BorrowedFrame<'a> {
    fn stride(&self) -> usize {
        self.stride
    }
}

impl<'a> formats::ImageData for BorrowedFrame<'a> {
    fn image_data(&self) -> &[u8] {
        self.data
    }
    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.height
    }
    fn pixel_format(&self) -> formats::PixelFormat {
        self.pixel_format
    }
}

impl<'a> HostTimeData for BorrowedFrame<'a> {
    fn host_timestamp(&self) -> chrono::DateTime<chrono::Utc> {
        self.host_timestamp
    }
    fn host_framenumber(&self) -> usize {
        self.host_framenumber
    }
}

pub(crate) fn borrow_fi<'a, C, D>(
    fid: &'a fastimage::FastImageData<C, D>,
    host_timestamp: chrono::DateTime<chrono::Utc>,
    host_framenumber: usize,
    pixel_format: formats::PixelFormat,
) -> Result<BorrowedFrame<'a>>
where
    C: fastimage::ChanTrait,
    D: Copy + num_traits::Zero + PartialEq,
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
        data,
        width,
        height,
        stride,
        host_timestamp,
        host_framenumber,
        pixel_format,
    })
}
