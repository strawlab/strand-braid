#[macro_use]
extern crate log;
#[macro_use]
extern crate quick_error;
extern crate libflycapture2_sys;

use libflycapture2_sys as ffi;
use std::path::Path;

mod overrides;

#[cfg(windows)]
fn filename_to_cstr(filename: &Path) -> Result<*const i8> {
    // This is tricky. See http://stackoverflow.com/a/38948854 .

    use std::ascii::AsciiExt;

    let fname = match filename.to_str() {
        Some(fname) => fname,
        None => {bail!("filename is not UTF8");},
    };
    let ascii = match fname.is_ascii() {
        false => {
            bail!("filename is not ASCII");
        },
        true => {
            fname
        }
    };

    let cstr = ascii.as_ptr() as *const i8;
    Ok(cstr)
}

#[cfg(not(windows))]
fn filename_to_cstr(filename: &Path) -> Result<*const i8> {
    use std::os::unix::ffi::OsStrExt;

    let fname_bytes = filename.as_os_str().as_bytes();
    let cstr = fname_bytes.as_ptr() as *const i8;
    Ok(cstr)
}

// ---------------------------
// errors


quick_error! {
    #[derive(Debug)]
    pub enum Error {
        // IoError(path: PathBuf, err: std::io::Error) {
        //     context(p: &'a Path, err: std::io::Error)
        //         -> (p.to_path_buf(), err)
        // }
        /// Error from the underlying flycapture2 library
        Fc2Error(code: ::libflycapture2_sys::_fc2Error) {}
        /// Error from the underlying flycapture2 library with camera context
        CameraFc2Error(guid: GUID, code: ::libflycapture2_sys::_fc2Error) {
            context(guid: &GUID, code: ::libflycapture2_sys::_fc2Error)
                -> (guid.clone(), code)
        }
        /// Error from the flycap2 rust library
        Flycap2Error(msg: String) {}
        /// GUID was not parseable
        UnparsableGuid {}
    }
}

pub type Result<T> = std::result::Result<T,Error>;

macro_rules! fc2try {
    ($x:expr) => {
        trace!("calling fc2try in {}:{}", file!(), line!());
        match unsafe { $x } {
            ffi::_fc2Error::FC2_ERROR_OK => {trace!("  fc2try OK");},
            e => { trace!("  fc2try err"); return Err(Error::Fc2Error(e)); },
        }
    }
}

macro_rules! fc2try_chain {
    ($x:expr, $guid:expr) => {
        trace!("calling fc2try_chain in {}:{}", file!(), line!());
        match unsafe { $x } {
            ffi::_fc2Error::FC2_ERROR_OK => {
                trace!("  fc2try_chain OK");
            },
            e => {
                trace!("  fc2try_chain err");
                return Err(quick_error::Context($guid,e).into());
            },
        }
    }
}

macro_rules! fc2errck {
    ($x:expr, $msg:expr) => {
        trace!("calling fc2errck in {}:{}:{}", file!(), $msg, line!());
        match unsafe { $x } {
            ffi::_fc2Error::FC2_ERROR_OK => {trace!("  fc2errck OK");},
            e => {
                let err = Error::Fc2Error(e);
                panic!("Error {} during {}.", err, $msg);
            },
        };
    }
}

// ---------------------------
// GUID

#[derive(Copy, Clone)]
pub struct GUID {
    inner: ffi::_fc2PGRGuid,
}

impl GUID {
    pub fn new(v0: u32, v1: u32, v2: u32, v3: u32) -> GUID {
        let inner = ffi::_fc2PGRGuid {value: [v0, v1, v2, v3]};
        GUID { inner: inner }
    }

    pub fn from_str(s: &str) -> Result<GUID> {
        let elements: Vec<&str> = s.split('-').collect();
        if elements.len() != 4 {
            return Err( Error::UnparsableGuid );
        }
        let mut v: Vec<_> = Vec::with_capacity(4);
        for e in elements.iter() {
            v.push(u32::from_str_radix(e, 16).map_err(|_| Error::UnparsableGuid )?);
        }
        Ok(GUID::new(v[0], v[1], v[2], v[3]))
    }
}

impl PartialEq<GUID> for GUID {
    fn eq(&self, other: &GUID) -> bool {
        let a = &self.inner.value;
        let b = &other.inner.value;
        a[0] == b[0] && a[1] == b[1] && a[2] == b[2] && a[3] == b[3]
    }
}

impl<'a> From<&'a GUID> for String {
    fn from(s: &'a GUID) -> Self {
        let v = &s.inner.value;
        format!("{:X}-{:X}-{:X}-{:X}", v[0], v[1], v[2], v[3] )
    }
}

impl From<ffi::_fc2PGRGuid> for GUID {
    fn from(s: ffi::_fc2PGRGuid) -> Self {
        GUID { inner: s }
    }
}

impl From<GUID> for ffi::_fc2PGRGuid {
    fn from(s: GUID) -> Self {
        s.inner
    }
}

