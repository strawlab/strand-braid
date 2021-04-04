extern crate log;
extern crate env_logger;

#[cfg(feature = "backend_aravis")]
extern crate ci2_aravis as backend;
#[cfg(feature = "backend_dc1394")]
extern crate ci2_dc1394 as backend;
#[cfg(feature = "backend_flycap2")]
extern crate ci2_flycap2 as backend;
#[cfg(feature = "backend_pyloncxx")]
extern crate ci2_pyloncxx as backend;
extern crate ci2;
extern crate machine_vision_formats as formats;
extern crate timestamped_frame;

use timestamped_frame::HostTimeData;
use ci2::{CameraModule, Camera};

fn main() -> ci2::Result<()> {
    env_logger::init();

    let mut mymod = backend::new_module()?;
    let infos = mymod.camera_infos()?;
    if infos.len() == 0 {
        return Err("no cameras detected".into());
    }
    for info in infos.iter() {
        println!("opening camera {}", info.name());
        let mut cam = mymod.camera(info.name())?;
        println!("got camera");
        cam.acquisition_start()?;
        for _ in 0..10 {
            match cam.next_frame() {
                Ok(frame) => {
                    println!("  got frame {}: {:?}", frame.host_framenumber(),
                        frame);
                },
                Err(ci2::Error::SingleFrameError(s)) => {
                    println!("  ignoring singleFrameError({})", s);
                },
                Err(e) => {
                    return Err(e);
                }
            }
        }
        cam.acquisition_stop()?;
        break;
    }

    Ok(())
}
