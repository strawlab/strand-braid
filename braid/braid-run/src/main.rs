#[macro_use]
extern crate log;

// For some reason, using Jemalloc prevents "corrupted size vs prev_size" error.
#[cfg(feature = "jemalloc")]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

use std::convert::TryInto;

use failure::Error;
use structopt::StructOpt;

use flydra_types::{AddrInfoIP, MainbrainBuiLocation, RealtimePointsDestAddr};
use strand_cam::ImPtDetectCfgSource;

use braid::{braid_start, parse_config_file, BraidCameraConfig};

#[derive(Debug, StructOpt)]
#[structopt(about = "run the multi-camera realtime 3D tracker")]
struct BraidRunCliArgs {
    /// Input directory
    #[structopt(parse(from_os_str))]
    config_file: std::path::PathBuf,
}

struct StrandCamInstance {}

fn launch_strand_cam(
    camera: BraidCameraConfig,
    camdata_addr: Option<RealtimePointsDestAddr>,
    mainbrain_internal_addr: Option<MainbrainBuiLocation>,
    handle: tokio::runtime::Handle,
) -> Result<StrandCamInstance, Error> {
    let tracker_cfg_src =
        ImPtDetectCfgSource::ChangesNotSavedToDisk(camera.point_detection_config.clone());

    let args = strand_cam::StrandCamArgs {
        camera_name: Some(camera.name),
        pixel_format: camera.pixel_format,
        camtrig_device_path: None,
        csv_save_dir: "/dev/null".to_string(),
        secret: None,
        http_server_addr: "127.0.0.1:0".to_string(),
        no_browser: true,
        mkv_filename_template: "movie%Y%m%d_%H%M%S.mkv".to_string(),
        fmf_filename_template: "movie%Y%m%d_%H%M%S.fmf".to_string(),
        ufmf_filename_template: "movie%Y%m%d_%H%M%S.ufmf".to_string(),
        #[cfg(feature = "fiducial")]
        apriltag_csv_filename_template: strand_cam_storetype::APRILTAG_CSV_TEMPLATE_DEFAULT
            .to_string(),
        ros_periodic_update_interval: std::time::Duration::from_millis(9999), // not actually used
        tracker_cfg_src,
        raise_grab_thread_priority: camera.raise_grab_thread_priority,
        #[cfg(feature = "stand-cam-posix-sched-fifo")]
        process_frame_priority: None,
        use_cbor_packets: true,
        mainbrain_internal_addr,
        camdata_addr,
        show_url: false,
        force_camera_sync_mode: true,
    };

    let (_, _, fut) = strand_cam::setup_app(handle, args).expect("setup_app");
    tokio::spawn(fut);
    Ok(StrandCamInstance {})
}

fn main() -> Result<(), Error> {
    braid_start("run")?;

    let args = BraidRunCliArgs::from_args();
    debug!("{:?}", args);

    let cfg = parse_config_file(&args.config_file)?;
    debug!("{:?}", cfg);

    let mut runtime = tokio::runtime::Builder::new()
        .threaded_scheduler()
        .enable_all()
        .core_threads(4)
        .thread_name("braid-runtime")
        .thread_stack_size(3 * 1024 * 1024)
        .build()
        .expect("runtime");

    let trig_cfg = cfg.trigger;
    let show_tracking_params = false;

    let handle = runtime.handle().clone();
    let phase1 = runtime.block_on(flydra2_mainbrain::pre_run(
        &handle,
        cfg.mainbrain.cal_fname,
        cfg.mainbrain.output_base_dirname,
        Some(cfg.mainbrain.tracking_params.try_into()?),
        show_tracking_params,
        // Raising the mainbrain thread priority is currently disabled.
        // cfg.mainbrain.sched_policy_priority,
        &cfg.mainbrain.lowlatency_camdata_udp_addr,
        trig_cfg,
        false,
        cfg.mainbrain.http_api_server_addr.clone(),
        cfg.mainbrain.http_api_server_token.clone(),
        cfg.mainbrain.model_server_addr.clone(),
        cfg.mainbrain.save_empty_data2d,
        cfg.mainbrain.jwt_secret.map(|x| x.as_bytes().to_vec()),
    ))?;

    let mainbrain_server_info = MainbrainBuiLocation(phase1.mainbrain_server_info.clone());

    let camdata_addr = phase1.camdata_socket.local_addr()?;

    let addr_info_ip = AddrInfoIP::from_socket_addr(&camdata_addr);

    let cfg_cameras = cfg.cameras;
    let handle = runtime.handle().clone();
    let _strand_cams: Vec<StrandCamInstance> = runtime.enter(|| {
        cfg_cameras
            .into_iter()
            .map(|camera| {
                let camdata_addr = Some(RealtimePointsDestAddr::IpAddr(addr_info_ip.clone()));
                launch_strand_cam(
                    camera,
                    camdata_addr,
                    Some(mainbrain_server_info.clone()),
                    handle.clone(),
                )
            })
            .collect::<Result<Vec<StrandCamInstance>, Error>>()
    })?;

    debug!("done launching cameras");

    // This runs the whole thing and blocks.
    runtime.block_on(flydra2_mainbrain::run(phase1))?;

    // Now wait for everything to end..

    debug!("done");

    Ok(())
}
