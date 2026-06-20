// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::path::PathBuf;

use clap::{Arg, ArgAction, Args, FromArgMatches};

use crate::{BraidArgs, StandaloneArgs, StandaloneOrBraid, StrandCamArgs, run_strand_cam_app};

use crate::APP_INFO;

use eyre::{Result, WrapErr, eyre};

/// Which camera vendor backend the merged Strand Camera binary should load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraBackend {
    /// Basler Pylon backend (`ci2-pylon`).
    Pylon,
    /// Allied Vision Vimba backend (`ci2-vimba`).
    Vimba,
    /// Consumer webcam backend (`ci2-webcam`), intended for development use.
    Webcam,
    /// Simulation backend (`ci2-sim`), which renders synthetic images of
    /// simulated insects for end-to-end testing. The scenario is given by the
    /// `STRAND_CAM_SIM_SPEC` environment variable.
    Sim,
}

impl CameraBackend {
    /// Parse the backend from the value of the `--camera-backend` argument.
    pub fn from_arg(value: &str) -> Result<Self> {
        match value {
            "pylon" => Ok(CameraBackend::Pylon),
            "vimba" => Ok(CameraBackend::Vimba),
            "webcam" => Ok(CameraBackend::Webcam),
            "sim" => Ok(CameraBackend::Sim),
            other => Err(eyre!(
                "unknown camera backend '{other}', expected 'pylon', 'vimba', 'webcam', or 'sim'"
            )),
        }
    }
}

/// Determine which camera backend was requested on the command line.
///
/// This peeks at the process arguments so that the merged Strand Camera binary
/// can construct the correct backend module *before* the full argument parser
/// (which is backend-agnostic) runs in [cli_main]. Returns `None` if
/// `--camera-backend` was not supplied, in which case the caller should apply
/// its own default.
pub fn requested_camera_backend() -> Result<Option<CameraBackend>> {
    let mut args = std::env::args();
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--camera-backend=") {
            return Ok(Some(CameraBackend::from_arg(value)?));
        }
        if arg == "--camera-backend" {
            let value = args
                .next()
                .ok_or_else(|| eyre!("--camera-backend requires a value ('pylon' or 'vimba')"))?;
            return Ok(Some(CameraBackend::from_arg(&value)?));
        }
    }
    Ok(None)
}

/// Peek at the process arguments to see whether `--list-cameras` was requested.
///
/// Like [requested_camera_backend], this is checked alongside the regular
/// argument parser so that the merged binary can enumerate and print the
/// cameras available for the selected backend and then exit, without launching
/// the full application or opening its web UI.
pub fn list_cameras_requested() -> bool {
    std::env::args().any(|arg| arg == "--list-cameras")
}

/// Enumerate the cameras visible to `mymod` and print them to stdout.
///
/// The first column of each row is the camera's name, which is exactly the
/// value to pass to `--camera-name` (and to use as a camera `name` in a Braid
/// configuration file).
fn list_cameras<M, C, G>(mymod: &ci2_async::ThreadedAsyncCameraModule<M, C, G>) -> Result<()>
where
    M: ci2::CameraModule<CameraType = C, Guard = G>,
    C: ci2::Camera,
    G: Send,
{
    use ci2::CameraModule;
    // The async wrapper prefixes the backend name with "async-"; strip it so the
    // user sees the same backend name they pass to `--camera-backend`.
    let name = mymod.name();
    let backend = name.strip_prefix("async-").unwrap_or(name);
    let infos = mymod
        .camera_infos()
        .with_context(|| format!("enumerating cameras for the '{backend}' backend"))?;
    if infos.is_empty() {
        println!("No cameras found for the '{backend}' backend.");
        return Ok(());
    }
    println!(
        "Found {} camera(s) for the '{backend}' backend:",
        infos.len()
    );
    println!();
    println!("Use a camera name below with `--camera-name`, or as a `name` in a Braid config.");
    println!();
    for info in &infos {
        println!(
            "  {}  (model: {}, serial: {})",
            info.name(),
            info.model(),
            info.serial()
        );
    }
    Ok(())
}

