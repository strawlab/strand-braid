extern crate env_logger;

extern crate pylon;
#[macro_use]
extern crate failure;

use pylon::{Result, Pylon, HasProperties, HasNodeMap, GrabStatus, AccessMode};

struct BinInfo {
    hbin: i64,
    vbin: i64,
}

fn start_binning(camera: &mut pylon::Device) -> Result<Option<BinInfo>> {
    let hbin_mode = match camera.enumeration_value("BinningHorizontalMode") {
        Ok(m) => m,
        Err(_) => return Ok(None),
    };
    let hbin = camera.integer_value("BinningHorizontal")?;
    println!("BinningHorizontalMode {:?}", hbin_mode);
    println!("BinningHorizontal {:?}", hbin);

    let vbin_mode = camera.enumeration_value("BinningVerticalMode")?;
    let vbin = camera.integer_value("BinningVertical")?;
    println!("BinningVerticalMode {:?}", vbin_mode);
    println!("BinningVertical {:?}", vbin);
    Ok(Some(BinInfo {
        hbin,
        vbin,
    }))
}

fn restore_binning(camera: &mut pylon::Device, bin_info: BinInfo) -> Result<()> {
    camera.set_integer_value("BinningHorizontal", bin_info.hbin)?;
    camera.set_integer_value("BinningVertical", bin_info.vbin)?;
    Ok(())
}

fn run() -> std::result::Result<(), failure::Error> {
    env_logger::init();

    let module = Pylon::new()?;
    let version_string = pylon::version_string()?;
    println!("pylon version {:?}", version_string.to_str()?);

    let tl_factory = module.tl_factory()?;

    match tl_factory.create_gige_transport_layer() {
        Ok(tl) => {
            let node_list = tl.node_map()?.nodes()?;
            for node in node_list.iter() {
                if !node.name(true).starts_with("Cust::") {
                    println!("  transport layer node: {:?} {:?}", node, node.visibility());
                }
            }
        },
        Err(e) => {
            println!("could not create GigE transport layer: {:?}", e);
        },
    }

    let device_list = tl_factory.enumerate_devices()?;
    for di in device_list.iter() {
        println!("device: {:?}", di);
        for pn in di.property_names()?.iter() {
            let value = di.property_value(&pn)?;
            println!("  {}: {}", pn, value);
        }
        println!("");
    }

    if device_list.len() == 0 {
        bail!("No devices found.");
    }

    let device_info = &device_list[0];
    let mut camera = tl_factory.create_device( device_info )?;
    camera.open(vec![AccessMode::Control, AccessMode::Stream])?;

    println!("opened device {:?}", device_info);

    let formats = camera
        .get_enumeration_entries("PixelFormat")
        .expect("getting pixel format entries");
    for pixfmt in formats.iter() {
        println!("  possible pixel format: {}", pixfmt);
    }

    match camera.integer_value("DeviceSFNCVersionMajor") {
        Ok(major) => println!("sfnc major: {}", major),
        Err(e) => println!("sfnc v query failed: {:?}", e),
    }

    camera.set_enumeration_value("TriggerMode", "Off")?;
    println!("disabled external trigger");

    let node_list = camera.node_map()?.nodes()?;
    for node in node_list.iter() {
        println!("  device node: {:?} {:?}", node, node.visibility());
    }

    // let gain = camera.float_value("Gain")?;
    // println!("  gain {}", gain);

    let original_binning = start_binning(&mut camera)?;

    if original_binning.is_some() {
        camera.set_integer_value("BinningHorizontal", 2)?;
        camera.set_integer_value("BinningVertical", 2)?;
    }

    if camera.num_stream_grabber_channels()?==0 {
        bail!("no stream grabber channel");
    }

    let mut stream_grabber = camera.stream_grabber(0)?;

    let node_list = stream_grabber.node_map()?.nodes()?;
    for node in node_list.iter() {
        println!("  stream grabber node: {:?} {:?}", node, node.visibility());
    }

    match stream_grabber.set_boolean_value("EnableResend", false) {
        Ok(()) => {
            println!("disabled packet resend");
        }
        Err(pylon::Error::NameNotFound) => {
            println!("packet resend feature not present");
        }
        Err(e) => {
            return Err(e.into());
        }
    }

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

    let pixfmt = camera
        .enumeration_value("PixelFormat")
        .expect("getting pixel format");

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
                let block_id = match grab_result.block_id() {
                    Ok(block_id) => block_id as i64,
                    Err(_err) => -1,
                };
                println!("got frame {} {}x{} {:?}",
                    block_id, grab_result.size_x(), grab_result.size_y(),
                    grab_result.time_stamp());
                let data_view = grab_result.data_view();
                let w = grab_result.size_x();
                let h = grab_result.size_y();
                let wh = w*h;
                println!("data_view.len()={}, {}x{}={} pixfmt={}", data_view.len(), w, h, wh, pixfmt);
            },
            GrabStatus::Failed => {
                println!("frame not grabbed successfully. Error code {}", grab_result.error_code());
            },
            r => {
                bail!("unmatched grab result {:?}", r);
            }
        }

        // stream_grabber.queue_buffer(grab_result.handle())?; // pass ownership into stream grabber

        if n_grabs >= 10 {
            break;
        }
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

    if let Some(bin_info) = original_binning {
        restore_binning(&mut camera,bin_info)?;
    }

    camera.close()?;

    Ok(())
}

fn main() {
    run().unwrap();
}
