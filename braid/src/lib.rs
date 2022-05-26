use anyhow::Result;

pub use braid_config_data::BraidConfig2;

pub fn braid_start(name: &str) -> Result<()> {
    human_panic::setup_panic!(human_panic::Metadata {
        version: format!("{} (git {})", env!("CARGO_PKG_VERSION"), env!("GIT_HASH")).into(),
        name: env!("CARGO_PKG_NAME").into(),
        authors: env!("CARGO_PKG_AUTHORS").replace(":", ", ").into(),
        homepage: env!("CARGO_PKG_HOMEPAGE").into(),
    });
    dotenv::dotenv().ok();

    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "braid=info,flydra2=info,braid_run=info,strand_cam=info,flydra_feature_detector=info,rt_image_viewer=info,flydra1_triggerbox=info,error");
    }

    env_tracing_logger::init();

    let version = format!("{} (git {})", env!("CARGO_PKG_VERSION"), env!("GIT_HASH"));
    log::info!("{} {}", name, version);
    Ok(())
}
