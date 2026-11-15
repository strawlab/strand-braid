// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use anyhow::Result;
use tracing::info;

use clap::Parser;
use convert_image::EncoderOptions;
use machine_vision_formats::{Stride, pixel_format, pixel_format::PixFmt};
use std::path::{Path, PathBuf};
use strand_dynamic_frame::{DynamicFrame, match_all_dynamic_fmts};
use y4m::Colorspace;

use pending_file::PendingFile;

/*

Examples of exporting from FMF to MKV with `ffv1` codec. Note these all loose
timestamp data:

    fmf export-y4m test_rgb8.fmf -o - | ffmpeg -i - -vcodec ffv1 test_yuv.mkv

Example export to mkv for mono8. Will lose timestamps:

    fmf export-y4m test_mono8.fmf -o - | ffmpeg -i - -vcodec ffv1 test_mono8.mkv

Example export to mkv via RGB (should be lossless for image data). Will loose timestamps:

    fmf export-bgr24 test_rgb8.fmf -o - | ffmpeg -i - -f rawvideo -pix_fmt bgr0 -s 332x332 -r 30 -vcodec ffv1 test_bgr.mkv

Example export to mp4 with hardware h264 encoding using VAAPI. Will loose timestamps:

    fmf export-y4m test_rgb8.fmf -o - | ffmpeg -vaapi_device /dev/dri/renderD128 -i - -vf format=nv12,hwupload -c:v h264_vaapi -b:v 5M from-fmf.mp4

Note that MKV's `DateUTC` metadata creation time can be set when creating an MKV
video in ffmpeg with the option `-metadata creation_time="2012-02-07 12:15:27"`.
However, as of the time of writing, ffmpeg only parses the command line date to
the second (whereas the MKV spec allows better precision).

To convert to mp4 and keep the timestamps, use `strand-convert`:

    strand-convert -i test_rgb8.fmf -o /tmp/test.mp4

*/

