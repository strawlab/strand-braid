// Copyright 2022-2023 Andrew D. Straw.
use std::{
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use machine_vision_formats::{pixel_format::pixfmt, ImageStride, OwnedImageStride, PixelFormat};
use openh264::formats::YUVSource;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Sets input file name
    input_fname: PathBuf,

    /// Sets the parser to use
    #[arg(short, long, value_enum)]
    parser: Option<MediaParser>,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum MediaParser {
    ImageCrate,
    OpenH264,
    Ffprobe,
    Y4m,
    ImageRsTiff,
}
impl MediaParser {
    fn dump<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        match self {
            MediaParser::ImageCrate => image_dump(path),
            MediaParser::OpenH264 => open_h264_dump(path),
            MediaParser::Ffprobe => ffprobe_dump(path),
            MediaParser::Y4m => y4m_dump(path),
            MediaParser::ImageRsTiff => tiff_dump(path),
        }
    }
}

fn image_dump<P: AsRef<Path>>(path: P) -> Result<()> {
    let rgb8 = image_to_frame(path.as_ref())?;
    simple_dump(rgb8)?;
    Ok(())
}

fn tiff_dump<P: AsRef<Path>>(path: P) -> Result<()> {
    use tiff::decoder::DecodingResult;

    let rdr = std::fs::File::open(&path)?;
    let mut decoder = tiff::decoder::Decoder::new(rdr)?;
    let buf = decoder.read_image()?;
    let color = decoder.colortype()?;

    let (width, height) = decoder.dimensions()?;
    let width: usize = width.try_into()?;
    let expected_size = width * height as usize;

    let byte_order = decoder.byte_order();

    println!("TIFF {color:?}{byte_order:?} {width}x{height}");

    match (color, buf) {
        (tiff::ColorType::Gray(16), DecodingResult::U16(vals)) => {
            for row in vals.chunks_exact(width) {
                dump_row_u16(row, "  ");
            }
        }
        (tiff::ColorType::Gray(8), DecodingResult::U8(vals)) => {
            assert_eq!(vals.len(), expected_size);
            for row in vals.chunks_exact(width) {
                dump_row(row, "  ");
            }
        }
        (tiff::ColorType::RGB(8), DecodingResult::U8(vals)) => {
            for row in vals.chunks_exact(width * 3) {
                dump_row(row, "  ");
            }
        }
        (tiff::ColorType::RGB(16), DecodingResult::U16(vals)) => {
            for row in vals.chunks_exact(width * 3) {
                dump_row_u16(row, "  ");
            }
        }
        _ => {
            anyhow::bail!("unsupported tiff type");
        }
    };
    Ok(())
}

fn simple_dump<FRAME, FMT>(frame: FRAME) -> Result<()>
where
    FRAME: ImageStride<FMT>,
    FMT: PixelFormat,
{
    let fmt = pixfmt::<FMT>().unwrap();
    let fmt_str = fmt.as_str();
    let width = frame.width();
    let height = frame.height();
    let stride = frame.stride();

    println!("machine-vision-formats {fmt_str} {width}x{height} (stride {stride})");
    let valid_stride = fmt.bits_per_pixel() as usize * width as usize / 8;

    for full_row in frame.image_data().chunks_exact(stride) {
        let row = &full_row[..valid_stride];
        dump_row(row, "  ");
    }
    Ok(())
}

fn y4m_dump<P: AsRef<Path>>(path: P) -> Result<()> {
    let mut reader = std::fs::File::open(&path)
        .with_context(|| format!("Opening {}", path.as_ref().display()))?;

    let mut decoder = y4m::decode(&mut reader)?;
    let (width, height) = (decoder.get_width(), decoder.get_height());
    println!("Y4M {width}x{height}");

    let colorspace = decoder.get_colorspace();
    let (luma_stride, chroma_stride) = match colorspace {
        y4m::Colorspace::C420jpeg | y4m::Colorspace::C420paldv => (width, width / 2),
        y4m::Colorspace::Cmono => (width, 0),
        y4m::Colorspace::Cmono12 => (width * 2, 0),
        y4m::Colorspace::C420p12 => (width * 2, width),
        other => {
            anyhow::bail!("unimplemented colorspace: {other:?}");
        }
    };

    let mut frame_num = 0;
    loop {
        match decoder.read_frame() {
            Ok(frame) => {
                println!("frame {frame_num} {colorspace:?}");
                frame_num += 1;

                println!("  luma (Y) ------");
                for row in frame.get_y_plane().chunks_exact(luma_stride) {
                    dump_row(row, "    ");
                }

                if chroma_stride > 0 {
                    println!("  chroma Cb (U) ------");
                    for row in frame.get_u_plane().chunks_exact(chroma_stride) {
                        dump_row(row, "    ");
                    }

                    println!("  chroma Cr (V) ------");
                    for row in frame.get_v_plane().chunks_exact(chroma_stride) {
                        dump_row(row, "    ");
                    }
                }
            }
            _ => break,
        }
    }
    Ok(())
}

