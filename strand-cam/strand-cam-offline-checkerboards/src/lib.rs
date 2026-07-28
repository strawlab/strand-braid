// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::path::PathBuf;

use camino::{Utf8Path, Utf8PathBuf};
use clap::Parser;
use eyre::{self as anyhow, Context, Result};
use image::{GenericImageView, Rgb, RgbImage};
use tracing::info;

use camcal::CalibrationResult;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Input directory name (with .png, .jpg, .bmp, or .tiff files)
    pub input_dirname: Utf8PathBuf,
    /// Width of checkerboard pattern, in number of corners (e.g. 8x8 checks
    /// would be 7x7 corners)
    #[arg(default_value_t = 7)]
    pub pattern_width: usize,
    /// Height of checkerboard pattern, in number of corners (e.g. 8x8 checks
    /// would be 7x7 corners)
    #[arg(default_value_t = 5)]
    pub pattern_height: usize,
}

fn get_image_files(dirname: &Utf8Path) -> Result<Vec<PathBuf>> {
    if !std::fs::metadata(dirname)
        .with_context(|| format!("While reading filesystem metadata from \"{dirname}\"."))?
        .is_dir()
    {
        anyhow::bail!("Attempting to open \"{dirname}\" because it is not a directory.");
    }
    let png_joined = dirname.join("*.png");
    let png_pattern = png_joined.to_string();

    let jpg_joined = dirname.join("*.jpg");
    let jpg_pattern = jpg_joined.to_string();

    let bmp_joined = dirname.join("*.bmp");
    let bmp_pattern = bmp_joined.to_string();

    let tiff_joined = dirname.join("*.tiff");
    let tiff_pattern = tiff_joined.to_string();

    let mut paths = vec![];
    for pattern in [png_pattern, jpg_pattern, bmp_pattern, tiff_pattern] {
        // First prefer PNG, then if none are found, look for JPG files. (Probably the logic
        // here could be improved.)
        for path in glob::glob_with(
            &pattern,
            glob::MatchOptions {
                case_sensitive: false,
                require_literal_separator: true,
                require_literal_leading_dot: true,
            },
        )? {
            paths.push(path?);
        }
    }

    if paths.is_empty() {
        anyhow::bail!("no image files in \"{}\"", dirname);
    }

    paths.sort();
    Ok(paths)
}

/// Directory into which annotated (corner-overlay) images are saved, as a
/// sibling of `dirname`.
fn annotated_dirname(dirname: &Utf8Path) -> Utf8PathBuf {
    let new_name = format!(
        "{}-annotated",
        dirname.file_name().unwrap_or("checkerboard-images")
    );
    let mut d = dirname.to_owned();
    d.set_file_name(new_name);
    d
}

/// Output path for the annotated version of `fname`. Images in which no
/// checkerboard was found get a `_NO_CORNERS_FOUND` suffix so they stand out
/// when browsing the directory.
fn annotated_path(annotated_dir: &Utf8Path, fname: &std::path::Path, found: bool) -> Utf8PathBuf {
    if found {
        let file_name = fname.file_name().unwrap().to_string_lossy();
        annotated_dir.join(file_name.as_ref())
    } else {
        let stem = fname.file_stem().unwrap().to_string_lossy();
        let ext = fname
            .extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_default();
        annotated_dir.join(format!("{stem}_NO_CORNERS_FOUND.{ext}"))
    }
}