impl std::fmt::Debug for GUID {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let s = String::from(self);
        write!(f, "GUID {{ {} }}", s )
    }
}

// ---------------------------
// Image

pub struct Image {
    inner: ffi::_fc2Image,
}

impl Image {
    pub fn new() -> Result<Image> {
        let mut image = ffi::_fc2Image::default();
        fc2try!(ffi::fc2CreateImage(&mut image));
        Ok(Image { inner: image })
    }

    pub fn get_data_view(&self) -> Result<&[u8]> {
        let data_view: &[u8] =
            unsafe { std::slice::from_raw_parts(self.inner.pData, self.inner.dataSize as usize) };
        Ok(data_view)
    }

    pub fn get_raw(&self) -> &ffi::_fc2Image {
        &self.inner
    }

    pub fn convert_to(&self, format: ffi::_fc2PixelFormat) -> Result<Image> {
        let mut result = Image::new()?;
        fc2try!(overrides::fc2ConvertImageTo(format, &self.inner, &mut result.inner));
        Ok(result)
    }

    pub fn save_to(&self, filename: &Path, format: ffi::_fc2ImageFileFormat ) -> Result<()> {
        let cstr = filename_to_cstr(filename)?;
        fc2try!(overrides::fc2SaveImage( &self.inner, cstr, format ));
        Ok(())
    }

    pub fn get_timestamp(&self) -> Result<ffi::fc2TimeStamp> {
        Ok(unsafe{overrides::fc2GetImageTimeStamp( &self.inner )})
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        fc2errck!(ffi::fc2DestroyImage(&mut self.inner), "Image::drop");
    }
}

// ---------------------------
// BareContext

#[derive(Debug)]
struct BareContext {
    inner: ffi::fc2Context,
}

unsafe impl Send for BareContext {}

impl BareContext {
    fn new() -> Result<BareContext> {
        let mut ctx: ffi::fc2Context = std::ptr::null_mut();
        debug!("ffi::fc2CreateContext()");
        fc2try!(ffi::fc2CreateContext(&mut ctx));
        Ok(BareContext{inner: ctx})
    }
}

impl Drop for BareContext {
    fn drop(&mut self) {
        fc2errck!(ffi::fc2DestroyContext(self.inner), "BareContext::drop");
    }
}

// ---------------------------
// FlycapContext

#[derive(Debug)]
pub struct FlycapContext {
    basecx: BareContext,
    guid: GUID,
    started: bool,
}

impl FlycapContext {
    pub fn new(guid: GUID) -> Result<FlycapContext> {
        let basecx = BareContext::new()?;
        let mut guid_copy = guid.clone().into();
        debug!("calling fc2Connect for {:?}", guid);

        fc2try_chain!( ffi::fc2Connect(basecx.inner, &mut guid_copy), &guid);
        Ok(FlycapContext {basecx: basecx, guid: guid, started: false})
    }

    pub fn guid(&self) -> &GUID {
        &self.guid
    }

    pub fn get_video_mode_and_frame_rate_info(&self, video_mode: ffi::_fc2VideoMode, frame_rate: ffi::_fc2FrameRate) -> Result<bool> {
        let mut result: i32 = 0;
        fc2try!(ffi::fc2GetVideoModeAndFrameRateInfo(self.basecx.inner, video_mode, frame_rate, &mut result));
        Ok(result != 0 )
    }

    pub fn set_video_mode_and_frame_rate_info(&self, video_mode: ffi::_fc2VideoMode, frame_rate: ffi::_fc2FrameRate) -> Result<()> {
        fc2try!(ffi::fc2SetVideoModeAndFrameRate(self.basecx.inner, video_mode, frame_rate));
        Ok(())
    }

    pub fn get_format7_info(&self, format7_info: ffi::_fc2Format7Info) -> Result<(ffi::_fc2Format7Info,bool)> {
        let mut format7_info: ffi::_fc2Format7Info = format7_info.clone();
        let mut p_supported: i32 = 0;
        fc2try!(ffi::fc2GetFormat7Info(self.basecx.inner, &mut format7_info, &mut p_supported));
        Ok((format7_info,p_supported != 0))
    }

    pub fn get_camera_info(&self) -> Result<ffi::fc2CameraInfo> {
        let mut result = ffi::fc2CameraInfo::default();
        fc2try!(ffi::fc2GetCameraInfo(self.basecx.inner, &mut result));
        Ok(result)
    }

    pub fn validate_format7_settings(&self, settings: ffi::_fc2Format7ImageSettings) -> Result<(ffi::_fc2Format7ImageSettings, bool, ffi::_fc2Format7PacketInfo)> {
        let mut settings: ffi::_fc2Format7ImageSettings = settings.clone();
        let mut settings_are_valid: i32 = 0;
        let mut packet_info = ffi::_fc2Format7PacketInfo::default();
        fc2try!(ffi::fc2ValidateFormat7Settings(self.basecx.inner, &mut settings, &mut settings_are_valid, &mut packet_info));
        Ok((settings, settings_are_valid!=0, packet_info))
    }

