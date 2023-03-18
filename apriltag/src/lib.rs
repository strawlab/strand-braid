use std::{convert::TryInto, os::raw::c_char};

use machine_vision_formats::pixel_format::Mono8;

/// Associates array pointer destroy function to a Zarray.
pub trait ArrayDealloc {
    // Call `apriltag_x_destroy()` for the correct array type.
    fn array_dealloc(zarray_ptr: *mut apriltag_sys::zarray);
}

/// An array of a single type.
#[repr(transparent)]
pub struct Zarray<T: ArrayDealloc> {
    inner: *mut apriltag_sys::zarray_t,
    marker: std::marker::PhantomData<T>,
}

impl<T: ArrayDealloc> Zarray<T> {
    unsafe fn from_raw(inner: *mut apriltag_sys::zarray_t) -> Zarray<T> {
        assert!(!inner.is_null());
        assert!((*inner).el_sz == std::mem::size_of::<T>().try_into().unwrap());

        Self {
            inner,
            marker: std::marker::PhantomData,
        }
    }

    /// Return the length of the array.
    pub fn len(&self) -> usize {
        unsafe {
            let ptr = *self.inner;
            ptr.size.try_into().unwrap()
        }
    }

    /// Return a slice viewing the array.
    pub fn as_slice(&self) -> &[T] {
        unsafe {
            let ptr = *self.inner;
            std::slice::from_raw_parts(ptr.data as *mut T, self.len())
        }
    }
}

impl<T: ArrayDealloc> Drop for Zarray<T> {
    fn drop(&mut self) {
        if !self.inner.is_null() {
            // This calls `apriltag_x_destroy()` for the correct array type.
            T::array_dealloc(self.inner);
            self.inner = std::ptr::null::<apriltag_sys::zarray_t>() as *mut _;
        }
    }
}

pub trait ImageU8 {
    fn inner(&self) -> &apriltag_sys::image_u8;

    fn inner_mut(&mut self) -> &mut apriltag_sys::image_u8;

    fn data(&self) -> &[u8];
    fn data_mut(&mut self) -> &mut [u8];

    fn width(&self) -> i32 {
        self.inner().width
    }

    fn height(&self) -> i32 {
        self.inner().height
    }

    fn stride(&self) -> i32 {
        self.inner().stride
    }
}

pub struct ImageU8Owned {
    inner: apriltag_sys::image_u8,
    data: Vec<u8>,
}

impl ImageU8Owned {
    pub fn new(width: i32, height: i32, stride: i32, mut data: Vec<u8>) -> Option<Self> {
        let min_size = (height as usize - 1) * stride as usize + width as usize;
        if data.len() >= min_size {
            let inner = apriltag_sys::image_u8 {
                width,
                height,
                stride,
                buf: data.as_mut_ptr(),
            };

            Some(Self { inner, data })
        } else {
            None
        }
    }
}

impl Into<Vec<u8>> for ImageU8Owned {
    fn into(self) -> Vec<u8> {
        self.data
    }
}

impl ImageU8 for ImageU8Owned {
    fn inner(&self) -> &apriltag_sys::image_u8 {
        &self.inner
    }

    fn inner_mut(&mut self) -> &mut apriltag_sys::image_u8 {
        &mut self.inner
    }

    fn data(&self) -> &[u8] {
        &self.data
    }

    fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }
}

pub struct ImageU8Borrowed<'a> {
    inner: apriltag_sys::image_u8,
    data_lifetime: std::marker::PhantomData<&'a [u8]>,
}

impl<'a> ImageU8Borrowed<'a> {
    pub fn new(width: i32, height: i32, stride: i32, data: &'a [u8]) -> Option<Self> {
        let min_size = (height as usize - 1) * stride as usize + width as usize;
        if data.len() >= min_size {
            let inner = apriltag_sys::image_u8 {
                width,
                height,
                stride,
                buf: data.as_ptr() as *mut u8,
            };

            Some(Self {
                inner,
                data_lifetime: std::marker::PhantomData,
            })
        } else {
            None
        }
    }
    pub fn view(im: &dyn machine_vision_formats::AsImageStride<Mono8>) -> Self {
        let inner = apriltag_sys::image_u8 {
            width: im.width().try_into().unwrap(),
            height: im.height().try_into().unwrap(),
            stride: im.stride().try_into().unwrap(),
            buf: im.buffer_ref().data.as_ptr() as *mut u8,
        };

        Self {
            inner,
            data_lifetime: std::marker::PhantomData,
        }
    }
}

impl<'a> ImageU8 for ImageU8Borrowed<'a> {
    fn inner(&self) -> &apriltag_sys::image_u8 {
        &self.inner
    }

    fn inner_mut(&mut self) -> &mut apriltag_sys::image_u8 {
        &mut self.inner
    }

    fn data(&self) -> &[u8] {
        let len = self.inner.height as usize * self.inner.stride as usize;
        unsafe { std::slice::from_raw_parts(self.inner.buf, len) }
    }

    fn data_mut(&mut self) -> &mut [u8] {
        let len = self.inner.height as usize * self.inner.stride as usize;
        unsafe { std::slice::from_raw_parts_mut(self.inner.buf, len) }
    }
}

