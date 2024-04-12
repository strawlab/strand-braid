fn main() -> anyhow::Result<()> {
    env_logger::init();
    let lib = vimba::VimbaLibrary::new()?;
    let n_cams = lib.n_cameras()?;
    let camera_infos = lib.camera_info(n_cams)?;
    if !camera_infos.is_empty() {
        let cam_id = camera_infos[0].camera_id_string.as_str();
        println!("Opening camera {}", cam_id);
        let camera = vimba::Camera::open(cam_id, vimba::access_mode::FULL, &lib.vimba_lib)?;
        // Settings to save the settings. Let's get meta.
        let settings_settings = vmbc_sys::VmbFeaturePersistSettings_t {
            persistType: vmbc_sys::VmbFeaturePersistType::VmbFeaturePersistNoLUT,
            ..vimba::default_feature_persist_settings()
        };
        let settings_path = format!("{}.xml", cam_id);
        println!("  saving settings to: {}", settings_path);
        camera.camera_settings_save(settings_path, &settings_settings)?;
    } else {
        println!("No camera, nothing to do.");
    }
    Ok(())
}
