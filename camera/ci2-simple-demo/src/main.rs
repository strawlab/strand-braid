extern crate env_logger;

extern crate ci2;
#[cfg(feature = "backend_pyloncxx")]
extern crate ci2_pyloncxx as backend;
#[cfg(feature = "backend_vimba")]
extern crate ci2_vimba as backend;
extern crate machine_vision_formats as formats;

use ci2::{Camera, CameraModule};

lazy_static::lazy_static! {
    static ref CAMLIB: backend::WrappedModule = backend::new_module().unwrap();
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let _guard = backend::make_singleton_guard(&&*CAMLIB)?;
    let mut wrapped_mod: &backend::WrappedModule = &*CAMLIB;
    let infos = wrapped_mod.camera_infos()?;
    if infos.len() == 0 {
        anyhow::bail!("no cameras detected");
    }
    for info in infos.iter() {
        println!("opening camera {}", info.name());
        let mut cam = wrapped_mod.camera(info.name())?;
        println!("got camera");
        cam.acquisition_start()?;
        for _ in 0..10 {
            match cam.next_frame() {
                Ok(frame) => {
                    println!(
                        "  got frame {}: {}x{} {}",
                        frame.host_timing.fno,
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
