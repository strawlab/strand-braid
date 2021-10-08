#[macro_use]
extern crate log;

use anyhow::Result;

use basic_frame::{match_all_dynamic_fmts, BasicExtra, DynamicFrame};
use ci2_remote_control::MkvRecordingConfig;
use convert_image::{encode_y4m_frame, ImageOptions, Y4MColorspace};
use machine_vision_formats::{
    pixel_format, pixel_format::PixFmt, ImageBuffer, ImageBufferRef, ImageData, ImageStride, Stride,
};
use std::path::{Path, PathBuf};
use structopt::StructOpt;
use timestamped_frame::ExtraTimeData;

const Y4M_MAGIC: &str = "YUV4MPEG2";
const Y4M_FRAME_MAGIC: &str = "FRAME";

/*

Example export to mkv for RGB8. Will change colors via YUV bit, will loose timestamps:

    fmf export-y4m test_rgb8.fmf -o - | ffmpeg -i - -vcodec ffv1 /tmp/test.mkv

Example export to mkv for mono8. Will loose timestamps:

    fmf export-y4m /extra2/straw/src2/python-alleskleber-2018-2019-ws/practical-02/data/short-movie20170810_182130.fmf -o - | ffmpeg -i - -vcodec ffv1 /tmp/short-movie20170810_182130-ffv1.mkv

Example export to mkv via RGB (should be lossless):

    fmf export-bgr24 test_rgb8.fmf -o - | ffmpeg -i - -f rawvideo -pix_fmt bgr0 -s 332x332 -r 30 -vcodec ffv1 /tmp/test.mkv

Example export to webm:

    fmf export-webm test_rgb8.fmf -o /tmp/test.webm

Idea: use FFMPEG to encode ffv1 stream (or x264 or ...) and then use
webm/matroska muxer directly to save it with original timestamps.

should be able to set DateUTC MKV meta data with the "creation_time" tag.
However, this has two problems: 1) the time is only parsed to the second level,
meaning millisecond (or better) precision is not currently possible and 2) it is
not possible to specify the timestamp of each frame with the y4m method, only an
overall framerate.

E.g.

    fmf export-y4m /extra2/straw/src2/python-alleskleber-2018-2019-ws/practical-02/data/short-movie20170810_182130.fmf -o - | ffmpeg -i - -vcodec ffv1 -metadata creation_time=978307200001234  /tmp/short-movie20170810_182130-ffv1.mkv

    fmf export-y4m /extra2/straw/src2/python-alleskleber-2018-2019-ws/practical-02/data/short-movie20170810_182130.fmf -o - | ffmpeg -i - -vcodec ffv1 -metadata creation_time="2012-02-07 12:15:27"  /tmp/short-movie20170810_182130-ffv1.mkv


*/

/// Convert to runtime specified pixel format and save to FMF file.
macro_rules! convert_and_write_fmf {
    ($new_pixel_format:expr, $writer:expr, $x:expr, $timestamp:expr) => {{
        use pixel_format::*;
        match $new_pixel_format {
            PixFmt::Mono8 => write_converted!(Mono8, $writer, $x, $timestamp),
            PixFmt::Mono32f => write_converted!(Mono32f, $writer, $x, $timestamp),
            PixFmt::RGB8 => write_converted!(RGB8, $writer, $x, $timestamp),
            PixFmt::BayerRG8 => write_converted!(BayerRG8, $writer, $x, $timestamp),
            PixFmt::BayerRG32f => write_converted!(BayerRG32f, $writer, $x, $timestamp),
            PixFmt::BayerGB8 => write_converted!(BayerGB8, $writer, $x, $timestamp),
            PixFmt::BayerGB32f => write_converted!(BayerGB32f, $writer, $x, $timestamp),
            PixFmt::BayerGR8 => write_converted!(BayerGR8, $writer, $x, $timestamp),
            PixFmt::BayerGR32f => write_converted!(BayerGR32f, $writer, $x, $timestamp),
            PixFmt::BayerBG8 => write_converted!(BayerBG8, $writer, $x, $timestamp),
            PixFmt::BayerBG32f => write_converted!(BayerBG32f, $writer, $x, $timestamp),
            PixFmt::YUV422 => write_converted!(YUV422, $writer, $x, $timestamp),
            _ => {
                anyhow::bail!("unsupported pixel format {}", $new_pixel_format);
            }
        }
    }};
}

