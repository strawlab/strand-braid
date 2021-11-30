extern crate env_logger;
extern crate log;

extern crate ci2;
#[cfg(feature = "backend_aravis")]
extern crate ci2_aravis as backend;
#[cfg(feature = "backend_pyloncxx")]
extern crate ci2_pyloncxx as backend;
extern crate machine_vision_formats as formats;

use ci2::{Camera, CameraModule};
use timestamped_frame::ExtraTimeData;

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let mymod = backend::new_module()?;
    let mut mymodref = &mymod;
    let infos = mymodref.camera_infos()?;
    if infos.len() == 0 {
        anyhow::bail!("no cameras detected");
    }
    for info in infos.iter() {
        println!("opening camera {}", info.name());
        let mut cam = mymodref.camera(info.name())?;
        println!("got camera");
        cam.acquisition_start()?;
        for _ in 0..10 {
            match cam.next_frame() {
                Ok(frame) => {
                    println!(
                        "  got frame {}: {}x{} {}",
                        frame.extra().host_framenumber(),
                        frame.width(),
                        frame.height(),
                        frame.pixel_format()
                    );
                }
                Err(ci2::Error::SingleFrameError(s)) => {
                    println!("  ignoring singleFrameError({})", s);
                }
                Err(e) => {
                    return Err(e.into());
                }
            }
        }
        cam.acquisition_stop()?;
        break;
    }

    Ok(())
}
