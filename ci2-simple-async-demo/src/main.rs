use futures::stream::StreamExt;
use std::convert::TryInto;

use ci2::{Camera, CameraModule};
use ci2_async::AsyncCamera;

use ci2_pyloncxx as backend;

async fn do_capture<C, T>(cam: &mut ci2_async::ThreadedAsyncCamera<C, T>) -> Result<(), ci2::Error>
where
    C: 'static + ci2::Camera<FrameType = T> + Send,
    T: 'static + timestamped_frame::FrameTrait + Send + std::fmt::Debug,
    Vec<u8>: From<T>,
{
    let mut stream = cam.frames(10, || {})?;
    let mut previous: Option<(i64, std::time::Instant)> = None;
    let start = std::time::Instant::now();
    let mut count: usize = 0;
    println!("count,device_timestamp,host_timestamp,dur_device,dur_host,diff");
    while let Some(frame) = stream.next().await {
        match frame {
            ci2_async::FrameResult::Frame(frame) => {
                let frame_any = &frame as &dyn std::any::Any;

                let frame = frame_any.downcast_ref::<backend::Frame>().unwrap();
                let device_timestamp: i64 = frame.device_timestamp.try_into().unwrap();
                if let Some((previous_device_timestamp, previous_instant)) = previous {
                    let dur_host: i64 = previous_instant.elapsed().as_nanos().try_into().unwrap();
                    let dur_device = device_timestamp - previous_device_timestamp;
                    let diff = dur_device - dur_host;
                    let host_timestamp = start.elapsed().as_nanos();
                    println!(
                        "{},{},{},{},{},{}",
                        count, device_timestamp, host_timestamp, dur_device, dur_host, diff
                    );
                }
                previous = Some((device_timestamp, std::time::Instant::now()));
            }
            ci2_async::FrameResult::SingleFrameError(e) => {
                println!("  got FrameResult::SingleFrameError: {}", e);
            }
        };
        count += 1;
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
        let mut cam = async_mod.threaded_async_camera(info.name())?;
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