/// Convert to runtime specified pixel format and save to FMF file.
macro_rules! convert_and_write_fmf {
    ($new_pixel_format:expr_2021, $writer:expr_2021, $x:expr_2021, $timestamp:expr_2021) => {{
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
    ($pixfmt:ty, $writer:expr_2021, $x:expr_2021, $timestamp:expr_2021) => {{
        let converted_frame = convert_image::convert_ref::<_, $pixfmt>($x)?;
        $writer.write(&converted_frame, $timestamp)?;
    }};
}

#[derive(Debug, Parser)]
#[command(name = "fmf", about, version)]
enum Opt {
    /// export an fmf file
    ExportFMF {
        /// new pixel_format (default: no change from input fmf)
        #[arg(long)]
        new_pixel_format: Option<PixFmt>,

        /// force input data to be interpreted with this pixel_format
        #[arg(long)]
        forced_input_pixel_format: Option<PixFmt>,

        /// Filename of input fmf
        input: PathBuf,

        /// Filename of output .fmf, "-" for stdout
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// print information about an fmf file
    Info {
        /// Filename of input fmf
        input: PathBuf,
    },

    /// export a sequence of jpeg images
    ExportJpeg {
        /// Filename of input fmf
        input: PathBuf,

        /// Quality (1-100 where 1 is the worst and 100 is the best)
        #[arg(short, long, default_value = "99")]
        quality: u8,
    },

    /// export a sequence of png images
    ExportPng {
        /// Filename of input fmf
        input: PathBuf,
    },

    /// export to y4m (YUV4MPEG2) format
    ExportY4m(ExportY4m),

    /// import a sequence of images, converting it to an FMF file
    ImportImages {
        /// Input images (glob pattern like "*.png")
        input: String,

        /// Filename of output fmf
        #[arg(short, long)]
        output: PathBuf,
    },
}

#[derive(Parser, Debug)]
struct ExportY4m {
    /// Filename of input fmf
    input: PathBuf,

    /// Filename of output .y4m, "-" for stdout
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// colorspace (e.g. 420paldv, mono)
    #[arg(short, long, default_value = "420paldv", value_parser = str_to_colorspace)]
    colorspace: Colorspace,

    /// frames per second numerator
    #[arg(long, default_value = "25")]
    fps_numerator: u32,

    /// frames per second denominator
    #[arg(long, default_value = "1")]
    fps_denominator: u32,

    /// aspect ratio numerator
    #[arg(long, default_value = "1")]
    aspect_numerator: u32,

    /// aspect ratio denominator
    #[arg(long, default_value = "1")]
    aspect_denominator: u32,
}

fn str_to_colorspace(s: &str) -> anyhow::Result<Colorspace> {
    match s {
        "420paldv" => Ok(Colorspace::C420paldv),
        "mono" => Ok(Colorspace::Cmono),
        s => anyhow::bail!("Unknown colorspace string: {s}"),
    }
}

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
    #[expect(dead_code)]
    struct Info {
        width: u32,
        height: u32,
        stride: usize,
        pixel_format: PixFmt,
    }
    let reader = fmf::FMFReader::new(&path)?;
    for (fno, res_frame) in reader.enumerate() {
        let (frame_o, timestamp) = res_frame?;
        let frame = frame_o.borrow();
        let i = Info {
            width: frame.width(),
            stride: frame.stride(),
            height: frame.height(),
            pixel_format: frame.pixel_format(),
        };
        if fno == 0 {
            println!("{:?}", i);
        }
        println!("frame {}: {}", fno, timestamp);
    }
    Ok(())
}

/// Write an fmf file
///
/// If the `forced_input_pixel_format` argument is not None, it forces the
/// interpretation of the original data into this format regardless of the pixel
/// format specified in the header of the input file.
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

    let (f, pending) = PendingFile::create(&output_fname)?;
    let mut writer = fmf::FMFWriter::new(f)?;

    for res_frame in reader {
        let (frame_o, fts) = res_frame?;
        let frame = frame_o.borrow();
        let frame: DynamicFrame = match forced_input_pixel_format {
            Some(forced_input_pixel_format) => {
                frame.force_pixel_format(forced_input_pixel_format).unwrap()
            }
            None => frame,
        };

        let fmt = match new_pixel_format {
            Some(new_pixel_format) => new_pixel_format,
            None => frame.pixel_format(),
        };

        match_all_dynamic_fmts!(
            frame,
            x,
            convert_and_write_fmf!(fmt, writer, &x, fts),
            anyhow::anyhow!("unimplemented pixel format {}", fmt)
        );
    }
    writer.close()?;
    pending.commit()?;
    Ok(())
}

fn import_images(pattern: &str, output_fname: PathBuf) -> Result<()> {
    let opts = glob::MatchOptions::new();
    let paths = glob::glob_with(pattern, opts)?;
    let (f, pending) = PendingFile::create(&output_fname)?;
    let mut writer = fmf::FMFWriter::new(f)?;

    for path in paths {
        let piston_image = image::open(&path?)?;
        let converted_frame = convert_image::image_to_rgb8(piston_image)?;
        writer.write(&converted_frame, chrono::Utc::now())?;
    }
    writer.close()?;
    pending.commit()?;
    Ok(())
}

fn export_images(path: PathBuf, opts: EncoderOptions) -> Result<()> {
    use std::io::Write;

    let stem = path.file_stem().unwrap().to_os_string(); // strip extension
    let dirname = path.with_file_name(&stem);

    let ext = match opts {
        EncoderOptions::Jpeg(_) => "jpg",
        EncoderOptions::Png => "png",
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

    for (i, res_frame) in reader.enumerate() {
        let (frame_o, _) = res_frame?;
        let frame = frame_o.borrow();
        let file = format!("frame{:05}.{}", i, ext);
        let fname = dirname.join(&file);
        let buf = frame.to_encoded_buffer(opts)?;
        let mut fd = std::fs::File::create(fname)?;
        fd.write_all(&buf)?;
    }
    Ok(())
}

fn export_y4m(x: ExportY4m) -> Result<()> {
    use std::io::Write;

    let output_fname = default_filename(&x.input, x.output, "y4m");

    info!(
        "exporting {} to {}",
        x.input.display(),
        display_filename(&output_fname, "<stdout>").display()
    );

    let (out_fd, pending): (Box<dyn Write>, _) = match output_fname {
        None => (Box::new(std::io::stdout()), None),
        Some(path) => {
            let (out_fd, pending) = PendingFile::create(&path)?;
            (Box::new(out_fd), Some(pending))
        }
    };

    let opts = y4m_writer::Y4MOptions {
        aspectn: x.aspect_numerator.try_into().unwrap(),
        aspectd: x.aspect_denominator.try_into().unwrap(),
        raten: x.fps_numerator.try_into().unwrap(),
        rated: x.fps_denominator.try_into().unwrap(),
    };
    let mut y4m_writer = y4m_writer::Y4MWriter::from_writer(out_fd, opts);

    let reader = fmf::FMFReader::new(&x.input)?;

    for res_frame in reader {
        let (frame, _) = res_frame?;
        y4m_writer.write_dynamic_frame(&frame.borrow())?;
    }
    drop(y4m_writer);
    if let Some(pending) = pending {
        pending.commit()?;
    }
    Ok(())
}

fn main() -> Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("RUST_LOG", "fmf=info,warn") };
    }

    env_logger::init();
    let opt = Opt::parse();

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
            export_images(input, EncoderOptions::Jpeg(quality))?;
        }
        Opt::ExportPng { input } => {
            export_images(input, EncoderOptions::Png)?;
        }
        Opt::ExportY4m(x) => {
            export_y4m(x)?;
        }
        // Opt::ExportBgr24(x) => {
        //     export_bgr24(x)?;
        // },
        Opt::ImportImages { input, output } => {
            import_images(&input, output)?;
        }
    }
    Ok(())
}

