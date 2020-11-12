extern crate env_logger;
extern crate crossbeam_channel;
extern crate failure;
extern crate fly_eye;
extern crate structopt;
extern crate image;
extern crate convert_image;
extern crate machine_vision_formats as formats;
extern crate crossbeam_ok;

use std::path::PathBuf;
use structopt::StructOpt;

use crossbeam_channel::unbounded;
use fly_eye::App;
use crossbeam_ok::CrossbeamOk;

#[derive(Debug, StructOpt)]
#[structopt(name = "fly-eye-cli", about = "run fly eye on image file")]
struct Opt {
    /// Filename of input image
    #[structopt(parse(from_os_str), name="INPUT")]
    input: PathBuf,
}

fn fly_eye_cli(input_image: PathBuf) -> Result<(),failure::Error> {
    let piston_image = image::open(&input_image)?;

    let (firehose_tx, firehose_rx) = unbounded();

    let frame = convert_image::piston_to_frame(piston_image)?;
    firehose_tx.send(frame.into()).cb_ok();

    let mut app = App { rx: firehose_rx };
    app.mainloop()?;

    Ok(())
}

fn main() -> Result<(),failure::Error> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "fly_eye=info,error");
    }

    env_logger::init();
    let opt = Opt::from_args();

    fly_eye_cli(opt.input)
}
