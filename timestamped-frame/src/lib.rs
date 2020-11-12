use machine_vision_formats::{ImageData,Stride};

/// Has host framenumber and timestamps
pub trait HostTimeData {
    /// the number of the image (in order)
    fn host_framenumber(&self) -> usize;  // TODO: make a new trait with this. TODO: rename to just "framenumber".
    /// the time the image was acquired on the host machine)
    fn host_timestamp(&self) -> chrono::DateTime<chrono::Utc>;  // TODO: make a new trait with this
}

// ------------------------------- compound traits ----------------------

/// An image with timestamps and a stride.
pub trait ImageStrideTime: HostTimeData + ImageData + Stride {}
impl<S: HostTimeData + ImageData + Stride> ImageStrideTime for S {}

/// An image with timestamps and a stride which can be moved into `Vec<u8>`.
pub trait FrameTrait: AsImageStrideTime + ImageStrideTime + Into<Vec<u8>> {}
impl<S> FrameTrait for S
    where
        S: AsImageStrideTime + ImageStrideTime,
        Vec<u8>: From<S>
{}

/// Can be converted into `ImageStrideTime`.
pub trait AsImageStrideTime: ImageStrideTime {
    fn as_image_stride_time(&self) -> &dyn ImageStrideTime;
}
impl<S: ImageStrideTime> AsImageStrideTime for S {
    fn as_image_stride_time(&self) -> &dyn ImageStrideTime {
        self
    }
}
