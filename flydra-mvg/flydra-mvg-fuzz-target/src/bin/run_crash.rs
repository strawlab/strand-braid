use anyhow::{Context, Result};
use flydra_mvg::FlydraMultiCameraSystem;
use flydra_mvg_fuzz_target::{do_test, CALIBRATION_FILE};

fn main() -> Result<(), anyhow::Error> {
    let cams = FlydraMultiCameraSystem::<f64>::from_flydra_xml(CALIBRATION_FILE.as_bytes())
        .expect("from_flydra_xml");

    let dirname = "out/crashes";
    for entry in std::fs::read_dir(dirname)
        .with_context(|| format!("Failed to read crashes from {}", dirname))?
    {
        let entry = entry?;
        let path = entry.path();

        if path.as_os_str() == std::ffi::OsStr::new("out/crashes/README.txt") {
            continue;
        }

        println!("reading {}", path.display());
        let buf = std::fs::read(path)?;
        do_test(&cams, &buf, true);
    }
    Ok(())
}
