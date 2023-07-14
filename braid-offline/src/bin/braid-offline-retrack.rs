use anyhow::Context;

use clap::Parser;
use tracing::info;
use tracing_futures::Instrument;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    /// Input .braidz file
    #[arg(short = 'd', long)]
    data_src: std::path::PathBuf,
    /// Output file (must end with .braidz)
    #[arg(short = 'o', long)]
    output: std::path::PathBuf,
    /// Set frames per second
    #[arg(long)]
    fps: Option<f64>,
    /// Set start frame to start tracking
    #[arg(long)]
    start_frame: Option<u64>,
    /// Set stop frame to stop tracking
    #[arg(long)]
    stop_frame: Option<u64>,
    /// Tracking parameters TOML file.
    #[arg(long)]
    tracking_params: Option<std::path::PathBuf>,

    /// Disable display of progress indicator
    #[arg(long)]
    no_progress: bool,
}

fn main() -> anyhow::Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "braid_offline=info,flydra2=info,error");
    }

    // console_subscriber::init();
    let _tracing_guard = env_tracing_logger::init();
    let future = async { braid_offline_retrack().await };
    let instrumented = future.instrument(tracing::info_span!("braid-offline-retrack"));

    // Multi-threaded runtime
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .thread_name("braid-offline-retrack")
        // .thread_stack_size(3 * 1024 * 1024)
        .build()?;
    // let rt = tokio::runtime::Runtime::new()?;

    // // Single-threaded runtime
    // let rt = tokio::runtime::Builder::new_current_thread()
    //     .enable_all()
    //     .build()
    //     .unwrap();

    // spawn the root task
    rt.block_on(instrumented)
}

/// This is our "real" main top-level function but we have some decoration we
/// need to do in [main], so we name this differently.
#[tracing::instrument]
async fn braid_offline_retrack() -> anyhow::Result<()> {
    let opt = Cli::parse();

    let data_src =
        braidz_parser::incremental_parser::IncrementalParser::open(opt.data_src.as_path())?;
    let data_src = data_src.parse_basics()?;

    let tracking_params: flydra_types::TrackingParams = match opt.tracking_params {
        Some(ref fname) => {
            info!("reading tracking parameters from file {}", fname.display());
            // read the traking parameters
            let buf = std::fs::read_to_string(fname)
                .context(format!("loading tracking parameters {}", fname.display()))?;
            let tracking_params: flydra_types::TrackingParams = toml::from_str(&buf)?;
            tracking_params
        }
        None => {
            let parsed = data_src.basic_info();
            match parsed.tracking_params.clone() {
                Some(tp) => tp,
                None => {
                    let num_cams = data_src.basic_info().cam_info.camid2camn.len();
                    match num_cams {
                        0 => {
                            anyhow::bail!(
                                "No tracking parameters specified, none found in \
                            data_src, and no default is reasonable because zero cameras present."
                            )
                        }
                        1 => flydra_types::default_tracking_params_flat_3d(),
                        _ => flydra_types::default_tracking_params_full_3d(),
                    }
                }
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
        "braid-offline-retrack",
        opt.no_progress,
    )
    .await?;
    Ok(())
}
