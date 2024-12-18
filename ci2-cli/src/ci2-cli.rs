#[cfg(feature = "backend_pyloncxx")]
extern crate ci2_pyloncxx as backend;

use clap::Parser;
use tracing::{error, info};

use ci2::{Camera, CameraModule};

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

/// camera utilities
#[derive(Debug, Parser)]
#[command(name = "ci2", author, version)]
enum Command {
    /// record frames
    #[structopt(name = "record")]
    Record(Record),

    /// list cameras
    #[structopt(name = "list")]
    List,
}

fn list(mymod: &backend::WrappedModule) -> ci2::Result<()> {
    let infos = mymod.camera_infos()?;
    for info in infos.iter() {
        println!("{}", info.name());
    }
    Ok(())
}

fn record(mut mymod: &backend::WrappedModule, recargs: Record) -> ci2::Result<()> {
    let name = if let Some(camera_name) = recargs.camera_name {
        camera_name
    } else {
        let infos = mymod.camera_infos()?;
        if infos.len() == 0 {
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

fn main() -> anyhow::Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "ci2=info,warn");
    }

    env_logger::init();
    let opt = Command::parse();

    let mymod = backend::new_module()?;

    match opt {
        Command::Record(recargs) => record(&mymod, recargs)?,
        Command::List => list(&mymod)?,
    };

    Ok(())
}