/// The main type for detecting tags
#[derive(Debug)]
pub struct Detector {
    td: *mut apriltag_sys::apriltag_detector,
    families: Vec<Family>,
}

unsafe impl Send for Detector {}

impl Detector {
    /// Constructor
    pub fn new() -> Self {
        let td: *mut apriltag_sys::apriltag_detector =
            unsafe { apriltag_sys::apriltag_detector_create() };
        Self {
            td,
            families: vec![],
        }
    }

    /// Add a tag family
    ///
    /// We take ownership of the family to keep its lifetime.
    pub fn add_family(&mut self, family: Family) {
        // In theory, we could add a phantomdata type to detector with the
        // lifetime of the families, but I'm not sure how we could do that with
        // multiple families with potentially different lifetimes. Furthermore,
        // that would make the type signature for Detector more complicated.
        self.add_family_bits(family, 2)
    }

    /// Add a tag family
    ///
    /// We take ownership of the family to keep its lifetime.
    pub fn add_family_bits(&mut self, family: Family, bits: i32) {
        unsafe { apriltag_sys::apriltag_detector_add_family_bits(self.td, family.0, bits) };
        self.families.push(family)
    }

    /// Remove all tag families
    pub fn clear_families(&mut self) {
        unsafe { apriltag_sys::apriltag_detector_clear_families(self.td) };
        self.families.clear()
    }

    /// Detect points in an image
    pub fn detect(&self, im: &apriltag_sys::image_u8) -> Zarray<Detection> {
        let detections: *mut apriltag_sys::zarray_t = unsafe {
            let ptr = &*im as *const apriltag_sys::image_u8;
            apriltag_sys::apriltag_detector_detect(self.td, ptr as *mut _)
        };
        unsafe { Zarray::from_raw(detections) }
    }
}

impl Drop for Detector {
    fn drop(&mut self) {
        if !self.td.is_null() {
            unsafe { apriltag_sys::apriltag_detector_destroy(self.td) };
            self.td = std::ptr::null::<apriltag_sys::apriltag_detector>() as *mut _;
        }
    }
}

impl std::convert::AsMut<apriltag_sys::apriltag_detector> for Detector {
    fn as_mut(&mut self) -> &mut apriltag_sys::apriltag_detector {
        unsafe { &mut *self.td }
    }
}

#[repr(transparent)]
#[derive(Debug)]
pub struct Family(*mut apriltag_sys::apriltag_family_t);

unsafe impl Send for Family {}

impl Family {
    /// Create a new detector family for 16h5 tags.
    pub fn new_tag_16h5() -> Self {
        let tf: *mut apriltag_sys::apriltag_family_t = unsafe { apriltag_sys::tag16h5_create() };
        Self(tf)
    }

    /// Create a new detector family for 25h9 tags.
    pub fn new_tag_25h9() -> Self {
        let tf: *mut apriltag_sys::apriltag_family_t = unsafe { apriltag_sys::tag25h9_create() };
        Self(tf)
    }

    /// Create a new detector family for 36h11 tags.
    pub fn new_tag_36h11() -> Self {
        let tf: *mut apriltag_sys::apriltag_family_t = unsafe { apriltag_sys::tag36h11_create() };
        Self(tf)
    }

    /// Create a new detector family for Circle21h7 tags.
    pub fn new_tag_circle_21h7() -> Self {
        let tf: *mut apriltag_sys::apriltag_family_t =
            unsafe { apriltag_sys::tagCircle21h7_create() };
        Self(tf)
    }

    /// Create a new detector family for Circle49h12 tags.
    pub fn new_tag_circle_49h12() -> Self {
        let tf: *mut apriltag_sys::apriltag_family_t =
            unsafe { apriltag_sys::tagCircle49h12_create() };
        Self(tf)
    }

    /// Create a new detector family for Custom48h12 tags.
    pub fn new_tag_custom_48h12() -> Self {
        let tf: *mut apriltag_sys::apriltag_family_t =
            unsafe { apriltag_sys::tagCustom48h12_create() };
        Self(tf)
    }

    /// Create a new detector family for standard 41h12 tags.
    pub fn new_tag_standard_41h12() -> Self {
        let tf: *mut apriltag_sys::apriltag_family_t =
            unsafe { apriltag_sys::tagStandard41h12_create() };
        Self(tf)
    }

    /// Create a new detector family for standard 52h13 tags.
    pub fn new_tag_standard_52h13() -> Self {
        let tf: *mut apriltag_sys::apriltag_family_t =
            unsafe { apriltag_sys::tagStandard52h13_create() };
        Self(tf)
    }

    pub fn family_type(&self) -> FamilyType {
        let name = unsafe { (*self.0).name };
        FamilyType::from_name(name)
    }
}

