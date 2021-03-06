#[macro_use]
extern crate log;
#[macro_use]
extern crate lazy_static;

use strand_cam_offline_kalmanize::{parse_configs_and_run, PseudoCalParams};

use std::io::Read;

use failure::ResultExt;
use structopt::StructOpt;

use flydra2::Result;

lazy_static! {
    static ref VAL_HELP: String = {
        let example = PseudoCalParams {
            physical_diameter_meters: 0.1,
            center_x: 640,
            center_y: 480,
            radius: 480,
        };
        let cal_buf = toml::to_string(&example).unwrap();

        let tparams1 = flydra2::TrackingParams::default();
        let tracking_example: flydra_types::TrackingParams = tparams1.into();
        let tracking_buf_buf = toml::to_string(&tracking_example).unwrap();

        format!("EXAMPLE .TOML FILES:\n\n# Example calibration .toml file:\n```\n{}```\n\n# Example tracking parameter .toml file:\n```\n{}```",
            cal_buf,
            tracking_buf_buf)
    };
}

/// This program will read a CSV file saved by strand-cam and, using Kalman
/// filtering and data association, track the any objects.
#[derive(Debug, StructOpt)]
#[structopt(after_help = VAL_HELP.as_str())]
struct Opt {
    /// Input CSV file with 2D detections
    #[structopt(long = "csv", short = "c", parse(from_os_str))]
    point_detection_csv: std::path::PathBuf,
    /// Output file
    #[structopt(long = "output", short = "o", parse(from_os_str))]
    output: Option<std::path::PathBuf>,
    /// Tracking parameters TOML file. (Includes `motion_noise_scale`, amongst others.)
    #[structopt(long = "tracking-params", short = "t", parse(from_os_str))]
    tracking_params: Option<std::path::PathBuf>,
    /// Calibration parameters TOML file. (Includes `center_x`, amongst others.)
    #[structopt(long = "cal", short = "p", parse(from_os_str))]
    calibration_params: std::path::PathBuf,
}

fn main() -> Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "info");
    }

    open_files_and_run()
}

fn open_files_and_run() -> Result<()> {
    env_logger::init();
    let opt = Opt::from_args();

    let calibration_params_buf = {
        info!(
            "reading calibration parameters from file {}",
            opt.calibration_params.display()
        );
        // read the calibration parameters
        let mut file = std::fs::File::open(&opt.calibration_params)
            .context(format!(
                "loading calibration parameters {}",
                opt.calibration_params.display()
            ))
            .map_err(|e| failure::Error::from(e))?;
        let mut buf = String::new();
        Read::read_to_string(&mut file, &mut buf)?;
        buf
    };

    let tracking_params_buf = match opt.tracking_params {
        Some(ref fname) => {
            info!("reading tracking parameters from file {}", fname.display());
            // read the traking parameters
            let mut file = std::fs::File::open(&fname)
                .context(format!("loading tracking parameters {}", fname.display()))
                .map_err(|e| failure::Error::from(e))?;
            let mut buf = String::new();
            Read::read_to_string(&mut file, &mut buf)?;
            Some(buf)
        }
        None => None,
    };

    let data_dir = tempdir::TempDir::new("strand-convert")?;

    info!("strand-cam csv conversion:");
    info!(
        "  {} -> {}",
        opt.point_detection_csv.display(),
        data_dir.as_ref().display()
    );

    let output_dirname = match opt.output {
        Some(op) => op,
        None => opt
            .point_detection_csv
            .to_path_buf()
            .with_extension("braid"), // replace '.csv' -> '.braid'
    };

    let data_file = std::fs::File::open(&opt.point_detection_csv)
        .context(format!(
            "Could not open point detection csv file: {}",
            opt.point_detection_csv.display()
        ))
        .map_err(|e| failure::Error::from(e))?;

    let point_detection_csv_reader = std::io::BufReader::new(data_file);

    parse_configs_and_run(
        point_detection_csv_reader,
        data_dir,
        &output_dirname,
        &calibration_params_buf,
        tracking_params_buf.as_ref().map(AsRef::as_ref),
    )
}
