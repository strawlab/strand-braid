extern crate byteorder;
extern crate cast;
extern crate chrono;
extern crate machine_vision_formats as formats;
extern crate timestamped_frame;

extern crate datetime_conversion;

#[macro_use]
extern crate structure;

use std::collections::BTreeMap;
use std::f64;
use std::io::{Seek, SeekFrom, Write};

use basic_frame::{match_all_dynamic_fmts, DynamicFrame};
use formats::{pixel_format::PixFmt, ImageStride, PixelFormat};
use timestamped_frame::{ExtraTimeData, ImageStrideTime};

pub type UFMFResult<M> = std::result::Result<M, UFMFError>;

mod save_indices;

#[derive(Debug, thiserror::Error)]
pub enum UFMFError {
    #[error("unimplemented pixel_format {0}")]
    UnimplementedPixelFormat(PixFmt),

    #[error("already closed")]
    AlreadyClosed,

    #[error("the pixel format changed")]
    FormatChanged,

    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Cast(#[from] cast::Error),
}

const KEYFRAME_CHUNK: u8 = 0;
const FRAME_CHUNK: u8 = 1;
const INDEX_DICT_CHUNK: u8 = 2;

fn pack_header(v: u32, index_loc: u64, w: u16, h: u16, cl: u8) -> std::io::Result<Vec<u8>> {
    structure!("<4sIQHHB").pack(b"ufmf", v, index_loc, w, h, cl)
}

fn write_header<F: Write + Seek>(
    f: &mut F,
    index_loc: usize,
    max_width: u16,
    max_height: u16,
    pixel_format: PixFmt,
) -> UFMFResult<usize> {
    let coding = get_format(pixel_format)?;

    let buf: Vec<u8> = pack_header(
        3,
        cast::u64(index_loc),
        max_width,
        max_height,
        cast::u8(coding.len())?,
    )?;

    let mut pos = 0;
    pos += f.write(&buf)?;
    pos += f.write(&coding)?;
    Ok(pos)
}

fn write_image<F: Write + Seek, FMT>(
    f: &mut F,
    frame: &dyn ImageStride<FMT>,
    bytes_per_pixel: u8,
    rect: &RectFromCorner,
) -> UFMFResult<usize> {
    let image_data = frame.image_data();
    let xoffset = rect.x0 as usize * bytes_per_pixel as usize;
    let row_bytes = rect.w as usize * bytes_per_pixel as usize;
    let mut pos = 0;

    for i in rect.y0 as usize..(rect.y0 + rect.h) as usize {
        let start = i * frame.stride() + xoffset;
        let stop = start + row_bytes;
        let row_data = &image_data[start..stop];
        pos += f.write(&row_data)?;
    }
    Ok(pos)
}

fn get_format(pixel_format: PixFmt) -> UFMFResult<Vec<u8>> {
    use PixFmt::*;
    let r = match pixel_format {
        Mono8 => b"MONO8".to_vec(),
        // Mono32f => b"MONO32f".to_vec(),
        BayerRG8 => b"RAW8:RGGB".to_vec(),
        BayerGB8 => b"RAW8:GBRG".to_vec(),
        BayerGR8 => b"RAW8:GRBG".to_vec(),
        BayerBG8 => b"RAW8:BGGR".to_vec(),
        YUV422 => b"YUV422".to_vec(),
        RGB8 => b"RGB8".to_vec(),
        f => {
            return Err(UFMFError::UnimplementedPixelFormat(f));
        }
    };
    Ok(r)
}

fn get_dtype(pixel_format: formats::pixel_format::PixFmt) -> UFMFResult<u8> {
    use formats::pixel_format::PixFmt::*;
    let r = match pixel_format {
        Mono8 | BayerRG8 | BayerGB8 | BayerGR8 | BayerBG8 | YUV422 | RGB8 => b'B',
        Mono32f | BayerRG32f | BayerGB32f | BayerGR32f | BayerBG32f => b'f',
        x => {
            return Err(UFMFError::UnimplementedPixelFormat(x));
        }
    };
    Ok(r)
}

pub struct UFMFWriter<F: Write + Seek> {
    f: Option<F>,
    pos: usize,
    max_width: u16,
    max_height: u16,
    xinc: u8,
    yinc: u8,
    index_frame: Vec<TimestampLoc>,
    index_keyframes: BTreeMap<Vec<u8>, Vec<TimestampLoc>>,
    bytes_per_pixel: u8,
    pixel_format: formats::pixel_format::PixFmt,
}

impl<F: Write + Seek> std::fmt::Debug for UFMFWriter<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "UFMFWriter {{ }}")
    }
}

