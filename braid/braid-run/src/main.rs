#![cfg_attr(feature = "backtrace", feature(error_generic_member_access))]

use anyhow::Result;
use clap::Parser;
use tracing::debug;

use braid::braid_start;
use braid_config_data::parse_config_file;
use flydra_types::{
    BraidCameraConfig, MainbrainBuiLocation, RawCamName, StartCameraBackend, TriggerType,
};

mod callback_handling;
mod mainbrain;
mod multicam_http_session_handler;

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct BraidRunCliArgs {
    /// Input directory
    config_file: std::path::PathBuf,
}

fn compute_strand_cam_args(
    camera: &BraidCameraConfig,
    mainbrain_internal_addr: &MainbrainBuiLocation,
) -> Result<Vec<String>> {
    let url = mainbrain_internal_addr.0.build_url();
    let url_string = format!("{url}");
    Ok(vec![
        "--camera-name".into(),
        camera.name.clone(),
        "--braid-url".into(),
        url_string,
    ])
}

fn launch_strand_cam(
    camera: &BraidCameraConfig,
    mainbrain_internal_addr: &MainbrainBuiLocation,
) -> Result<()> {
    use anyhow::Context;

    // On initial startup strand cam queries for
    // [flydra_types::RemoteCameraInfoResponse] and thus we do not need to
    // provide much info.

    let braid_run_exe = std::env::current_exe().unwrap();
    let exe_dir = braid_run_exe
        .parent()
        .expect("Executable must be in some directory");
    #[cfg(target_os = "windows")]
    let ext = ".exe";
    #[cfg(not(target_os = "windows"))]
    let ext = "";
    let exe = exe_dir.join(format!(
        "{}{}",
        camera.start_backend.strand_cam_exe_name().unwrap(),
        ext
    ));
    debug!("strand cam executable name: \"{}\"", exe.display());

    let mut exec = std::process::Command::new(&exe);
    let args = compute_strand_cam_args(camera, mainbrain_internal_addr)?;
    exec.args(&args);
    debug!("exec: {:?}", exec);
    let mut obj = exec.spawn().context(format!(
        "Starting Strand Cam executable \"{}\"",
        exe.display()
    ))?;
    debug!("obj: {:?}", obj);

    std::thread::spawn(move || {
        let exit_code = obj.wait().unwrap();
        debug!("done. exit_code: {:?}", exit_code);
    });

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    braid_start("run")?;

    let args = BraidRunCliArgs::parse();
    debug!("{:?}", args);

    let cfg = parse_config_file(&args.config_file)?;
    debug!("{:?}", cfg);

    let camera_configs = cfg
        .cameras
        .iter()
        .map(|cfg| {
            let raw_cam_name = RawCamName::new(cfg.name.to_string());
            (raw_cam_name, cfg.clone())
        })
        .collect();

    let trig_cfg = cfg.trigger;

    let (force_camera_sync_mode, software_limit_framerate) = match &trig_cfg {
        TriggerType::TriggerboxV1(_) => (true, flydra_types::StartSoftwareFrameRateLimit::NoChange),
        TriggerType::FakeSync(cfg) => (
            false,
            flydra_types::StartSoftwareFrameRateLimit::Enable(cfg.framerate),
        ),
        TriggerType::PtpSync(_) => (false, flydra_types::StartSoftwareFrameRateLimit::NoChange),
    };
    let show_tracking_params = false;

    // let handle = runtime.handle().clone();
    let all_expected_cameras = cfg
        .cameras
        .iter()
        .map(|x| RawCamName::new(x.name.clone()))
        .collect();

    let address_string: String = cfg.mainbrain.http_api_server_addr.clone();
    let (listener, mainbrain_server_info) = flydra_types::start_listener(&address_string).await?;
    let mainbrain_internal_addr = MainbrainBuiLocation(mainbrain_server_info.clone());

    let cfg_cameras = cfg.cameras;
    let _strand_cams = cfg_cameras
        .into_iter()
        .filter_map(|camera| {
            if camera.start_backend != StartCameraBackend::Remote {
                Some(launch_strand_cam(&camera, &mainbrain_internal_addr))
            } else {
                tracing::info!(
                    "Not starting remote camera \"{}\". Use args: {}",
                    camera.name,
                    compute_strand_cam_args(&camera, &mainbrain_internal_addr)
                        .unwrap()
                        .join(" ")
                );
                None
            }
        })
        .collect::<Result<Vec<()>>>()?;

    debug!("done launching cameras");

    let secret_base64 = cfg.mainbrain.secret_base64.as_ref().map(Clone::clone);

    // This runs the whole thing and "blocks". Now wait for everything to end.
    mainbrain::do_run_forever(
        show_tracking_params,
        // Raising the mainbrain thread priority is currently disabled.
        // cfg.mainbrain.sched_policy_priority,
        camera_configs,
        trig_cfg,
        cfg.mainbrain,
        secret_base64,
        all_expected_cameras,
        force_camera_sync_mode,
        software_limit_framerate.clone(),
        "braid",
        listener,
        mainbrain_server_info,
    )
    .await?;

    debug!("done {}:{}", file!(), line!());

    Ok(())
}
