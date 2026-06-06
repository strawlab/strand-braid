// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use clap::{Parser, Subcommand};
use tracing::{error, info};

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
struct Record {
    /// set the recording duration in number of frames. 0 means infinite.
    ///
    #[arg(short, long, default_value = "10")]
    num_frames: usize,

    /// specify the name of the camera to use
    #[arg(short, long)]
    camera_name: Option<String>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// record frames
    Record(Record),

    /// list cameras
    List,
}

/// camera utilities
#[derive(Debug, Parser)]
#[command(name = "ci2", author, version)]
struct Cli {
    /// Which camera backend library to load.
    #[arg(long, value_enum, default_value = "pylon", global = true)]
    camera_backend: CameraBackend,

    #[command(subcommand)]
    command: Command,
}

fn list<M: CameraModule>(mymod: M) -> ci2::Result<()> {
    let infos = mymod.camera_infos()?;
    for info in infos.iter() {
        println!("{}", info.name());
    }
    Ok(())
}

fn record<M: CameraModule>(mut mymod: M, recargs: Record) -> ci2::Result<()> {
    let name = if let Some(camera_name) = recargs.camera_name {
        camera_name
    } else {
        let infos = mymod.camera_infos()?;
        if infos.is_empty() {
            return Err("no cameras detected".into());
        }
        infos[0].name().to_string()
    };

    let mut cam = mymod.camera(&name)?;

    info!("got camera");
    cam.acquisition_start()?;
    let mut count = 0;
    loop {
        if recargs.num_frames != 0 && count >= recargs.num_frames {
            break;
        }
        count += 1;

        match cam.next_frame() {
            Ok(frame) => {
                info!("got frame: {}x{}", frame.width(), frame.height(),);
            }
            Err(ci2::Error::SingleFrameError(s)) => {
                error!("SingleFrameError({})", s);
            }
            Err(e) => {
                return Err(e);
            }
        }
    }
    cam.acquisition_stop()?;

    Ok(())
}

fn dispatch<M: CameraModule>(mymod: M, command: Command) -> ci2::Result<()> {
    match command {
        Command::Record(recargs) => record(mymod, recargs),
        Command::List => list(mymod),
    }
}

fn main() -> anyhow::Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("RUST_LOG", "ci2=info,warn") };
    }

    env_logger::init();
    let cli = Cli::parse();

    // Only the selected backend's module is constructed, and neither backend
    // loads its vendor SDK until a camera is enumerated or opened.
    match cli.camera_backend {
        CameraBackend::Pylon => {
            let module = ci2_pylon::new_module()?;
            let _guard = ci2_pylon::make_singleton_guard(&&module)?;
            dispatch(&module, cli.command)?;
        }
        CameraBackend::Vimba => {
            let module = ci2_vimba::new_module()?;
            let _guard = ci2_vimba::make_singleton_guard(&&module)?;
            dispatch(&module, cli.command)?;
        }
    }

    Ok(())
}