/// The names in `dir`, sorted.
#[cfg(test)]
fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

/// Write an FMF file of three small frames at `path`.
#[cfg(test)]
fn write_test_fmf(path: &Path) -> anyhow::Result<()> {
    use machine_vision_formats::pixel_format::Mono8;

    let mut writer = fmf::FMFWriter::new(std::fs::File::create(path)?)?;
    for i in 0..3u8 {
        let frame =
            machine_vision_formats::owned::OImage::<Mono8>::new(32, 16, 32, vec![i * 60; 32 * 16])
                .unwrap();
        let stamp = chrono::DateTime::from_timestamp(61 + i64::from(i), 0).unwrap();
        writer.write(&frame, stamp)?;
    }
    writer.close()?;
    Ok(())
}

/// Each export writes under a temporary name beside its output and moves the
/// finished file into place, so afterwards only the output is there.
#[test]
fn exports_leave_only_their_output() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let input = dir.path().join("in.fmf");
    write_test_fmf(&input)?;

    export_fmf(input.clone(), None, Some(dir.path().join("out.fmf")), None)?;
    assert_eq!(names_in(dir.path()), ["in.fmf", "out.fmf"]);
    assert_eq!(fmf::FMFReader::new(dir.path().join("out.fmf"))?.count(), 3);

    export_y4m(ExportY4m {
        input,
        output: Some(dir.path().join("out.y4m")),
        colorspace: Colorspace::Cmono,
        fps_numerator: 25,
        fps_denominator: 1,
        aspect_numerator: 1,
        aspect_denominator: 1,
    })?;
    assert_eq!(names_in(dir.path()), ["in.fmf", "out.fmf", "out.y4m"]);
    Ok(())
}

/// An export which fails partway, here because its input is cut short, leaves
/// nothing behind, and leaves an output from an earlier run as it was.
#[test]
fn failed_export_leaves_no_output() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let complete = dir.path().join("complete.fmf");
    write_test_fmf(&complete)?;
    let truncated = dir.path().join("truncated.fmf");
    let bytes = std::fs::read(&complete)?;
    std::fs::write(&truncated, &bytes[..bytes.len() - 100])?;
    let before = names_in(dir.path());

    assert!(
        export_fmf(
            truncated.clone(),
            None,
            Some(dir.path().join("out.fmf")),
            None
        )
        .is_err()
    );
    assert_eq!(names_in(dir.path()), before);

    let earlier = dir.path().join("earlier.fmf");
    std::fs::write(&earlier, "from an earlier run")?;
    assert!(export_fmf(truncated, None, Some(earlier.clone()), None).is_err());
    assert_eq!(std::fs::read_to_string(&earlier)?, "from an earlier run");
    Ok(())
}

