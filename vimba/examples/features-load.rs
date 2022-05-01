fn main() -> anyhow::Result<()> {
    env_logger::init();
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 2 {
        anyhow::bail!("Usage: features-load <SETTINGS_PATH>");
    }
    let settings_path = &args[1];
    let lib = vimba::VimbaLibrary::new()?;
    let n_cams = lib.n_cameras()?;
    if n_cams != 1 {
        anyhow::bail!("will only run if exactly one camera is detected");
    }
    let camera_infos = lib.camera_info(n_cams)?;
    if !camera_infos.is_empty() {
        let cam_id = camera_infos[0].camera_id_string.as_str();
        println!("Opening camera {}", cam_id);
        let camera = vimba::Camera::open(cam_id, vimba::access_mode::FULL)?;
        let mut settings_settings = vimba::FeaturePersistentSettings::default(); // let's get meta. settings to load the settings.
        println!(
            "  loading settings from: {}",
            settings_path.to_string_lossy()
        );
        camera.camera_settings_load(settings_path, &mut settings_settings)?;
    }
    Ok(())
}