/// For a specified runtime specified pixel format, convert and save to FMF file.
macro_rules! write_converted {
    ($pixfmt:ty, $writer:expr, $x:expr, $timestamp:expr) => {{
        let converted_frame = convert_image::convert::<_, $pixfmt>($x)?;
        $writer.write(&converted_frame, $timestamp)?;
    }};
}

#[derive(Debug, StructOpt)]
#[structopt(name = "fmf", about = "work with .fmf (fly movie format) files")]
enum Opt {
    /// export an fmf file
    #[structopt(name = "export-fmf")]
    ExportFMF {
        /// new pixel_format (default: no change from input fmf)
        #[structopt(long = "pixel-format", name = "NEW-PIXEL-FORMAT")]
        new_pixel_format: Option<PixFmt>,

        /// force input data to be interpreted with this pixel_format
        #[structopt(long = "force-input-pixel-format", name = "FORCED-INPUT-PIXEL-FORMAT")]
        forced_input_pixel_format: Option<PixFmt>,

        /// Filename of input fmf
        #[structopt(parse(from_os_str), name = "INPUT-FMF")]
        input: PathBuf,

        /// Filename of output .fmf, "-" for stdout
        #[structopt(long = "output", short = "o", name = "OUTPUT-FMF", parse(from_os_str))]
        output: Option<PathBuf>,
    },

    /// print information about an fmf file
    #[structopt(name = "info")]
    Info {
        /// Filename of input fmf
        #[structopt(parse(from_os_str), name = "INPUT-FMF")]
        input: PathBuf,
    },

    /// export a sequence of jpeg images
    #[structopt(name = "export-jpeg")]
    ExportJpeg {
        /// Filename of input fmf
        #[structopt(parse(from_os_str), name = "INPUT-FMF")]
        input: PathBuf,

        /// Quality (1-100 where 1 is the worst and 100 is the best)
        #[structopt(name = "QUALITY", long = "quality", short = "q", default_value = "99")]
        quality: u8,
    },

    /// export a sequence of png images
    #[structopt(name = "export-png")]
    ExportPng {
        /// Filename of input fmf
        #[structopt(parse(from_os_str), name = "INPUT-FMF")]
        input: PathBuf,
    },

    /// export to y4m (YUV4MPEG2) format
    #[structopt(name = "export-y4m")]
    ExportY4m(ExportY4m),

    // /// export to bgr24 raw
    // #[structopt(name = "export-bgr24")]
    // ExportBgr24(ExportBgr24),
    /// export to mkv
    #[structopt(name = "export-mkv")]
    ExportMkv(ExportMkv),

    /// import a sequence of images, converting it to an FMF file
    #[structopt(name = "import-images")]
    ImportImages {
        /// Input images (glob pattern like "*.png")
        #[structopt(name = "INPUT-GLOB")]
        input: String,

        /// Filename of output fmf
        #[structopt(parse(from_os_str), long = "output", short = "o", name = "OUTPUT-FMF")]
        output: PathBuf,
    },

    /// import a webm file, converting it to an FMF file
    #[cfg(feature = "import-webm")]
    #[structopt(name = "import-webm")]
    ImportWebm(ImportWebm),
}

#[derive(StructOpt, Debug)]
struct ExportY4m {
    /// Filename of input fmf
    #[structopt(parse(from_os_str), name = "INPUT-FMF")]
    input: PathBuf,

    /// Filename of output .y4m, "-" for stdout
    #[structopt(parse(from_os_str), long = "output", short = "o")]
    output: Option<PathBuf>,

