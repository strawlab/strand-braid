use clap::Parser;
use eyre::Result;

use strand_cam_offline_checkerboards::{run_cal, Cli};

fn main() -> Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        // SAFETY: We ensure that this only happens in single-threaded code
        // because this is immediately at the start of main() and no other
        // threads have started.
        unsafe { std::env::set_var("RUST_LOG", "info") };
    }

    env_logger::init();
    let cli = Cli::parse();
    run_cal(cli)
}
