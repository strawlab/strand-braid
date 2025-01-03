use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};

use lazy_static::lazy_static;
use std::sync::mpsc::SyncSender;

const N_BUFFER_FRAMES: usize = 3;
const N_CHANNEL_FRAMES: usize = 10;

lazy_static! {
    static ref VIMBA: vimba::VimbaLibrary = vimba::VimbaLibrary::new().unwrap();
    static ref IS_DONE: AtomicBool = AtomicBool::new(false);
    static ref SENDER: Mutex<Option<SyncSender<Frame>>> = Mutex::new(None);
}

struct Frame {
    buffer: Vec<u8>,
    width: u32,
    height: u32,
    pixel_format: u32,
}

#[no_mangle]
pub unsafe extern "C" fn callback_c(
    camera_handle: vmbc_sys::VmbHandle_t,
    _stream_handle: vmbc_sys::VmbHandle_t,
    frame: *mut vmbc_sys::VmbFrame_t,
) {
    match std::panic::catch_unwind(|| {
        if !IS_DONE.load(Ordering::Relaxed) {
            let err = VIMBA
                .vimba_lib
                .VmbCaptureFrameQueue(camera_handle, frame, Some(callback_c));

            if err != vmbc_sys::VmbErrorType::VmbErrorSuccess {
                eprintln!("CB: capture error: {}", err);
            } else {
                // no error

                let buf_ref1 = (*frame).buffer;
                let buf_len = (*frame).bufferSize as usize;

                let buf_ref = std::slice::from_raw_parts(buf_ref1 as *const u8, buf_len);
                let buffer = buf_ref.to_vec(); // makes copy

                let msg = Frame {
                    buffer,
                    width: (*frame).width,
                    height: (*frame).height,
                    pixel_format: (*frame).pixelFormat,
                };

                {
                    // In this scope, we keep the lock on the SENDER mutex.
                    let opt_sender = &mut *SENDER.lock().unwrap();
                    if let Some(sender) = opt_sender {
                        // We could clone the sender here and release the lock,
                        // but since this loop will be the only thing acquiring
                        // the lock, there's no point in doing so.
                        match sender.send(msg) {
                            Ok(()) => {}
                            Err(e) => {
                                eprintln!("CB: send frame error: {}", e);
                                IS_DONE.store(true, Ordering::Relaxed); // indicate we are done
                            }
                        }
                    }
                };
            }
        }
    }) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("CB: Error: Panic {:?}", e);
            IS_DONE.store(true, Ordering::Relaxed); // indicate we are done
        }
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let version_info = vimba::VersionInfo::new(&VIMBA.vimba_lib)?;
    println!(
        "Vimba X API Version {}.{}.{}",
        version_info.major, version_info.minor, version_info.patch
    );

    let (tx, rx) = std::sync::mpsc::sync_channel(N_CHANNEL_FRAMES);
    {
        let mut sender_ref = SENDER.lock().unwrap();
        *sender_ref = Some(tx);
    }
    let n_cams = VIMBA.n_cameras()?;
    println!("{} cameras found", n_cams);
    let camera_infos = VIMBA.camera_info(n_cams)?;
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

        camera.capture_start()?;

        for mut frame in frames.iter_mut() {
            camera.capture_frame_queue_with_callback(&mut frame, Some(callback_c))?;
        }

        camera.command_run("AcquisitionStart")?;

        println!("acquiring frames for 10 seconds");

        let start = std::time::Instant::now();

        while start.elapsed() < std::time::Duration::from_secs(10) {
            let frame = rx.recv()?;
            println!(
                "got frame {}x{}: {} bytes (pixel format: {})",
                frame.width,
                frame.height,
                frame.buffer.len(),
                frame.pixel_format,
            );
        }

        IS_DONE.store(true, Ordering::Relaxed); // indicate we are done

        println!("done acquiring frames");
        camera.command_run("AcquisitionStop")?;
        camera.capture_end()?;
        camera.capture_queue_flush()?;
        for mut frame in frames.into_iter() {
            camera.frame_revoke(&mut frame)?;
        }
        camera.close()?;
    }
    unsafe { VIMBA.shutdown() };
    Ok(())
}
