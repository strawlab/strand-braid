// Copyright 2022-2023 Andrew D. Straw.

use strand_cam_remote_control::Mp4RecordingConfig;

type IType = usize;

fn next16(x: IType) -> IType {
    let v = 16;
    x.div_ceil(v) * 16
}

fn main() -> eyre::Result<()> {
    env_tracing_logger::init();
    let n_frames = 1;

    let start = chrono::DateTime::from_timestamp(61, 0).unwrap();

    let mut outputs = Vec::new();
    for pixfmt in ["mono8", "rgb8"].iter() {
        for codec_str in ["less_avc"].iter() {
            for width in [640usize, 16, 30, 32].iter() {
                for height in [480usize, 30, 32].iter() {
                    let output_fname =
                        format!("movie-{}-{}-{}x{}.mp4", pixfmt, codec_str, width, height);
                    outputs.push((
                        output_fname,
                        pixfmt.to_string(),
                        codec_str.to_string(),
                        *width,
                        *height,
                    ));
                }
            }
        }
    }

    for (output_fname, pixfmt_str, codec_str, width, height) in outputs.iter() {
        let width: usize = *width; // dereference
        let height: usize = *height;
        println!("saving {}", output_fname);

        let out_fd = std::fs::File::create(output_fname)?;

        #[cfg(feature = "nv-encode")]
        #[allow(unused_assignments)]
        let mut nvenc_libs = None;

        let h264_bitrate = None;

        #[allow(unused_variables)]
        let (codec, libs_and_nv_enc) = match codec_str.as_str() {
            "open-h264" => {
                let codec = strand_cam_remote_control::Mp4Codec::H264OpenH264({
                    let preset = if let Some(bitrate) = h264_bitrate {
                        strand_cam_remote_control::OpenH264Preset::SkipFramesBitrate(bitrate)
                    } else {
                        strand_cam_remote_control::OpenH264Preset::AllFrames
                    };
                    strand_cam_remote_control::OpenH264Options {
                        preset,
                        debug: false,
                    }
                });
                #[cfg(not(feature = "nv-encode"))]
                let none = Option::<()>::None;
                #[cfg(feature = "nv-encode")]
                let none = None;
                (codec, none)
            }
            #[cfg(feature = "nv-encode")]
            "nv-h264" => {
                nvenc_libs = Some(nvenc::Dynlibs::new()?);
                let codec = strand_cam_remote_control::Mp4Codec::H264NvEnc(Default::default());
                (
                    codec,
                    Some(nvenc::NvEnc::new(nvenc_libs.as_ref().unwrap())?),
                )
            }
            "less_avc" => (strand_cam_remote_control::Mp4Codec::H264LessAvc, None),
            _ => {
                panic!("unknown codec str");
            }
        };

        let cfg = Mp4RecordingConfig {
            codec,
            max_framerate: Default::default(),
            h264_metadata: None,
        };

        #[cfg(feature = "nv-encode")]
        let mut my_mp4_writer = mp4_writer::Mp4Writer::new(out_fd, cfg, libs_and_nv_enc)?;
        #[cfg(not(feature = "nv-encode"))]
        let mut my_mp4_writer = mp4_writer::Mp4Writer::new(out_fd, cfg)?;

        let image = {
            match pixfmt_str.as_str() {
                "mono8" => {
                    let image_row_mono8: Vec<u8> = (0..width)
                        .map(|idx| ((idx as f64) * 255.0 / (width - 1) as f64) as u8)
                        .collect();
                    assert_eq!(image_row_mono8.len(), width);
                    assert_eq!(image_row_mono8[0], 0);
                    assert_eq!(image_row_mono8[image_row_mono8.len() - 1], 255);

                    let stride = next16(width);
                    let mut image_data = vec![0u8; stride * height];
                    for row in 0..height {
                        let start_idx = row * stride;
                        let dest_row = &mut image_data[start_idx..(start_idx + width)];
                        dest_row.copy_from_slice(&image_row_mono8);
                    }
                    strand_dynamic_frame::DynamicFrame::new(
                        (width).try_into().unwrap(),
                        height.try_into().unwrap(),
                        stride.try_into().unwrap(),
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

                    let stride = next16(width) * 3;
                    let mut image_data = vec![0u8; stride * height];
                    for row in 0..height {
                        let start_idx = row * stride;
                        let dest_row = &mut image_data[start_idx..(start_idx + width * 3)];
                        dest_row.copy_from_slice(&image_row_rgb8[..]);
                    }
                    strand_dynamic_frame::DynamicFrame::new(
                        (width).try_into().unwrap(),
                        height.try_into().unwrap(),
                        stride.try_into().unwrap(),
                        image_data,
                        machine_vision_formats::PixFmt::RGB8,
                    )
                }
                _ => {
                    panic!("unknown pix format");
                }
            }
        };

        {
            // Save .png to verify input image is OK.
            let png_fname = format!(
                "frame-{}-{}-{}x{}.png",
                pixfmt_str, codec_str, width, height
            );
            let opts = convert_image::EncoderOptions::Png;
            use strand_dynamic_frame::{match_all_dynamic_fmts, DynamicFrame};
            let png_buf = match_all_dynamic_fmts!(
                &image,
                x,
                convert_image::frame_to_encoded_buffer(x, opts)
            )?;
            let mut fd = std::fs::File::create(png_fname)?;
            use std::io::Write;
            fd.write_all(&png_buf)?;
        }

        for count in 0..n_frames {
            let dt_msec = 5;
            let dt = chrono::Duration::try_milliseconds(count as i64 * dt_msec).unwrap();

            let ts = start.checked_add_signed(dt).unwrap();
            my_mp4_writer.write_dynamic(&image, ts)?;
        }

        my_mp4_writer.finish()?;
    }

    Ok(())
}
