// Copyright 2022-2023 Andrew D. Straw.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use ci2_remote_control::Mp4RecordingConfig;
use machine_vision_formats::{pixel_format::Mono8, ImageData};

#[test]
fn test_save_then_read_with_ffmpeg() -> Result<()> {
    let start = chrono::DateTime::from_utc(
        chrono::NaiveDateTime::from_timestamp_opt(61, 0).unwrap(),
        Utc,
    );

    let tmpdir = tempfile::tempdir()?;
    let base_path = tmpdir.path().to_path_buf();

    let env_var_name = "MP4_WRITER_SAVE_TEST";
    // Potentially do not delete temporary directory
    let save_output = match std::env::var_os(env_var_name) {
        Some(v) => {
            &v != "0"
        }
        None => false,
    };

    if save_output {
        std::mem::forget(tmpdir); // do not drop it, so do not delete it
    }

    let mut codecs = vec!["less_avc"];

    #[cfg(feature = "openh264")]
    codecs.push("open-h264");

    // TODO: runtime test for nvidia
    if false {
        codecs.push("nvenc");
    } else {
        println!("not testing nvenc");
    }

    let mut outputs = Vec::new();
    for pixfmt in ["mono8", "rgb8"].iter() {
        for codec_str in codecs.iter() {
            for width in [16u32, 640, 30, 32].iter() {
                for height in [16u32, 480, 30, 32].iter() {
                    outputs.push((pixfmt.to_string(), codec_str.to_string(), *width, *height));
                }
            }
        }
    }

    for (pixfmt_str, codec_str, width, height) in outputs.iter() {
        let output_name = base_path.join(format!(
            "test-movie-{}-{}-{}x{}.mp4",
            pixfmt_str, codec_str, width, height
        ));
        println!("testing {}", output_name.display());
        let out_fd = std::fs::File::create(&output_name)?;

        #[allow(unused_assignments)]
        let mut nvenc_libs = None;

        let h264_bitrate = None;

        let (codec, libs_and_nv_enc, is_lossy) = match codec_str.as_str() {
            "open-h264" => {
                let codec = ci2_remote_control::Mp4Codec::H264OpenH264({
                    let preset = if let Some(bitrate) = h264_bitrate {
                        ci2_remote_control::OpenH264Preset::SkipFramesBitrate(bitrate)
                    } else {
                        ci2_remote_control::OpenH264Preset::AllFrames
                    };
                    ci2_remote_control::OpenH264Options {
                        preset,
                        debug: false,
                    }
                });
                (codec, None, true)
            }
            "nv-h264" => {
                nvenc_libs = Some(nvenc::Dynlibs::new()?);
                let codec = ci2_remote_control::Mp4Codec::H264NvEnc(Default::default());
                (
                    codec,
                    Some(nvenc::NvEnc::new(nvenc_libs.as_ref().unwrap())?),
                    true,
                )
            }
            "less_avc" => (ci2_remote_control::Mp4Codec::H264LessAvc, None, false),
            _ => {
                panic!("unknown codec str");
            }
        };

        let max_diff = if is_lossy { 22 } else { 0 };

        let cfg = Mp4RecordingConfig {
            codec,
            sample_duration: std::time::Duration::from_millis(50),
            max_framerate: Default::default(),
            h264_metadata: None,
        };

        let frame = generate_image(pixfmt_str, *width, *height, start)?;
        {
            let mut my_mp4_writer = mp4_writer::Mp4Writer::new(out_fd, cfg, libs_and_nv_enc)?;
            my_mp4_writer.write_dynamic(&frame, start)?;
            // close file at end of this block
        }

        // --------

        let decoded = ffmpeg_to_frame(&output_name)?;
        assert_eq!(decoded.width(), frame.width());
        assert_eq!(decoded.height(), frame.height());

        // Do image comparison only with monochrome data because YUV420 chroma
        // downsampling is something we expect. (Alternative: convert original
        // to yuv420 and compare that?)
        let decoded_mono8 = convert_image::convert::<_, Mono8>(&decoded)?;
        let orig_mono8 = frame.into_pixel_format()?;
        if !are_images_similar(&orig_mono8, &decoded_mono8, max_diff) {
            if save_output {
                panic!("movie {} too different from input", output_name.display());
            } else {
                panic!(
                    "{pixfmt_str} {codec_str} {width}x{height} too different \
                    from input. Save output by setting env var {env_var_name}"
                );
            }
        }
    }

    Ok(())
}