struct TimestampLoc {
    timestamp: f64,
    loc: u64,
}

/// Specifies a rectangular region, drawn from center.
pub struct RectFromCenter {
    /// x center of region
    x: u16,
    /// y center of region
    y: u16,
    /// width of region
    w: u16,
    /// height of region
    h: u16,
}

impl RectFromCenter {
    pub fn from_xy_wh(x: u16, y: u16, w: u16, h: u16) -> Self {
        Self { x, y, w, h }
    }
}

/// Specifies a rectangular region, drawn from lower-left.
pub struct RectFromCorner {
    /// x lower left of region
    x0: u16,
    /// y lower left of region
    y0: u16,
    /// width of region
    w: u16,
    /// height of region
    h: u16,
}

struct Region<'a> {
    origframe: &'a DynamicFrame,
    rect: &'a RectFromCorner,
}

fn do_size(x: u16, mut w: u16, xinc: u16, max_width: u16) -> (u16, u16) {
    let w_radius = w / 2;

    let xmin = x.saturating_sub(w_radius) / xinc * xinc; // # keep 2x2 Bayer

    let xmax = xmin + w;
    let newxmax = if xmax < max_width { xmax } else { max_width };
    if newxmax != xmax {
        w = newxmax - xmin;
    }
    (xmin, w)
}