    /// colorspace (e.g. 420paldv, mono)
    #[structopt(long = "colorspace", short = "c", default_value = "420paldv")]
    colorspace: Y4MColorspace,

    /// frames per second numerator
    #[structopt(default_value = "25", long = "fps-numerator")]
    fps_numerator: u32,

    /// frames per second denominator
    #[structopt(default_value = "1", long = "fps-denominator")]
    fps_denominator: u32,

    /// aspect ratio numerator
    #[structopt(default_value = "1", long = "aspect-numerator")]
    aspect_numerator: u32,

    /// aspect ratio denominator
    #[structopt(default_value = "1", long = "aspect-denominator")]
    aspect_denominator: u32,
}

// #[derive(StructOpt, Debug)]
// struct ExportBgr24 {
//     /// Filename of input fmf
//     #[structopt(parse(from_os_str), name="INPUT-FMF")]
//     input: PathBuf,

//     /// Filename of output .bgr24, "-" for stdout
//     #[structopt(parse(from_os_str), long="output", short="o")]
//     output: Option<PathBuf>,

//     /// autocrop (e.g. none, even, mod16)
//     #[structopt(long="autocrop", short="a", default_value="mod16")]
//     autocrop: Autocrop,
// }

#[derive(StructOpt, Debug)]
struct ExportMkv {
    /// Filename of input fmf
    #[structopt(parse(from_os_str), name = "INPUT-FMF")]
    input: PathBuf,

    /// Filename of output .mkv, "-" for stdout
    #[structopt(parse(from_os_str), long = "output", short = "o")]
    output: Option<PathBuf>,

    // /// autocrop (e.g. none, even, mod16)
    // #[structopt(long="autocrop", short="a", default_value="mod16")]
    // autocrop: Autocrop,
    /// video bitrate
    #[structopt(long = "bitrate", short = "b", default_value = "1000")]
    bitrate: u32,

    /// video codec
    #[structopt(long = "codec", default_value = "vp9")]
    codec: Codec,

    /// clip the width of the incoming frames to be divisible by this number
    #[structopt(long = "clip-divisible", default_value = "1")]
    clip_so_width_is_divisible_by: u8,
}

#[derive(Debug)]
enum Codec {
    Vp8,
    Vp9,
    #[cfg(feature = "nv-h264")]
    H264,
}

impl std::str::FromStr for Codec {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "vp8" | "Vp8" | "VP8" => Ok(Codec::Vp8),
            "vp9" | "Vp9" | "VP9" => Ok(Codec::Vp9),
            #[cfg(feature = "nv-h264")]
            "h264" | "H264" => Ok(Codec::H264),
            c => Err(format!("unknown codec: {}", c)),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Autocrop {
    None,
    Even,
    Mod16,
}

impl std::str::FromStr for Autocrop {
    type Err = &'static str;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "None" | "none" => Ok(Autocrop::None),
            "Even" | "even" => Ok(Autocrop::Even),
            "Mod16" | "mod16" => Ok(Autocrop::Mod16),
            _ => Err("unknown autocrop"),
        }
    }
}

#[cfg(feature = "import-webm")]
#[derive(StructOpt, Debug)]
struct ImportWebm {
    /// Filename of input webm
    #[structopt(parse(from_os_str), name = "INPUT-WEBM")]
    input: PathBuf,

    /// Filename of output .fmf, "-" for stdout
    #[structopt(parse(from_os_str), long = "output", short = "o")]
    output: Option<PathBuf>,
}

/// convert None into default name, convert "-" into None (for stdout)
fn default_filename(path: &Path, output: Option<PathBuf>, ext: &str) -> Option<PathBuf> {
    match output {
        Some(x) => {
            if x.to_str() == Some("-") {
                None
            } else {
                Some(x)
            }
        }
        None => {
            let mut stem = path.file_stem().unwrap().to_os_string(); // strip extension
            stem.push(format!(".exported.{}", ext));
            Some(path.with_file_name(&stem))
        }
    }
}

