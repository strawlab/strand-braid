use eyre::Result;

use strand_cam::cli_app::{CameraBackend, cli_main, requested_camera_backend};

const APP_NAME: &str = "strand-cam";

fn main() -> Result<()> {
    // Default to Pylon when `--camera-backend` is not supplied.
    let backend = requested_camera_backend()?.unwrap_or(CameraBackend::Pylon);

    // Only the selected backend's module is constructed, and neither backend
    // loads its vendor SDK until a camera is actually enumerated or opened. The
    // module is leaked to obtain the `'static` reference that `cli_main`
    // requires (the process exits immediately afterwards regardless).
    match backend {
        CameraBackend::Pylon => {
            let module: &'static ci2_pyloncxx::WrappedModule =
                Box::leak(Box::new(ci2_pyloncxx::new_module()?));
            let guard = ci2_pyloncxx::make_singleton_guard(&module)?;
            let mymod = ci2_async::into_threaded_async(module, &guard);
            cli_main(mymod, APP_NAME)?;
        }
        CameraBackend::Vimba => {
            let module: &'static ci2_vimba::WrappedModule =
                Box::leak(Box::new(ci2_vimba::new_module()?));
            let guard = ci2_vimba::make_singleton_guard(&module)?;
            let mymod = ci2_async::into_threaded_async(module, &guard);
            cli_main(mymod, APP_NAME)?;
        }
    }
    Ok(())
}