impl<F> UFMFWriter<F>
where
    F: Write + Seek,
{
    pub fn new(
        mut f: F,
        max_width: u16,
        max_height: u16,
        pixel_format: PixFmt,
        frame0: Option<&DynamicFrame>,
    ) -> UFMFResult<Self> {
        if let Some(frame0) = frame0.as_ref() {
            if frame0.pixel_format() != pixel_format {
                return Err(UFMFError::FormatChanged);
            }
        }
        let pos = write_header(&mut f, 0, max_width, max_height, pixel_format)?;

        use PixFmt::*;
        let (xinc, yinc) = match pixel_format {
            Mono8 | BayerRG8 | BayerGB8 | BayerGR8 | BayerBG8 => (2, 2),
            YUV422 => (4, 1),
            e => {
                return Err(UFMFError::UnimplementedPixelFormat(e));
            }
        };

        let bytes_per_pixel = pixel_format.bits_per_pixel() / 8;

        let f = Some(f);

        let mut result = Self {
            f,
            pos,
            max_width,
            max_height,
            xinc,
            yinc,
            index_frame: Vec::new(),
            index_keyframes: BTreeMap::new(),
            bytes_per_pixel,
            pixel_format,
        };

        if let Some(frame0) = frame0 {
            match_all_dynamic_fmts!(frame0, x, { result.add_keyframe(b"frame0", x)? });
        }

        Ok(result)
    }

    pub fn add_frame(
        &mut self,
        origframe: &DynamicFrame,
        point_data: &Vec<RectFromCenter>,
    ) -> UFMFResult<Vec<RectFromCorner>> {
        if origframe.pixel_format() != self.pixel_format {
            return Err(UFMFError::FormatChanged);
        }
        let timestamp = datetime_conversion::datetime_to_f64(&origframe.extra().host_timestamp());

        let rects: Vec<RectFromCorner> = point_data
            .iter()
            .map(|i| {
                let (x0, w) = do_size(i.x, i.w, self.xinc as u16, self.max_width);
                let (y0, h) = do_size(i.y, i.h, self.yinc as u16, self.max_height);
                RectFromCorner { x0, y0, w, h }
            })
            .collect();

        {
            // This is a scope in which we borrow from `rects`.
            let regions = rects
                .iter()
                .map(|rect| Region { origframe, rect })
                .collect();
            self.add_frame_regions(timestamp, regions)?;
        }
        Ok(rects)
    }

    fn add_frame_regions(&mut self, timestamp: f64, regions: Vec<Region>) -> UFMFResult<()> {
        let mut self_f = match self.f {
            Some(ref mut f) => f,
            None => {
                return Err(UFMFError::AlreadyClosed.into());
            }
        };

        self.index_frame.push(TimestampLoc {
            timestamp,
            loc: self.pos as u64,
        });

        let n_pts = cast::u16(regions.len())?;
        let bytes_per_pixel = self.bytes_per_pixel;

        let buf0 = vec![FRAME_CHUNK];
        let buf1 = structure!("<dH").pack(timestamp, n_pts)?;

        self.pos += self_f.write(&buf0)?;
        self.pos += self_f.write(&buf1)?;

        for region in regions.iter() {
            let this_str_head = structure!("<HHHH").pack(
                region.rect.x0,
                region.rect.y0,
                region.rect.w,
                region.rect.h,
            )?;
            self.pos += self_f.write(&this_str_head)?;
            self.pos += match_all_dynamic_fmts!(region.origframe, frame, {
                write_image(&mut self_f, frame, bytes_per_pixel, &region.rect)?
            });
        }
        Ok(())
    }

    pub fn add_keyframe<FRAME, FMT>(
        &mut self,
        keyframe_type: &[u8],
        frame: &FRAME,
    ) -> UFMFResult<()>
    where
        FRAME: ImageStrideTime<FMT>,
        FMT: PixelFormat,
    {
        let mut self_f = match self.f {
            Some(ref mut f) => f,
            None => {
                return Err(UFMFError::AlreadyClosed.into());
            }
        };

        let dtl = frame.extra().host_timestamp();

        let bytes_per_pixel = machine_vision_formats::pixel_format::pixfmt::<FMT>()
            .unwrap()
            .bits_per_pixel()
            / 8;

        let timestamp = datetime_conversion::datetime_to_f64(&dtl);
        let dtype = get_dtype(machine_vision_formats::pixel_format::pixfmt::<FMT>().unwrap())?;
        let width = cast::u16(frame.width())?;
        let height = cast::u16(frame.height())?;

        {
            let entry = self
                .index_keyframes
                .entry(keyframe_type.to_vec())
                .or_insert_with(|| Vec::new());
            let timestamp = datetime_conversion::datetime_to_f64(&dtl);
            entry.push(TimestampLoc {
                timestamp,
                loc: self.pos as u64,
            });
        }

        let buf = vec![KEYFRAME_CHUNK, cast::u8(keyframe_type.len())?];
        self.pos += self_f.write(&buf)?;
        self.pos += self_f.write(&keyframe_type)?;

        let buf = structure!("<BHHd").pack(dtype, width, height, timestamp)?;
        let rect = RectFromCorner {
            x0: 0,
            y0: 0,
            w: width,
            h: height,
        };
        self.pos += self_f.write(&buf)?;
        // let frame: &dyn ImageStrideTime<FMT> = frame;
        // let frame = AsImageStrideTime::as_image_stride_time(frame);
        // let frame: &dyn ImageStride<FMT> = frame.as_image_stride();
        self.pos += write_image(
            &mut self_f,
            frame,
            // AsImageStride::as_image_stride(frame),
            // frame.as_image_stride(),
            bytes_per_pixel,
            &rect,
        )?;
        Ok(())
    }
}

