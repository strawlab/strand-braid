use basic_frame::DynamicFrame;
use channellib::Sender;
#[cfg(feature = "camsrc_pyloncxx")]
use ci2_pyloncxx as camsrc;

use ci2::{Camera, CameraInfo, CameraModule};
use crossbeam_ok::CrossbeamOk;

fn thread_loop(firehose_tx: Sender<DynamicFrame>) -> anyhow::Result<()> {
    let mymod = camsrc::new_module()?;
    log::info!("camera module: {}", (&mymod).name());

    let infos = (&mymod).camera_infos().expect("get camera info");
    if infos.len() == 0 {
        panic!("No cameras found.")
    }
    let mut cam = (&mymod).camera(&infos[0].name())?;
    log::info!("  got camera {:?}", cam.name());

    cam.set_acquisition_mode(ci2::AcquisitionMode::Continuous)?;
    cam.acquisition_start()?;

    loop {
        let frame = cam.next_frame()?;
        firehose_tx.send(frame.into()).cb_ok();
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let (firehose_tx, firehose_rx) = channellib::unbounded();

    std::thread::spawn(move || {
        thread_loop(firehose_tx).unwrap();
    });

    fly_eye::mainloop(firehose_rx)?;
    Ok(())
}
