use clap::Parser;

use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    src_dir: PathBuf,
    /// Destination .braidz filename.
    ///
    /// If not specified, `.braidz` will be appended to the source directory
    /// name.
    #[arg(long)]
    dest: Option<PathBuf>,
}

fn add_extension(path: &mut std::path::PathBuf, extension: impl AsRef<std::path::Path>) {
    match path.extension() {
        Some(ext) => {
            let mut ext = ext.to_os_string();
            ext.push(".");
            ext.push(extension.as_ref());
            path.set_extension(ext)
        }
        None => path.set_extension(extension.as_ref()),
    };
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let dest = if let Some(dest) = cli.dest {
        dest
    } else {
        let mut dest = cli.src_dir.clone();
        add_extension(&mut dest, "braidz");
        dest
    };

    braidz_writer::dir_to_braidz(&cli.src_dir, dest)?;

    Ok(())
}
