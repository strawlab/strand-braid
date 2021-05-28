use std::any::Any;

use dyn_clone::DynClone;

use machine_vision_formats::{ImageData, Stride};

/// Has host framenumber and timestamps
pub trait HostTimeData: DynClone + Send + AsAny {
    /// the number of the image (in order)
    fn host_framenumber(&self) -> usize; // TODO: make a new trait with this. TODO: rename to just "framenumber".
    /// the time the image was acquired on the host machine)
    fn host_timestamp(&self) -> chrono::DateTime<chrono::Utc>; // TODO: make a new trait with this
}

// implement Clone for HostTimeData
dyn_clone::clone_trait_object!(HostTimeData);

// see https://users.rust-lang.org/t/calling-any-downcast-ref-requires-static/52071
pub trait AsAny {
    fn as_any(&self) -> &dyn Any;
}
impl<T: Any> AsAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub trait ExtraTimeData {
    fn extra(&self) -> &dyn HostTimeData;
}

// ------------------------------- compound traits ----------------------

/// An image with timestamps and a stride.
pub trait ImageStrideTime<F>: ExtraTimeData + ImageData<F> + Stride {}
impl<S: ExtraTimeData + ImageData<F> + Stride, F> ImageStrideTime<F> for S {}

/// An image with timestamps and a stride which can be moved into `Vec<u8>`.
pub trait FrameTrait<F>: AsImageStrideTime<F> + ImageStrideTime<F> + Into<Vec<u8>> {}

impl<S, F> FrameTrait<F> for S
where
    S: AsImageStrideTime<F> + ImageStrideTime<F>,
    Vec<u8>: From<S>,
{
}

/// Can be converted into `ImageStrideTime`.
pub trait AsImageStrideTime<F>: ImageStrideTime<F> {
    fn as_image_stride_time(&self) -> &dyn ImageStrideTime<F>;
    // where
    //     Self: Sized;
}
impl<S: ImageStrideTime<F>, F> AsImageStrideTime<F> for S {
    fn as_image_stride_time(&self) -> &dyn ImageStrideTime<F> {
        self
    }
}
