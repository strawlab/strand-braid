use machine_vision_formats as formats;

use basic_frame::BasicExtra;
use std::path::PathBuf;
use structopt::StructOpt;

use channellib::unbounded;
use crossbeam_ok::CrossbeamOk;

use formats::pixel_format::RGB8;

#[derive(Debug, StructOpt)]
#[structopt(name = "fly-eye-cli", about = "run fly eye on image file")]
struct Opt {
    /// Filename of input image
    #[structopt(parse(from_os_str), name = "INPUT")]
    input: PathBuf,
}

fn fly_eye_cli(input_image: PathBuf) -> Result<(), failure::Error> {
    let piston_image = image::open(&input_image)?;

    let (firehose_tx, firehose_rx) = unbounded();

    let frame = convert_image::piston_to_frame(piston_image)?;
    let extra = Box::new(BasicExtra {
        host_timestamp: chrono::Utc::now(),
        host_framenumber: 0,
    });
    let frame: basic_frame::BasicFrame<RGB8> = basic_frame::BasicFrame {
        width: frame.width,
        height: frame.height,
        stride: frame.stride,
        image_data: frame.image_data,
        pixel_format: std::marker::PhantomData,
        extra,
    };
    let dynframe = basic_frame::DynamicFrame::from(frame);
    firehose_tx.send(dynframe).cb_ok();

    fly_eye::mainloop(firehose_rx)?;

    Ok(())
}

fn main() -> Result<(), failure::Error> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "fly_eye=info,error");
    }

    env_logger::init();
    let opt = Opt::from_args();

    fly_eye_cli(opt.input)
}
