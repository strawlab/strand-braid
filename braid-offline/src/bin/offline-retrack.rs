use anyhow::Context;
use std::convert::TryInto;

use log::info;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "offline-retrack")]
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

fn main() -> anyhow::Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var(
            "RUST_LOG",
            "braid_offline=info,flydra2=info,flydra2_mainbrain=info,error",
        );
    }

    env_tracing_logger::init();

    let mut runtime = tokio::runtime::Builder::new()
        .threaded_scheduler()
        .enable_all()
        .build()
        .expect("runtime");

    let rt_handle = runtime.handle().clone();
    runtime.block_on(inner(rt_handle))
}

async fn inner(rt_handle: tokio::runtime::Handle) -> anyhow::Result<()> {
    let opt = Opt::from_args();

    // TODO: open data_src with braidz_parser here?

    let tracking_params: flydra2::TrackingParams = match opt.tracking_params {
        Some(ref fname) => {
            info!("reading tracking parameters from file {}", fname.display());
            // read the traking parameters
            let mut file = std::fs::File::open(&fname)
                .map_err(|e| anyhow::Error::from(e))
                .context(format!("loading tracking parameters {}", fname.display()))?;
            let mut buf = String::new();
            std::io::Read::read_to_string(&mut file, &mut buf)?;
            let tracking_params: flydra_types::TrackingParams = toml::from_str(&buf)?;
            tracking_params.try_into()?
        }
        None => {
            // TODO: check if parameters are in textlog of input file and re-use those if present.
            flydra2::TrackingParams::default()
        }
    };
    let mut opts = braid_offline::KalmanizeOptions::default();
    opts.start_frame = opt.start_frame;
    opts.stop_frame = opt.stop_frame;
    let data_src = zip_or_dir::ZipDirArchive::auto_from_path(opt.data_src.as_path())?;

    // The user specifies an output .braidz file. But we will save initially to
    // a .braid directory. We here ensure the user's name had ".braidz"
    // extension and then calculate the name of the new output directory.
    let output_braidz = opt.output;

    // Raise an error if outputs exist.
    if output_braidz.exists() {
        return Err(anyhow::format_err!(
            "Path {} exists. Will not overwrite.",
            output_braidz.display()
        )
        .into());
    }

    let save_performance_histograms = true;

    braid_offline::kalmanize(
        data_src,
        output_braidz,
        opt.fps,
        tracking_params,
        opts,
        rt_handle,
        save_performance_histograms,
    )
    .await?;
    Ok(())
}
