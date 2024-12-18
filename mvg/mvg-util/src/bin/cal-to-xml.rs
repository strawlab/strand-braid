use clap::Parser;

use eyre::Context;
use flydra_mvg::FlydraMultiCameraSystem;

#[derive(Debug, Parser)]
#[command(name = "cal-to-xml", version)]
struct Opt {
    /// Input path (pymvg file, flydra xml file, or MCSC directory)
    input: std::path::PathBuf,
}

fn main() -> eyre::Result<()> {
    let opt = Opt::parse();
    let cal_path = opt.input;

    println!("Reading calibration at {}", cal_path.display());
    let out_base = format!("{}", cal_path.display());
    let out_base = if out_base.ends_with("/") {
        &out_base[..out_base.len() - 1]
    } else {
        &out_base[..]
    };
    let out_fname = format!("{out_base}.xml");

    let calibration = FlydraMultiCameraSystem::<f64>::from_path(&cal_path)
        .with_context(|| format!("while reading calibration at {}", cal_path.display()))?;

    println!("Writing calibration to {out_fname}");
    let mut out_fd = std::fs::File::create(&out_fname)?;
    calibration.to_flydra_xml(&mut out_fd)?;
    Ok(())
}
