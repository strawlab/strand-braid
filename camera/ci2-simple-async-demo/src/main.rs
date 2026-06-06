// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use clap::Parser;
use futures::stream::StreamExt;

use ci2::{BackendData, Camera, CameraModule};
use ci2_async::AsyncCamera;

/// Which camera vendor backend to load at runtime.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum CameraBackend {
    /// Basler Pylon backend (`ci2-pylon`).
    Pylon,
    /// Allied Vision Vimba backend (`ci2-vimba`).
    Vimba,
}

#[derive(Debug, Parser)]
#[command(author, version)]
struct Cli {
    /// Which camera backend library to load.
    #[arg(long, value_enum, default_value = "pylon")]
    camera_backend: CameraBackend,
}

fn print_backend_specific_data(backend_data: &dyn BackendData) {
    let any = backend_data.as_any();
    if let Some(pylon_extra) = any.downcast_ref::<ci2_pylon_types::PylonExtra>() {
        println!(
            "    device_timestamp: {}, block_id: {}",
            pylon_extra.device_timestamp, pylon_extra.block_id
        );
    } else if let Some(vimba_extra) = any.downcast_ref::<ci2_vimba_types::VimbaExtra>() {
        println!(
            "    device_timestamp: {}, frame_id: {}",
            vimba_extra.device_timestamp, vimba_extra.frame_id
        );
    }
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

fn run<M, C, G>(mut async_mod: ci2_async::ThreadedAsyncCameraModule<M, C, G>) -> anyhow::Result<()>
where
    M: ci2::CameraModule<CameraType = C, Guard = G> + 'static,
    C: 'static + ci2::Camera + Send,
    G: Send + 'static,
{
    let infos = async_mod.camera_infos()?;

    if infos.is_empty() {
        anyhow::bail!("no cameras detected");
    }

    if let Some(info) = infos.first() {
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
    }

    Ok(())
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let cli = Cli::parse();

    // Only the selected backend's module is constructed, and neither backend
    // loads its vendor SDK until a camera is enumerated or opened. The module is
    // leaked to obtain the `'static` reference the threaded async module
    // requires (the process exits immediately afterwards regardless).
    match cli.camera_backend {
        CameraBackend::Pylon => {
            let module: &'static ci2_pylon::WrappedModule =
                Box::leak(Box::new(ci2_pylon::new_module()?));
            let guard = ci2_pylon::make_singleton_guard(&module)?;
            run(ci2_async::into_threaded_async(module, &guard))?;
        }
        CameraBackend::Vimba => {
            let module: &'static ci2_vimba::WrappedModule =
                Box::leak(Box::new(ci2_vimba::new_module()?));
            let guard = ci2_vimba::make_singleton_guard(&module)?;
            run(ci2_async::into_threaded_async(module, &guard))?;
        }
    }

    Ok(())
}
