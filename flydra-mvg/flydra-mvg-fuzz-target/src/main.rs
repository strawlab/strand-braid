#[macro_use]
extern crate afl;

use flydra_mvg::FlydraMultiCameraSystem;
use flydra_mvg_fuzz_target::{do_test, CALIBRATION_FILE};

fn main() {
    let cams = FlydraMultiCameraSystem::<f64>::from_flydra_xml(CALIBRATION_FILE.as_bytes())
        .expect("from_flydra_xml");

    fuzz!(|data: &[u8]| { crate::do_test(&cams, data, false) });
    // do_test(&cams, b"123456789012345678901234");
}
