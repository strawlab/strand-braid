#[cfg(feature = "jemalloc")]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

use std::path::PathBuf;

use clap::{Arg, ArgAction};

use crate::{run_app, BraidArgs, StandaloneArgs, StandaloneOrBraid, StrandCamArgs};

use crate::APP_INFO;

use anyhow::{Context, Result};

pub fn cli_main<M, C, G>(
    mymod: ci2_async::ThreadedAsyncCameraModule<M, C, G>,
    app_name: &'static str,
) -> Result<ci2_async::ThreadedAsyncCameraModule<M, C, G>>
where
    M: ci2::CameraModule<CameraType = C, Guard = G> + 'static,
    C: 'static + ci2::Camera + Send,
{
    dotenv::dotenv().ok();

    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var(
            "RUST_LOG",
            "strand_cam=info,flydra_feature_detector=info,rt_image_viewer=info,warn",
        );
    }

    let args = parse_args(app_name).with_context(|| format!("parsing args"))?;

    run_app(mymod, args, app_name)
}

fn get_cli_args() -> Vec<String> {
    std::env::args().collect()
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

fn parse_args(app_name: &str) -> anyhow::Result<StrandCamArgs> {
    let cli_args = get_cli_args();

    let arg_default_box: Box<StrandCamArgs> = Default::default();
    let arg_default: &'static StrandCamArgs = Box::leak(arg_default_box);

    let app_name_box = Box::new(clap::builder::Str::from(app_name.to_string()));
    let app_name: &'static clap::builder::Str = Box::leak(app_name_box);

    let matches = {
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

        parser = parser
                .arg(
                    Arg::new("pixel_format")
                        .long("pixel-format")
                        .help("The desired pixel format. (incompatible with braid).")
                        ,
                )
                .arg(
                    clap::Arg::new("strand_cam_cookie_secret")
                        .help("The secret (base64 encoded) for signing HTTP cookies.")
                        .long("strand-cam-cookie-secret")
                        .env("STRAND_CAM_COOKIE_SECRET")
                        .action(ArgAction::Set),
                )
                .arg(
                    Arg::new("force_camera_sync_mode")
                        .long("force_camera_sync_mode")
                        .action(clap::ArgAction::Count)
                        .help("Force the camera to synchronize to external trigger. (incompatible with braid)."),
                );

        parser = parser.arg(
            Arg::new("braid_url")
                .long("braid-url")
                .help("Braid HTTP URL address (e.g. 'http://host:port/')"),
        );

        #[cfg(feature = "posix_sched_fifo")]
        {
            parser = parser.arg(Arg::new("sched_policy")
                    .long("sched-policy")
                    .help("The scheduler policy (integer, e.g. SCHED_FIFO is 1). Requires also sched-priority."))
            .arg(Arg::new("sched_priority")
                    .long("sched-priority")
                    .help("The scheduler priority (integer, e.g. 99). Requires also sched-policy."))
        }

        parser = parser.arg(
            Arg::new("led_box_device")
                .long("led-box")
                .help("The filename of the LED box device"),
        );

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

    let secret = matches
        .get_one::<String>("strand_cam_cookie_secret")
        .cloned()
        .clone();

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

    let braid_url: Option<String> = matches.get_one::<String>("braid_url").map(Into::into);

    let standalone_or_braid = if let Some(braid_url) = braid_url {
        for argname in &[
            "pixel_format",
            "JWT_SECRET",
            "camera_settings_filename",
            "http_server_addr",
        ] {
            // These values are not relevant or are set via
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

        let camera_name = camera_name.ok_or_else(|| {
            anyhow::anyhow!(
                "camera name must be set using command-line argument when running with braid"
            )
        })?;

        StandaloneOrBraid::Braid(BraidArgs {
            braid_url,
            camera_name,
        })
    } else {
        // not braid
        let pixel_format = matches.get_one::<String>("pixel_format").map(Into::into);
        let force_camera_sync_mode = !matches!(matches.get_count("force_camera_sync_mode"), 0);
        let software_limit_framerate = flydra_types::StartSoftwareFrameRateLimit::NoChange;

        let acquisition_duration_allowed_imprecision_msec =
            flydra_types::DEFAULT_ACQUISITION_DURATION_ALLOWED_IMPRECISION_MSEC;

        let tracker_cfg_src = get_tracker_cfg(&matches)?;

        #[cfg(not(feature = "flydra_feat_detect"))]
        let _ = tracker_cfg_src; // This is unused without `flydra_feat_detect` feature.

        StandaloneOrBraid::Standalone(StandaloneArgs {
            camera_name,
            pixel_format,
            force_camera_sync_mode,
            software_limit_framerate,
            acquisition_duration_allowed_imprecision_msec,
            camera_settings_filename,
            #[cfg(feature = "flydra_feat_detect")]
            tracker_cfg_src,
            http_server_addr,
        })
    };

    let raise_grab_thread_priority = process_frame_priority.is_some();

    let no_browser_default = match &standalone_or_braid {
        StandaloneOrBraid::Braid(_) => true,
        StandaloneOrBraid::Standalone(_) => false,
    };

    let no_browser = match matches.get_count("no_browser") {
        0 => match matches.get_count("browser") {
            0 => no_browser_default,
            _ => false,
        },
        _ => true,
    };

    #[cfg(feature = "fiducial")]
    let apriltag_csv_filename_template =
        strand_cam_storetype::APRILTAG_CSV_TEMPLATE_DEFAULT.to_string();

    // There are some fields set by `Default::default()` but only when various
    // cargo features are used. So turn off this clippy warning.
    #[allow(clippy::needless_update)]
    Ok(StrandCamArgs {
        standalone_or_braid,
        secret,
        no_browser,
        mp4_filename_template: mkv_filename_template,
        fmf_filename_template,
        ufmf_filename_template,

        csv_save_dir,
        raise_grab_thread_priority,
        led_box_device_path,
        #[cfg(feature = "posix_sched_fifo")]
        process_frame_priority,
        #[cfg(feature = "flydratrax")]
        flydratrax_calibration_source,
        #[cfg(feature = "flydratrax")]
        save_empty_data2d,
        #[cfg(feature = "flydratrax")]
        model_server_addr,
        #[cfg(feature = "fiducial")]
        apriltag_csv_filename_template,
        ..Default::default()
    })
}
