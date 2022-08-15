use std::sync::atomic::{AtomicBool, Ordering};

use lazy_static::lazy_static;

const N_BUFFER_FRAMES: usize = 3;

lazy_static! {
    // Prevent multiple concurrent access to structures and functions in Vimba
    // which are not threadsafe.
    static ref VIMBA: vimba::VimbaLibrary = vimba::VimbaLibrary::new().unwrap();
    static ref IS_DONE: AtomicBool = AtomicBool::new(false);
}

#[no_mangle]
pub unsafe extern "C" fn callback_c(
    camera_handle: vimba_sys::VmbHandle_t,
    frame: *mut vimba_sys::VmbFrame_t,
) {
    match std::panic::catch_unwind(|| {
        println!("got frame {}", (*frame).frameID);
        if !IS_DONE.load(Ordering::Relaxed) {
            let err = {
                VIMBA
                    .vimba_lib
                    .VmbCaptureFrameQueue(camera_handle, frame, Some(callback_c))
            };

            if err != vimba_sys::VmbErrorType::VmbErrorSuccess {
                println!("CB: err: {}", err);
            }
        }
    }) {
        Ok(_) => {}
        Err(_) => {
            println!("CB: ERROR: ignoring panic");
            IS_DONE.store(true, Ordering::Relaxed); // indicate we are done
        }
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let lib = vimba::VimbaLibrary::new()?;
    let version_info = vimba::VersionInfo::new(&lib.vimba_lib)?;
    println!(
        "Vimba API Version {}.{}.{}",
        version_info.major, version_info.minor, version_info.patch
    );

    let n_cams = lib.n_cameras()?;
    println!("{} cameras found", n_cams);
    let camera_infos = lib.camera_info(n_cams)?;
    if !camera_infos.is_empty() {
        let cam_id = camera_infos[0].camera_id_string.as_str();
        println!("Opening camera {}", cam_id);
        println!("  {:?}", camera_infos[0]);

        let camera = vimba::Camera::open(cam_id, vimba::access_mode::FULL, &VIMBA.vimba_lib)?;
        let pixel_format = camera.pixel_format()?;
        println!("  pixel_format: {:?}", pixel_format);

        let mut frames = Vec::with_capacity(N_BUFFER_FRAMES);
        for _ in 0..N_BUFFER_FRAMES {
            let buffer = camera.allocate_buffer()?;
            let mut frame = vimba::Frame::new(buffer);
            camera.frame_announce(&mut frame)?;
            frames.push(frame);
        }

        {
            camera.capture_start()?;

            for mut frame in frames.iter_mut() {
                camera.capture_frame_queue_with_callback(&mut frame, Some(callback_c))?;
            }

            camera.command_run("AcquisitionStart")?;
        }
        println!("acquiring frames for 1 second");
        std::thread::sleep(std::time::Duration::from_secs(1));
        println!("done acquiring frames");
        {
            IS_DONE.store(true, Ordering::Relaxed); // indicate we are done
            camera.command_run("AcquisitionStop")?;
            camera.capture_end()?;
            camera.capture_queue_flush()?;
            for mut frame in frames.into_iter() {
                camera.frame_revoke(&mut frame)?;
            }
        }
        camera.close()?;
    }
    // When `lib` is dropped, `VmbShutdown` will automatically be called.
    Ok(())
}
