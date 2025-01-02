use machine_vision_formats as formats;

use clap::Parser;
use std::path::PathBuf;

use formats::{pixel_format::RGB8, ImageData, Stride};

/// run fly eye on image file
#[derive(Debug, Parser)]
#[command(name = "fly-eye-cli", version)]
struct Opt {
    /// Filename of input image
    input: PathBuf,
}

fn fly_eye_cli(input_image: PathBuf) -> anyhow::Result<()> {
    let piston_image = image::open(&input_image)?;

    let (firehose_tx, firehose_rx) = std::sync::mpsc::channel();

    let frame = convert_image::image_to_rgb8(piston_image)?;
    let frame: basic_frame::BasicFrame<RGB8> = basic_frame::BasicFrame {
        width: frame.width(),
        height: frame.height(),
        stride: frame.stride().try_into().unwrap(),
        image_data: frame.buffer().data,
        pixel_format: std::marker::PhantomData,
    };
    let dynframe = basic_frame::DynamicFrame::from(frame);
    firehose_tx
        .send(dynframe)
        .map_err(|_| anyhow::anyhow!("receiver disconnected"))?;

    fly_eye::mainloop(firehose_rx)?;

    Ok(())
}

fn main() -> anyhow::Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "fly_eye=info,warn");
    }

    env_logger::init();
    let opt = Opt::parse();

    fly_eye_cli(opt.input)
}
