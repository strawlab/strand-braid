extern crate libdc1394_sys;
#[macro_use]
extern crate log;
extern crate failure;
#[macro_use]
extern crate failure_derive;

use libdc1394_sys as ffi;
use std::os::unix::io::{AsRawFd, RawFd};

pub type Result<M> = std::result::Result<M,Error>;

#[derive(Fail, Debug)]
pub enum Error {
    #[fail(display = "DC1394Error {}", code)]
    DC1394Error {
        code: ffi::dc1394error_t::Type,
    },
    #[fail(display = "Dc1394NewFailed")]
    Dc1394NewFailed,
    #[fail(display = "EnumerateCamerasFailed")]
    EnumerateCamerasFailed,
    #[fail(display = "CameraNewFailed")]
    CameraNewFailed,
    #[fail(display = "CaptureDequeueFailed")]
    CaptureDequeueFailed,
    #[fail(display = "RequiresFormat7Mode")]
    RequiresFormat7Mode,
    #[fail(display = "{}", _0)]
    Utf8Error(#[cause] std::str::Utf8Error),
}

macro_rules! dc1394_try {
    ($x:expr) => {
        trace!("calling dc1394_try in {}:{}", file!(), line!());
        match unsafe { $x } {
            ffi::dc1394error_t::DC1394_SUCCESS => {
                trace!("  dc1394_try OK");
            },
            e => {
                trace!("  dc1394_try err");
                return Err(Error::DC1394Error { code: e });
            },
        }
    }
}

// ---------------------------
// GUID

type GUID = u64;

// ---------------------------
// CameraList

pub struct CameraList {
    list: *mut ffi::dc1394camera_list_t,
}

impl CameraList {
    pub fn new(dc1394: &DC1394) -> Result<CameraList> {
        let mut list: *mut ffi::dc1394camera_list_t = std::ptr::null_mut();
        dc1394_try!(ffi::dc1394_camera_enumerate(dc1394.ctx, &mut list));
        if list.is_null() {
            return Err(Error::EnumerateCamerasFailed);
        }
        Ok(CameraList { list: list })
    }

    pub fn as_slice(&self) -> &[ffi::dc1394camera_id_t] {
        unsafe {
            let ptr = (*self.list).ids;
            let amt = (*self.list).num as usize;
            std::slice::from_raw_parts(ptr, amt)
        }
    }
}

impl Drop for CameraList {
    fn drop(&mut self) {
        trace!("calling ffi::dc1394_camera_free_list");
        unsafe {
            ffi::dc1394_camera_free_list(self.list);
        };
    }
}

// ---------------------------
// Camera

pub struct Camera {
    ptr: *mut ffi::dc1394camera_t,
    guid: GUID,
    transmission: ffi::dc1394switch_t::Type,
}

// // Supposedly DC1394 is thread safe...
// // https://damien.douxchamps.net/ieee1394/libdc1394/faq/#Is_it_safe_to_multi_thread_with_libdc1394
unsafe impl Send for Camera {}

fn _test_camera_is_send() {
    // Compile-time test to ensure Camera implements Send trait.
    fn implements<T: Send>() {}
    implements::<Camera>();
}

impl Camera {
    pub fn new(dc1934: &DC1394, guid: &GUID) -> Result<Camera> {
        trace!("calling ffi::dc1394_camera_new for {:?}", guid);
        let ptr: *mut ffi::dc1394camera_t = unsafe { ffi::dc1394_camera_new(dc1934.ctx, *guid) };
        if ptr.is_null() {
            return Err(Error::CameraNewFailed);
        }

        let mut transmission = ffi::dc1394switch_t::DC1394_OFF;
        dc1394_try!(ffi::dc1394_video_get_transmission(ptr, &mut transmission));

        Ok(Camera {
               ptr: ptr,
               guid: *guid,
               transmission: transmission,
           })
    }

    pub fn guid(&self) -> GUID {
        self.guid
    }

