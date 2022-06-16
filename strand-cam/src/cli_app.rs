use std::path::PathBuf;

use clap::Arg;

use crate::{run_app, StrandCamArgs};

use crate::APP_INFO;

use anyhow::Result;

fn jwt_secret(matches: &clap::ArgMatches) -> Option<Vec<u8>> {
    matches
        .value_of("JWT_SECRET")
        .map(|s| s.into())
        .or_else(|| std::env::var("JWT_SECRET").ok())
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
    human_panic::setup_panic!(human_panic::Metadata {
        version: format!("{}", env!("CARGO_PKG_VERSION")).into(),
        name: env!("CARGO_PKG_NAME").into(),
        authors: env!("CARGO_PKG_AUTHORS").replace(":", ", ").into(),
        homepage: env!("CARGO_PKG_HOMEPAGE").into(),
    });
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

fn parse_led_box_device(matches: &clap::ArgMatches) -> Option<String> {
    matches.value_of("led_box_device").map(Into::into)
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

    let arg_default = StrandCamArgs::default();

    let matches = {
        #[allow(unused_mut)]
        let mut parser = clap::App::new(app_name)
            .version(env!("CARGO_PKG_VERSION"))
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
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("csv_save_dir")
                    .long("csv-save-dir")
                    .help("The directory in which to save CSV data files.")
                    .default_value("~/DATA")
                    .takes_value(true),
            );

        // #[cfg(not(feature = "braid-config"))]
        {
            parser = parser
                .arg(
                    Arg::with_name("pixel_format")
                        .long("pixel-format")
                        .help("The desired pixel format. (incompatible with braid).")
                        .takes_value(true),
                )
                .arg(
                    clap::Arg::with_name("JWT_SECRET")
                        .long("jwt-secret")
                        .help(
                            "Specifies the JWT secret. Falls back to the JWT_SECRET \
                    environment variable if unspecified. (incompatible with braid).",
                        )
                        .global(true)
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("force_camera_sync_mode")
                        .long("force_camera_sync_mode")
                        .help("Force the camera to synchronize to external trigger. (incompatible with braid)."),
                );
        }

        // #[cfg(feature = "braid-config")]
        {
            parser = parser.arg(
                Arg::with_name("braid_addr")
                    .long("braid_addr")
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

        {
            parser = parser.arg(
                Arg::with_name("led_box_device")
                    .long("led-box")
                    .help("The filename of the LED box device")
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
            (None, None) => crate::CalSource::PseudoCal,
            (Some(xml_fname), None) => crate::CalSource::XmlFile(PathBuf::from(xml_fname)),
            (None, Some(json_fname)) => crate::CalSource::PymvgJsonFile(PathBuf::from(json_fname)),
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

    let http_server_addr: Option<String> = matches.value_of("http_server_addr").map(Into::into);

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

    let led_box_device_path = parse_led_box_device(&matches);

    let braid_addr: Option<String> = matches.value_of("braid_addr").map(Into::into);

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
    ) = if let Some(braid_addr) = braid_addr {
        for argname in &[
            "pixel_format",
            "JWT_SECRET",
            "force_camera_sync_mode",
            "camera_settings_filename",
            "http_server_addr",
        ] {
            // Typically these values are not relevant or are set via
            // [flydra_types::RemoteCameraInfoResponse].
            if matches.value_of(argname).is_some() {
                anyhow::bail!(
                    "'{}' cannot be set from the command line when calling strand-cam from braid.",
                    argname
                );
            }
        }

        let (mainbrain_internal_addr, camdata_addr, tracker_cfg_src, remote_info) = {
            log::info!("Will connect to braid at \"{}\"", braid_addr);
            let mainbrain_internal_addr = flydra_types::MainbrainBuiLocation(
                flydra_types::BuiServerInfo::parse_url_with_token(&braid_addr)?,
            );

            let mut mainbrain_session = handle.block_on(
                braid_http_session::mainbrain_future_session(mainbrain_internal_addr.clone()),
            )?;

            let camera_name = camera_name
                .as_ref()
                .ok_or(crate::StrandCamError::CameraNameRequired)?;

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

            let tracker_cfg_src = crate::ImPtDetectCfgSource::ChangesNotSavedToDisk(
                remote_info.config.point_detection_config.clone(),
            );

            (
                Some(mainbrain_internal_addr),
                camdata_addr,
                tracker_cfg_src,
                remote_info,
            )
        };

        let pixel_format = remote_info.config.pixel_format;
        let force_camera_sync_mode = remote_info.force_camera_sync_mode;
        let software_limit_framerate = remote_info.software_limit_framerate;
        let acquisition_duration_allowed_imprecision_msec = remote_info
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
        )
    } else {
        // not braid

        let mainbrain_internal_addr = None;
        let camdata_addr = None;
        let pixel_format = matches.value_of("pixel_format").map(|s| s.to_string());
        let force_camera_sync_mode = !matches!(matches.occurrences_of("force_camera_sync_mode"), 0);
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
        )
    };

    let show_url = true;

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
        mkv_filename_template,
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
