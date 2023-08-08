#[macro_use]
extern crate log;
#[macro_use]
extern crate lazy_static;

use anyhow::Context;

use flydra_types::{MiniArenaConfig, XYGridConfig};
use flytrax_csv_to_braidz::{parse_configs_and_run, PseudoCalParams, RowFilter};

use clap::Parser;

lazy_static! {
    static ref VAL_HELP: String = {
        let example_simple_cal = PseudoCalParams {
            physical_diameter_meters: 0.1,
            center_x: 640,
            center_y: 480,
            radius: 480,
        };
        let simple_cal_toml_buf = toml::to_string(&example_simple_cal).unwrap();

        let mut tracking_example = flydra_types::default_tracking_params_flat_3d();
        tracking_example.mini_arena_config =
            MiniArenaConfig::XYGrid(XYGridConfig::new(&[0.1, 0.2, 0.3], &[0.1, 0.2, 0.3], 0.05));
        let tracking_example_buf = toml::to_string(&tracking_example).unwrap();

        let program_name = env!("CARGO_PKG_NAME");
        format!(
            "This program will read a flytrax CSV file saved by strand-cam and, using \
            Kalman filtering and data association, track objects there.\n\n\
            \
            FURTHER INFORMATION:\n\n\
            # Information regarding Braid calibrations\n\n\
            The following documentation describes camera calibrations in Braid, including the XML \
            file format used here (in the `{program_name}` program):\n
        https://strawlab.github.io/strand-braid/braid_calibration.html\n\n\
            Such calibrations can be generated with this tool:\n
        https://strawlab.org/braid-april-cal-webapp/\n\n\
            EXAMPLE INPUT FILES:\n\n# Calibration\n\n\
            Either a simple calibration .toml file or Braid calibration .xml file is expected.\n\n\
            ## Example simple calibration .toml file:\n\n\
        ```\n{simple_cal_toml_buf}```\n\n## Calibration .xml file:\n\n\
        See above for links to the documentation regarding Braid XML calibration files.\n\n\
        # Example tracking parameter .toml file:\n\n\
        ```\n{tracking_example_buf}```\n\n"
        )
    };
}

#[derive(Parser, Debug)]
#[command(author, version, about, after_help = VAL_HELP.as_str())]
struct Cli {
    /// Input CSV file with 2D detections
    #[arg(long = "csv", short = 'c')]
    flytrax_csv: std::path::PathBuf,
    /// Output file, must end with '.braidz'
    #[arg(long = "output", short = 'o')]
    output_braidz: Option<std::path::PathBuf>,
    /// Tracking parameters TOML file. (Includes `motion_noise_scale`, amongst others.)
    #[arg(long = "tracking-params", short = 't')]
    tracking_params: Option<std::path::PathBuf>,
    /// Calibration parameters file.
    ///
    /// Can either be:
    ///
    /// - TOML file containing a "Simple Calibration" of type `PseudoCalParams`
    ///   which has fields `physical_diameter_meters`, `center_x`, `center_y`
    ///   and `radius`.
    ///
    /// - XML file containing a full Braid XML calibration. See below for
    ///   further information.
    ///
    /// - YAML file containing a camera intrinsic parameters. In this case, an
    ///   april tag 3D coordinates file must be given to allow solving for camera
    ///   extrinsic parameters.
    #[arg(long = "cal", short = 'p')]
    calibration_params: std::path::PathBuf,

    /// An april tag 3D coordinates CSV file to allow solving for camera
    /// extrinsic parameters.
    #[arg(long)]
    apriltags_3d_fiducial_coords: Option<std::path::PathBuf>,

    /// Set start frame to start tracking
    #[arg(long)]
    pub start_frame: Option<u64>,
    /// Set stop frame to stop tracking
    #[arg(long)]
    pub stop_frame: Option<u64>,

    /// Include all data from outside the calibration region in tracking
    ///
    /// By default, if the calibration parameters are given as a simple
    /// calibration TOML file, the tracking excludes points outside the
    /// calibration region. If this option is given, no exclusion is performed.
    ///
    /// If the calibration parameters are given as a XML file, all points are
    /// always included for tracking.
    #[arg(long = "include-all", short = 'a')]
    track_all_points_outside_calibration_region: bool,

    /// Hide the progress bar
    #[arg(long)]
    no_progress: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "info");
    }

    open_files_and_run().await
}

async fn open_files_and_run() -> anyhow::Result<()> {
    let _tracing_guard = env_tracing_logger::init();

    let cli = Cli::parse();

    let cal_file_name = cli
        .calibration_params
        .file_name()
        .unwrap()
        .to_str()
        .unwrap();

    let tracking_params_buf = match cli.tracking_params {
        Some(ref fname) => {
            info!("reading tracking parameters from file {}", fname.display());
            // read the tracking parameters
            let buf = std::fs::read_to_string(fname)
                .map_err(anyhow::Error::from)
                .context(format!("loading tracking parameters {}", fname.display()))?;
            Some(buf)
        }
        None => None,
    };

    let flydra_csv_temp_dir = tempfile::Builder::new()
        .prefix("strand-convert")
        .tempdir()?;

    info!("strand-cam csv conversion to temporary flydra format:");
    info!(
        "  {} -> {}",
        cli.flytrax_csv.display(),
        flydra_csv_temp_dir.as_ref().display()
    );

    let output_braidz = match cli.output_braidz {
        Some(op) => op,
        None => cli.flytrax_csv.with_extension("braidz"), // replace '.csv' -> '.braidz'
    };

    let data_file = std::fs::File::open(&cli.flytrax_csv)
        .map_err(anyhow::Error::from)
        .context(format!(
            "Could not open point detection csv file: {}",
            cli.flytrax_csv.display()
        ))?;

    let point_detection_csv_reader = std::io::BufReader::new(data_file);

    let mut flytrax_image = None;
    let mut flytrax_jpeg_fname = cli.flytrax_csv.clone();
    flytrax_jpeg_fname.set_extension("jpg");
    if flytrax_jpeg_fname.exists() {
        let jpeg_buf = std::fs::read(&flytrax_jpeg_fname)
            .with_context(|| format!("reading {}", flytrax_jpeg_fname.display()))?;
        flytrax_image = Some(
            image::load_from_memory_with_format(&jpeg_buf, image::ImageFormat::Jpeg)
                .with_context(|| format!("parsing {}", flytrax_jpeg_fname.display()))?,
        );
    } else {
        log::warn!(
            "File {} did not exist - cannot preserve flytrax image.",
            flytrax_jpeg_fname.display()
        );
    }

    let mut filters = Vec::new();

    if !cli.track_all_points_outside_calibration_region {
        filters.push(RowFilter::InPseudoCalRegion);
    }

    let eargs = cli
        .apriltags_3d_fiducial_coords
        .map(
            |apriltags_3d_fiducial_coords| flytrax_csv_to_braidz::ExtrinsicsArgs {
                apriltags_3d_fiducial_coords,
                flytrax_csv: cli.flytrax_csv,
                image_filename: flytrax_jpeg_fname,
            },
        );

    let opt2 = braid_offline::KalmanizeOptions {
        start_frame: cli.start_frame,
        stop_frame: cli.stop_frame,
        ..Default::default()
    };

    parse_configs_and_run(
        point_detection_csv_reader,
        Some(&flydra_csv_temp_dir),
        flytrax_image,
        &output_braidz,
        cal_file_name,
        tracking_params_buf.as_ref().map(AsRef::as_ref),
        &filters,
        cli.no_progress,
        eargs,
        opt2,
    )
    .await?;

    flydra_csv_temp_dir.close()?;

    Ok(())
}