/// Select the camera backend from the command line (defaulting to Pylon),
/// construct the corresponding camera module, and run the Strand Camera
/// application.
///
/// Only the selected backend's module is constructed, and neither backend loads
/// its vendor SDK until a camera is actually enumerated or opened. The module is
/// leaked to obtain the `'static` reference [cli_main] requires (the process
/// exits immediately afterwards regardless).
pub fn cli_main_dispatch(app_name: &'static str) -> Result<()> {
    let backend = requested_camera_backend()?.unwrap_or(CameraBackend::Pylon);

    match backend {
        CameraBackend::Pylon => {
            let module: &'static ci2_pylon::WrappedModule =
                Box::leak(Box::new(ci2_pylon::new_module()?));
            let guard = ci2_pylon::make_singleton_guard(&module)?;
            let mymod = ci2_async::into_threaded_async(module, &guard);
            cli_main(mymod, app_name)?;
        }
        CameraBackend::Vimba => {
            let module: &'static ci2_vimba::WrappedModule =
                Box::leak(Box::new(ci2_vimba::new_module()?));
            let guard = ci2_vimba::make_singleton_guard(&module)?;
            let mymod = ci2_async::into_threaded_async(module, &guard);
            cli_main(mymod, app_name)?;
        }
        CameraBackend::Webcam => {
            let module: &'static ci2_webcam::WrappedModule =
                Box::leak(Box::new(ci2_webcam::new_module()?));
            let guard = ci2_webcam::make_singleton_guard(&module)?;
            let mymod = ci2_async::into_threaded_async(module, &guard);
            cli_main(mymod, app_name)?;
        }
        CameraBackend::Sim => {
            let module: &'static ci2_sim::WrappedModule =
                Box::leak(Box::new(ci2_sim::new_module()?));
            let guard = ci2_sim::make_singleton_guard(&module)?;
            let mymod = ci2_async::into_threaded_async(module, &guard);
            cli_main(mymod, app_name)?;
        }
    }
    Ok(())
}

pub fn cli_main<M, C, G>(
    mymod: ci2_async::ThreadedAsyncCameraModule<M, C, G>,
    app_name: &'static str,
) -> Result<ci2_async::ThreadedAsyncCameraModule<M, C, G>>
where
    M: ci2::CameraModule<CameraType = C, Guard = G> + 'static,
    C: 'static + ci2::Camera + Send,
    G: Send + 'static,
{
    std::panic::set_hook(Box::new(tracing_panic::panic_hook));
    dotenv::dotenv().ok();

    if std::env::var_os("RUST_LOG").is_none() {
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe {
            std::env::set_var(
                "RUST_LOG",
                "strand_cam=info,flydra_feature_detector=info,bg_movie_writer=info,warn",
            )
        };
    }

    let args = parse_args(app_name).with_context(|| "parsing args".to_string())?;

    // Handle `--list-cameras` after argument parsing (so `--help` and argument
    // validation still work), but before launching the full application.
    if list_cameras_requested() {
        return list_cameras(&mymod).map(|()| mymod);
    }

    run_strand_cam_app(mymod, args, app_name)
}

fn parse_led_box_device(matches: &clap::ArgMatches) -> Option<String> {
    matches.get_one::<String>("led_box_device").map(Into::into)
}

fn get_tracker_cfg(_matches: &clap::ArgMatches) -> Result<crate::ImPtDetectCfgSource> {
    let ai = (&APP_INFO, "object-detection".to_string());
    let tracker_cfg_src = crate::ImPtDetectCfgSource::ChangedSavedToDisk(ai);
    Ok(tracker_cfg_src)
}

// We started strand-cam before the `derive` capability of clap and thus we have
// a bunch of stuff with the builder API. We should convert existing code to the
// derive API. For now, we just write new code to use the derive API but keep
// the existing builder API.
#[derive(Args, Debug)]
struct DerivedArgs {
    #[cfg(target_os = "linux")]
    /// If set, output a copy of the video stream on this v4l2 device (e.g. `/dev/video0`)
    #[arg(long)]
    v4l2loopback: Option<PathBuf>,

    /// If set, .mp4 videos and log files are saved to this directory.
    #[arg(long)]
    data_dir: Option<PathBuf>,
}

