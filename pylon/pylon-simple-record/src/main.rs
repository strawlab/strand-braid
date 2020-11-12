#[macro_use]
extern crate error_chain;
extern crate env_logger;
#[macro_use]
extern crate structopt;
extern crate pylon;

use structopt::StructOpt;
use pylon::{Result, Pylon, HasProperties, HasNodeMap, GrabStatus, AccessMode};

#[derive(StructOpt, Debug)]
#[structopt(name = "pylon-simple-record")]
struct Opt {
    /// Camera file
    #[structopt(short = "c", long = "camera")]
    camera_name: Option<String>,
}

fn to_name(info: &pylon::DeviceInfo) -> String {
    // &info.property_value("FullName").unwrap()
    let serial = &info.property_value("SerialNumber").unwrap();
    let vendor = &info.property_value("VendorName").unwrap();
    format!("{}-{}", vendor, serial)
}

fn run() -> Result<()> {
    env_logger::init();

    let opt = Opt::from_args();
    println!("{:?}", opt);


    let module = Pylon::new()?;
    let version_string = pylon::version_string()?;
    println!("pylon version {:?}", version_string.to_str()?);

    let tl_factory = module.tl_factory()?;

    let device_list = tl_factory.enumerate_devices()?;

    let mut info = None;
    for di in device_list.iter() {
        println!("found device: {:?}", to_name(di));
        if let Some(ref desired_name) = opt.camera_name {
            if desired_name == &to_name(di) {

                info = Some(di);
            }
        }
    }

    if device_list.len() == 0 {
        bail!("No devices found.");
    }

    let device_info = match info {
        None => &device_list[0],
        Some(di) => di,
    };

    println!("choosing {}", to_name(device_info));

    let mut camera = tl_factory.create_device( device_info )?;
    camera.open(vec![AccessMode::Control, AccessMode::Stream])?;

    println!("opened device {:?}", device_info);

    camera.set_enumeration_value("TriggerMode", "Off")?;
    println!("disabled external trigger");

    if camera.num_stream_grabber_channels()?==0 {
        bail!("no stream grabber channel");
    }

    let mut stream_grabber = camera.stream_grabber(0)?;
    stream_grabber.open()?;

    let payload_size = camera.integer_value("PayloadSize")?;
    stream_grabber.set_integer_value("MaxBufferSize",payload_size)?;

    // Get a handle for the stream grabber's wait object. The wait object
    // allows waiting for buffers to be filled with grabbed data.
    let wait_object = stream_grabber.get_wait_object()?;

    let num_buffers = 10;
    stream_grabber.set_integer_value("MaxNumBuffer",num_buffers)?;

    stream_grabber.prepare_grab()?;

    let mut buf_handles = Vec::with_capacity(num_buffers as usize);
    for _ in 0..num_buffers {
        let buf = pylon::Buffer::new(vec![0; payload_size as usize]);
        let handle = stream_grabber.register_buffer(buf)?; // push the buffer in, get a handle out
        buf_handles.push(handle);
    }

    for handle in buf_handles.into_iter() {
        stream_grabber.queue_buffer(handle)?; // pass ownership into stream grabber
    }

    camera.execute_command("AcquisitionStart")?;

    let mut n_grabs = 0;

    loop {
        let is_ready = wait_object.wait(1000)?;
        if !is_ready {
            bail!("Grab timeout occurred. (Hint: if using GigE camera, is your network set for jumbo frames?)");
        }

        let grab_result_opt = stream_grabber.retrieve_result()?;
        let grab_result = match grab_result_opt {
            Some(gr) => gr,
            None => bail!("failed to retrieve a grab result"),
        };

        n_grabs += 1;

        match grab_result.status() {
            GrabStatus::Grabbed => {
                print!(".");
                use std::io::Write;
                std::io::stdout().flush().ok().expect("Could not flush stdout");
            },
            GrabStatus::Failed => {
                println!("frame not grabbed successfully. Error code {}", grab_result.error_code());
            },
            r => {
                bail!("unmatched grab result {:?}", r);
            }
        }

        stream_grabber.queue_buffer(grab_result.handle())?; // pass ownership into stream grabber
    }

    camera.execute_command("AcquisitionStop")?;
    stream_grabber.cancel_grab()?;

    loop {
        let grab_result_opt = stream_grabber.retrieve_result()?;
        if grab_result_opt.is_none() {
            break;
        }
    }
    for _ in 0..num_buffers {
        let _handle = stream_grabber.pop_buffer().unwrap();
    }
    stream_grabber.finish_grab()?;
    stream_grabber.close()?;

    camera.close()?;

    Ok(())
}

quick_main!(run);
