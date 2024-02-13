#![cfg_attr(feature = "backtrace", feature(error_generic_member_access))]

use clap::Parser;
use color_eyre::{
    eyre::{self, WrapErr},
    Result,
};
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
    /// Flag if logging to console should be disabled.
    #[arg(short, long)]
    disable_console: bool,
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
    strand_cam_set: &mut tokio::task::JoinSet<()>,
    camera: &BraidCameraConfig,
    mainbrain_internal_addr: &MainbrainBuiLocation,
) -> Result<()> {
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

    let cam_name = camera.name.clone();

    let mut exec = std::process::Command::new(&exe);
    let args = compute_strand_cam_args(&camera, mainbrain_internal_addr)?;
    exec.args(&args);
    debug!("exec: {:?}", exec);
    let mut obj = exec.spawn().context(format!(
        "Starting Strand Cam executable \"{}\"",
        exe.display()
    ))?;
    debug!("obj: {:?}", obj);

    let _abort_handle = strand_cam_set.spawn_blocking(move || {
        let exit_code = obj.wait().unwrap();
        if !exit_code.success() {
            tracing::error!(
                "Strand Cam executable for {cam_name} exited with error {}",
                exit_code.code().unwrap()
            );
        } else {
            debug!("Strand Cam executable done.");
        }
    });
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    braid_start("run")?;

    let args = BraidRunCliArgs::parse();
    let cfg = parse_config_file(&args.config_file).with_context(|| {
        format!(
            "when parsing configuration file {}",
            args.config_file.display()
        )
    })?;

    let log_file_name = format!(
        "~/.braid-{}.log",
        std::time::SystemTime::UNIX_EPOCH
            .elapsed()
            .unwrap()
            .as_micros(),
    );
    let log_file_name = std::path::PathBuf::from(shellexpand::full(&log_file_name)?.to_string());
    // TODO: delete log files older than, e.g. one week.

    let _guard = env_tracing_logger::initiate_logging(Some(&log_file_name), args.disable_console)
        .map_err(|e| eyre::eyre!("error initiating logging: {e}"))?;

    let version = format!("{} (git {})", env!("CARGO_PKG_VERSION"), env!("GIT_HASH"));
    tracing::info!("{} {}", "run", version);
    tracing::debug!("{:?}", cfg);

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
        TriggerType::PtpSync(_) | TriggerType::DeviceTimestamp => {
            (false, flydra_types::StartSoftwareFrameRateLimit::NoChange)
        }
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
    let mut strand_cam_set = tokio::task::JoinSet::new();
    for camera in cfg_cameras.into_iter() {
        if camera.start_backend != StartCameraBackend::Remote {
            launch_strand_cam(&mut strand_cam_set, &camera, &mainbrain_internal_addr)?;
        } else {
            tracing::info!(
                "Not starting remote camera \"{}\". Use args: {}",
                camera.name,
                compute_strand_cam_args(&camera, &mainbrain_internal_addr)
                    .unwrap()
                    .join(" ")
            );
            // Insert dummy future that never completes so that the JoinSet does
            // not complete.
            strand_cam_set.spawn(std::future::pending());
        }
    }

    debug!("done launching cameras");

    let secret_base64 = cfg.mainbrain.secret_base64.clone();

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
        strand_cam_set,
    )
    .await?;

    debug!("done {}:{}", file!(), line!());

    Ok(())
}