fn display_filename(p: &Option<PathBuf>, default: &str) -> PathBuf {
    match p {
        Some(x) => x.clone(),
        None => std::path::Path::new(default).to_path_buf(),
    }
}

fn info(path: PathBuf) -> Result<()> {
    #[derive(Debug)]
    #[allow(dead_code)]
    struct Info {
        width: u32,
        height: u32,
        stride: usize,
        pixel_format: PixFmt,
    }
    let reader = fmf::FMFReader::new(&path)?;
    for (fno, frame) in reader.enumerate() {
        let i = Info {
            width: frame.width(),
            stride: frame.stride(),
            height: frame.height(),
            pixel_format: frame.pixel_format(),
        };
        if fno == 0 {
            println!("{:?}", i);
        }
        println!("frame {}: {}", fno, frame.extra().host_timestamp());
    }
    Ok(())
}

/// Write an fmf file
///
/// If the `forced_input_pixel_format` argument is not None, it forces the
/// interpretation of the original data into this format regardless of the pixel
/// format specied in the header of the input file.
fn export_fmf(
    path: PathBuf,
    new_pixel_format: Option<PixFmt>,
    output: Option<PathBuf>,
    forced_input_pixel_format: Option<PixFmt>,
) -> Result<()> {
    let output_fname = default_filename(&path, output, "fmf");

    info!(
        "exporting {} to {}",
        path.display(),
        display_filename(&output_fname, "<stdout>").display()
    );
    let reader = fmf::FMFReader::new(&path)?;

    let output_fname = output_fname.unwrap(); // XXX temp hack FIXME

    let f = std::fs::File::create(&output_fname)?;
    let mut writer = fmf::FMFWriter::new(f)?;

    for frame in reader {
        let fts = frame.extra().host_timestamp();
        let frame: DynamicFrame = match forced_input_pixel_format {
            Some(forced_input_pixel_format) => frame.force_pixel_format(forced_input_pixel_format),
            None => frame,
        };

        let fmt = match new_pixel_format {
            Some(new_pixel_format) => new_pixel_format,
            None => frame.pixel_format(),
        };

        match_all_dynamic_fmts!(frame, x, convert_and_write_fmf!(fmt, writer, &x, fts));
    }
    Ok(())
}

fn import_images(pattern: &str, output_fname: PathBuf) -> Result<()> {
    let opts = glob::MatchOptions::new();
    let paths = glob::glob_with(pattern, opts)?;
    let f = std::fs::File::create(&output_fname)?;
    let mut writer = fmf::FMFWriter::new(f)?;

    for path in paths {
        let piston_image = image::open(&path?)?;
        let converted_frame = convert_image::piston_to_frame(piston_image)?;
        writer.write(&converted_frame, chrono::Utc::now())?;
    }
    Ok(())
}

#[cfg(feature = "import-webm")]
fn import_webm(x: ImportWebm) -> Result<()> {
    let output_fname = default_filename(&x.input, x.output, "fmf");

    info!(
        "importing {} to {}",
        x.input.display(),
        display_filename(&output_fname, "<stdout>").display()
    );

    let in_fd = std::fs::File::open(&x.input).unwrap();

    let _reader = webm::parser::Reader::new(in_fd);

    unimplemented!();

    // let f = std::fs::File::create(&output_fname)?;
    // let mut writer = fmf::FMFWriter::new(f)?;

    // Ok(())
}

fn convert_to_rgb8(
    frame: &DynamicFrame,
) -> std::result::Result<Box<dyn ImageStride<pixel_format::RGB8> + '_>, convert_image::Error> {
    let f: Box<dyn ImageStride<_>> =
        match_all_dynamic_fmts!(frame, x, Box::new(convert_image::convert(x)?));
    Ok(f)
}

