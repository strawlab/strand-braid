#[macro_use]
extern crate log;
extern crate env_logger;

extern crate imagefmt;

extern crate camiface;

use camiface::{CamIface, CamContext};
use std::process;
use std::sync::{Arc, Mutex};

use std::error::Error;

fn main() {
    env_logger::init().unwrap();

    let v = camiface::get_api_version().unwrap();
    println!("camiface version: {}", v);

    let module = Arc::new(Mutex::new(CamIface::new().unwrap()));
    let cam_iface = module.lock().unwrap();
    println!("driver: {}", cam_iface.get_driver_name().unwrap());

    let n_cams = cam_iface.get_num_cameras().unwrap();

    if n_cams < 1 {
        println!("no cameras found, will now exit");
        return;
    }
    println!("{} camera(s) found.", n_cams);

    let mut device_number = -1;
    for i in 0..n_cams {
        println!("  camera {}", i);
        let cam_info_result = cam_iface.get_camera_info(i);
        match cam_info_result {
            Ok(cam_info_struct) => {
                println!("   vendor: {}", cam_info_struct.vendor);
                println!("   model: {}", cam_info_struct.model);
                println!("   chip: {}", cam_info_struct.chip);
                device_number = i;
            }
            Err(err) => {
                println!("   error: {}", err.description());
            }
        }
    }

    if device_number == -1 {
        println!("no available cameras found, will now exit");
        process::exit(1);
    }


    println!("choosing camera {}", device_number);
    let num_modes = cam_iface.get_num_modes(device_number).unwrap();
    println!("{} mode(s) available:", num_modes);

    let mut mode_number = 0;

    for i in 0..num_modes {
        let mode_string = cam_iface.get_mode_string(device_number, i).unwrap();
        println!(" mode {}: {}", i, mode_string);
        if mode_string.contains("FORMAT7_0") {
            if mode_string.contains("MONO8") {
                mode_number = i;
            }
        }
    }

    println!("Choosing mode {}",
             cam_iface.get_mode_string(device_number, mode_number).unwrap());

    let mut cc = CamContext::new(module.clone(), device_number, 5, mode_number).unwrap();


    println!("ROI: {}", cc.get_frame_roi().unwrap());
    println!("allocated {} buffers", cc.get_num_framebuffers().unwrap());

    let num_props = cc.get_num_camera_properties().unwrap();
    println!("{} camera properties:", num_props);
    for i in 0..num_props {
        let prop_info = cc.get_camera_property_info(i).unwrap();
        println!(" {}", prop_info);
    }

    cc.start_camera().unwrap();

    for i in 0..10 {
        let capture_data = cc.get_capture_blocking(-1.0).unwrap();
        println!("{}", capture_data);
        let fname = format!("frame{:03}.png", i);
        imagefmt::write(fname,
                        capture_data.roi.width as usize,
                        capture_data.roi.height as usize,
                        imagefmt::ColFmt::Y,
                        &capture_data.image_data[..capture_data.image_data.len()],
                        imagefmt::ColType::Auto)
            .expect("failed writing image");
    }

}
