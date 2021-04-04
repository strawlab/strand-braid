use futures::stream::StreamExt;

use ci2::{CameraModule, Camera};
use ci2_async::AsyncCamera;

#[cfg(feature = "backend_aravis")]
use ci2_aravis as backend;
#[cfg(feature = "backend_dc1394")]
use ci2_dc1394 as backend;
#[cfg(feature = "backend_flycap2")]
use ci2_flycap2 as backend;
#[cfg(feature = "backend_pyloncxx")]
use ci2_pyloncxx as backend;

use machine_vision_formats as formats;

#[cfg(feature = "backend_pyloncxx")]
pub fn print_backend_specific_metadata<F: formats::ImageStride + std::any::Any>(frame: &F) {
    let frame_any = frame as &dyn std::any::Any;

    let frame = frame_any.downcast_ref::<backend::Frame>().unwrap();
    println!("    Pylon device_timestamp: {}, block_id: {}", frame.device_timestamp, frame.block_id);
}

#[cfg(feature = "backend_flycap2")]
pub fn print_backend_specific_metadata<F: formats::ImageStride + std::any::Any>(frame: &F) {
    let frame_any = frame as &dyn std::any::Any;

    let frame = frame_any.downcast_ref::<ci2_flycap2::Frame>().unwrap();
    println!("    flycap2 device_timestamp: {:?}", frame.device_timestamp());
}

#[cfg(not(any(feature = "backend_pyloncxx", feature = "backend_flycap2")))]
pub fn print_backend_specific_metadata<F: formats::ImageStride>(_frame: &F) {
    // do nothing
}

async fn do_capture<C,T>(cam: &mut ci2_async::ThreadedAsyncCamera<C,T>) -> Result<(), ci2::Error>
    where
        C: 'static + ci2::Camera<FrameType=T> + Send,
        T: 'static + timestamped_frame::FrameTrait + Send + std::fmt::Debug,
        Vec<u8>: From<T>,
{
    let mut stream = cam.frames(10, || {})?.take(10);
    while let Some(frame) = stream.next().await {
        match frame {
            ci2_async::FrameResult::Frame(frame) => {
                println!("  got frame {}: {}x{}", frame.host_framenumber(), frame.width(), frame.height());
                print_backend_specific_metadata(&frame);
            }
            m => {
                println!("  got FrameResult: {:?}", m);
            }
        };
    }
    Ok(())
}

fn main() -> ci2::Result<()> {
    env_logger::init();

    let sync_mod = backend::new_module()?;
    let mut async_mod = ci2_async::into_threaded_async(sync_mod);
    let infos = async_mod.camera_infos()?;

    if infos.len() == 0 {
        return Err("no cameras detected".into());
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