    pub fn model(&self) -> Result<String> {
        let val_cstr = unsafe { std::ffi::CStr::from_ptr((*self.ptr).model) };
        Ok(val_cstr.to_str().map_err(|e| Error::Utf8Error(e))?.to_string())
    }

    pub fn vendor(&self) -> Result<String> {
        let val_cstr = unsafe { std::ffi::CStr::from_ptr((*self.ptr).vendor) };
        Ok(val_cstr.to_str().map_err(|e| Error::Utf8Error(e))?.to_string())
    }

    pub fn image_position(&self) -> Result<(u32, u32)> {
        let video_mode: ffi::dc1394video_mode_t::Type = self.video_mode()?;
        if unsafe { ffi::dc1394_is_video_mode_scalable(video_mode) } ==
           ffi::dc1394bool_t::DC1394_TRUE {
            let mut left = 0u32;
            let mut top = 0u32;
            dc1394_try!(ffi::dc1394_format7_get_image_position(self.ptr,
                                                                video_mode,
                                                                &mut left,
                                                                &mut top));
            Ok((left, top))
        } else {
            Ok((0, 0))
        }
    }

    pub fn set_roi(&self, left: u32, top: u32, width: u32, height: u32) -> Result<()> {
        let video_mode: ffi::dc1394video_mode_t::Type = self.video_mode()?;
        if unsafe { ffi::dc1394_is_video_mode_scalable(video_mode) } !=
           ffi::dc1394bool_t::DC1394_TRUE {
            return Err(Error::RequiresFormat7Mode);
        }
        let coding = self.color_coding()?;
        let mut packet_size = 0;
        dc1394_try!(ffi::dc1394_format7_get_packet_size(self.ptr,
                                                            video_mode,
                                                            &mut packet_size));
        dc1394_try!(ffi::dc1394_format7_set_roi(self.ptr,
                                                video_mode,
                                                coding,
                                                packet_size as i32,
                                                left as i32,
                                                top as i32,
                                                width as i32,
                                                height as i32));
        Ok(())
    }

    pub fn image_size(&self) -> Result<(u32, u32)> {
        let video_mode: ffi::dc1394video_mode_t::Type = self.video_mode()?;
        if unsafe { ffi::dc1394_is_video_mode_scalable(video_mode) } ==
           ffi::dc1394bool_t::DC1394_TRUE {
            let mut w = 0u32;
            let mut h = 0u32;
            dc1394_try!(ffi::dc1394_format7_get_image_size(self.ptr,
                                                            video_mode,
                                                            &mut w,
                                                            &mut h));
            Ok((w, h))
        } else {
            let mut w = 0u32;
            let mut h = 0u32;
            dc1394_try!(ffi::dc1394_get_image_size_from_video_mode(self.ptr,
                                                                         video_mode,
                                                                         &mut w,
                                                                         &mut h));
            Ok((w, h))
        }
    }

    pub fn max_image_size(&self) -> Result<(u32, u32)> {
        let video_mode: ffi::dc1394video_mode_t::Type = self.video_mode()?;
        if unsafe { ffi::dc1394_is_video_mode_scalable(video_mode) } ==
           ffi::dc1394bool_t::DC1394_TRUE {
            let mut w = 0u32;
            let mut h = 0u32;
            dc1394_try!(ffi::dc1394_format7_get_max_image_size(self.ptr,
                                                                video_mode,
                                                                &mut w,
                                                                &mut h));
            Ok((w, h))
        } else {
            let mut w = 0u32;
            let mut h = 0u32;
            dc1394_try!(ffi::dc1394_get_image_size_from_video_mode(self.ptr,
                                                                    video_mode,
                                                                    &mut w,
                                                                    &mut h));
            Ok((w, h))
        }
    }
    pub fn color_coding(&self) -> Result<ffi::dc1394color_coding_t::Type> {
        let mut coding = ffi::dc1394color_coding_t::DC1394_COLOR_CODING_MONO8;
        let video_mode: ffi::dc1394video_mode_t::Type = self.video_mode()?;
        if unsafe { ffi::dc1394_is_video_mode_scalable(video_mode) } ==
           ffi::dc1394bool_t::DC1394_TRUE {
            dc1394_try!(ffi::dc1394_format7_get_color_coding(self.ptr,
                                                                video_mode,
                                                                &mut coding));
        } else {
            dc1394_try!(ffi::dc1394_get_color_coding_from_video_mode(self.ptr,
                                                                    video_mode,
                                                                    &mut coding));
        }
        Ok(coding)
    }

