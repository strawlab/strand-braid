use futures::stream::StreamExt;

use ci2::{Camera, CameraModule};
use ci2_async::AsyncCamera;
use timestamped_frame::{ExtraTimeData, HostTimeData};

#[cfg(feature = "backend_aravis")]
use ci2_aravis as backend;
#[cfg(feature = "backend_flycap2")]
use ci2_flycap2 as backend;
#[cfg(feature = "backend_pyloncxx")]
use ci2_pyloncxx as backend;

#[cfg(feature = "backend_pyloncxx")]
pub fn print_backend_specific_data(extra: &dyn HostTimeData) {
    // Downcast to pylon specific type.
    let pylon_extra = extra
        .as_any()
        .downcast_ref::<backend::PylonExtra>()
        .unwrap();
    println!(
        "    device_timestamp: {}, block_id: {}",
        pylon_extra.device_timestamp, pylon_extra.block_id
    );
}

#[cfg(feature = "backend_flycap2")]
pub fn print_backend_specific_data(frame: &DynamicFrame) {
    // do nothing for now.
}

#[cfg(not(any(feature = "backend_pyloncxx", feature = "backend_flycap2")))]
pub fn print_backend_specific_data(_extra: &dyn HostTimeData) {
    // do nothing
}

async fn do_capture<C>(cam: &mut ci2_async::ThreadedAsyncCamera<C>) -> Result<(), ci2::Error>
where
    C: 'static + ci2::Camera + Send,
{
    let mut stream = cam.frames(10, || {})?.take(10);
    while let Some(frame) = stream.next().await {
        match frame {
            ci2_async::FrameResult::Frame(frame) => {
                println!(
                    "  got frame {}: {}x{}",
                    frame.extra().host_framenumber(),
                    frame.width(),
                    frame.height()
                );
                print_backend_specific_data(frame.extra());
            }
            ci2_async::FrameResult::SingleFrameError(e) => {
                println!("  got SingleFrameError: {:?}", e);
            }
        };
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let sync_mod = backend::new_module()?;
    let mut async_mod = ci2_async::into_threaded_async(sync_mod);
    let infos = async_mod.camera_infos()?;

    if infos.len() == 0 {
        anyhow::bail!("no cameras detected");
    }

    for info in infos.iter() {
        println!("opening camera {}", info.name());
        let mut cam = async_mod.threaded_async_camera(info.name())?;
        println!("got camera");
        cam.acquisition_start()?;
        let stream_future = do_capture(&mut cam);
        futures::executor::block_on(stream_future)?;
        cam.acquisition_stop()?;
        if let Some((control, join_handle)) = cam.control_and_join_handle() {
            control.stop();
            while !control.is_done() {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            join_handle.join().expect("joining camera thread");
        }
        break;
    }

    Ok(())
}
