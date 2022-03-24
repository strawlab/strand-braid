fn main() -> anyhow::Result<()> {
    env_logger::init();
    let lib = vimba::VimbaLibrary::new()?;
    let n_cams = lib.n_cameras()?;
    let camera_infos = lib.camera_info(n_cams)?;
    if !camera_infos.is_empty() {
        let cam_id = camera_infos[0].camera_id_string.as_str();
        println!("Opening camera {}", cam_id);
        let camera = vimba::Camera::open(cam_id, vimba::access_mode::FULL)?;
        let mut settings_settings = vimba::FeaturePersistentSettings::default(); // let's get meta. settings to save the settings.
        let settings_path = format!("{}.xml", cam_id);
        println!("  saving settings to: {}", settings_path);
        camera.camera_settings_save(settings_path, &mut settings_settings)?;
    }
    Ok(())
}