    pub fn set_color_coding(&self, coding: ffi::dc1394color_coding_t::Type) -> Result<()> {
        let video_mode: ffi::dc1394video_mode_t::Type = self.video_mode()?;
        if unsafe { ffi::dc1394_is_video_mode_scalable(video_mode) } ==
           ffi::dc1394bool_t::DC1394_TRUE {
            dc1394_try!(ffi::dc1394_format7_set_color_coding(self.ptr, video_mode, coding));
        } else {
            return Err(Error::RequiresFormat7Mode);
        }
        Ok(())
    }

    pub fn color_filter(&self) -> Result<ffi::dc1394color_filter_t::Type> {
        let mut filter = ffi::dc1394color_filter_t::DC1394_COLOR_FILTER_RGGB;
        let video_mode: ffi::dc1394video_mode_t::Type = self.video_mode()?;
        if unsafe { ffi::dc1394_is_video_mode_scalable(video_mode) } ==
           ffi::dc1394bool_t::DC1394_TRUE {
            dc1394_try!(ffi::dc1394_format7_get_color_filter(self.ptr,
                                                            video_mode,
                                                            &mut filter));
        } else {
            return Err(Error::RequiresFormat7Mode);
        }
        Ok(filter)
    }
    pub fn possible_color_codings(&self) -> Result<Vec<ffi::dc1394color_coding_t::Type>> {
        let mut result = Vec::new();
        let video_mode: ffi::dc1394video_mode_t::Type = self.video_mode()?;
        if unsafe { ffi::dc1394_is_video_mode_scalable(video_mode) } ==
           ffi::dc1394bool_t::DC1394_TRUE {
            let mut codings: ffi::dc1394color_codings_t = unsafe{std::mem::zeroed()};
            dc1394_try!(ffi::dc1394_format7_get_color_codings(self.ptr,
                                                                video_mode,
                                                                &mut codings));
            for i in 0..codings.num {
                result.push(codings.codings[i as usize]);
            }
        } else {
            result.push(self.color_coding()?);
        }
        Ok(result)
    }
    pub fn video_mode(&self) -> Result<ffi::dc1394video_mode_t::Type> {
        let mut video_mode = ffi::dc1394video_mode_t::DC1394_VIDEO_MODE_FORMAT7_0;

        dc1394_try!(ffi::dc1394_video_get_mode(self.ptr, &mut video_mode));
        Ok(video_mode)
    }

    pub fn set_video_mode(&self, video_mode: ffi::dc1394video_mode_t::Type) -> Result<()> {
        dc1394_try!(ffi::dc1394_video_set_mode(self.ptr, video_mode));
        Ok(())
    }

    /*
    pub fn format7_mode_info<'a,'b>(&'a self, video_mode: ffi::dc1394video_mode_t, modeset: &'b ffi::dc1394format7modeset_t) -> Result<&'b ffi::dc1394format7mode_t> {
        Ok(modeset.mode[video_mode])
    }
    */

    pub fn video_supported_modes(&self) -> Result<ffi::dc1394video_modes_t> {
        let mut modes = unsafe{std::mem::zeroed()};

        dc1394_try!(ffi::dc1394_video_get_supported_modes(self.ptr, &mut modes));
        Ok(modes)
    }

    pub fn format7_modeset(&self) -> Result<ffi::dc1394format7modeset_t> {

        const DC1394_VIDEO_MODE_FORMAT7_NUM: usize = 8; // magic number found by inspecting source

        let mut result = ffi::dc1394format7modeset_t {
            mode: [unsafe{std::mem::zeroed()}; DC1394_VIDEO_MODE_FORMAT7_NUM],
        };

        dc1394_try!(ffi::dc1394_format7_get_modeset(self.ptr, &mut result));
        Ok(result)
    }

