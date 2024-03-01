use anyhow::Context;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[structopt(name = "braidz-cli")]
#[command(author, version)]
struct Opt {
    /// Input braidz filename
    input: PathBuf,

    /// The command to run. Defaults to "print".
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Print a summary of the .braidz file
    Print {
        /// print all data in the `data2d_distorted` table
        #[arg(short, long)]
        data2d_distorted: bool,
    },
}

fn main() -> anyhow::Result<()> {
    env_tracing_logger::init();
    let opt = Opt::parse();
    let command = opt.command.unwrap_or(Commands::Print {
        data2d_distorted: false,
    });
    println!("{:?}", command);
    let attr = std::fs::metadata(&opt.input)
        .with_context(|| format!("Getting file metadata for {}", opt.input.display()))?;

    let mut archive = braidz_parser::braidz_parse_path(&opt.input)
        .with_context(|| format!("Parsing file {}", opt.input.display()))?;

    let summary =
        braidz_parser::summarize_braidz(&archive, opt.input.display().to_string(), attr.len());

    match command {
        Commands::Print { data2d_distorted } => {
            let yaml_buf = serde_yaml::to_string(&summary)?;
            println!("{}", yaml_buf);

            if data2d_distorted {
                println!("data2d_distorted table: --------------");
                for row in archive.iter_data2d_distorted()? {
                    println!("{:?}", row);
                }
            }
        }
    }

    Ok(())
}
