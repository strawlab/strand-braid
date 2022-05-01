fn main() -> anyhow::Result<()> {
    env_logger::init();
    let version_info = vimba::VersionInfo::new()?;
    println!(
        "Vimba API Version {}.{}.{}",
        version_info.major, version_info.minor, version_info.patch
    );
    let lib = vimba::VimbaLibrary::new()?;
    let n_cams = lib.n_cameras()?;
    println!("{} cameras found", n_cams);
    let camera_infos = lib.camera_info(n_cams)?;
    if !camera_infos.is_empty() {
        let cam_id = camera_infos[0].camera_id_string.as_str();
        println!("Opening camera {}", cam_id);
        println!("  {:?}", camera_infos[0]);

        let camera = vimba::Camera::open(cam_id, vimba::access_mode::FULL)?;
        let pixel_format = camera.pixel_format()?;
        println!("  pixel_format: {:?}", pixel_format);

        let buffer = camera.allocate_buffer()?;
        let mut frame = vimba::Frame::new(buffer);

        camera.frame_announce(&mut frame)?;
        camera.capture_start()?;
        camera.capture_frame_queue(&mut frame)?;
        camera.command_run("AcquisitionStart")?;

        for i in 0..1000 {
            let timeout = 2000;
            camera.capture_frame_wait(&mut frame, timeout)?;

            if frame.is_complete() {
                println!(
                    "  captured frame {}: {}x{} ({} bytes)",
                    i,
                    frame.width(),
                    frame.height(),
                    frame.image_size()
                );
            } else {
                println!("  capture finished, but no completed frame.");
            }
        }
        camera.command_run("AcquisitionStop")?;
        camera.capture_end()?;
        camera.frame_revoke(&mut frame)?;
        camera.close()?;
    }
    // When `lib` is dropped, `VmbShutdown` will automatically be called.
    Ok(())
}