    pub fn transmission(&self) -> ffi::dc1394switch_t::Type {
        self.transmission
    }

    pub fn set_transmission(&mut self, value: ffi::dc1394switch_t::Type) -> Result<()> {
        dc1394_try!(ffi::dc1394_video_set_transmission(self.ptr, value));
        self.transmission = value;
        Ok(())
    }

    pub fn exposure_time(&self) -> Result<f64> {
        let mut result: u32 = 0;
        dc1394_try!(
            ffi::dc1394_feature_get_value(self.ptr, ffi::dc1394feature_t::DC1394_FEATURE_SHUTTER, &mut result));
        Ok(result as f64)
    }
    pub fn exposure_time_range(&self) -> Result<(f64, f64)> {
        let mut min: u32 = 0;
        let mut max: u32 = 0;
        dc1394_try!(
            ffi::dc1394_feature_get_boundaries(self.ptr, ffi::dc1394feature_t::DC1394_FEATURE_SHUTTER, &mut min, &mut max));
        Ok((min as f64, max as f64))
    }
    pub fn set_exposure_time(&mut self, value: f64) -> Result<()> {
        let v2 = value as u32;
        dc1394_try!(
            ffi::dc1394_feature_set_value(self.ptr, ffi::dc1394feature_t::DC1394_FEATURE_SHUTTER, v2));
        Ok(())
    }

    pub fn gain(&self) -> Result<f64> {
        let mut result: u32 = 0;
        dc1394_try!(ffi::dc1394_feature_get_value(self.ptr,
                                                        ffi::dc1394feature_t::DC1394_FEATURE_GAIN,
                                                        &mut result));
        Ok(result as f64)
    }
    pub fn gain_range(&self) -> Result<(f64, f64)> {
        let mut min: u32 = 0;
        let mut max: u32 = 0;
        dc1394_try!(
            ffi::dc1394_feature_get_boundaries(self.ptr, ffi::dc1394feature_t::DC1394_FEATURE_GAIN, &mut min, &mut max));
        Ok((min as f64, max as f64))
    }
    pub fn set_gain(&mut self, value: f64) -> Result<()> {
        let v2 = value as u32;
        dc1394_try!(ffi::dc1394_feature_set_value(self.ptr,
                                                        ffi::dc1394feature_t::DC1394_FEATURE_GAIN,
                                                        v2));
        Ok(())
    }

    pub fn exposure_auto(&self) -> Result<ExposureAuto> {
        let mut mode: ffi::dc1394feature_mode_t::Type =
            ffi::dc1394feature_mode_t::DC1394_FEATURE_MODE_MANUAL;
        dc1394_try!(
            ffi::dc1394_feature_get_mode(self.ptr, ffi::dc1394feature_t::DC1394_FEATURE_SHUTTER, &mut mode));
        let result = match mode {
            ffi::dc1394feature_mode_t::DC1394_FEATURE_MODE_MANUAL => ExposureAuto::Off,
            ffi::dc1394feature_mode_t::DC1394_FEATURE_MODE_AUTO => ExposureAuto::Continuous,
            ffi::dc1394feature_mode_t::DC1394_FEATURE_MODE_ONE_PUSH_AUTO => ExposureAuto::Once,
            e => panic!("invalid enum {}", e),
        };
        Ok(result)
    }
    pub fn set_exposure_auto(&mut self, value: ExposureAuto) -> Result<()> {
        let v2 = match value {
            ExposureAuto::Off => ffi::dc1394feature_mode_t::DC1394_FEATURE_MODE_MANUAL,
            ExposureAuto::Continuous => ffi::dc1394feature_mode_t::DC1394_FEATURE_MODE_AUTO,
            ExposureAuto::Once => ffi::dc1394feature_mode_t::DC1394_FEATURE_MODE_ONE_PUSH_AUTO,
        };
        dc1394_try!(
            ffi::dc1394_feature_set_mode(self.ptr, ffi::dc1394feature_t::DC1394_FEATURE_SHUTTER, v2));
        Ok(())
    }

