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
        let camera = vimba::Camera::open(cam_id, vimba::access_mode::FULL, &lib.vimba_lib)?;
        // Settings to load the settings. Let's get meta.
        let settings_settings = vmbc_sys::VmbFeaturePersistSettings_t {
            persistType: vmbc_sys::VmbFeaturePersistType::VmbFeaturePersistNoLUT,
            ..vimba::default_feature_persist_settings()
        };
        println!(
            "  loading settings from: {}",
            settings_path.to_string_lossy()
        );
        camera.camera_settings_load(settings_path, &settings_settings)?;
    }
    Ok(())
}