fn are_images_similar<FMT>(
    frame1: &dyn machine_vision_formats::ImageStride<FMT>,
    frame2: &dyn machine_vision_formats::ImageStride<FMT>,
    max_diff: u8,
) -> bool
where
    FMT: machine_vision_formats::PixelFormat,
{
    let width = frame1.width();

    if frame1.width() != frame2.width() {
        dbg!(1);
        return false;
    }
    if frame1.height() != frame2.height() {
        dbg!(1);
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
        for (f1val, f2val) in f1_valid.iter().zip(f2_valid.iter()) {
            let diff: u8 = f1val.abs_diff(*f2val);
            if diff > max_diff {
                dbg!(diff, &f1_row, &f2_row);
                return false;
            }
        }
    }

    true
}

fn generate_image(
    pixfmt_str: &str,
    width: u32,
    height: u32,
    start: DateTime<Utc>,
) -> Result<basic_frame::DynamicFrame> {
    let width = width as usize;
    let height = height as usize;
    let image = {
        match pixfmt_str {
            "mono8" => {
                let image_row_mono8: Vec<u8> = (0..width)
                    .map(|idx| ((idx as f64) * 255.0 / (width - 1) as f64) as u8)
                    .collect();
                assert_eq!(image_row_mono8.len(), width);
                assert_eq!(image_row_mono8[0], 0);
                assert_eq!(image_row_mono8[image_row_mono8.len() - 1], 255);

                let stride = next16(width as IType) as usize;
                let mut image_data = vec![0u8; stride * height];
                for row in 0..height {
                    let start_idx = row * stride;
                    let dest_row = &mut image_data[start_idx..(start_idx + width)];
                    dest_row.copy_from_slice(&image_row_mono8);
                }
                basic_frame::DynamicFrame::new(
                    (width).try_into().unwrap(),
                    height.try_into().unwrap(),
                    stride.try_into().unwrap(),
                    Box::new(basic_frame::BasicExtra {
                        host_framenumber: 0,
                        host_timestamp: start,
                    }),
                    image_data,
                    machine_vision_formats::PixFmt::Mono8,
                )
            }
            "rgb8" => {
                let image_row_rgb8: Vec<u8> = (0..width)
                    .flat_map(|idx| {
                        let val = ((idx as f64) * 255.0 / (width - 1) as f64) as u8;
                        [val; 3]
                    })
                    .collect();
                assert_eq!(image_row_rgb8.len(), width * 3);
                assert_eq!(image_row_rgb8[0], 0);
                assert_eq!(image_row_rgb8[1], 0);
                assert_eq!(image_row_rgb8[2], 0);
                assert_eq!(image_row_rgb8[image_row_rgb8.len() - 3], 255);
                assert_eq!(image_row_rgb8[image_row_rgb8.len() - 2], 255);
                assert_eq!(image_row_rgb8[image_row_rgb8.len() - 1], 255);

                let stride = next16(width as IType) as usize * 3;
                let mut image_data = vec![0u8; stride * height];
                for row in 0..height {
                    let start_idx = row * stride;
                    let dest_row = &mut image_data[start_idx..(start_idx + width * 3)];
                    dest_row.copy_from_slice(&image_row_rgb8[..]);
                }
                basic_frame::DynamicFrame::new(
                    (width).try_into().unwrap(),
                    height.try_into().unwrap(),
                    stride.try_into().unwrap(),
                    Box::new(basic_frame::BasicExtra {
                        host_framenumber: 0,
                        host_timestamp: start,
                    }),
                    image_data,
                    machine_vision_formats::PixFmt::RGB8,
                )
            }
            _ => {
                panic!("unknown pix format");
            }
        }
    };
    Ok(image)
}

type IType = u32;
fn next16(x: IType) -> IType {
    fn div_ceil(a: IType, b: IType) -> IType {
        // See https://stackoverflow.com/a/72442854
        (a + b - 1) / b
    }
    let v = 16;
    div_ceil(x, v) * 16
}

fn ffmpeg_to_frame(
    fname: &std::path::Path,
) -> Result<simple_frame::SimpleFrame<machine_vision_formats::pixel_format::RGB8>> {
    let tmpdir = tempfile::tempdir()?;

    let png_fname = tmpdir.path().join("frame1.png");
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
    let decoded = convert_image::piston_to_frame(piston_image)?;
    Ok(decoded)
}
