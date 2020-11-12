#[macro_use]
extern crate log;

use failure::{Error, ResultExt};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "braid", about = "multi-camera realtime 3D tracker")]
struct BraidLauncherCliArgs {
    command: String,
    options: Vec<String>,
}

fn main() -> Result<(), Error> {
    env_logger::init();
    // braid::braid_start("braid")?;

    let args = BraidLauncherCliArgs::from_args();
    debug!("{:?}", args);

    let cmd_name = format!("braid-{}", args.command);

    let status = std::process::Command::new(&cmd_name)
        .args(args.options)
        .status()
        .context(format!("running '{}'", cmd_name))?;

    if let Some(code) = status.code() {
        std::process::exit(code);
    }

    debug!("done");

    Ok(())
}
