use anyhow::Result;

pub fn braid_start(_name: &str) -> Result<()> {
    dotenv::dotenv().ok();

    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "braid=info,flydra2=info,braid_run=info,strand_cam=info,flydra_feature_detector=info,rt_image_viewer=info,flydra1_triggerbox=info,warn");
    }
    Ok(())
}