    pub fn gain_auto(&self) -> Result<GainAuto> {
        let mut mode: ffi::dc1394feature_mode_t::Type =
            ffi::dc1394feature_mode_t::DC1394_FEATURE_MODE_MANUAL;
        dc1394_try!(ffi::dc1394_feature_get_mode(self.ptr,
                                                ffi::dc1394feature_t::DC1394_FEATURE_GAIN,
                                                &mut mode));
        let result = match mode {
            ffi::dc1394feature_mode_t::DC1394_FEATURE_MODE_MANUAL => GainAuto::Off,
            ffi::dc1394feature_mode_t::DC1394_FEATURE_MODE_AUTO => GainAuto::Continuous,
            ffi::dc1394feature_mode_t::DC1394_FEATURE_MODE_ONE_PUSH_AUTO => GainAuto::Once,
            e => panic!("invalid enum {}", e),
        };
        Ok(result)
    }
    pub fn set_gain_auto(&mut self, value: GainAuto) -> Result<()> {
        let v2 = match value {
            GainAuto::Off => ffi::dc1394feature_mode_t::DC1394_FEATURE_MODE_MANUAL,
            GainAuto::Continuous => ffi::dc1394feature_mode_t::DC1394_FEATURE_MODE_AUTO,
            GainAuto::Once => ffi::dc1394feature_mode_t::DC1394_FEATURE_MODE_ONE_PUSH_AUTO,
        };
        dc1394_try!(ffi::dc1394_feature_set_mode(self.ptr,
                                                ffi::dc1394feature_t::DC1394_FEATURE_GAIN,
                                                v2));
        Ok(())
    }

    pub fn trigger_mode(&self) -> Result<TriggerMode> {
        let mut pwr = ffi::dc1394switch_t::DC1394_OFF;
        dc1394_try!(ffi::dc1394_feature_get_power(self.ptr,
                    ffi::dc1394feature_t::DC1394_FEATURE_TRIGGER,
                    &mut pwr));
        let result = match pwr {
            ffi::dc1394switch_t::DC1394_OFF => TriggerMode::Off,
            ffi::dc1394switch_t::DC1394_ON => TriggerMode::On,
            e => panic!("invalid enum {}", e),
        };
        Ok(result)
    }
    pub fn set_trigger_mode(&mut self, _value: TriggerMode) -> Result<()> {
        unimplemented!();
    }
    pub fn trigger_selector(&self) -> Result<TriggerSelector> {
        Ok(TriggerSelector::FrameStart)
    }
    pub fn set_trigger_selector(&mut self, _value: TriggerSelector) -> Result<()> {
        unimplemented!();
    }

    pub fn capture_setup(&self, num_buffers: u32) -> Result<()> {
        dc1394_try!(ffi::dc1394_capture_setup(self.ptr,
                                                num_buffers,
                                                ffi::DC1394_CAPTURE_FLAGS_DEFAULT));
        Ok(())
    }

    pub fn capture_stop(&self) -> Result<()> {
        dc1394_try!(ffi::dc1394_capture_stop(self.ptr));
        Ok(())
    }

    pub fn capture_dequeue(&self, policy: &ffi::dc1394capture_policy_t::Type) -> Result<Frame> {
        Frame::capture(&self, policy)
    }
}

impl AsRawFd for Camera {
    fn as_raw_fd(&self) -> RawFd {
        unsafe{ ffi::dc1394_capture_get_fileno(self.ptr) }
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        trace!("calling ffi::dc1394_camera_free for {:?}", self.guid);
        unsafe {
            ffi::dc1394_camera_free(self.ptr);
        };
    }
}

// ---------------------------
// Frame

pub struct Frame<'a> {
    cam: &'a Camera,
    ptr: *const ffi::dc1394video_frame_t,
}