#[test]
fn test_y4m() -> anyhow::Result<()> {
    use machine_vision_formats::pixel_format::{Mono8, RGB8};

    let start = chrono::DateTime::from_timestamp(61, 0).unwrap();
    for output_colorspace in [Colorspace::Cmono, Colorspace::C420paldv] {
        for input_colorspace in [PixFmt::Mono8, PixFmt::RGB8] {
            let tmpdir = tempfile::tempdir()?;
            let base_path = tmpdir.path().to_path_buf();
            println!("files in base_path: {}", base_path.display());
            std::mem::forget(tmpdir);

            const W: usize = 8;
            const STEP: u8 = 32;
            assert_eq!(W * STEP as usize, 256);

            let width: u32 = W.try_into().unwrap();
            let height = 4;

            let mut image_data = vec![0u8; W * height as usize];
            for row_data in image_data.as_chunks_mut::<W>().0.iter_mut() {
                for (col, el) in row_data.iter_mut().enumerate() {
                    let col: u8 = col.try_into().unwrap();
                    *el = STEP * col;
                }
            }

            // make mono8 image. Will covert to input_colorspace below.
            let frame = machine_vision_formats::owned::OImage::<Mono8>::new(
                width,
                height,
                width.try_into().unwrap(),
                image_data,
            )
            .unwrap();
            let orig_rgb8 = convert_image::convert_ref::<_, RGB8>(&frame)?;

            let fmf_fname = base_path.join("test.fmf");
            let y4m_fname = base_path.join("test.y4m");
            {
                let fd = std::fs::File::create(&fmf_fname)?;
                let mut writer = fmf::FMFWriter::new(fd)?;

                match input_colorspace {
                    PixFmt::Mono8 => {
                        let converted_frame = convert_image::convert_ref::<_, Mono8>(&frame)?;
                        writer.write(&converted_frame, start)?;
                    }
                    PixFmt::RGB8 => {
                        let converted_frame = convert_image::convert_ref::<_, RGB8>(&frame)?;
                        writer.write(&converted_frame, start)?;
                    }
                    _ => {
                        todo!();
                    }
                }
            }

            let x = ExportY4m {
                input: fmf_fname,
                output: Some(y4m_fname.clone()),
                colorspace: output_colorspace,
                fps_numerator: 25,
                fps_denominator: 1,
                aspect_numerator: 1,
                aspect_denominator: 1,
            };

            export_y4m(x)?;

            let loaded = ffmpeg_to_frame(&y4m_fname, &base_path)?;
            let loaded: &dyn machine_vision_formats::ImageStride<_> = &loaded;
            let orig_rgb8: &dyn machine_vision_formats::ImageStride<_> = &orig_rgb8;
            println!("{input_colorspace:?} -> {output_colorspace:?}");
            for im in [loaded, orig_rgb8].iter() {
                println!("{:?}", im.image_data());
            }
            assert!(are_images_equal(loaded, orig_rgb8));
        }
    }
    Ok(())
}

#[cfg(test)]
fn are_images_equal<FMT>(
    frame1: &dyn machine_vision_formats::ImageStride<FMT>,
    frame2: &dyn machine_vision_formats::ImageStride<FMT>,
) -> bool
where
    FMT: machine_vision_formats::PixelFormat,
{
    let width = frame1.width();

    if frame1.width() != frame2.width() {
        return false;
    }
    if frame1.height() != frame2.height() {
        return false;
    }

    let fmt = machine_vision_formats::pixel_format::pixfmt::<FMT>().unwrap();
    let valid_stride = fmt.bits_per_pixel() as usize * width as usize / 8;

    for (f1_row, f2_row) in frame1
        .image_data()
        .chunks_exact(frame1.stride())
        .zip(frame2.image_data().chunks_exact(frame2.stride()))
    {
        let f1_valid = &f1_row[..valid_stride];
        let f2_valid = &f2_row[..valid_stride];
        if f1_valid != f2_valid {
            return false;
        }
    }

    true
}

#[cfg(test)]
fn ffmpeg_to_frame(
    fname: &std::path::Path,
    base_path: &std::path::Path,
) -> anyhow::Result<
    impl machine_vision_formats::OwnedImageStride<machine_vision_formats::pixel_format::RGB8> + use<>,
> {
    use anyhow::Context;

    let png_fname = base_path.join("frame1.png");
    let args = [
        "-i",
        &format!("{}", fname.display()),
        &format!("{}", png_fname.display()),
    ];
    let output = std::process::Command::new("ffmpeg")
        .args(args)
        .output()
        .with_context(|| format!("When running: ffmpeg {:?}", args))?;

    if !output.status.success() {
        anyhow::bail!(
            "'ffmpeg {}' failed. stdout: {}, stderr: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let piston_image =
        image::open(&png_fname).with_context(|| format!("Opening {}", png_fname.display()))?;
    let decoded = convert_image::image_to_rgb8(piston_image)?;
    Ok(decoded)
}
