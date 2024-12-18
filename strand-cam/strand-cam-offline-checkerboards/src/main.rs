use std::path::PathBuf;

use clap::Parser;
use eyre::{self as anyhow, Context, Result};
use image::GenericImageView;
use tracing::{error, info};

#[derive(Parser, Debug)]
struct Cli {
    /// Input directory name (with .png or .jpg files)
    input_dirname: std::path::PathBuf,
    /// Width of checkerboard pattern, in number of corners (e.g. 8x8 checks
    /// would be 7x7 corners)
    #[arg(default_value_t = 7)]
    pattern_width: usize,
    /// Height of checkerboard pattern, in number of corners (e.g. 8x8 checks
    /// would be 7x7 corners)
    #[arg(default_value_t = 5)]
    pattern_height: usize,
}

fn get_image_files(dirname: &std::path::Path) -> Result<Vec<PathBuf>> {
    if !std::fs::metadata(&dirname)?.is_dir() {
        anyhow::bail!("Attempting to open \"{}\" as directory with PNG or JPG stack failed because it is not a directory.", dirname.display());
    }
    let png_joined = dirname.join("*.png");
    let png_pattern = png_joined.to_str().unwrap();

    let jpg_joined = dirname.join("*.jpg");
    let jpg_pattern = jpg_joined.to_str().unwrap();

    // First prefer PNG, then if none are found, look for JPG files. (Probably the logic
    // here could be improved.)
    let mut paths = vec![];
    for path in glob::glob_with(
        png_pattern,
        glob::MatchOptions {
            case_sensitive: false,
            require_literal_separator: true,
            require_literal_leading_dot: true,
        },
    )? {
        paths.push(path?);
    }

    if paths.is_empty() {
        for path in glob::glob_with(
            jpg_pattern,
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
        anyhow::bail!("no PNG or JPG files in \"{}\"", png_pattern);
    }

    paths.sort();
    Ok(paths)
}

fn main() -> Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "info");
    }

    env_logger::init();
    let cli = Cli::parse();

    let dirname = cli.input_dirname;
    let fnames = get_image_files(&dirname)?;

    let mut checkerboard_data = strand_cam_storetype::CheckerboardCalState::default();
    checkerboard_data.width = cli.pattern_width.try_into().unwrap();
    checkerboard_data.height = cli.pattern_height.try_into().unwrap();

    info!(
        "Attempting to find {}x{} chessboard.",
        checkerboard_data.width, checkerboard_data.height
    );

    let mut image_width = 0;
    let mut image_height = 0;

    let mut collected_corners = Vec::with_capacity(fnames.len());
    for fname in fnames.iter() {
        info!("{}", fname.display());
        let img = image::open(&fname).with_context(|| format!("Opening {}", fname.display()))?;
        let (w, h) = img.dimensions();
        image_width = w;
        image_height = h;
        let rgb = img.to_rgb8().into_raw();

        let corners = opencv_calibrate::find_chessboard_corners(
            &rgb,
            w,
            h,
            checkerboard_data.width as usize,
            checkerboard_data.height as usize,
        )?;
        info!("    {:?} corners.", corners.as_ref().map(|x| x.len()));
        if let Some(corners) = corners {
            collected_corners.push(corners);
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
    match camcal::compute_intrinsics_with_raw_opencv::<f64>(size, &goodcorners) {
        Ok(raw_opencv_cal) => {
            let intrinsics = camcal::convert_to_cam_geom::<f64>(&raw_opencv_cal);

            info!(
                "Mean reprojection error: {}",
                raw_opencv_cal.mean_reprojection_distance_pixels
            );
            info!("got calibrated intrinsics: {:?}", intrinsics);

            let cam_name = dirname.as_os_str().to_str().unwrap().to_string();

            let format_str = format!("{}.%Y%m%d_%H%M%S.yaml", cam_name.as_str());
            let local = chrono::Local::now();
            let cam_info_file_stamped = local.format(&format_str).to_string();

            let cam_info_file = format!("{}.yaml", &cam_name);

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
        }
        Err(e) => {
            error!("failed doing calibration {:?} {}", e, e);
        }
    };

    Ok(())
}
