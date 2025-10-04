use clap::Parser;
use eyre::{self as anyhow, Context};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// CSV file with April Tags 3D fiducial coordinates.
    pub apriltags_3d_fiducial_coords: camino::Utf8PathBuf,

    /// YAML file with camera intrinsics.
    pub intrinsics_yaml: camino::Utf8PathBuf,

    /// JPEG image with april tags which will be detected.
    ///
    /// This is typically the JPEG saved alongside
    /// the flytrax CSV file.
    pub image_filename: camino::Utf8PathBuf,

    /// CSV data from the experiment.
    pub flytrax_csv: camino::Utf8PathBuf,

    /// Calibration XML output filename. An SVG debug image will also be saved.
    pub output_xml: camino::Utf8PathBuf,
}

impl Cli {
    fn into_args(
        self,
    ) -> anyhow::Result<(
        flytrax_apriltags_calibration::ComputeExtrinsicsArgs,
        camino::Utf8PathBuf,
    )> {
        let Cli {
            apriltags_3d_fiducial_coords,
            intrinsics_yaml,
            image_filename,
            flytrax_csv,
            output_xml,
        } = self;

        let intrinsics_buf = std::fs::read_to_string(&intrinsics_yaml)
            .with_context(|| format!("opening {intrinsics_yaml}"))?;

        let intrinsics: opencv_ros_camera::RosCameraInfo<f64> =
            serde_yaml::from_str(&intrinsics_buf)
                .with_context(|| format!("while parsing {intrinsics_yaml}"))?;

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
    env_logger::init();
    let cli = Cli::parse();
    let (args, output_xml) = cli.into_args()?;
    let cal = flytrax_apriltags_calibration::compute_extrinsics(&args)?;

    flytrax_apriltags_calibration::save_cal_result_to_xml(&output_xml, &cal)?;

    let mut out_svg_fname = output_xml.clone();
    out_svg_fname.set_extension("svg");
    flytrax_apriltags_calibration::save_cal_svg_and_png_images(out_svg_fname, &cal)?;

    Ok(())
}