fn export_images(path: PathBuf, opts: ImageOptions) -> Result<()> {
    use std::io::Write;

    let stem = path.file_stem().unwrap().to_os_string(); // strip extension
    let dirname = path.with_file_name(&stem);

    let ext = match opts {
        ImageOptions::Jpeg(_) => "jpg",
        ImageOptions::Png => "png",
    };

    info!("saving {} images to {}", ext, dirname.display());

    match std::fs::create_dir(&dirname) {
        Ok(()) => {}
        Err(e) => match e.kind() {
            std::io::ErrorKind::AlreadyExists => {}
            _ => {
                return Err(e.into());
            }
        },
    }

    let reader = fmf::FMFReader::new(&path)?;

    for (i, frame) in reader.enumerate() {
        let file = format!("frame{:05}.{}", i, ext);
        let fname = dirname.join(&file);
        let frame = convert_to_rgb8(&frame)?;
        let buf = convert_image::frame_to_image(frame.as_ref(), opts)?;
        let mut fd = std::fs::File::create(fname)?;
        fd.write_all(&buf)?;
    }
    Ok(())
}

// fn do_autocrop(w: usize, h: usize, autocrop: Autocrop) -> (usize,usize) {
//     match autocrop {
//         Autocrop::None => (w,h),
//         Autocrop::Even => ((w/2)*2,h),
//         Autocrop::Mod16 => ((w/16)*16,(h/16)*16),
//     }
// }

// fn encode_bgr24_frame( frame: fmf::DynamicFrame, autocrop: Autocrop ) -> fmf::FMFResult<Vec<u8>> {
//     use PixFmt::*;

//     // convert bayer formats
//     let frame: ConvertImageFrame = match frame.pixel_format() {
//         BayerRG8 | BayerGB8 | BayerGR8 | BayerBG8 => {
//             convert_image::bayer_to_rgb(&frame).unwrap()
//         }
//         _ => {
//             frame.into()
//         }
//     };

//     match frame.pixel_format() {
//         // MONO8 => {
//         //     // Should we set RGB each to mono? Or use conversion YUV to RGB?
//         //     unimplemented!();
//         // }
//         RGB8 => {
//             let w = frame.width() as usize;
//             let h = frame.height() as usize;
//             let (w,h) = do_autocrop(w,h,autocrop);
//             let mut buf: Vec<u8> = Vec::with_capacity(w*h*3);
//             for i in 0..h {
//                 let rowidx = i*frame.stride();
//                 for j in 0..w {
//                     let colidx = j*3;
//                     let start = rowidx + colidx;
//                     let stop = start+3;
//                     let rgb = &frame.image_data()[start..stop];
//                     let b = rgb[2];
//                     let g = rgb[1];
//                     let r = rgb[0];
//                     buf.push(b);
//                     buf.push(g);
//                     buf.push(r);
//                 }
//             }
//             Ok(buf)
//         }
//         fmt => {
//             Err(fmf::FMFError::UnimplementedPixelFormat(fmt))
//         }
//     }
// }

// fn export_bgr24(x: ExportBgr24) -> Result<()> {
//     use std::io::Write;

//     let output_fname = default_filename(&x.input, x.output, "bgr24");

//     let reader = fmf::FMFReader::new(&x.input)?;
//     let (w,h) = do_autocrop(reader.width() as usize, reader.height() as usize, x.autocrop);

//     info!("exporting {} ({}x{}) to {}", x.input.display(), w, h,
//         display_filename(&output_fname, "<stdout>").display());

//     let mut out_fd: Box<dyn Write> = match output_fname {
//         None => Box::new(std::io::stdout()),
//         Some(path) => Box::new(std::fs::File::create(&path)?),
//     };

//     for frame in reader {
//         let buf = encode_bgr24_frame( frame, x.autocrop )?;
//         out_fd.write_all(&buf)?;
//     }
//     out_fd.flush()?;
//     Ok(())
// }

