use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// CSV file with April Tags 3D fiducial coordinates.
    pub apriltags_3d_fiducial_coords: PathBuf,

    /// YAML file with camera intrinsics.
    pub intrinsics_yaml: PathBuf,

    /// JPEG image with april tags which will be detected.
    ///
    /// This is typically the JPEG saved alongside
    /// the flytrax CSV file.
    pub image_filename: PathBuf,

    /// CSV data from the experiment.
    pub flytrax_csv: PathBuf,

    /// Calibration XML output filename. An SVG debug image will also be saved.
    pub output_xml: PathBuf,
}

impl Cli {
    fn into_args(
        self,
    ) -> anyhow::Result<(
        flytrax_apriltags_calibration::ComputeExtrinsicsArgs,
        PathBuf,
    )> {
        let Cli {
            apriltags_3d_fiducial_coords,
            intrinsics_yaml,
            image_filename,
            flytrax_csv,
            output_xml,
        } = self;

        let intrinsics_buf = std::fs::read_to_string(&intrinsics_yaml)
            .with_context(|| format!("opening {}", intrinsics_yaml.display()))?;

        let intrinsics: opencv_ros_camera::RosCameraInfo<f64> =
            serde_yaml::from_str(&intrinsics_buf)
                .with_context(|| format!("while parsing {}", intrinsics_yaml.display()))?;

        Ok((
            flytrax_apriltags_calibration::ComputeExtrinsicsArgs {
                apriltags_3d_fiducial_coords,
                intrinsics,
                image_filename,
                flytrax_csv,
            },
            output_xml,
        ))
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let (args, output_xml) = cli.into_args()?;
    let cal = flytrax_apriltags_calibration::compute_extrinsics(&args)?;
    flytrax_apriltags_calibration::save_cal_result(output_xml, cal)?;
    Ok(())
}
