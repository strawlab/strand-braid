use clap::Parser;

use apriltag_track_movie::{Cli, run_cli};

fn main() -> eyre::Result<()> {
    let cli = Cli::parse();
    run_cli(cli)
}
