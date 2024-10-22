// Copyright 2022-2023 Andrew D. Straw.

fn main() -> anyhow::Result<()> {
    let n_frames = 1;

    let start = chrono::DateTime::from_timestamp(61, 0).unwrap();

    let mut outputs = Vec::new();
    for pixfmt in ["mono8", "rgb8"].iter() {
        for width in [640usize, 16, 30, 32].iter() {
            for height in [480usize, 30, 32].iter() {
                let output_fname = format!("movie-{}-{}x{}.h264", pixfmt, width, height);
                outputs.push((output_fname, pixfmt.to_string(), *width, *height));
            }
        }
    }

    for (output_fname, pixfmt_str, width, height) in outputs.iter() {
        let width: usize = *width; // dereference
        let height: usize = *height;
        println!("saving {}", output_fname);

        let raw_h264 = {
            let output_h264_fname = std::path::PathBuf::from(output_fname);

            println!("saving {}", output_h264_fname.display());

            std::fs::File::create(output_h264_fname)?
        };

        let mut my_h264_writer = less_avc_wrapper::H264WriterWrapper::new(raw_h264)?;

        let image = {
            match pixfmt_str.as_str() {
                "mono8" => {
                    let image_row_mono8: Vec<u8> = (0..width)
                        .map(|idx| ((idx as f64) * 255.0 / (width - 1) as f64) as u8)
                        .collect();
                    assert_eq!(image_row_mono8.len(), width);
                    assert_eq!(image_row_mono8[0], 0);
                    assert_eq!(image_row_mono8[image_row_mono8.len() - 1], 255);

                    let stride = width;
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

                    let stride = width * 3;
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

        {
            // Save .png to verify input image is OK.
            let png_fname = format!("frame-{}-{}x{}.png", pixfmt_str, width, height);
            let opts = convert_image::ImageOptions::Png;
            use basic_frame::{match_all_dynamic_fmts, DynamicFrame};
            let png_buf =
                match_all_dynamic_fmts!(&image, x, convert_image::frame_to_encoded_buffer(x, opts))?;
            let mut fd = std::fs::File::create(png_fname)?;
            use std::io::Write;
            fd.write_all(&png_buf)?;
        }

        for _ in 0..n_frames {
            my_h264_writer.write_dynamic(&image)?;
        }
    }

    Ok(())
}
