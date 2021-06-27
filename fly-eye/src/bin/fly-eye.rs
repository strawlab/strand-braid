#[macro_use]
extern crate log;

extern crate ci2;
#[cfg(feature = "camsrc_dc1394")]
use ci2_dc1394 as camsrc;
#[cfg(feature = "camsrc_flycap2")]
use ci2_flycap2 as camsrc;

use ci2::{Camera, CameraInfo, CameraModule};
use crossbeam_ok::CrossbeamOk;
use fly_eye::{run_func, App};

fn main() -> Result<(), failure::Error> {
    env_logger::init();

    let (firehose_tx, firehose_rx) = channellib::unbounded();
    std::thread::spawn(move || {
        run_func(move || {
            let mut mymod = camsrc::new_module()?;
            info!("camera module: {}", mymod.name());

            let infos = mymod.camera_infos().expect("get camera info");
            if infos.len() == 0 {
                panic!("No cameras found.")
            }
            let mut cam = mymod.camera(&infos[0].name())?;
            info!("  got camera {:?}", cam.name());

            cam.set_acquisition_mode(ci2::AcquisitionMode::Continuous)?;
            cam.acquisition_start()?;

            loop {
                let frame = cam.next_frame()?;
                firehose_tx.send(frame.into()).cb_ok();
            }
        });
    });

    let mut app = App { rx: firehose_rx };
    app.mainloop()?;
    Ok(())
}
