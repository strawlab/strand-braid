use clap::Parser;

use color_eyre::eyre::Context;
use flydra_mvg::FlydraMultiCameraSystem;

#[derive(Debug, Parser)]
#[command(name = "cal-to-xml", version)]
struct Opt {
    /// Input path (pymvg file, flydra xml file, or MCSC directory)
    input: std::path::PathBuf,
}

fn main() -> color_eyre::Result<()> {
    let opt = Opt::parse();
    let cal_path = opt.input;

    println!("Reading calibration at {}", cal_path.display());
    let out_fname = format!("{}.xml", cal_path.display());

    let calibration = FlydraMultiCameraSystem::<f64>::from_path(&cal_path)
        .with_context(|| format!("while reading calibration at {}", cal_path.display()))?;

    let mut out_fd = std::fs::File::create(&out_fname)?;
    calibration.to_flydra_xml(&mut out_fd)?;
    Ok(())
}