/// Rainbow color for corner `i` of `n`, used so that the corner order (and
/// thus board orientation) is visible in the saved image.
fn corner_color(i: usize, n: usize) -> Rgb<u8> {
    let hue = 300.0 * (i as f32) / (n.max(1) as f32);
    hsv_to_rgb(hue, 1.0, 1.0)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> Rgb<u8> {
    let c = v * s;
    let hp = h / 60.0;
    let x = c * (1.0 - ((hp % 2.0) - 1.0).abs());
    let (r1, g1, b1) = if hp < 1.0 {
        (c, x, 0.0)
    } else if hp < 2.0 {
        (x, c, 0.0)
    } else if hp < 3.0 {
        (0.0, c, x)
    } else if hp < 4.0 {
        (0.0, x, c)
    } else if hp < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    let m = v - c;
    Rgb([
        ((r1 + m) * 255.0).round() as u8,
        ((g1 + m) * 255.0).round() as u8,
        ((b1 + m) * 255.0).round() as u8,
    ])
}

/// Draws the detected corners (colored circles, in detection order) and
/// connects corners within each row with a line, mirroring the style used by
/// ROS's `camera_calibration` monocular calibration tool.
fn draw_corners(img: &mut RgbImage, corners: &[(f32, f32)], n_cols: usize) {
    for (i, &(x, y)) in corners.iter().enumerate() {
        let color = corner_color(i, corners.len());
        if i % n_cols != 0 {
            let (px, py) = corners[i - 1];
            imageproc::drawing::draw_line_segment_mut(img, (px, py), (x, y), Rgb([0, 255, 0]));
        }
        imageproc::drawing::draw_filled_circle_mut(
            img,
            (x.round() as i32, y.round() as i32),
            5,
            color,
        );
    }
}

pub fn run_cal(cli: Cli) -> Result<CalibrationResult> {
    let dirname = cli.input_dirname;
    let fnames = get_image_files(&dirname)?;

    let checkerboard_data = strand_cam_storetype::CheckerboardCalState {
        width: cli.pattern_width.try_into().unwrap(),
        height: cli.pattern_height.try_into().unwrap(),
        ..Default::default()
    };

    info!(
        "Attempting to find {}x{} chessboard.",
        checkerboard_data.width, checkerboard_data.height
    );

    let mut image_width = 0;
    let mut image_height = 0;

    let annotated_dir = annotated_dirname(&dirname);
    std::fs::create_dir_all(&annotated_dir)
        .with_context(|| format!("Creating directory {annotated_dir}"))?;
    info!("Saving corner-annotated images to: {annotated_dir}");

    let mut collected_corners = Vec::with_capacity(fnames.len());
    let mut good_fnames = Vec::with_capacity(fnames.len());
    for fname in fnames.iter() {
        info!("{}", fname.display());
        let img = image::open(fname).with_context(|| format!("Opening {}", fname.display()))?;
        let (w, h) = img.dimensions();
        image_width = w;
        image_height = h;
        let mut rgb_img = img.to_rgb8();

        let corners = camcal::find_chessboard_corners(
            rgb_img.as_raw(),
            w,
            h,
            checkerboard_data.width as usize,
            checkerboard_data.height as usize,
        )?;
        info!("{:?} corners.", corners.as_ref().map(|x| x.len()));

        if let Some(corners) = &corners {
            draw_corners(&mut rgb_img, corners, checkerboard_data.width as usize);
        }
        let out_path = annotated_path(&annotated_dir, fname, corners.is_some());
        rgb_img
            .save(out_path.as_std_path())
            .with_context(|| format!("Saving annotated image {out_path}"))?;

        if let Some(corners) = corners {
            collected_corners.push(corners);
            good_fnames.push(fname.clone());
        }
    }

    let n_rows = checkerboard_data.height;
    let n_cols = checkerboard_data.width;

    let goodcorners: Vec<camcal::CheckerBoardData> = collected_corners
        .iter()
        .map(|corners| {
            let x: Vec<(f64, f64)> = corners.iter().map(|x| (x.0 as f64, x.1 as f64)).collect();
            camcal::CheckerBoardData::new(n_rows as usize, n_cols as usize, &x)
        })
        .collect();

    let size = camcal::PixelSize::new(image_width as usize, image_height as usize);
    let raw_opencv_cal = match camcal::compute_intrinsics_with_raw_opencv::<f64>(size, &goodcorners)
    {
        Ok(raw_opencv_cal) => {
            let intrinsics = camcal::convert_to_cam_geom::<f64>(&raw_opencv_cal);

            info!(
                "Mean reprojection error: {}",
                raw_opencv_cal.mean_reprojection_distance_pixels
            );
            for (fname, dist) in good_fnames
                .iter()
                .zip(&raw_opencv_cal.per_image_reprojection_distances_pixels)
            {
                info!("  {}: reprojection error {dist:.3} px", fname.display());
            }
            info!("got calibrated intrinsics: {:?}", intrinsics);

            let cam_name = dirname.to_string();

            let format_str = format!("{}.%Y%m%d_%H%M%S.yaml", cam_name.as_str());
            let local = chrono::Local::now();
            let cam_info_file_stamped = local.format(&format_str).to_string();

            let cam_info_file = format!("{cam_name}.yaml");

            // Save timestamped version first for backup purposes (since below
            // we overwrite the non-timestamped file).
            camcal::save_yaml(
                &cam_info_file_stamped,
                env!["CARGO_PKG_NAME"],
                local,
                &raw_opencv_cal,
                &cam_name,
            )?;

            // Now copy the successfully saved file into the non-timestamped
            // name. This will overwrite an existing file.
            std::fs::copy(&cam_info_file_stamped, &cam_info_file)
                .with_context(|| format!("Copying to file {cam_info_file}"))?;

            info!("Saved camera calibration to file: {cam_info_file}");
            raw_opencv_cal
        }
        Err(e) => {
            eyre::bail!("failed doing calibration {:?} {}", e, e);
        }
    };

    Ok(raw_opencv_cal)
}
