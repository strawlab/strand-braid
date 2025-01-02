use basic_frame::DynamicFrame;
#[cfg(feature = "backend_pyloncxx")]
use ci2_pyloncxx as backend;
use std::sync::mpsc::Sender;

use ci2::{Camera, CameraInfo, CameraModule};

fn thread_loop(firehose_tx: Sender<DynamicFrame>) -> anyhow::Result<()> {
    let mymod = backend::new_module()?;
    tracing::info!("camera module: {}", (&mymod).name());

    let infos = (&mymod).camera_infos().expect("get camera info");
    if infos.len() == 0 {
        panic!("No cameras found.")
    }
    let mut cam = (&mymod).camera(&infos[0].name())?;
    tracing::info!("  got camera {:?}", cam.name());

    cam.set_acquisition_mode(ci2::AcquisitionMode::Continuous)?;
    cam.acquisition_start()?;

    loop {
        let frame = cam.next_frame()?;
        firehose_tx
            .send(frame.image.into())
            .map_err(|_| anyhow::anyhow!("receiver disconnected"))?;
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let (firehose_tx, firehose_rx) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        thread_loop(firehose_tx).unwrap();
    });

    fly_eye::mainloop(firehose_rx)?;
    Ok(())
}
