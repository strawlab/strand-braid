#[cfg(feature = "jemalloc")]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

use std::path::PathBuf;

use clap::Arg;

use strand_cam::{run_app, StrandCamArgs};

#[cfg(feature = "cfg-pt-detect-src-prefs")]
use strand_cam::APP_INFO;

type Result<T> = std::result::Result<T, anyhow::Error>;

fn jwt_secret(matches: &clap::ArgMatches) -> Option<Vec<u8>> {
    matches
        .value_of("JWT_SECRET")
        .map(|s| s.into())
        .or_else(|| std::env::var("JWT_SECRET").ok())
        .map(|s| s.into_bytes())
}

fn main() -> std::result::Result<(), anyhow::Error> {
    human_panic::setup_panic!(human_panic::Metadata {
        version: format!("{} (git {})", env!("CARGO_PKG_VERSION"), env!("GIT_HASH")).into(),
        name: env!("CARGO_PKG_NAME").into(),
        authors: env!("CARGO_PKG_AUTHORS").replace(":", ", ").into(),
        homepage: env!("CARGO_PKG_HOMEPAGE").into(),
    });
    dotenv::dotenv().ok();

    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var(
            "RUST_LOG",
            "strand_cam=info,image_tracker=info,rt_image_viewer=info,error",
        );
    }

    env_tracing_logger::init();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(4)
        .thread_name("strand-cam-runtime")
        .thread_stack_size(3 * 1024 * 1024)
        .build()?;

    let handle = runtime.handle();

    let args = parse_args(handle)?;

    run_app(args)
}

fn get_cli_args() -> Vec<String> {
    std::env::args().collect()
}

fn no_browser_default() -> bool {
    false
}

#[cfg(feature = "posix_sched_fifo")]
fn parse_sched_policy_priority(matches: &clap::ArgMatches) -> Result<Option<(i32, i32)>> {
    let errstr = "Set --sched-policy if and only if --sched-priority also set.";
    match matches.value_of("sched_policy") {
        Some(policy) => match matches.value_of("sched_priority") {
            Some(priority) => {
                let policy = policy.parse()?;
                let priority = priority.parse()?;
                Ok(Some((policy, priority)))
            }
            None => Err(anyhow::anyhow!(errstr)),
        },
        None => match matches.value_of("sched_priority") {
            Some(_priority) => Err(anyhow::anyhow!(errstr)),
            None => Ok(None),
        },
    }
}

#[cfg(not(feature = "posix_sched_fifo"))]
fn parse_sched_policy_priority(_matches: &clap::ArgMatches) -> Result<Option<(i32, i32)>> {
    Ok(None)
}

fn parse_ros_periodic_update_interval(_matches: &clap::ArgMatches) -> Result<u8> {
    Ok(1)
}

#[cfg(target_os = "macos")]
#[cfg(feature = "with_camtrig")]
const DEFAULT_CAMTRIG_PATH: &str = "/dev/tty.usbmodem1423";

#[cfg(target_os = "linux")]
#[cfg(feature = "with_camtrig")]
const DEFAULT_CAMTRIG_PATH: &str = "/dev/ttyACM0";

#[cfg(target_os = "windows")]
#[cfg(feature = "with_camtrig")]
const DEFAULT_CAMTRIG_PATH: &str = r#"COM3"#;

#[cfg(feature = "with_camtrig")]
fn parse_camtrig_device(matches: &clap::ArgMatches) -> Result<Option<String>> {
    let path = match matches.value_of("camtrig_device") {
        Some(camtrig_device) => camtrig_device,
        None => DEFAULT_CAMTRIG_PATH,
    };
    Ok(Some(path.into()))
}

#[cfg(not(feature = "with_camtrig"))]
fn parse_camtrig_device(_matches: &clap::ArgMatches) -> Result<Option<String>> {
    Ok(None)
}

#[cfg(feature = "cfg-pt-detect-src-prefs")]
fn get_tracker_cfg(_matches: &clap::ArgMatches) -> Result<strand_cam::ImPtDetectCfgSource> {
    let ai = (&APP_INFO, "object-detection".to_string());
    let tracker_cfg_src = strand_cam::ImPtDetectCfgSource::ChangedSavedToDisk(ai);
    Ok(tracker_cfg_src)
}

