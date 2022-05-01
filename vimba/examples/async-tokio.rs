use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};

use tokio::sync::mpsc::{channel, Receiver, Sender};

use lazy_static::lazy_static;

const N_BUFFER_FRAMES: usize = 3;
const N_CHANNEL_FRAMES: usize = 10;

lazy_static! {
    // Prevent multiple concurrent access to structures and functions in Vimba
    // which are not threadsafe.
    static ref VIMBA_MUTEX: Mutex<()> = Mutex::new(());
    static ref IS_DONE: AtomicBool = AtomicBool::new(false);
    static ref SENDER: Mutex<Option<Sender<Frame>>> = Mutex::new(None);
}

struct Frame {
    buffer: Vec<u8>,
    width: u32,
    height: u32,
    pixel_format: u32,
}

#[no_mangle]
pub unsafe extern "C" fn callback_c(
    camera_handle: vimba_sys::VmbHandle_t,
    frame: *mut vimba_sys::VmbFrame_t,
) {
    match std::panic::catch_unwind(|| {
        if !IS_DONE.load(Ordering::Relaxed) {
            let err = {
                let _guard = VIMBA_MUTEX.lock().unwrap();
                vimba_sys::VmbCaptureFrameQueue(camera_handle, frame, Some(callback_c))
            };

            if err != vimba_sys::VmbErrorType::VmbErrorSuccess {
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
                        match sender.blocking_send(msg) {
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

async fn handle_frames(mut rx: Receiver<Frame>) {
    while let Some(frame) = rx.recv().await {
        println!(
            "got frame {}x{}: {} bytes (pixel format: {})",
            frame.width,
            frame.height,
            frame.buffer.len(),
            frame.pixel_format,
        );
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let (tx, rx) = channel(N_CHANNEL_FRAMES);
    {
        let mut sender_ref = SENDER.lock().unwrap();
        *sender_ref = Some(tx);
    }
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

        let mut frames = Vec::with_capacity(N_BUFFER_FRAMES);
        for _ in 0..N_BUFFER_FRAMES {
            let buffer = camera.allocate_buffer()?;
            let mut frame = vimba::Frame::new(buffer);
            camera.frame_announce(&mut frame)?;
            frames.push(frame);
        }

        {
            let _guard = VIMBA_MUTEX.lock().unwrap();

            camera.capture_start()?;

            for mut frame in frames.iter_mut() {
                camera.capture_frame_queue_with_callback(&mut frame, Some(callback_c))?;
            }

            camera.command_run("AcquisitionStart")?;
        }

        println!("acquiring frames for 1 second");
        let frames_future = handle_frames(rx);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(1), frames_future).await;
        IS_DONE.store(true, Ordering::Relaxed); // indicate we are done
        println!("done acquiring frames");
        {
            let mut _guard = VIMBA_MUTEX.lock().unwrap();
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
