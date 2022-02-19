#[macro_use]
extern crate log;

// For some reason, using Jemalloc prevents "corrupted size vs prev_size" error.
#[cfg(feature = "jemalloc")]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

use anyhow::Result;
use structopt::StructOpt;

use flydra_types::{
    AddrInfoIP, MainbrainBuiLocation, RawCamName, RealtimePointsDestAddr, TriggerType,
};
use strand_cam::{ImPtDetectCfgSource, StrandCamApp};

use braid::braid_start;
use braid_config_data::parse_config_file;
use flydra_types::BraidCameraConfig;

#[derive(Debug, StructOpt)]
#[structopt(about = "run the multi-camera realtime 3D tracker")]
struct BraidRunCliArgs {
    /// Input directory
    #[structopt(parse(from_os_str))]
    config_file: std::path::PathBuf,
}

fn launch_strand_cam(
    handle: tokio::runtime::Handle,
    camera: BraidCameraConfig,
    camdata_addr: Option<RealtimePointsDestAddr>,
    mainbrain_internal_addr: Option<MainbrainBuiLocation>,
    force_camera_sync_mode: bool,
    software_limit_framerate: flydra_types::StartSoftwareFrameRateLimit,
    camera_settings_filename: Option<std::path::PathBuf>,
    acquisition_duration_allowed_imprecision_msec: Option<f64>,
) -> Result<StrandCamApp> {
    let tracker_cfg_src =
        ImPtDetectCfgSource::ChangesNotSavedToDisk(camera.point_detection_config.clone());

    let args = strand_cam::StrandCamArgs {
        handle: Some(handle.clone()),
        is_braid: true,
        camera_name: Some(camera.name),
        pixel_format: camera.pixel_format,
        camtrig_device_path: None,
        csv_save_dir: "/dev/null".to_string(),
        secret: None,
        http_server_addr: "127.0.0.1:0".to_string(),
        no_browser: true,
        mkv_filename_template: "movie%Y%m%d_%H%M%S_{CAMNAME}.mkv".to_string(),
        fmf_filename_template: "movie%Y%m%d_%H%M%S_{CAMNAME}.fmf".to_string(),
        ufmf_filename_template: "movie%Y%m%d_%H%M%S_{CAMNAME}.ufmf".to_string(),
        #[cfg(feature = "fiducial")]
        apriltag_csv_filename_template: strand_cam_storetype::APRILTAG_CSV_TEMPLATE_DEFAULT
            .to_string(),
        tracker_cfg_src,
        raise_grab_thread_priority: camera.raise_grab_thread_priority,
        #[cfg(feature = "stand-cam-posix-sched-fifo")]
        process_frame_priority: None,
        mainbrain_internal_addr,
        camdata_addr,
        show_url: false,
        force_camera_sync_mode,
        software_limit_framerate,
        camera_settings_filename,
        acquisition_duration_allowed_imprecision_msec,
    };

    let (_, _, fut, app) = handle.block_on(strand_cam::setup_app(handle.clone(), args))?;
    handle.spawn(fut);
    Ok(app)
}

fn main() -> Result<()> {
    braid_start("run")?;

    let args = BraidRunCliArgs::from_args();
    debug!("{:?}", args);

    let cfg = parse_config_file(&args.config_file)?;
    debug!("{:?}", cfg);

    let n_local_cameras = cfg.cameras.iter().filter(|c| !c.remote_camera).count();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(4 + 4 * n_local_cameras)
        .thread_name("braid-runtime")
        .thread_stack_size(3 * 1024 * 1024)
        .build()?;

    let pixel_formats = cfg
        .cameras
        .iter()
        .map(|cfg| (cfg.name.clone(), cfg.clone()))
        .collect();

    let trig_cfg = cfg.trigger;

    let (force_camera_sync_mode, software_limit_framerate) = match &trig_cfg {
        TriggerType::TriggerboxV1(_) => (true, flydra_types::StartSoftwareFrameRateLimit::NoChange),
        TriggerType::FakeSync(cfg) => (
            false,
            flydra_types::StartSoftwareFrameRateLimit::Enable(cfg.framerate),
        ),
    };
    let show_tracking_params = false;

    let handle = runtime.handle().clone();
    let all_expected_cameras = cfg
        .cameras
        .iter()
        .map(|x| RawCamName::new(x.name.clone()).to_ros())
        .collect();
    let phase1 = runtime.block_on(flydra2_mainbrain::pre_run(
        &handle,
        show_tracking_params,
        // Raising the mainbrain thread priority is currently disabled.
        // cfg.mainbrain.sched_policy_priority,
        pixel_formats,
        trig_cfg,
        &cfg.mainbrain,
        cfg.mainbrain
            .jwt_secret
            .as_ref()
            .map(|x| x.as_bytes().to_vec()),
        all_expected_cameras,
        force_camera_sync_mode,
        software_limit_framerate.clone(),
        "braid",
    ))?;

    let mainbrain_server_info = MainbrainBuiLocation(phase1.mainbrain_server_info.clone());

    let camdata_addr = phase1.camdata_socket.local_addr()?;

    let addr_info_ip = AddrInfoIP::from_socket_addr(&camdata_addr);

    let cfg_cameras = cfg.cameras;
    let acquisition_duration_allowed_imprecision_msec =
        cfg.mainbrain.acquisition_duration_allowed_imprecision_msec;

    let handle = runtime.handle().clone();
    let _enter_guard = runtime.enter();
    let _strand_cams = cfg_cameras
        .into_iter()
        .filter_map(|camera| {
            let camera_settings_filename = camera.camera_settings_filename.clone();
            if !camera.remote_camera {
                let camdata_addr = Some(RealtimePointsDestAddr::IpAddr(addr_info_ip.clone()));
                Some(launch_strand_cam(
                    handle.clone(),
                    camera,
                    camdata_addr,
                    Some(mainbrain_server_info.clone()),
                    force_camera_sync_mode,
                    software_limit_framerate.clone(),
                    camera_settings_filename,
                    acquisition_duration_allowed_imprecision_msec,
                ))
            } else {
                log::info!("Not starting remote camera \"{}\"", camera.name);
                None
            }
        })
        .collect::<Result<Vec<StrandCamApp>>>()?;

    debug!("done launching cameras");

    // This runs the whole thing and blocks.
    runtime.block_on(flydra2_mainbrain::run(phase1))?;

    // Now wait for everything to end..

    debug!("done {}:{}", file!(), line!());

    Ok(())
}
