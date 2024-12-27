use clap::Parser;

use apriltag_track_movie::{run_cli, Cli};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    run_cli(cli)
}