fn parse_args(app_name: &str) -> Result<StrandCamArgs> {
    let cli_args: Vec<String> = std::env::args().collect();

    let arg_default_box: Box<StrandCamArgs> = Default::default();
    let arg_default: &'static StrandCamArgs = Box::leak(arg_default_box);

    let app_name_box = Box::new(clap::builder::Str::from(app_name.to_string()));
    let app_name: &'static clap::builder::Str = Box::leak(app_name_box);

    let matches = {
        let parser = clap::Command::new(app_name)
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
                Arg::new("mp4_filename_template")
                    .action(ArgAction::Set)
                    .long("mp4_filename_template")
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
                Arg::new("camera_backend")
                    .long("camera-backend")
                    .value_parser(["pylon", "vimba", "webcam", "sim"])
                    .help(
                        "Which camera backend library to load. Only meaningful for \
                        the merged Strand Camera binary that supports multiple backends.",
                    ),
            )
            .arg(
                Arg::new("list_cameras")
                    .long("list-cameras")
                    .action(clap::ArgAction::SetTrue)
                    .help(
                        "List the cameras available for the selected backend and exit, \
                        without launching the application or opening a browser.",
                    ),
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

        let parser = parser
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
                    clap::Arg::new("trusted_network")
                        .help(
                            "A client network (CIDR, e.g. 100.64.0.0/10) trusted to have already \
                             authenticated the peer (e.g. Tailscale/WireGuard). Clients from it \
                             need no access token. May be given multiple times.",
                        )
                        .long("trusted-network")
                        .env("STRAND_CAM_TRUSTED_NETWORKS")
                        .value_delimiter(',')
                        .action(ArgAction::Append),
                )
                .arg(
                    Arg::new("force_camera_sync_mode")
                        .long("force_camera_sync_mode")
                        .action(clap::ArgAction::Count)
                        .help("Force the camera to synchronize to external trigger. (incompatible with braid)."),
                );

        let parser = parser.arg(
            Arg::new("braid_url")
                .long("braid-url")
                .help("Braid HTTP URL address (e.g. 'http://host:port/')"),
        );

        let parser = parser.arg(
            Arg::new("led_box_device")
                .long("led-box")
                .help("The filename of the LED box device"),
        );

        #[cfg(feature = "flydratrax")]
        let parser = {
            parser
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
                        .default_value(braid_types::DEFAULT_MODEL_SERVER_ADDR),
                )
        };

        let parser = DerivedArgs::augment_args(parser);

        parser.get_matches_from(cli_args)
    };

    let secret = matches
        .get_one::<String>("strand_cam_cookie_secret")
        .cloned()
        .clone();

    let trusted_networks: Vec<String> = matches
        .get_many::<String>("trusted_network")
        .map(|vals| vals.cloned().collect())
        .unwrap_or_default();

    let mp4_filename_template = matches
        .get_one::<String>("mp4_filename_template")
        .ok_or_else(|| eyre!("expected mp4_filename_template"))?
        .to_string();

    let fmf_filename_template = matches
        .get_one::<String>("fmf_filename_template")
        .ok_or_else(|| eyre!("expected fmf_filename_template"))?
        .to_string();

    let ufmf_filename_template = matches
        .get_one::<String>("ufmf_filename_template")
        .ok_or_else(|| eyre!("expected ufmf_filename_template"))?
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
                eyre::bail!("Can only specify xml or pymvg calibration, not both.");
            }
        }
    };

    let csv_save_dir = matches
        .get_one::<String>("csv_save_dir")
        .ok_or_else(|| eyre!("expected csv_save_dir"))?
        .to_string();

    let csv_save_dir = shellexpand::full(&csv_save_dir)
        .map_err(|e| eyre!("{}", e))?
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
        .ok_or_else(|| eyre!("expected model_server_addr"))?
        .to_string()
        .parse()
        .unwrap();

    let led_box_device_path = parse_led_box_device(&matches);

    let braid_url: Option<String> = matches.get_one::<String>("braid_url").map(Into::into);

    let standalone_or_braid = if let Some(braid_url) = braid_url {
        for argname in &[
            "pixel_format",
            "strand_cam_cookie_secret",
            "camera_settings_filename",
            "http_server_addr",
        ] {
            // These values are not relevant or are set via
            // [braid_types::RemoteCameraInfoResponse].
            //
            // Use `try_contains_id` rather than `contains_id`: the latter panics
            // if `argname` is not a defined argument id, which silently breaks if
            // an argument is ever renamed. `try_contains_id` surfaces that as an
            // error instead.
            if matches
                .try_contains_id(argname)
                .map_err(|e| eyre!("checking argument '{argname}': {e}"))?
            {
                eyre::bail!(
                    "'{argname}' cannot be set from the command line when calling \
                    strand-cam from braid.",
                );
            }
        }

        if matches.get_count("force_camera_sync_mode") != 0 {
            eyre::bail!(
                "'force_camera_sync_mode' cannot be set from the command line when calling \
                strand-cam from braid.",
            );
        }

        let camera_name = camera_name.ok_or_else(|| {
            eyre!("camera name must be set using command-line argument when running with braid")
        })?;

        StandaloneOrBraid::Braid(BraidArgs {
            braid_url,
            camera_name,
        })
    } else {
        // not braid
        let pixel_format = matches.get_one::<String>("pixel_format").map(Into::into);
        let force_camera_sync_mode = !matches!(matches.get_count("force_camera_sync_mode"), 0);
        let software_limit_framerate = braid_types::StartSoftwareFrameRateLimit::NoChange;

        let acquisition_duration_allowed_imprecision_msec =
            braid_types::DEFAULT_ACQUISITION_DURATION_ALLOWED_IMPRECISION_MSEC;

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

    // Since DerivedArgs implements FromArgMatches, we can extract it from the unstructured ArgMatches.
    // This is the main benefit of using derived arguments.
    let derived_matches = DerivedArgs::from_arg_matches(&matches)
        .map_err(|err| err.exit())
        .unwrap();

    // There are some fields set by `Default::default()` but only when various
    // cargo features are used. So turn off this clippy warning.
    Ok(StrandCamArgs {
        standalone_or_braid,
        secret,
        trusted_networks,
        no_browser,
        mp4_filename_template,
        fmf_filename_template,
        ufmf_filename_template,

        csv_save_dir,
        led_box_device_path,
        #[cfg(feature = "flydratrax")]
        flydratrax_calibration_source,
        #[cfg(feature = "flydratrax")]
        save_empty_data2d,
        #[cfg(feature = "flydratrax")]
        model_server_addr,
        #[cfg(feature = "fiducial")]
        apriltag_csv_filename_template,
        #[cfg(target_os = "linux")]
        v4l2loopback: derived_matches.v4l2loopback,
        data_dir: derived_matches.data_dir,
        ..Default::default()
    })
}
