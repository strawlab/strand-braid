use std::convert::TryInto;

use failure::ResultExt;
use log::info;
use structopt::StructOpt;

use flydra2::Result;

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

fn main() -> Result<()> {
    let mut runtime = tokio::runtime::Builder::new()
        .threaded_scheduler()
        .enable_all()
        .build()
        .expect("runtime");

    let rt_handle = runtime.handle().clone();
    runtime.block_on(inner(rt_handle))
}

async fn inner(rt_handle: tokio::runtime::Handle) -> Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var(
            "RUST_LOG",
            "braid_offline=info,flydra2=info,flydra2_mainbrain=info,error",
        );
    }

    env_logger::init();
    let opt = Opt::from_args();

    // TODO: open data_src with braidz_parser here?

    let tracking_params: flydra2::TrackingParams = match opt.tracking_params {
        Some(ref fname) => {
            info!("reading tracking parameters from file {}", fname.display());
            // read the traking parameters
            let mut file = std::fs::File::open(&fname)
                .context(format!("loading tracking parameters {}", fname.display()))
                .map_err(|e| failure::Error::from(e))?;
            let mut buf = String::new();
            std::io::Read::read_to_string(&mut file, &mut buf)?;
            let tracking_params: flydra_types::TrackingParams = toml::from_str(&buf)?;
            tracking_params.try_into()?
        }
        None => flydra2::TrackingParams::default(),
    };
    let mut opts = flydra2::KalmanizeOptions::default();
    opts.start_frame = opt.start_frame;
    opts.stop_frame = opt.stop_frame;
    let data_src = zip_or_dir::ZipDirArchive::auto_from_path(opt.data_src.as_path())?;

    // The user specifies an output .braidz file. But we will save initially to
    // a .braid directory. We here ensure the user's name had ".braidz"
    // extension and then calculate the name of the new output directory.
    let output_braidz = opt.output;
    let output_dirname = if output_braidz.extension() == Some(std::ffi::OsStr::new("braidz")) {
        let mut output_dirname: std::path::PathBuf = output_braidz.clone();
        output_dirname.set_extension("braid");
        output_dirname
    } else {
        return Err(failure::format_err!("output file must end in '.braidz'").into());
    };

    // Raise an error if outputs exist.
    for test_path in &[&output_braidz, &output_dirname] {
        if test_path.exists() {
            return Err(failure::format_err!(
                "Path {} exists. Will not overwrite.",
                test_path.display()
            )
            .into());
        }
    }

    flydra2::kalmanize(
        data_src,
        output_dirname,
        opt.fps,
        tracking_params,
        opts,
        rt_handle,
    )
    .await
}
