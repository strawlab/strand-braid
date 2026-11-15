// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

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

    let require_radfiles = false;
    let calibration = FlydraMultiCameraSystem::<f64>::from_path(&cal_path, require_radfiles)
        .with_context(|| format!("while reading calibration at {}", cal_path.display()))?;

    println!("Writing calibration to {out_fname}");
    // Serialize before creating the file, so that a failure to do so does not
    // leave an empty file behind.
    let mut xml_buf: Vec<u8> = Vec::new();
    calibration.to_flydra_xml(&mut xml_buf)?;
    std::fs::write(&out_fname, xml_buf)?;
    Ok(())
}