impl<'a> Frame<'a> {
    fn capture<'b>(cam: &'a Camera, policy: &'b ffi::dc1394capture_policy_t::Type) -> Result<Frame<'a>> {
        let mut ptr: *mut ffi::dc1394video_frame_t = std::ptr::null_mut();
        dc1394_try!(ffi::dc1394_capture_dequeue(cam.ptr, *policy, &mut ptr));
        if ptr.is_null() {
            return Err(Error::CaptureDequeueFailed);
        }
        let frame = Frame { cam: cam, ptr: ptr };
        Ok(frame)
    }

    #[inline]
    fn as_raw(&self) -> &ffi::dc1394video_frame_t {
        unsafe { &*self.ptr }
    }

    #[inline]
    pub fn data_view(&self) -> &[u8] {
        let raw = self.as_raw();
        unsafe { std::slice::from_raw_parts(raw.image, raw.total_bytes as usize) }
    }

    #[inline]
    pub fn rows(&self) -> u32 {
        self.as_raw().size[1]
    }

    #[inline]
    pub fn cols(&self) -> u32 {
        self.as_raw().size[0]
    }

    #[inline]
    pub fn stride(&self) -> u32 {
        self.as_raw().stride
    }

    #[inline]
    pub fn data_depth(&self) -> u32 {
        self.as_raw().data_depth
    }

    #[inline]
    pub fn size(&self) -> [u32; 2] {
        self.as_raw().size
    }

    #[inline]
    pub fn position(&self) -> [u32; 2] {
        self.as_raw().position
    }

    #[inline]
    pub fn color_coding(&self) -> ffi::dc1394color_coding_t::Type {
        self.as_raw().color_coding
    }

    #[inline]
    pub fn color_filter(&self) -> ffi::dc1394color_filter_t::Type {
        self.as_raw().color_filter
    }

    #[inline]
    pub fn yuv_byte_order(&self) -> ffi::dc1394byte_order_t::Type {
        self.as_raw().yuv_byte_order
    }

    #[inline]
    pub fn little_endian(&self) -> ffi::dc1394bool_t::Type {
        self.as_raw().little_endian
    }
}

impl<'a> Drop for Frame<'a> {
    fn drop(&mut self) {
        trace!("calling ffi::dc1394_capture_enqueue");
        match unsafe {
                  ffi::dc1394_capture_enqueue(self.cam.ptr,
                                              self.ptr as *mut ffi::dc1394video_frame_t)
              } {
            ffi::dc1394error_t::DC1394_SUCCESS => {
                trace!("  dc1394_capture_enqueue OK");
            }
            e => {
                trace!("  dc1394_capture_enqueue err");
                panic!("dc1394_capture_enqueue err {:?}", e);
            }
        }
    }
}

// ---------------------------
// DC1394

pub struct DC1394 {
    ctx: *mut ffi::dc1394_t,
}

// // Supposedly DC1394 is thread safe...
// // https://damien.douxchamps.net/ieee1394/libdc1394/faq/#Is_it_safe_to_multi_thread_with_libdc1394
unsafe impl Send for DC1394 {}

fn _test_dc1394_is_send() {
    // Compile-time test to ensure Camera implements Send trait.
    fn implements<T: Send>() {}
    implements::<DC1394>();
}

impl DC1394 {
    pub fn new() -> Result<DC1394> {
        trace!("calling ffi::dc1394_new");
        let ctx = unsafe { ffi::dc1394_new() };
        if ctx.is_null() {
            return Err(Error::Dc1394NewFailed);
        }
        Ok(DC1394 { ctx: ctx })
    }

    pub fn get_camera_list(&self) -> Result<CameraList> {
        CameraList::new(&self)
    }
}

impl Drop for DC1394 {
    fn drop(&mut self) {
        trace!("calling ffi::dc1394_free");
        unsafe {
            ffi::dc1394_free(self.ctx);
        };
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TriggerMode {
    Off,
    On,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TriggerSelector {
    AcquisitionStart,
    FrameStart,
    FrameBurstStart,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AcquisitionMode {
    Continuous,
    SingleFrame,
    MultiFrame,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExposureAuto {
    Off,
    Once,
    Continuous,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GainAuto {
    Off,
    Once,
    Continuous,
}
