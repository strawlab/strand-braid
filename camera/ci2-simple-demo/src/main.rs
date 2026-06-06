// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use clap::Parser;

use ci2::{Camera, CameraModule};

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

fn run<M>(mut wrapped_mod: M) -> anyhow::Result<()>
where
    M: CameraModule,
{
    let infos = wrapped_mod.camera_infos()?;
    if infos.is_empty() {
        anyhow::bail!("no cameras detected");
    }
    if let Some(info) = infos.first() {
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
    }

    Ok(())
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let cli = Cli::parse();

    // Only the selected backend's module is constructed, and neither backend
    // loads its vendor SDK until a camera is enumerated or opened.
    match cli.camera_backend {
        CameraBackend::Pylon => {
            let module = ci2_pylon::new_module()?;
            let _guard = ci2_pylon::make_singleton_guard(&&module)?;
            run(&module)?;
        }
        CameraBackend::Vimba => {
            let module = ci2_vimba::new_module()?;
            let _guard = ci2_vimba::make_singleton_guard(&&module)?;
            run(&module)?;
        }
    }

    Ok(())
}