impl Drop for Family {
    fn drop(&mut self) {
        if !self.0.is_null() {
            use FamilyType::*;
            match self.family_type() {
                Family16H5 => unsafe { apriltag_sys::tag16h5_destroy(self.0) },
                Family25H9 => unsafe { apriltag_sys::tag25h9_destroy(self.0) },
                Family36H11 => unsafe { apriltag_sys::tag36h11_destroy(self.0) },
                FamilyCircle21h7 => unsafe { apriltag_sys::tagCircle21h7_destroy(self.0) },
                FamilyCircle49H12 => unsafe { apriltag_sys::tagCircle49h12_destroy(self.0) },
                FamilyCustom48h12 => unsafe { apriltag_sys::tagCustom48h12_destroy(self.0) },
                Family41H12 => unsafe { apriltag_sys::tagStandard41h12_destroy(self.0) },
                Family52H13 => unsafe { apriltag_sys::tagStandard52h13_destroy(self.0) },
            }
            self.0 = std::ptr::null::<apriltag_sys::apriltag_family_t>() as *mut _;
        }
    }
}

#[derive(Debug)]
pub enum FamilyType {
    Family16H5,
    Family25H9,
    Family36H11,
    FamilyCircle21h7,
    FamilyCircle49H12,
    FamilyCustom48h12,
    Family41H12,
    Family52H13,
}

impl FamilyType {
    fn from_name(name: *mut c_char) -> Self {
        let slice = unsafe { std::ffi::CStr::from_ptr(name) };

        use FamilyType::*;
        match slice.to_bytes() {
            b"tag16h5" => Family16H5,
            b"tag25h9" => Family25H9,
            b"tag36h11" => Family36H11,
            b"tagCircle21h7" => FamilyCircle21h7,
            b"tagCircle49h12" => FamilyCircle49H12,
            b"tagCustom48h12" => FamilyCustom48h12,
            b"tagStandard41h12" => Family41H12,
            b"tagStandard52h13" => Family52H13,
            other => match std::str::from_utf8(other) {
                Ok(name) => panic!("unknown tag: {}", name),
                Err(_) => panic!("unknown non-utf8 tag: {:?}", other),
            },
        }
    }

    pub fn to_str(&self) -> &str {
        use FamilyType::*;
        match self {
            Family16H5 => "tag16h5",
            Family25H9 => "tag25h9",
            Family36H11 => "tag36h11",
            FamilyCircle21h7 => "tagCircle21h7",
            FamilyCircle49H12 => "tagCircle49h12",
            FamilyCustom48h12 => "tagCustom48h12",
            Family41H12 => "tagStandard41h12",
            Family52H13 => "tagStandard52h13",
        }
    }
}

#[repr(transparent)]
pub struct Detection(*mut apriltag_sys::apriltag_detection_t);

impl Detection {
    pub fn id(&self) -> i32 {
        unsafe { (*self.0).id }
    }
    pub fn family_type(&self) -> FamilyType {
        let fam_ptr = unsafe { (*self.0).family };
        let name = unsafe { (*fam_ptr).name };
        FamilyType::from_name(name)
    }
    pub fn hamming(&self) -> i32 {
        unsafe { (*self.0).hamming }
    }
    pub fn decision_margin(&self) -> f32 {
        unsafe { (*self.0).decision_margin }
    }
    pub fn h(&self) -> &[f64] {
        unsafe { (*(*self.0).H).data.as_slice(9) }
    }
    pub fn center(&self) -> &[f64] {
        unsafe { &(*self.0).c }
    }
}

impl std::fmt::Debug for Detection {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let d = unsafe { *self.0 };
        write!(
            f,
            "Detection{{id: {}, hamming: {}, decision_margin: {}, c: {:?}, p: {:?}}}",
            d.id, d.hamming, d.decision_margin, d.c, d.p
        )
    }
}

impl ArrayDealloc for Detection {
    fn array_dealloc(zarray_ptr: *mut apriltag_sys::zarray) {
        unsafe { apriltag_sys::apriltag_detections_destroy(zarray_ptr) };
    }
}

#[cfg(test)]
mod test {
    use crate::*;

    #[test]
    fn family_types() {
        let f = Family::new_tag_16h5();
        let ft = f.family_type();
        let s = ft.to_str();
        assert_eq!(s, "tag16h5");

        let f = Family::new_tag_25h9();
        let ft = f.family_type();
        let s = ft.to_str();
        assert_eq!(s, "tag25h9");

        let f = Family::new_tag_36h11();
        let ft = f.family_type();
        let s = ft.to_str();
        assert_eq!(s, "tag36h11");

        let f = Family::new_tag_circle_21h7();
        let ft = f.family_type();
        let s = ft.to_str();
        assert_eq!(s, "tagCircle21h7");

        let f = Family::new_tag_circle_49h12();
        let ft = f.family_type();
        let s = ft.to_str();
        assert_eq!(s, "tagCircle49h12");

        let f = Family::new_tag_custom_48h12();
        let ft = f.family_type();
        let s = ft.to_str();
        assert_eq!(s, "tagCustom48h12");

        let f = Family::new_tag_standard_41h12();
        let ft = f.family_type();
        let s = ft.to_str();
        assert_eq!(s, "tagStandard41h12");

        let f = Family::new_tag_standard_52h13();
        let ft = f.family_type();
        let s = ft.to_str();
        assert_eq!(s, "tagStandard52h13");
    }
}