fn export_mkv(x: ExportMkv) -> Result<()> {
    // TODO: read this https://www.webmproject.org/docs/encoder-parameters/
    // also this https://www.webmproject.org/docs/webm-sdk/example_vp9_lossless_encoder.html

    let output_fname = default_filename(&x.input, x.output, "mkv");

    info!(
        "exporting {} to {}",
        x.input.display(),
        display_filename(&output_fname, "<stdout>").display()
    );

    let out_fd = match &output_fname {
        None => {
            anyhow::bail!("Cannot export mkv to stdout."); // Seek required
        }
        Some(path) => std::fs::File::create(&path)?,
    };

    let reader = fmf::FMFReader::new(&x.input)?;

    let codec = match x.codec {
        Codec::Vp8 => {
            let opts = ci2_remote_control::VP8Options { bitrate: x.bitrate };
            ci2_remote_control::MkvCodec::VP8(opts)
        }
        Codec::Vp9 => {
            let opts = ci2_remote_control::VP9Options { bitrate: x.bitrate };
            ci2_remote_control::MkvCodec::VP9(opts)
        }
        #[cfg(feature = "nv-h264")]
        Codec::H264 => {
            let opts = ci2_remote_control::H264Options {
                bitrate: x.bitrate,
                ..Default::default()
            };
            ci2_remote_control::MkvCodec::H264(opts)
        }
    };

    let cfg = MkvRecordingConfig {
        codec,
        max_framerate: ci2_remote_control::RecordingFrameRate::Unlimited,
        writing_application: Some("fmf-cli".to_string()),
        ..Default::default()
    };

    #[cfg(feature = "nv-h264")]
    let libs = nvenc::Dynlibs::new()?;

    #[cfg(feature = "nv-h264")]
    let nv_enc = Some(nvenc::NvEnc::new(&libs)?);

    #[cfg(not(feature = "nv-h264"))]
    let nv_enc = None;

    debug!("opening file {}", output_fname.unwrap().display());
    let mut my_mkv_writer = mkv_writer::MkvWriter::new(out_fd, cfg, nv_enc)?;

    for (fno, fmf_frame) in reader.enumerate() {
        debug!("saving frame {}", fno);
        let ts = fmf_frame.extra().host_timestamp();
        match fmf_frame {
            DynamicFrame::Mono8(mono_frame) => {
                let fmf_frame_clipped =
                    mono_frame.clip_to_power_of_2(x.clip_so_width_is_divisible_by);
                my_mkv_writer.write(&fmf_frame_clipped, ts)?;
            }
            other_frame => {
                let host_framenumber = other_frame.extra().host_framenumber();
                let host_timestamp = other_frame.extra().host_timestamp();
                let rgb_frame = convert_to_rgb8(&other_frame)?;
                let rgb_frame = {
                    let width = rgb_frame.width();
                    let height = rgb_frame.height();
                    let stride = rgb_frame.stride() as u32;
                    let image_data = rgb_frame.buffer_ref().data.to_vec(); // copy data
                    let extra = Box::new(BasicExtra {
                        host_framenumber,
                        host_timestamp,
                    });
                    basic_frame::BasicFrame::<pixel_format::RGB8> {
                        width,
                        height,
                        stride,
                        extra,
                        image_data,
                        pixel_format: std::marker::PhantomData,
                    }
                };
                let fmf_frame_clipped =
                    rgb_frame.clip_to_power_of_2(x.clip_so_width_is_divisible_by);
                my_mkv_writer.write(&fmf_frame_clipped, ts)?;
            }
        }
    }

    debug!("finishing file");
    my_mkv_writer.finish()?;
    Ok(())
}

/// A view of a source image in which the rightmost pixels may be clipped
struct ClippedFrame<'a, FMT> {
    src: &'a basic_frame::BasicFrame<FMT>,
    width: u32,
}

