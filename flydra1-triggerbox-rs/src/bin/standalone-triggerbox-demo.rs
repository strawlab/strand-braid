#[macro_use]
extern crate log;

use flydra1_triggerbox::{launch_background_thread, make_trig_fps_cmd, Cmd};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "standalone-triggerbox-demo")]
struct Opt {
    /// Filename of device
    #[structopt(parse(from_os_str), long = "device", default_value = "/dev/trig1")]
    device: std::path::PathBuf,
    /// Framerate
    #[structopt(long = "fps", default_value = "100")]
    fps: f64,
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    info!("flydra1_triggerbox starting");
    let opt = Opt::from_args();

    let (tx, rx) = crossbeam_channel::unbounded();

    tx.send(Cmd::StopPulsesAndReset)?;
    tx.send(make_trig_fps_cmd(opt.fps))?;
    tx.send(Cmd::StartPulses)?;

    let cb = Box::new(|tm| {
        println!("got new time model: {:?}", tm);
    });

    let query_dt = std::time::Duration::from_secs(1);

    let (control, _handle) = launch_background_thread(cb, opt.device, rx, None, query_dt)?;

    while !control.is_done() {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    Ok(())
}