impl<F> UFMFWriter<F>
where
    F: Write + Seek,
{
    /// Close the writer.
    ///
    /// Ideally, this is called prior to dropping to prevent the possibility of
    /// silently ignoring errors.
    pub fn close(&mut self) -> UFMFResult<F> {
        let opt_f = self.f.take();

        let mut self_f = match opt_f {
            Some(f) => f,
            None => {
                return Err(UFMFError::AlreadyClosed.into());
            }
        };

        self.pos += self_f.write(&[INDEX_DICT_CHUNK])?;
        save_indices::save_indices(&mut self_f, &self.index_frame, &self.index_keyframes)?;
        self_f.seek(SeekFrom::Start(0))?;
        write_header(
            &mut self_f,
            self.pos,
            self.max_width,
            self.max_height,
            self.pixel_format,
        )?;
        Ok(self_f)
    }
}

/// This will silently ignore any error.
impl<F: Write + Seek> Drop for UFMFWriter<F> {
    fn drop(&mut self) {
        // We silently drop error.
        match self.close() {
            Ok(_f) => {}
            Err(_e) => {} // See https://github.com/rust-lang/rfcs/issues/814
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::*;
    use basic_frame::{BasicExtra, BasicFrame, DynamicFrame};
    use byteorder::WriteBytesExt;

    fn arange(start: u8, timestamp: f64) -> DynamicFrame {
        let w = 10;
        let h = 10;
        let mut image_data = Vec::new();
        for i in 0..100 {
            image_data.push(start + i as u8);
        }

        let ts_local = datetime_conversion::f64_to_datetime(timestamp);
        let host_timestamp = ts_local.with_timezone(&chrono::Utc);

        let roundtrip = datetime_conversion::datetime_to_f64(&host_timestamp);
        assert_eq!(timestamp, roundtrip); // Although this is a float and thus
                                          // not guaranteed in general to roundtrip without change, it must pass
                                          // through the roundtrip without change in order to hope that the byte-
                                          // by-byte comparison in the test will succeed.
        let extra = Box::new(BasicExtra {
            host_timestamp,
            host_framenumber: 0,
        });

        DynamicFrame::Mono8(BasicFrame {
            width: w,
            height: h,
            stride: w,
            image_data,
            pixel_format: std::marker::PhantomData,
            extra,
        })
    }

    fn arange_float(start: f32, timestamp: f64) -> DynamicFrame {
        let w = 10;
        let h = 10;

        let mut f = std::io::Cursor::new(Vec::with_capacity(4 * 100));
        for i in 0..100 {
            let value: f32 = start + i as f32;
            f.write_f32::<byteorder::LittleEndian>(value).unwrap();
        }
        let image_data = f.into_inner();

        let ts_local = datetime_conversion::f64_to_datetime(timestamp);
        let host_timestamp = ts_local.with_timezone(&chrono::Utc);

        let roundtrip = datetime_conversion::datetime_to_f64(&host_timestamp);
        assert_eq!(timestamp, roundtrip); // Although this is a float and thus
                                          // not guaranteed in general to roundtrip without change, it must pass
                                          // through the roundtrip without change in order to hope that the byte-
                                          // by-byte comparison in the test will succeed.
        let extra = Box::new(BasicExtra {
            host_timestamp,
            host_framenumber: 0,
        });

        DynamicFrame::Mono32f(BasicFrame {
            width: w,
            height: h,
            stride: w * 4,
            image_data,
            pixel_format: std::marker::PhantomData,
            extra,
        })
    }

    #[test]
    fn test_pack_header() {
        let buf = pack_header(1, 2, 3, 4, 5).unwrap();
        assert_eq!(
            buf,
            &[b'u', b'f', b'm', b'f', 1, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 3, 0, 4, 0, 5,]
        );
    }

    #[test]
    fn test_empty_file() {
        let w = 320;
        let h = 240;
        let pixel_format = formats::pixel_format::PixFmt::Mono8;
        let f = std::io::Cursor::new(Vec::new());
        let mut writer = UFMFWriter::new(f, w, h, pixel_format, None).unwrap();
        let f = writer.close().unwrap();

        // check that we cannot close again
        match writer.close() {
            Ok(_) => panic!("expected error"),
            Err(_e) => {} // TODO: check this is AlreadyClosed error.
        };

        // check what was writen
        let buf = f.into_inner();

        // The expected values are from this Python program:

        /*
        import ufmf

        fname = 'empty.ufmf'
        saver = ufmf.UfmfSaverV3(fname,max_width=320,max_height=240,coding='MONO8')
        saver.close()

        buf = open(fname,mode='r').read()
        print(', '.join(['0x%x'%ord(c) for c in buf]))
        */
        let expected: &[u8] = &[
            0x75, 0x66, 0x6d, 0x66, 0x3, 0x0, 0x0, 0x0, 0x1b, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
            0x40, 0x1, 0xf0, 0x0, 0x5, 0x4d, 0x4f, 0x4e, 0x4f, 0x38, 0x2, 0x64, 0x2, 0x5, 0x0,
            0x66, 0x72, 0x61, 0x6d, 0x65, 0x64, 0x0, 0x8, 0x0, 0x6b, 0x65, 0x79, 0x66, 0x72, 0x61,
            0x6d, 0x65, 0x64, 0x0,
        ];
        assert_eq!(&buf[0..], expected);
    }

    #[test]
    fn test_saving_regions() {
        let arr = arange(0, 123.456);
        let frame0 = Some(&arr);
        let w = 10;
        let h = 10;
        let pixel_format = formats::pixel_format::PixFmt::Mono8;
        let f = std::io::Cursor::new(Vec::new());
        let mut writer = UFMFWriter::new(f, w, h, pixel_format, frame0).unwrap();

        let arr2 = arange(100, 42.42);
        let point_data = vec![
            RectFromCenter::from_xy_wh(0, 0, 4, 4),
            RectFromCenter::from_xy_wh(4, 4, 4, 4),
            RectFromCenter::from_xy_wh(9, 9, 4, 4),
        ];

        writer.add_frame(&arr2, &point_data).unwrap();

        let f = writer.close().unwrap();

        // check that we cannot close again
        match writer.close() {
            Ok(_) => panic!("expected error"),
            Err(_e) => {} // TODO: check this is AlreadyClosed error.
        };

        // check what was writen
        let buf = f.into_inner();

        // The expected values are from this Python program:

        /*

        from __future__ import print_function
        import ufmf
        import numpy as np

        frame0 = np.arange(100, dtype=np.uint8)
        frame0.shape = (10,10)

        print(frame0)

        timestamp0 = 123.456

        fname = 'small.ufmf'
        saver = ufmf.UfmfSaverV3(fname,max_width=10,max_height=10,coding='MONO8',
            frame0=frame0,timestamp0=timestamp0)

        print('---')
        frame1 = np.arange(100, dtype=np.uint8) + 100
        frame1.shape = frame0.shape
        print(frame1)

        point_data = [
            # xidx, yidx, w, h
            (0, 0, 4, 4),
            (4, 4, 4, 4),
            (9, 9, 4, 4),
        ]

        saver.add_frame(frame1,timestamp=42.42,point_data=point_data)

        saver.close()

        print('---')

        buf = open(fname,mode='r').read()
        print(', '.join(['0x%x'%ord(c) for c in buf]))

        */

        let expected: &[u8] = &[
            0x75, 0x66, 0x6d, 0x66, 0x3, 0x0, 0x0, 0x0, 0xe7, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
            0xa, 0x0, 0xa, 0x0, 0x5, 0x4d, 0x4f, 0x4e, 0x4f, 0x38, 0x0, 0x6, 0x66, 0x72, 0x61,
            0x6d, 0x65, 0x30, 0x42, 0xa, 0x0, 0xa, 0x0, 0x77, 0xbe, 0x9f, 0x1a, 0x2f, 0xdd, 0x5e,
            0x40, 0x0, 0x1, 0x2, 0x3, 0x4, 0x5, 0x6, 0x7, 0x8, 0x9, 0xa, 0xb, 0xc, 0xd, 0xe, 0xf,
            0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d,
            0x1e, 0x1f, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x2b,
            0x2c, 0x2d, 0x2e, 0x2f, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39,
            0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47,
            0x48, 0x49, 0x4a, 0x4b, 0x4c, 0x4d, 0x4e, 0x4f, 0x50, 0x51, 0x52, 0x53, 0x54, 0x55,
            0x56, 0x57, 0x58, 0x59, 0x5a, 0x5b, 0x5c, 0x5d, 0x5e, 0x5f, 0x60, 0x61, 0x62, 0x63,
            0x1, 0xf6, 0x28, 0x5c, 0x8f, 0xc2, 0x35, 0x45, 0x40, 0x3, 0x0, 0x0, 0x0, 0x0, 0x0, 0x4,
            0x0, 0x4, 0x0, 0x64, 0x65, 0x66, 0x67, 0x6e, 0x6f, 0x70, 0x71, 0x78, 0x79, 0x7a, 0x7b,
            0x82, 0x83, 0x84, 0x85, 0x2, 0x0, 0x2, 0x0, 0x4, 0x0, 0x4, 0x0, 0x7a, 0x7b, 0x7c, 0x7d,
            0x84, 0x85, 0x86, 0x87, 0x8e, 0x8f, 0x90, 0x91, 0x98, 0x99, 0x9a, 0x9b, 0x6, 0x0, 0x6,
            0x0, 0x4, 0x0, 0x4, 0x0, 0xa6, 0xa7, 0xa8, 0xa9, 0xb0, 0xb1, 0xb2, 0xb3, 0xba, 0xbb,
            0xbc, 0xbd, 0xc4, 0xc5, 0xc6, 0xc7, 0x2, 0x64, 0x2, 0x5, 0x0, 0x66, 0x72, 0x61, 0x6d,
            0x65, 0x64, 0x2, 0x3, 0x0, 0x6c, 0x6f, 0x63, 0x61, 0x6c, 0x8, 0x0, 0x0, 0x0, 0x93, 0x0,
            0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x9, 0x0, 0x74, 0x69, 0x6d, 0x65, 0x73, 0x74, 0x61, 0x6d,
            0x70, 0x61, 0x64, 0x8, 0x0, 0x0, 0x0, 0xf6, 0x28, 0x5c, 0x8f, 0xc2, 0x35, 0x45, 0x40,
            0x8, 0x0, 0x6b, 0x65, 0x79, 0x66, 0x72, 0x61, 0x6d, 0x65, 0x64, 0x1, 0x6, 0x0, 0x66,
            0x72, 0x61, 0x6d, 0x65, 0x30, 0x64, 0x2, 0x3, 0x0, 0x6c, 0x6f, 0x63, 0x61, 0x6c, 0x8,
            0x0, 0x0, 0x0, 0x1a, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x9, 0x0, 0x74, 0x69, 0x6d,
            0x65, 0x73, 0x74, 0x61, 0x6d, 0x70, 0x61, 0x64, 0x8, 0x0, 0x0, 0x0, 0x77, 0xbe, 0x9f,
            0x1a, 0x2f, 0xdd, 0x5e, 0x40,
        ];
        assert_eq!(&buf[0..], expected);
    }

    #[test]
    fn test_float_keyframe() {
        use machine_vision_formats::pixel_format::Mono32f;
        let w = 10;
        let h = 10;
        let pixel_format = formats::pixel_format::PixFmt::Mono8;
        let f = std::io::Cursor::new(Vec::new());
        let mut writer = UFMFWriter::new(f, w, h, pixel_format, None).unwrap();
        let running_mean = arange_float(0.1, 123.456);

        let running_mean = running_mean.into_basic::<Mono32f>().unwrap();

        writer.add_keyframe(b"mean", &running_mean).unwrap();
        let f = writer.close().unwrap();

        // check that we cannot close again
        match writer.close() {
            Ok(_) => panic!("expected error"),
            Err(_e) => {} // TODO: check this is AlreadyClosed error.
        };

        // check what was writen
        let buf = f.into_inner();

        // The expected values are from this Python program:

        /*
        from __future__ import print_function
        import ufmf
        import numpy as np

        running_mean_im = np.arange(100, dtype=np.float32) + 0.1
        running_mean_im.shape = (10,10)

        print(running_mean_im)

        fname = 'with_mean.ufmf'
        saver = ufmf.UfmfSaverV3(fname,max_width=10,max_height=10,coding='MONO8')
        saver.add_keyframe('mean',running_mean_im,timestamp=123.456)
        saver.close()

        print('---')

        buf = open(fname,mode='r').read()
        print(', '.join(['0x%x'%ord(c) for c in buf]))

        */

        let expected: &[u8] = &[
            0x75, 0x66, 0x6d, 0x66, 0x3, 0x0, 0x0, 0x0, 0xbe, 0x1, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
            0xa, 0x0, 0xa, 0x0, 0x5, 0x4d, 0x4f, 0x4e, 0x4f, 0x38, 0x0, 0x4, 0x6d, 0x65, 0x61,
            0x6e, 0x66, 0xa, 0x0, 0xa, 0x0, 0x77, 0xbe, 0x9f, 0x1a, 0x2f, 0xdd, 0x5e, 0x40, 0xcd,
            0xcc, 0xcc, 0x3d, 0xcd, 0xcc, 0x8c, 0x3f, 0x66, 0x66, 0x6, 0x40, 0x66, 0x66, 0x46,
            0x40, 0x33, 0x33, 0x83, 0x40, 0x33, 0x33, 0xa3, 0x40, 0x33, 0x33, 0xc3, 0x40, 0x33,
            0x33, 0xe3, 0x40, 0x9a, 0x99, 0x1, 0x41, 0x9a, 0x99, 0x11, 0x41, 0x9a, 0x99, 0x21,
            0x41, 0x9a, 0x99, 0x31, 0x41, 0x9a, 0x99, 0x41, 0x41, 0x9a, 0x99, 0x51, 0x41, 0x9a,
            0x99, 0x61, 0x41, 0x9a, 0x99, 0x71, 0x41, 0xcd, 0xcc, 0x80, 0x41, 0xcd, 0xcc, 0x88,
            0x41, 0xcd, 0xcc, 0x90, 0x41, 0xcd, 0xcc, 0x98, 0x41, 0xcd, 0xcc, 0xa0, 0x41, 0xcd,
            0xcc, 0xa8, 0x41, 0xcd, 0xcc, 0xb0, 0x41, 0xcd, 0xcc, 0xb8, 0x41, 0xcd, 0xcc, 0xc0,
            0x41, 0xcd, 0xcc, 0xc8, 0x41, 0xcd, 0xcc, 0xd0, 0x41, 0xcd, 0xcc, 0xd8, 0x41, 0xcd,
            0xcc, 0xe0, 0x41, 0xcd, 0xcc, 0xe8, 0x41, 0xcd, 0xcc, 0xf0, 0x41, 0xcd, 0xcc, 0xf8,
            0x41, 0x66, 0x66, 0x0, 0x42, 0x66, 0x66, 0x4, 0x42, 0x66, 0x66, 0x8, 0x42, 0x66, 0x66,
            0xc, 0x42, 0x66, 0x66, 0x10, 0x42, 0x66, 0x66, 0x14, 0x42, 0x66, 0x66, 0x18, 0x42,
            0x66, 0x66, 0x1c, 0x42, 0x66, 0x66, 0x20, 0x42, 0x66, 0x66, 0x24, 0x42, 0x66, 0x66,
            0x28, 0x42, 0x66, 0x66, 0x2c, 0x42, 0x66, 0x66, 0x30, 0x42, 0x66, 0x66, 0x34, 0x42,
            0x66, 0x66, 0x38, 0x42, 0x66, 0x66, 0x3c, 0x42, 0x66, 0x66, 0x40, 0x42, 0x66, 0x66,
            0x44, 0x42, 0x66, 0x66, 0x48, 0x42, 0x66, 0x66, 0x4c, 0x42, 0x66, 0x66, 0x50, 0x42,
            0x66, 0x66, 0x54, 0x42, 0x66, 0x66, 0x58, 0x42, 0x66, 0x66, 0x5c, 0x42, 0x66, 0x66,
            0x60, 0x42, 0x66, 0x66, 0x64, 0x42, 0x66, 0x66, 0x68, 0x42, 0x66, 0x66, 0x6c, 0x42,
            0x66, 0x66, 0x70, 0x42, 0x66, 0x66, 0x74, 0x42, 0x66, 0x66, 0x78, 0x42, 0x66, 0x66,
            0x7c, 0x42, 0x33, 0x33, 0x80, 0x42, 0x33, 0x33, 0x82, 0x42, 0x33, 0x33, 0x84, 0x42,
            0x33, 0x33, 0x86, 0x42, 0x33, 0x33, 0x88, 0x42, 0x33, 0x33, 0x8a, 0x42, 0x33, 0x33,
            0x8c, 0x42, 0x33, 0x33, 0x8e, 0x42, 0x33, 0x33, 0x90, 0x42, 0x33, 0x33, 0x92, 0x42,
            0x33, 0x33, 0x94, 0x42, 0x33, 0x33, 0x96, 0x42, 0x33, 0x33, 0x98, 0x42, 0x33, 0x33,
            0x9a, 0x42, 0x33, 0x33, 0x9c, 0x42, 0x33, 0x33, 0x9e, 0x42, 0x33, 0x33, 0xa0, 0x42,
            0x33, 0x33, 0xa2, 0x42, 0x33, 0x33, 0xa4, 0x42, 0x33, 0x33, 0xa6, 0x42, 0x33, 0x33,
            0xa8, 0x42, 0x33, 0x33, 0xaa, 0x42, 0x33, 0x33, 0xac, 0x42, 0x33, 0x33, 0xae, 0x42,
            0x33, 0x33, 0xb0, 0x42, 0x33, 0x33, 0xb2, 0x42, 0x33, 0x33, 0xb4, 0x42, 0x33, 0x33,
            0xb6, 0x42, 0x33, 0x33, 0xb8, 0x42, 0x33, 0x33, 0xba, 0x42, 0x33, 0x33, 0xbc, 0x42,
            0x33, 0x33, 0xbe, 0x42, 0x33, 0x33, 0xc0, 0x42, 0x33, 0x33, 0xc2, 0x42, 0x33, 0x33,
            0xc4, 0x42, 0x33, 0x33, 0xc6, 0x42, 0x2, 0x64, 0x2, 0x5, 0x0, 0x66, 0x72, 0x61, 0x6d,
            0x65, 0x64, 0x0, 0x8, 0x0, 0x6b, 0x65, 0x79, 0x66, 0x72, 0x61, 0x6d, 0x65, 0x64, 0x1,
            0x4, 0x0, 0x6d, 0x65, 0x61, 0x6e, 0x64, 0x2, 0x3, 0x0, 0x6c, 0x6f, 0x63, 0x61, 0x6c,
            0x8, 0x0, 0x0, 0x0, 0x1a, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x9, 0x0, 0x74, 0x69,
            0x6d, 0x65, 0x73, 0x74, 0x61, 0x6d, 0x70, 0x61, 0x64, 0x8, 0x0, 0x0, 0x0, 0x77, 0xbe,
            0x9f, 0x1a, 0x2f, 0xdd, 0x5e, 0x40,
        ];
        assert_eq!(&buf[0..], expected);
    }
}