impl<'a, FMT> ImageData<FMT> for ClippedFrame<'a, FMT> {
    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.src.height()
    }
    fn buffer_ref(&self) -> ImageBufferRef<'a, FMT> {
        ImageBufferRef::new(self.src.image_data())
    }
    fn buffer(self) -> ImageBuffer<FMT> {
        ImageBuffer::new(self.buffer_ref().data.to_vec()) // copy data
    }
}

impl<'a, FMT> Stride for ClippedFrame<'a, FMT> {
    fn stride(&self) -> usize {
        self.src.stride()
    }
}

trait ClipFrame<FMT> {
    fn clip_to_power_of_2(&self, val: u8) -> ClippedFrame<FMT>;
}

impl<FMT> ClipFrame<FMT> for basic_frame::BasicFrame<FMT> {
    fn clip_to_power_of_2(&self, val: u8) -> ClippedFrame<FMT> {
        let width = (self.width() / val as u32) * val as u32;
        debug!("clipping image of width {} to {}", self.width(), width);
        ClippedFrame { src: self, width }
    }
}

fn export_y4m(x: ExportY4m) -> Result<()> {
    use std::io::Write;

    let output_fname = default_filename(&x.input, x.output, "y4m");

    info!(
        "exporting {} to {}",
        x.input.display(),
        display_filename(&output_fname, "<stdout>").display()
    );

    let mut out_fd: Box<dyn Write> = match output_fname {
        None => Box::new(std::io::stdout()),
        Some(path) => Box::new(std::fs::File::create(&path)?),
    };

    let reader = fmf::FMFReader::new(&x.input)?;
    let mut buffer_width = reader.width();
    let buffer_height = reader.height();

    if reader.format() == PixFmt::RGB8 {
        buffer_width *= 3;
    }

    let final_width = match reader.format() {
        PixFmt::RGB8 => buffer_width / 3,
        _ => buffer_width,
    };
    let final_height = buffer_height;

    let inter = "Ip"; // progressive

    let buf = format!(
        "{magic} W{width} H{height} \
                    F{raten}:{rated} {inter} A{aspectn}:{aspectd} \
                    C{colorspace} Xconverted_by-fmf-cli\n",
        magic = Y4M_MAGIC,
        width = final_width,
        height = final_height,
        raten = x.fps_numerator,
        rated = x.fps_denominator,
        inter = inter,
        aspectn = x.aspect_numerator,
        aspectd = x.aspect_denominator,
        colorspace = x.colorspace
    );
    out_fd.write_all(buf.as_bytes())?;

    for frame in reader {
        let buf = format!("{magic}\n", magic = Y4M_FRAME_MAGIC);
        out_fd.write_all(buf.as_bytes())?;

        basic_frame::match_all_dynamic_fmts!(frame, f, {
            let buf = encode_y4m_frame(&f, x.colorspace)?;
            out_fd.write_all(&buf)?;
        });
    }
    out_fd.flush()?;
    Ok(())
}

fn main() -> Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "fmf=info,error");
    }

    env_logger::init();
    let opt = Opt::from_args();

    match opt {
        Opt::ExportFMF {
            input,
            new_pixel_format,
            output,
            forced_input_pixel_format,
        } => {
            export_fmf(input, new_pixel_format, output, forced_input_pixel_format)?;
        }
        Opt::Info { input } => {
            info(input)?;
        }
        Opt::ExportJpeg { input, quality } => {
            export_images(input, ImageOptions::Jpeg(quality))?;
        }
        Opt::ExportPng { input } => {
            export_images(input, ImageOptions::Png)?;
        }
        Opt::ExportY4m(x) => {
            export_y4m(x)?;
        }
        // Opt::ExportBgr24(x) => {
        //     export_bgr24(x)?;
        // },
        Opt::ExportMkv(x) => {
            export_mkv(x)?;
        }
        Opt::ImportImages { input, output } => {
            import_images(&input, output)?;
        }
        #[cfg(feature = "import-webm")]
        Opt::ImportWebm(x) => {
            import_webm(x)?;
        }
    }
    Ok(())
}
