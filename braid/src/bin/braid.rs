use clap::{CommandFactory, Parser};
use eyre::{Result, WrapErr};

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct BraidLauncherCliArgs {
    /// Command to execute (e.g. run, show-config, default-config, help)
    command: String,
    /// Options specific to the command
    options: Vec<String>,
}

fn main() -> Result<()> {
    env_tracing_logger::init();

    // TODO: In case of no command given (or a query command), iterate all dirs
    // on environment path, collect braid-* executables, show these as possible
    // commands.

    let args = BraidLauncherCliArgs::parse();
    tracing::debug!("{:?}", args);

    if args.command == "help" {
        let help = BraidLauncherCliArgs::command().render_long_help();
        println!("{}", help.ansi());
        return Ok(());
    }

    let cmd_name = format!("braid-{}", args.command);

    let status = std::process::Command::new(&cmd_name)
        .args(args.options)
        .status()
        .with_context(|| format!("running '{}'", cmd_name))?;

    if let Some(code) = status.code() {
        std::process::exit(code);
    }

    tracing::debug!("done");

    Ok(())
}
