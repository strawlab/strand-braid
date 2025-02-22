use futures::stream::StreamExt;

use ci2::{BackendData, Camera, CameraModule};
use ci2_async::AsyncCamera;

#[cfg(feature = "backend_pyloncxx")]
use ci2_pyloncxx as backend;
#[cfg(feature = "backend_vimba")]
use ci2_vimba as backend;

lazy_static::lazy_static! {
    static ref CAMLIB: backend::WrappedModule = backend::new_module().unwrap();
}

#[cfg(feature = "backend_pyloncxx")]
pub fn print_backend_specific_data(backend_data: &dyn BackendData) {
    let pylon_extra = backend_data
        .as_any()
        .downcast_ref::<ci2_pylon_types::PylonExtra>()
        .unwrap();
    println!(
        "    device_timestamp: {}, block_id: {}",
        pylon_extra.device_timestamp, pylon_extra.block_id
    );
}

#[cfg(feature = "backend_vimba")]
pub fn print_backend_specific_data(backend_data: &dyn BackendData) {
    // Downcast to vimba specific type.
    let vimba_extra = backend_data
        .as_any()
        .downcast_ref::<ci2_vimba_types::VimbaExtra>()
        .unwrap();
    println!(
        "    device_timestamp: {}, frame_id: {}",
        vimba_extra.device_timestamp, vimba_extra.frame_id
    );
}

#[cfg(not(any(feature = "backend_pyloncxx", feature = "backend_vimba")))]
pub fn print_backend_specific_data(_: &dyn BackendData) {
    // do nothing
}

async fn do_capture<C>(cam: &mut ci2_async::ThreadedAsyncCamera<C>) -> Result<(), ci2::Error>
where
    C: 'static + ci2::Camera + Send,
{
    let mut stream = cam.frames(10)?.take(10);
    while let Some(frame) = stream.next().await {
        match frame {
            ci2_async::FrameResult::Frame(frame) => {
                println!(
                    "  got frame {}: {}x{}",
                    frame.host_timing.fno,
                    frame.width(),
                    frame.height()
                );
                if let Some(bd) = &frame.backend_data {
                    print_backend_specific_data(bd.as_ref());
                }
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

    let guard = backend::make_singleton_guard(&&*CAMLIB)?;
    let mut async_mod = ci2_async::into_threaded_async(&*CAMLIB, &guard);
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