fn parse_args(
    handle: &tokio::runtime::Handle,
) -> std::result::Result<StrandCamArgs, anyhow::Error> {
    let cli_args = get_cli_args();

    let arg_default = StrandCamArgs::default();

    let version = format!("{} (git {})", env!("CARGO_PKG_VERSION"), env!("GIT_HASH"));

    let matches = {
        #[allow(unused_mut)]
        let mut parser = clap::App::new(env!("APP_NAME"))
            .version(version.as_str())
            .arg(
                Arg::with_name("no_browser")
                    .long("no-browser")
                    .conflicts_with("browser")
                    .help("Prevent auto-opening of browser"),
            )
            .arg(
                Arg::with_name("browser")
                    .long("browser")
                    .conflicts_with("no_browser")
                    .help("Force auto-opening of browser"),
            )
            .arg(
                Arg::with_name("mkv_filename_template")
                    .long("mkv_filename_template")
                    .default_value(&arg_default.mkv_filename_template)
                    .help("Set the initial filename template of the destination to be saved to.")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("fmf_filename_template")
                    .long("fmf_filename_template")
                    .default_value(&arg_default.fmf_filename_template)
                    .help("Set the initial filename template of the destination to be saved to.")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("ufmf_filename_template")
                    .long("ufmf_filename_template")
                    .default_value(&arg_default.ufmf_filename_template)
                    .help("Set the initial filename template of the destination to be saved to.")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("camera_name")
                    .long("camera-name")
                    .help("The name of the desired camera.")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("camera_settings_filename")
                    .long("camera-settings-filename")
                    .help("Path to file with camera settings which will be loaded.")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("http_server_addr")
                    .long("http-server-addr")
                    .help("The port to open the HTTP server.")
                    .default_value("127.0.0.1:3440")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("csv_save_dir")
                    .long("csv-save-dir")
                    .help("The directory in which to save CSV data files.")
                    .default_value("~/DATA")
                    .takes_value(true),
            );

        #[cfg(not(feature = "braid-config"))]
        {
            parser = parser
                .arg(
                    Arg::with_name("pixel_format")
                        .long("pixel-format")
                        .help("The desired pixel format.")
                        .takes_value(true),
                )
                .arg(
                    clap::Arg::with_name("JWT_SECRET")
                        .long("jwt-secret")
                        .help(
                            "Specifies the JWT secret. Falls back to the JWT_SECRET \
                    environment variable if unspecified.",
                        )
                        .global(true)
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("force_camera_sync_mode")
                        .long("force_camera_sync_mode")
                        .help("Force the camera to synchronize to external trigger"),
                );
        }

        #[cfg(feature = "braid-config")]
        {
            parser = parser.arg(
                Arg::with_name("braid_addr")
                    .help("Braid HTTP API address (IP:Port)")
                    .takes_value(true),
            );
        }

        #[cfg(feature = "posix_sched_fifo")]
        {
            parser = parser.arg(Arg::with_name("sched_policy")
                    .long("sched-policy")
                    .help("The scheduler policy (integer, e.g. SCHED_FIFO is 1). Requires also sched-priority.")
                    .takes_value(true))
            .arg(Arg::with_name("sched_priority")
                    .long("sched-priority")
                    .help("The scheduler priority (integer, e.g. 99). Requires also sched-policy.")
                    .takes_value(true));
        }

        #[cfg(feature = "with_camtrig")]
        {
            parser = parser.arg(
                Arg::with_name("camtrig_device")
                    .long("camtrig-device")
                    .help("The filename of the camtrig device")
                    .default_value(DEFAULT_CAMTRIG_PATH)
                    .takes_value(true),
            )
        }

        #[cfg(feature = "debug-images")]
        {
            parser = parser.arg(
                Arg::with_name("debug_addr")
                    .long("debug-addr")
                    .help("The port to open the HTTP server for debug images.")
                    .default_value(strand_cam::DEBUG_ADDR_DEFAULT)
                    .takes_value(true),
            )
        }

        #[cfg(feature = "flydratrax")]
        {
            parser = parser
                .arg(
                    Arg::with_name("camera_xml_calibration")
                        .long("camera-xml-calibration")
                        .help("Filename of flydra .xml camera calibration.")
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("camera_pymvg_calibration")
                        .long("camera-pymvg-calibration")
                        .help("Filename of pymvg json camera calibration.")
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("no_save_empty_data2d")
                        .long("no-save-empty-data2d")
                        .help("do not save data2d_distoted also when no detections found"),
                )
                .arg(
                    Arg::with_name("model_server_addr")
                        .long("model-server-addr")
                        .help("The address of the model server.")
                        .default_value(flydra_types::DEFAULT_MODEL_SERVER_ADDR)
                        .takes_value(true),
                );
        }

        parser.get_matches_from(cli_args)
    };

    let secret = jwt_secret(&matches);

    let mkv_filename_template = matches
        .value_of("mkv_filename_template")
        .ok_or_else(|| anyhow::anyhow!("expected mkv_filename_template"))?
        .to_string();

    let fmf_filename_template = matches
        .value_of("fmf_filename_template")
        .ok_or_else(|| anyhow::anyhow!("expected fmf_filename_template"))?
        .to_string();

    let ufmf_filename_template = matches
        .value_of("ufmf_filename_template")
        .ok_or_else(|| anyhow::anyhow!("expected ufmf_filename_template"))?
        .to_string();

    let camera_name = matches.value_of("camera_name").map(|s| s.to_string());
    let camera_settings_filename = matches
        .value_of("camera_settings_filename")
        .map(|s| PathBuf::from(s));

    #[cfg(feature = "flydratrax")]
    let camera_xml_calibration = matches
        .value_of("camera_xml_calibration")
        .map(|s| s.to_string());

    #[cfg(feature = "flydratrax")]
    let camera_pymvg_calibration = matches
        .value_of("camera_pymvg_calibration")
        .map(|s| s.to_string());

    #[cfg(feature = "flydratrax")]
    let flydratrax_calibration_source = {
        match (camera_xml_calibration, camera_pymvg_calibration) {
            (None, None) => strand_cam::CalSource::PseudoCal,
            (Some(xml_fname), None) => strand_cam::CalSource::XmlFile(PathBuf::from(xml_fname)),
            (None, Some(json_fname)) => {
                strand_cam::CalSource::PymvgJsonFile(PathBuf::from(json_fname))
            }
            (Some(_), Some(_)) => {
                anyhow::bail!("Can only specify xml or pymvg calibration, not both.");
            }
        }
    };

    let csv_save_dir = matches
        .value_of("csv_save_dir")
        .ok_or_else(|| anyhow::anyhow!("expected csv_save_dir"))?
        .to_string();

    let csv_save_dir = shellexpand::full(&csv_save_dir)
        .map_err(|e| anyhow::anyhow!("{}", e))?
        .into();

    let http_server_addr = matches
        .value_of("http_server_addr")
        .ok_or_else(|| anyhow::anyhow!("expected http_server_addr"))?
        .to_string();

    let no_browser = match matches.occurrences_of("no_browser") {
        0 => match matches.occurrences_of("browser") {
            0 => no_browser_default(),
            _ => false,
        },
        _ => true,
    };

    #[cfg(feature = "flydratrax")]
    let save_empty_data2d = match matches.occurrences_of("no_save_empty_data2d") {
        0 => true,
        _ => false,
    };

    #[cfg(feature = "flydratrax")]
    let model_server_addr = matches
        .value_of("model_server_addr")
        .ok_or_else(|| anyhow::anyhow!("expected model_server_addr"))?
        .to_string()
        .parse()
        .unwrap();

    let process_frame_priority = parse_sched_policy_priority(&matches)?;
    let ros_periodic_update_interval = parse_ros_periodic_update_interval(&matches)?;
    let ros_periodic_update_interval =
        std::time::Duration::from_secs(ros_periodic_update_interval as u64);

    let camtrig_device_path = parse_camtrig_device(&matches)?;

    #[cfg(feature = "debug-images")]
    let debug_addr = matches
        .value_of("debug_addr")
        .map(|s| s.parse().unwrap())
        .expect("required debug_addr");

    #[cfg(feature = "braid-config")]
    let (mainbrain_internal_addr, camdata_addr, tracker_cfg_src, remote_info) = {
        let braid_addr = matches
            .value_of("braid_addr")
            .ok_or_else(|| anyhow::anyhow!("expected braid_addr"))?
            .to_string();
        log::info!("Will connect to braid at \"{}\"", braid_addr);
        let mainbrain_internal_addr = flydra_types::MainbrainBuiLocation(
            flydra_types::BuiServerInfo::parse_url_with_token(&braid_addr)?,
        );

        let mut mainbrain_session = handle.block_on(
            braid_http_session::mainbrain_future_session(mainbrain_internal_addr.clone()),
        )?;

        let camera_name = camera_name
            .as_ref()
            .ok_or(strand_cam::StrandCamError::CameraNameRequired)?;

        let camera_name = flydra_types::RawCamName::new(camera_name.to_string());

        let remote_info = handle.block_on(mainbrain_session.get_remote_info(&camera_name))?;

        let camdata_addr = {
            let camdata_addr = remote_info.camdata_addr.parse::<std::net::SocketAddr>()?;
            let addr_info_ip = flydra_types::AddrInfoIP::from_socket_addr(&camdata_addr);
            let camdata_addr = Some(flydra_types::RealtimePointsDestAddr::IpAddr(
                addr_info_ip.clone(),
            ));
            camdata_addr
        };

        let tracker_cfg_src = strand_cam::ImPtDetectCfgSource::ChangesNotSavedToDisk(
            remote_info.config.point_detection_config.clone(),
        );

        (
            Some(mainbrain_internal_addr),
            camdata_addr,
            tracker_cfg_src,
            remote_info,
        )
    };

    #[cfg(feature = "braid-config")]
    let force_camera_sync_mode = remote_info.force_camera_sync_mode;

    #[cfg(not(feature = "braid-config"))]
    let force_camera_sync_mode = !matches!(matches.occurrences_of("force_camera_sync_mode"), 0);

    log::warn!("force_camera_sync_mode: {}", force_camera_sync_mode);

    #[cfg(feature = "braid-config")]
    let pixel_format = remote_info.config.pixel_format;

    #[cfg(not(feature = "braid-config"))]
    let (mainbrain_internal_addr, camdata_addr, pixel_format) = {
        let pixel_format = matches.value_of("pixel_format").map(|s| s.to_string());
        (None, None, pixel_format)
    };

    #[cfg(not(feature = "braid-config"))]
    let software_limit_framerate = flydra_types::StartSoftwareFrameRateLimit::NoChange;

    #[cfg(feature = "braid-config")]
    let software_limit_framerate = remote_info.software_limit_framerate;

    log::warn!("software_limit_framerate: {:?}", software_limit_framerate);

    #[cfg(all(feature = "image_tracker", not(feature = "braid-config")))]
    let tracker_cfg_src = get_tracker_cfg(&matches)?;

    let show_url = true;

    let raise_grab_thread_priority = process_frame_priority.is_some();

    #[cfg(feature = "fiducial")]
    let apriltag_csv_filename_template =
        strand_cam_storetype::APRILTAG_CSV_TEMPLATE_DEFAULT.to_string();

    let defaults = StrandCamArgs::default();

    Ok(StrandCamArgs {
        handle: Some(handle.clone()),
        secret,
        camera_name,
        pixel_format,
        http_server_addr,
        no_browser,
        mkv_filename_template,
        fmf_filename_template,
        ufmf_filename_template,
        #[cfg(feature = "image_tracker")]
        tracker_cfg_src,
        csv_save_dir,
        raise_grab_thread_priority,
        camtrig_device_path,
        #[cfg(feature = "posix_sched_fifo")]
        process_frame_priority,
        ros_periodic_update_interval,
        #[cfg(feature = "debug-images")]
        debug_addr,
        mainbrain_internal_addr,
        camdata_addr,
        show_url,
        #[cfg(feature = "flydratrax")]
        flydratrax_calibration_source,
        #[cfg(feature = "flydratrax")]
        save_empty_data2d,
        #[cfg(feature = "flydratrax")]
        model_server_addr,
        #[cfg(feature = "fiducial")]
        apriltag_csv_filename_template,
        force_camera_sync_mode,
        software_limit_framerate,
        camera_settings_filename,
        ..defaults
    })
}
