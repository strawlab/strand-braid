use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use image::GenericImageView;
use log::{error, info};

#[derive(Parser, Debug)]
struct Cli {
    /// Input directory name (with .png files)
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

fn get_png_files(dirname: &std::path::Path) -> Result<Vec<PathBuf>> {
    if !std::fs::metadata(&dirname)?.is_dir() {
        anyhow::bail!("Attempting to open \"{}\" as directory with PNG stack failed because it is not a directory.", dirname.display());
    }
    let joined = dirname.join("*.png");
    let pattern = joined.to_str().unwrap();

    let mut paths = vec![];
    for path in glob::glob_with(
        pattern,
        glob::MatchOptions {
            case_sensitive: false,
            require_literal_separator: true,
            require_literal_leading_dot: true,
        },
    )? {
        paths.push(path?);
    }
    if paths.is_empty() {
        anyhow::bail!("no files in \"{}\"", pattern);
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
    let fnames = get_png_files(&dirname)?;

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

            let ros_cam_name = dirname.as_os_str().to_str().unwrap().to_string();

            // Convert from mvg to ROS format.
            let ci: opencv_ros_camera::RosCameraInfo<_> =
                opencv_ros_camera::NamedIntrinsicParameters {
                    intrinsics,
                    width: image_width as usize,
                    height: image_height as usize,
                    name: ros_cam_name.clone(),
                }
                .into();

            let format_str = format!("{}.%Y%m%d_%H%M%S.yaml", ros_cam_name.as_str());
            let local = chrono::Local::now();
            let cam_info_file_stamped = local.format(&format_str).to_string();

            let cam_info_file = format!("{}.yaml", ros_cam_name);

            // Save timestamped version first for backup purposes (since below
            // we overwrite the non-timestamped file).
            {
                let mut f = std::fs::File::create(&cam_info_file_stamped)
                    .with_context(|| format!("Saving file {cam_info_file_stamped}"))?;
                std::io::Write::write_all(
                    &mut f,
                    format!(
                        "# Saved by {} at {}\n\
                        # Mean reprojection distance: {:.2}\n",
                        env!["CARGO_PKG_NAME"],
                        local,
                        raw_opencv_cal.mean_reprojection_distance_pixels
                    )
                    .as_bytes(),
                )?;
                serde_yaml::to_writer(f, &ci)?;
            }

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