fn image_to_frame(
    fname: &std::path::Path,
) -> Result<impl OwnedImageStride<machine_vision_formats::pixel_format::RGB8>> {
    let piston_image =
        image::open(&fname).with_context(|| format!("Opening {}", fname.display()))?;
    let decoded = convert_image::image_to_rgb8(piston_image)?;
    Ok(decoded)
}

fn ffprobe_dump<P: AsRef<Path>>(fname: P) -> Result<()> {
    let fname = fname.as_ref();
    let args = [
        "-i",
        &format!("{}", fname.display()),
        "-show_data",
        "-show_packets",
    ];
    let output = std::process::Command::new("ffprobe")
        .args(&args)
        .output()
        .with_context(|| format!("When running: ffprobe {:?}", args))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        anyhow::bail!(
            "'ffprobe {}' failed. stdout: {stdout}, stderr: {stderr}",
            args.join(" "),
        );
    }
    println!("{stderr}");
    println!("{stdout}");
    Ok(())
}

fn open_h264_dump<P: AsRef<Path>>(path: P) -> Result<()> {
    let mut fd = std::fs::File::open(path)?;
    let mut h264_raw_buf = vec![];
    fd.read_to_end(&mut h264_raw_buf)?;
    // decode a single frame
    let mut decoder = openh264::decoder::Decoder::new()?;
    let decoded_yuv = if let Some(decoded_yuv) = decoder.decode(&h264_raw_buf)? {
        decoded_yuv
    } else {
        anyhow::bail!("could not decode single frame with openh264");
    };

    let (width, height) = decoded_yuv.dimensions();
    println!("openh264 YUV420 size: {width}x{height}");
    let (oys, ous, ovs) = decoded_yuv.strides();

    println!("luma (Y) ------");
    for decoded_y_row in decoded_yuv.y().chunks_exact(oys) {
        dump_row(&decoded_y_row[..width as usize], "  ");
    }

    let chroma_width = (width / 2) as usize;

    println!("chroma Cb (U) ------");
    for decoded_u_row in decoded_yuv.u().chunks_exact(ous) {
        dump_row(&decoded_u_row[..chroma_width], "  ");
    }

    println!("chroma Cr (V) ------");
    for decoded_v_row in decoded_yuv.v().chunks_exact(ovs) {
        dump_row(&decoded_v_row[..chroma_width], "  ");
    }
    Ok(())
}

fn dump_row(slice: &[u8], prefix: &str) {
    let byte_strs: Vec<String> = slice.iter().map(|byte| format!("{byte:02X}")).collect();
    let long_str = byte_strs.join(" ");
    println!("{prefix}{long_str}");
}

fn dump_row_u16(slice: &[u16], prefix: &str) {
    let byte_strs: Vec<String> = slice
        .iter()
        .map(|val_u16| format!("{val_u16:04X}"))
        .collect();
    let long_str = byte_strs.join(" ");
    println!("{prefix}{long_str}");
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let parser = match cli.parser {
        Some(p) => Ok(p),
        None => {
            let ext: Option<&str> = cli.input_fname.extension().map(|x| x.to_str()).flatten();
            match ext {
                Some("tiff") | Some("tif") => Ok(MediaParser::ImageRsTiff),
                Some("bmp") | Some("jpeg") | Some("jpg") | Some("png") | Some("pbm")
                | Some("pgm") | Some("ppm") | Some("pam") | Some("gif") => {
                    Ok(MediaParser::ImageCrate)
                }
                Some("y4m") => Ok(MediaParser::Y4m),
                Some("h264") => Ok(MediaParser::OpenH264),
                Some(ext) => Err(anyhow::anyhow!(
                    "Cannot automatically determine parser based on file extension \"{ext}\"."
                )),
                None => Err(anyhow::anyhow!(
                    "Cannot automatically determine parser because no file extension found. Try 'ffprobe' parser?"
                )),
            }
        }
    }?;

    parser.dump(cli.input_fname)?;
    Ok(())
}