    pub fn set_format7_configuration_packet(&self, settings: ffi::_fc2Format7ImageSettings, packet_size: u32) -> Result<()> {
        let mut settings: ffi::_fc2Format7ImageSettings = settings.clone();
        fc2try!(ffi::fc2SetFormat7ConfigurationPacket(self.basecx.inner, &mut settings, packet_size));
        Ok(())
    }

    pub fn start_capture(&mut self) -> Result<()> {
        debug!("calling fc2StartCapture for {:?}", self.guid);
        fc2try!(ffi::fc2StartCapture(self.basecx.inner));
        self.started = true;
        Ok(())
    }

    pub fn stop_capture(&mut self) -> Result<()> {
        debug!("calling fc2StopCapture for {:?}", self.guid);
        fc2try!(ffi::fc2StopCapture(self.basecx.inner));
        self.started = false;
        Ok(())
    }

    pub fn retrieve_buffer(&self) -> Result<Image> {
        let mut im = Image::new()?;
        fc2try!(ffi::fc2RetrieveBuffer(self.basecx.inner, &mut im.inner));
        Ok(im)
    }

    pub fn get_property_info(&self, prop_info: ffi::_fc2PropertyInfo) -> Result<ffi::_fc2PropertyInfo> {
        let mut result: ffi::_fc2PropertyInfo = prop_info.clone();
        fc2try!(ffi::fc2GetPropertyInfo(self.basecx.inner, &mut result));
        Ok(result)
    }
}

impl Drop for FlycapContext {
    fn drop(&mut self) {

        // Stop the capture if we started it.
        if self.started {
            debug!("calling fc2StopCapture for {:?}", self.guid);
            fc2errck!(ffi::fc2StopCapture(self.basecx.inner), "FlycapContext::drop::fc2StopCapture");
        }

        // Disconnect the context from the camera.
        debug!("calling fc2Disconnect for {:?}", self.guid);
        fc2errck!(ffi::fc2Disconnect(self.basecx.inner), "FlycapContext::drop::fc2Disconnect");
    }
}

// ---------------------------
// bare functions

pub fn get_num_cameras() -> Result<usize> {
    let mut n_cams = 0;
    let bcx = BareContext::new()?;
    fc2try!(ffi::fc2GetNumOfCameras(bcx.inner, &mut n_cams));
    Ok(n_cams as usize)
}

pub fn get_guid_for_index(index: usize) -> Result<GUID> {
    let mut result = ffi::_fc2PGRGuid::default();
    let bcx = BareContext::new()?;
    fc2try!(ffi::fc2GetCameraFromIndex(bcx.inner, index as u32, &mut result));
    Ok(result.into())
}

pub fn get_lowest_pixel_format(pixel_formats: u32) -> ffi::_fc2PixelFormat {
    if (pixel_formats & ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_MONO8 as u32) != 0 {
        return ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_MONO8;
    }

    if (pixel_formats & ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_MONO12 as u32) != 0 {
        return ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_MONO12;
    }

    if (pixel_formats & ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_MONO16 as u32) != 0 {
        return ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_MONO16;
    }

    if (pixel_formats & ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_RAW8 as u32) != 0 {
        return ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_RAW8;
    }

    if (pixel_formats & ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_RAW12 as u32) != 0 {
        return ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_RAW12;
    }

    if (pixel_formats & ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_RAW16 as u32) != 0 {
        return ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_RAW16;
    }

    if (pixel_formats & ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_411YUV8 as u32) != 0 {
        return ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_411YUV8;
    }

    if (pixel_formats & ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_422YUV8 as u32) != 0 {
        return ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_422YUV8;
    }

    if (pixel_formats & ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_444YUV8 as u32) != 0 {
        return ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_444YUV8;
    }

    if (pixel_formats & ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_RGB8 as u32) != 0 {
        return ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_RGB8;
    }

    if (pixel_formats & ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_RGB16 as u32) != 0 {
        return ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_RGB16;
    }

    if (pixel_formats & ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_S_MONO16 as u32) != 0 {
        return ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_S_MONO16;
    }

    if (pixel_formats & ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_S_RGB16 as u32) != 0 {
        return ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_S_RGB16;
    }

    return ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_MONO8;
}

// ---------------------------

#[cfg(test)]
mod tests {
    use super::{get_num_cameras, GUID};

    #[test]
    fn test_get_num_cameras() {
        get_num_cameras().unwrap();
    }

    #[test]
    fn test_guid_string_roundtrip() {
        let expected = GUID::new(1,2,3,4);
        let s = String::from(&expected);
        let actual = GUID::from_str(&s).unwrap();
        assert!(expected==actual);
    }

    #[test]
    fn parse_real_guid() {
        let guid = "2D8357D5-21DB8DAD-ECACF53C-24A87704";
        GUID::from_str(guid).unwrap();
    }

}
