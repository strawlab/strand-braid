#[macro_use]
extern crate log;
#[macro_use]
extern crate lazy_static;

use anyhow::Context;

use strand_cam_offline_kalmanize::{parse_configs_and_run, PseudoCalParams, RowFilter};

use std::io::Read;

use structopt::StructOpt;

lazy_static! {
    static ref VAL_HELP: String = {
        let example = PseudoCalParams {
            physical_diameter_meters: 0.1,
            center_x: 640,
            center_y: 480,
            radius: 480,
        };
        let cal_buf = toml::to_string(&example).unwrap();

        let tparams1 = flydra2::SwitchingTrackingParams::default();
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
    /// Output file, must end with '.braidz'
    #[structopt(long = "output", short = "o", parse(from_os_str))]
    output_braidz: Option<std::path::PathBuf>,
    /// Tracking parameters TOML file. (Includes `motion_noise_scale`, amongst others.)
    #[structopt(long = "tracking-params", short = "t", parse(from_os_str))]
    tracking_params: Option<std::path::PathBuf>,
    /// Calibration parameters TOML file. (Includes `center_x`, amongst others.)
    #[structopt(long = "cal", short = "p", parse(from_os_str))]
    calibration_params: std::path::PathBuf,

    /// Include all data from outside the calibration region in tracking
    #[structopt(long = "include-all", short = "a")]
    track_all_points_outside_calibration_region: bool,
}

fn main() -> anyhow::Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "info");
    }

    open_files_and_run()
}

fn open_files_and_run() -> anyhow::Result<()> {
    env_logger::init();
    let opt = Opt::from_args();

    let calibration_params_buf = {
        info!(
            "reading calibration parameters from file {}",
            opt.calibration_params.display()
        );
        // read the calibration parameters
        let mut file = std::fs::File::open(&opt.calibration_params)
            .map_err(|e| anyhow::Error::from(e))
            .context(format!(
                "loading calibration parameters {}",
                opt.calibration_params.display()
            ))?;
        let mut buf = String::new();
        Read::read_to_string(&mut file, &mut buf)?;
        buf
    };

    let tracking_params_buf = match opt.tracking_params {
        Some(ref fname) => {
            info!("reading tracking parameters from file {}", fname.display());
            // read the traking parameters
            let mut file = std::fs::File::open(&fname)
                .map_err(|e| anyhow::Error::from(e))
                .context(format!("loading tracking parameters {}", fname.display()))?;
            let mut buf = String::new();
            Read::read_to_string(&mut file, &mut buf)?;
            Some(buf)
        }
        None => None,
    };

    let flydra_csv_temp_dir = tempdir::TempDir::new("strand-convert")?;

    info!("strand-cam csv conversion to temporary flydra format:");
    info!(
        "  {} -> {}",
        opt.point_detection_csv.display(),
        flydra_csv_temp_dir.as_ref().display()
    );

    let output_braidz = match opt.output_braidz {
        Some(op) => op,
        None => opt
            .point_detection_csv
            .to_path_buf()
            .with_extension("braidz"), // replace '.csv' -> '.braidz'
    };

    let data_file = std::fs::File::open(&opt.point_detection_csv)
        .map_err(|e| anyhow::Error::from(e))
        .context(format!(
            "Could not open point detection csv file: {}",
            opt.point_detection_csv.display()
        ))?;

    let point_detection_csv_reader = std::io::BufReader::new(data_file);

    let mut filters = Vec::new();

    if !opt.track_all_points_outside_calibration_region {
        filters.push(RowFilter::InPseudoCalRegion);
    }

    parse_configs_and_run(
        point_detection_csv_reader,
        Some(&flydra_csv_temp_dir),
        &output_braidz,
        &calibration_params_buf,
        tracking_params_buf.as_ref().map(AsRef::as_ref),
        &filters,
    )?;

    flydra_csv_temp_dir.close()?;

    Ok(())
}
