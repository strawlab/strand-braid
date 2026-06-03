use eyre::Result;

const APP_NAME: &str = "strand-cam-gui";

fn main() -> Result<()> {
    // Supports both the Basler Pylon and Allied Vision Vimba backends, selected
    // at runtime via `--camera-backend` (defaulting to pylon).
    strand_cam::cli_app::cli_main_dispatch(APP_NAME)
}
