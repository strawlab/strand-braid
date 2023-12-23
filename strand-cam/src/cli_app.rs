#[cfg(feature = "jemalloc")]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

use std::path::PathBuf;

use clap::{Arg, ArgAction};

use crate::{run_app, StrandCamArgs};

use crate::APP_INFO;

use anyhow::Result;

fn jwt_secret(matches: &clap::ArgMatches) -> Option<Vec<u8>> {
    matches
        .get_one::<String>("JWT_SECRET")
        .map(|s| s.to_string())
        .or_else(|| std::env::var("JWT_SECRET").ok().clone())
        .map(|s| s.into_bytes())
}

pub fn cli_main<M, C>(
    mymod: ci2_async::ThreadedAsyncCameraModule<M, C>,
    app_name: &'static str,
) -> Result<()>
where
    M: ci2::CameraModule<CameraType = C>,
    C: 'static + ci2::Camera + Send,
{
    dotenv::dotenv().ok();

    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var(
            "RUST_LOG",
            "strand_cam=info,flydra_feature_detector=info,rt_image_viewer=info,error",
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

    let args = parse_args(handle, app_name)?;

    // run_app(mymod, args, app_name).map_err(|e| {
    //     #[cfg(feature = "backtrace")]
    //     match std::error::Error::backtrace(&e) {
    //         None => log::error!("no backtrace in upcoming error {}", e),
    //         Some(bt) => log::error!("backtrace in upcoming error {}: {}", e, bt),
    //     }
    //     #[cfg(not(feature = "backtrace"))]
    //     {
    //         log::error!(
    //             "compiled without backtrace support. No backtrace in upcoming error {}",
    //             e
    //         );
    //     }
    //     anyhow::Error::new(e)
    // })
    run_app(mymod, args, app_name).map_err(anyhow::Error::new)
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
    match matches.get_one::<String>("sched_policy") {
        Some(policy) => match matches.get_one::<String>("sched_priority") {
            Some(priority) => {
                let policy = policy.parse()?;
                let priority = priority.parse()?;
                Ok(Some((policy, priority)))
            }
            None => Err(anyhow::anyhow!(errstr)),
        },
        None => match matches.get_one::<String>("sched_priority") {
            Some(_priority) => Err(anyhow::anyhow!(errstr)),
            None => Ok(None),
        },
    }
}

#[cfg(not(feature = "posix_sched_fifo"))]
fn parse_sched_policy_priority(_matches: &clap::ArgMatches) -> Result<Option<(i32, i32)>> {
    Ok(None)
}

fn parse_led_box_device(matches: &clap::ArgMatches) -> Option<String> {
    matches.get_one::<String>("led_box_device").map(Into::into)
}

fn get_tracker_cfg(_matches: &clap::ArgMatches) -> Result<crate::ImPtDetectCfgSource> {
    let ai = (&APP_INFO, "object-detection".to_string());
    let tracker_cfg_src = crate::ImPtDetectCfgSource::ChangedSavedToDisk(ai);
    Ok(tracker_cfg_src)
}

fn parse_args(
    handle: &tokio::runtime::Handle,
    app_name: &str,
) -> std::result::Result<StrandCamArgs, anyhow::Error> {
    let cli_args = get_cli_args();

    let arg_default_box: Box<StrandCamArgs> = Default::default();
    let arg_default: &'static StrandCamArgs = Box::leak(arg_default_box);

    let app_name_box = Box::new(clap::builder::Str::from(app_name.to_string()));
    let app_name: &'static clap::builder::Str = Box::leak(app_name_box);

    let matches = {
        #[allow(unused_mut)]
        let mut parser = clap::Command::new(app_name)
            .version(env!("CARGO_PKG_VERSION"))
            .arg(
                Arg::new("no_browser")
                    .long("no-browser")
                    .action(clap::ArgAction::Count)
                    .conflicts_with("browser")
                    .help("Prevent auto-opening of browser"),
            )
            .arg(
                Arg::new("browser")
                    .long("browser")
                    .action(clap::ArgAction::Count)
                    .conflicts_with("no_browser")
                    .help("Force auto-opening of browser"),
            )
            .arg(
                Arg::new("mkv_filename_template")
                    .action(ArgAction::Set)
                    .long("mkv_filename_template")
                    .default_value(&*arg_default.mp4_filename_template)
                    .help("Set the initial filename template of the destination to be saved to."),
            )
            .arg(
                Arg::new("fmf_filename_template")
                    .long("fmf_filename_template")
                    .default_value(&*arg_default.fmf_filename_template)
                    .help("Set the initial filename template of the destination to be saved to."),
            )
            .arg(
                Arg::new("ufmf_filename_template")
                    .long("ufmf_filename_template")
                    .default_value(&*arg_default.ufmf_filename_template)
                    .help("Set the initial filename template of the destination to be saved to."),
            )
            .arg(
                Arg::new("camera_name")
                    .long("camera-name")
                    .help("The name of the desired camera."),
            )
            .arg(
                Arg::new("camera_settings_filename")
                    .long("camera-settings-filename")
                    .help("Path to file with camera settings which will be loaded."),
            )
            .arg(
                Arg::new("http_server_addr")
                    .long("http-server-addr")
                    .help("The port to open the HTTP server."),
            )
            .arg(
                Arg::new("csv_save_dir")
                    .long("csv-save-dir")
                    .help("The directory in which to save CSV data files.")
                    .default_value("~/DATA"),
            );

        // #[cfg(not(feature = "braid-config"))]
        {
            parser = parser
                .arg(
                    Arg::new("pixel_format")
                        .long("pixel-format")
                        .help("The desired pixel format. (incompatible with braid).")
                        ,
                )
                .arg(
                    clap::Arg::new("JWT_SECRET")
                        .long("jwt-secret")
                        .help(
                            "Specifies the JWT secret. Falls back to the JWT_SECRET \
                    environment variable if unspecified. (incompatible with braid).",
                        )
                        .global(true)
                        ,
                )
                .arg(
                    Arg::new("force_camera_sync_mode")
                        .long("force_camera_sync_mode")
                        .action(clap::ArgAction::Count)
                        .help("Force the camera to synchronize to external trigger. (incompatible with braid)."),
                );
        }

        // #[cfg(feature = "braid-config")]
        {
            parser = parser.arg(
                Arg::new("braid_addr")
                    .long("braid_addr")
                    .help("Braid HTTP API address (e.g. 'http://host:port/')"),
            );
        }

        #[cfg(feature = "posix_sched_fifo")]
        {
            parser = parser.arg(Arg::new("sched_policy")
                    .long("sched-policy")
                    .help("The scheduler policy (integer, e.g. SCHED_FIFO is 1). Requires also sched-priority."))
            .arg(Arg::new("sched_priority")
                    .long("sched-priority")
                    .help("The scheduler priority (integer, e.g. 99). Requires also sched-policy."))
        }

        {
            parser = parser.arg(
                Arg::new("led_box_device")
                    .long("led-box")
                    .help("The filename of the LED box device"),
            )
        }

        #[cfg(feature = "flydratrax")]
        {
            parser = parser
                .arg(
                    Arg::new("camera_xml_calibration")
                        .long("camera-xml-calibration")
                        .help("Filename of flydra .xml camera calibration."),
                )
                .arg(
                    Arg::new("camera_pymvg_calibration")
                        .long("camera-pymvg-calibration")
                        .help("Filename of pymvg json camera calibration."),
                )
                .arg(
                    Arg::new("no_save_empty_data2d")
                        .action(clap::ArgAction::Count)
                        .long("no-save-empty-data2d")
                        .help("do not save data2d_distoted also when no detections found"),
                )
                .arg(
                    Arg::new("model_server_addr")
                        .long("model-server-addr")
                        .help("The address of the model server.")
                        .default_value(flydra_types::DEFAULT_MODEL_SERVER_ADDR),
                );
        }

        parser.get_matches_from(cli_args)
    };

    let secret = jwt_secret(&matches);

    let mkv_filename_template = matches
        .get_one::<String>("mkv_filename_template")
        .ok_or_else(|| anyhow::anyhow!("expected mkv_filename_template"))?
        .to_string();

    let fmf_filename_template = matches
        .get_one::<String>("fmf_filename_template")
        .ok_or_else(|| anyhow::anyhow!("expected fmf_filename_template"))?
        .to_string();

    let ufmf_filename_template = matches
        .get_one::<String>("ufmf_filename_template")
        .ok_or_else(|| anyhow::anyhow!("expected ufmf_filename_template"))?
        .to_string();

    let camera_name: Option<String> = matches.get_one::<String>("camera_name").map(Into::into);
    let camera_settings_filename = matches
        .get_one::<String>("camera_settings_filename")
        .map(PathBuf::from);

    #[cfg(feature = "flydratrax")]
    let camera_xml_calibration = matches
        .get_one::<String>("camera_xml_calibration")
        .map(|s| s.to_string());

    #[cfg(feature = "flydratrax")]
    let camera_pymvg_calibration = matches
        .get_one::<String>("camera_pymvg_calibration")
        .map(|s| s.to_string());

    #[cfg(feature = "flydratrax")]
    let flydratrax_calibration_source = {
        match (camera_xml_calibration, camera_pymvg_calibration) {
            (None, None) => crate::CalSource::PseudoCal,
            (Some(xml_fname), None) => crate::CalSource::XmlFile(PathBuf::from(xml_fname)),
            (None, Some(json_fname)) => crate::CalSource::PymvgJsonFile(PathBuf::from(json_fname)),
            (Some(_), Some(_)) => {
                anyhow::bail!("Can only specify xml or pymvg calibration, not both.");
            }
        }
    };

    let csv_save_dir = matches
        .get_one::<String>("csv_save_dir")
        .ok_or_else(|| anyhow::anyhow!("expected csv_save_dir"))?
        .to_string();

    let csv_save_dir = shellexpand::full(&csv_save_dir)
        .map_err(|e| anyhow::anyhow!("{}", e))?
        .into();

    let http_server_addr: Option<String> = matches
        .get_one::<String>("http_server_addr")
        .map(Into::into);

    let no_browser = match matches.get_count("no_browser") {
        0 => match matches.get_count("browser") {
            0 => no_browser_default(),
            _ => false,
        },
        _ => true,
    };

    #[cfg(feature = "flydratrax")]
    let save_empty_data2d = match matches.get_count("no_save_empty_data2d") {
        0 => true,
        _ => false,
    };

    #[cfg(feature = "flydratrax")]
    let model_server_addr = matches
        .get_one::<String>("model_server_addr")
        .ok_or_else(|| anyhow::anyhow!("expected model_server_addr"))?
        .to_string()
        .parse()
        .unwrap();

    let process_frame_priority = parse_sched_policy_priority(&matches)?;

    let led_box_device_path = parse_led_box_device(&matches);

    let braid_addr: Option<String> = matches.get_one::<String>("braid_addr").map(Into::into);

    let (
        mainbrain_internal_addr,
        camdata_addr,
        pixel_format,
        force_camera_sync_mode,
        software_limit_framerate,
        tracker_cfg_src,
        acquisition_duration_allowed_imprecision_msec,
        http_server_addr,
        no_browser,
        show_url,
    ) = if let Some(braid_addr) = braid_addr {
        for argname in &[
            "pixel_format",
            "JWT_SECRET",
            "camera_settings_filename",
            "http_server_addr",
        ] {
            // Typically these values are not relevant or are set via
            // [flydra_types::RemoteCameraInfoResponse].
            if matches.contains_id(argname) {
                anyhow::bail!(
                    "'{argname}' cannot be set from the command line when calling \
                    strand-cam from braid.",
                );
            }
        }

        if matches.get_count("force_camera_sync_mode") != 0 {
            anyhow::bail!(
                "'force_camera_sync_mode' cannot be set from the command line when calling \
                strand-cam from braid.",
            );
        }

        let (mainbrain_internal_addr, camdata_addr, tracker_cfg_src, config_from_braid) = {
            log::info!("Will connect to braid at \"{}\"", braid_addr);
            let mainbrain_internal_addr = flydra_types::MainbrainBuiLocation(
                flydra_types::StrandCamBuiServerInfo::parse_url_with_token(&braid_addr)?,
            );

            let mut mainbrain_session = handle.block_on(
                braid_http_session::mainbrain_future_session(mainbrain_internal_addr.clone()),
            )?;

            let camera_name = camera_name
                .as_ref()
                .ok_or(crate::StrandCamError::CameraNameRequired)?;

            let camera_name = flydra_types::RawCamName::new(camera_name.to_string());

            let config_from_braid: flydra_types::RemoteCameraInfoResponse =
                handle.block_on(mainbrain_session.get_remote_info(&camera_name))?;

            let camdata_addr = {
                let camdata_addr = config_from_braid
                    .camdata_addr
                    .parse::<std::net::SocketAddr>()?;
                let addr_info_ip = flydra_types::AddrInfoIP::from_socket_addr(&camdata_addr);

                Some(flydra_types::RealtimePointsDestAddr::IpAddr(addr_info_ip))
            };

            let tracker_cfg_src = crate::ImPtDetectCfgSource::ChangesNotSavedToDisk(
                config_from_braid.config.point_detection_config.clone(),
            );

            (
                Some(mainbrain_internal_addr),
                camdata_addr,
                tracker_cfg_src,
                config_from_braid,
            )
        };

        let pixel_format = config_from_braid.config.pixel_format;
        let force_camera_sync_mode = config_from_braid.force_camera_sync_mode;
        let software_limit_framerate = config_from_braid.software_limit_framerate;
        let acquisition_duration_allowed_imprecision_msec = config_from_braid
            .config
            .acquisition_duration_allowed_imprecision_msec;

        (
            mainbrain_internal_addr,
            camdata_addr,
            pixel_format,
            force_camera_sync_mode,
            software_limit_framerate,
            tracker_cfg_src,
            acquisition_duration_allowed_imprecision_msec,
            Some("127.0.0.1:0".to_string()),
            true,
            false,
        )
    } else {
        // not braid

        let mainbrain_internal_addr = None;
        let camdata_addr = None;
        let pixel_format = matches.get_one::<String>("pixel_format").map(Into::into);
        let force_camera_sync_mode = !matches!(matches.get_count("force_camera_sync_mode"), 0);
        let software_limit_framerate = flydra_types::StartSoftwareFrameRateLimit::NoChange;

        let tracker_cfg_src = get_tracker_cfg(&matches)?;

        let acquisition_duration_allowed_imprecision_msec =
            flydra_types::DEFAULT_ACQUISITION_DURATION_ALLOWED_IMPRECISION_MSEC;
        (
            mainbrain_internal_addr,
            camdata_addr,
            pixel_format,
            force_camera_sync_mode,
            software_limit_framerate,
            tracker_cfg_src,
            acquisition_duration_allowed_imprecision_msec,
            http_server_addr,
            no_browser,
            true,
        )
    };

    let raise_grab_thread_priority = process_frame_priority.is_some();

    #[cfg(feature = "fiducial")]
    let apriltag_csv_filename_template =
        strand_cam_storetype::APRILTAG_CSV_TEMPLATE_DEFAULT.to_string();

    #[cfg(not(feature = "flydra_feat_detect"))]
    std::mem::drop(tracker_cfg_src); // prevent compiler warning of unused variable

    let defaults = StrandCamArgs::default();

    Ok(StrandCamArgs {
        handle: Some(handle.clone()),
        secret,
        camera_name,
        pixel_format,
        http_server_addr,
        no_browser,
        mp4_filename_template: mkv_filename_template,
        fmf_filename_template,
        ufmf_filename_template,
        #[cfg(feature = "flydra_feat_detect")]
        tracker_cfg_src,
        csv_save_dir,
        raise_grab_thread_priority,
        led_box_device_path,
        #[cfg(feature = "posix_sched_fifo")]
        process_frame_priority,
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
        acquisition_duration_allowed_imprecision_msec,
        ..defaults
    })
}
