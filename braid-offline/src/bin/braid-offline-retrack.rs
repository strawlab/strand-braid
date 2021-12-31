use anyhow::Context;
use std::convert::TryInto;

use log::info;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "braid-offline-retrack")]
struct Opt {
    /// Input .braid directory
    #[structopt(short = "d", parse(from_os_str))]
    data_src: std::path::PathBuf,
    /// Output file (must end with .braidz)
    #[structopt(short = "o", parse(from_os_str))]
    output: std::path::PathBuf,
    /// Set frames per second
    #[structopt(long = "fps")]
    fps: Option<f64>,
    /// Set start frame to start tracking
    #[structopt(long = "start-frame")]
    start_frame: Option<u64>,
    /// Set stop frame to stop tracking
    #[structopt(long = "stop-frame")]
    stop_frame: Option<u64>,
    /// Tracking parameters TOML file.
    #[structopt(long = "tracking-params", parse(from_os_str))]
    tracking_params: Option<std::path::PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var(
            "RUST_LOG",
            "braid_offline=info,flydra2=info,flydra2_mainbrain=info,error",
        );
    }

    env_tracing_logger::init();
    let opt = Opt::from_args();

    let data_src =
        braidz_parser::incremental_parser::IncrementalParser::open(opt.data_src.as_path())?;
    let data_src = data_src.parse_basics()?;

    let tracking_params: flydra2::SwitchingTrackingParams = match opt.tracking_params {
        Some(ref fname) => {
            info!("reading tracking parameters from file {}", fname.display());
            // read the traking parameters
            let buf = std::fs::read_to_string(&fname)
                .context(format!("loading tracking parameters {}", fname.display()))?;
            let tracking_params: flydra_types::TrackingParams = toml::from_str(&buf)?;
            tracking_params.try_into()?
        }
        None => {
            let parsed = data_src.basic_info();
            match parsed.tracking_params.clone() {
                Some(tp) => tp.try_into().unwrap(),
                None => flydra2::SwitchingTrackingParams::default(),
            }
        }
    };
    let opts = braid_offline::KalmanizeOptions {
        start_frame: opt.start_frame,
        stop_frame: opt.stop_frame,
        ..Default::default()
    };

    // The user specifies an output .braidz file. But we will save initially to
    // a .braid directory. We here ensure the user's name had ".braidz"
    // extension and then calculate the name of the new output directory.
    let output_braidz = opt.output;

    // Raise an error if outputs exist.
    if output_braidz.exists() {
        return Err(anyhow::format_err!(
            "Path {} exists. Will not overwrite.",
            output_braidz.display()
        ));
    }

    let rt_handle = tokio::runtime::Handle::current();

    let save_performance_histograms = true;

    braid_offline::kalmanize(
        data_src,
        output_braidz,
        opt.fps,
        tracking_params,
        opts,
        rt_handle,
        save_performance_histograms,
        "braid offline-retrack",
    )
    .await?;
    Ok(())
}
