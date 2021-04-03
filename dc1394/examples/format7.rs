use dc1394::{Camera, Frame, DC1394};
use libdc1394_sys as ffi;
use std::fs::File;
use std::io::Write;

fn write_pgm_image(im: &Frame, fname: &str) -> Result<(), std::io::Error> {
    // TODO check format and save as color if needed.
    let data = im.data_view();
    let mut f = File::create(fname)?;

    let line1 = format!("P5\n{} {} 255\n", im.cols(), im.rows());
    f.write(line1.as_bytes())?;

    for row in 0..im.rows() {
        let start_idx = (row * im.stride()) as usize;
        let stop_idx = start_idx + (im.cols() * im.data_depth() / 8) as usize;
        let buf = &data[start_idx..stop_idx];
        f.write(buf)?;
    }
    Ok(())
}

fn run() -> dc1394::Result<()> {
    env_logger::init();

    let dc1394 = DC1394::new()?;
    let list = dc1394.get_camera_list()?;
    let list = list.as_slice();
    println!("{} cameras found", list.len());
    for cam_id in list.iter() {
        println!("  {:?}", cam_id);
        let mut cam = Camera::new(&dc1394, &cam_id.guid)?;

        // set format7 mode 0
        cam.set_video_mode(ffi::dc1394video_mode_t::DC1394_VIDEO_MODE_FORMAT7_0)?;
        let (color_coding, color_filter) = {
            let x = cam.format7_modeset()?;
            if x.mode[0].present == ffi::dc1394bool_t::DC1394_TRUE {
                (x.mode[0].color_coding, x.mode[0].color_filter)
            } else {
                panic!("format7 mode is not present for mode[0]");
            }
        };
        println!("      color coding: {:?}", color_coding);
        println!("      color filter: {:?}", color_filter);

        if cam.transmission() == ffi::dc1394switch_t::DC1394_ON {
            cam.set_transmission(ffi::dc1394switch_t::DC1394_OFF)?;
        }

        let num_buffers = 20;
        cam.capture_setup(num_buffers)?;
        cam.set_transmission(ffi::dc1394switch_t::DC1394_ON)?;
        println!("      started capture");

        let dequeue_policy = ffi::dc1394capture_policy_t::DC1394_CAPTURE_POLICY_WAIT;
        for i in 0..15 {
            let frame = cam.capture_dequeue(&dequeue_policy)?;
            let filename = format!("image{:02}.pgm", i);
            write_pgm_image(&frame, &filename).expect("writing image");
        }

        cam.set_transmission(ffi::dc1394switch_t::DC1394_OFF)?;
        cam.capture_stop()?;
    }
    Ok(())
}

fn main() {
    run().unwrap();
}
