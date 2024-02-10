use clap::Parser;
use tracing_futures::Instrument;

fn main() -> anyhow::Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "braid_offline=info,flydra2=info,warn");
    }

    // console_subscriber::init();
    let _tracing_guard = env_tracing_logger::init();

    let opt = braid_offline::Cli::parse();

    let future = async { braid_offline::braid_offline_retrack(opt).await };
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
