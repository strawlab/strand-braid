#[macro_use]
extern crate pretty_print_nalgebra;

use clap::Parser;

type Result<T> = std::result::Result<T, mvg::MvgError>;

#[derive(Debug, Parser)]
#[command(name = "print-cal", version)]
struct Opt {
    /// Input and output directory
    filename: std::path::PathBuf,
}

fn print_cal(filename: &std::path::Path) -> Result<()> {
    println!("# ----- {:?} ----- ", filename);
    let fd = std::fs::File::open(&filename)?;
    let cams = flydra_mvg::FlydraMultiCameraSystem::<f64>::from_flydra_xml(fd)?;
    for cam_name in cams.cam_names() {
        let cam = cams.cam_by_name(cam_name).unwrap();
        // let cam_name = cams.get_name(cam_name).unwrap();
        println!("  {}", cam_name);
        let intrinsics = cam.do_not_use_intrinsics();
        println!("P {}", pretty_print!(intrinsics.p));
        println!("K {}", pretty_print!(intrinsics.k));
    }
    Ok(())
}

fn main() -> Result<()> {
    let opt = Opt::parse();
    print_cal(&opt.filename)?;
    Ok(())
}
