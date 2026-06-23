// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::path::PathBuf;

use clap::{ArgAction, CommandFactory, FromArgMatches, Parser};

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

    let cli_args: Vec<String> = std::env::args().collect();
    let args = parse_args(app_name, cli_args)
        .inspect_err(|e| {
            // Preserve clap's native behavior for the binary: print the
            // formatted help/version/usage-error message and exit with the
            // appropriate status code instead of bubbling up as an `eyre`
            // error.
            if let Some(clap_err) = e.downcast_ref::<clap::Error>() {
                clap_err.exit();
            }
        })
        .with_context(|| "parsing args".to_string())?;

    // Handle `--list-cameras` after argument parsing (so `--help` and argument
    // validation still work), but before launching the full application.
    if list_cameras_requested() {
        return list_cameras(&mymod).map(|()| mymod);
    }

    run_strand_cam_app(mymod, args, app_name)
}

fn get_tracker_cfg() -> Result<crate::ImPtDetectCfgSource> {
    let ai = (&APP_INFO, "object-detection".to_string());
    let tracker_cfg_src = crate::ImPtDetectCfgSource::ChangedSavedToDisk(ai);
    Ok(tracker_cfg_src)
}

/// Intermediate, flat representation of the Strand Camera command line.
///
/// clap parses the raw arguments into this struct, which [parse_args] then
/// translates into the richer [StrandCamArgs] (with its standalone/braid split
/// and feature-gated fields). The field names double as the argument ids that
/// the braid-mode validation reports in its error messages, so renaming a field
/// changes those messages.
///
/// This doc comment is deliberately kept out of `--help`: the
/// `#[command(about = None, long_about = None)]` attribute below resets the
/// description that clap would otherwise derive from this comment, matching the
/// former builder-based parser, which had no top-level description.
#[derive(Parser, Debug)]
#[command(version, about = None, long_about = None)]
struct CliArgs {
    /// Prevent auto-opening of browser
    #[arg(long = "no-browser", action = ArgAction::Count, conflicts_with = "browser")]
    no_browser: u8,

    /// Force auto-opening of browser
    #[arg(long, action = ArgAction::Count)]
    browser: u8,

    /// Set the initial filename template of the destination to be saved to.
    #[arg(long = "mp4_filename_template", default_value = crate::MP4_FILENAME_TEMPLATE_DEFAULT)]
    mp4_filename_template: String,

    /// Set the initial filename template of the destination to be saved to.
    #[arg(long = "fmf_filename_template", default_value = crate::FMF_FILENAME_TEMPLATE_DEFAULT)]
    fmf_filename_template: String,

    /// Set the initial filename template of the destination to be saved to.
    #[arg(long = "ufmf_filename_template", default_value = crate::UFMF_FILENAME_TEMPLATE_DEFAULT)]
    ufmf_filename_template: String,

    /// The name of the desired camera.
    #[arg(long = "camera-name")]
    camera_name: Option<String>,

    /// Which camera backend library to load. Only meaningful for the merged
    /// Strand Camera binary that supports multiple backends.
    #[arg(long = "camera-backend", value_parser = ["pylon", "vimba", "webcam", "sim"])]
    camera_backend: Option<String>,

    /// List the cameras available for the selected backend and exit, without
    /// launching the application or opening a browser.
    #[arg(long = "list-cameras")]
    list_cameras: bool,

    /// Path to file with camera settings which will be loaded.
    #[arg(long = "camera-settings-filename")]
    camera_settings_filename: Option<PathBuf>,

    /// The port to open the HTTP server.
    #[arg(long = "http-server-addr")]
    http_server_addr: Option<String>,

    /// The directory in which to save CSV data files.
    #[arg(long = "csv-save-dir", default_value = "~/DATA")]
    csv_save_dir: String,

    /// The desired pixel format. (incompatible with braid).
    #[arg(long = "pixel-format")]
    pixel_format: Option<String>,

    /// The secret (base64 encoded) for signing HTTP cookies.
    #[arg(long = "strand-cam-cookie-secret", env = "STRAND_CAM_COOKIE_SECRET")]
    strand_cam_cookie_secret: Option<String>,

    /// A client network (CIDR, e.g. 100.64.0.0/10) trusted to have already
    /// authenticated the peer (e.g. Tailscale/WireGuard). Clients from it need
    /// no access token. May be given multiple times.
    #[arg(
        long = "trusted-network",
        env = "STRAND_CAM_TRUSTED_NETWORKS",
        value_delimiter = ','
    )]
    trusted_network: Vec<String>,

    /// Force the camera to synchronize to external trigger. (incompatible with braid).
    #[arg(long = "force_camera_sync_mode", action = ArgAction::Count)]
    force_camera_sync_mode: u8,

    /// Braid HTTP URL address (e.g. 'http://host:port/')
    #[arg(long = "braid-url")]
    braid_url: Option<String>,

    /// The filename of the LED box device
    #[arg(long = "led-box")]
    led_box_device: Option<String>,

    /// Filename of flydra .xml camera calibration.
    #[cfg(feature = "flydratrax")]
    #[arg(long = "camera-xml-calibration")]
    camera_xml_calibration: Option<String>,

    /// Filename of pymvg json camera calibration.
    #[cfg(feature = "flydratrax")]
    #[arg(long = "camera-pymvg-calibration")]
    camera_pymvg_calibration: Option<String>,

    /// do not save data2d_distoted also when no detections found
    #[cfg(feature = "flydratrax")]
    #[arg(long = "no-save-empty-data2d", action = ArgAction::Count)]
    no_save_empty_data2d: u8,

    /// The address of the model server.
    #[cfg(feature = "flydratrax")]
    #[arg(long = "model-server-addr", default_value = braid_types::DEFAULT_MODEL_SERVER_ADDR)]
    model_server_addr: String,

    /// If set, output a copy of the video stream on this v4l2 device (e.g. `/dev/video0`)
    #[cfg(target_os = "linux")]
    #[arg(long)]
    v4l2loopback: Option<PathBuf>,

    /// If set, .mp4 videos and log files are saved to this directory.
    #[arg(long)]
    data_dir: Option<PathBuf>,
}

fn parse_args(app_name: &str, cli_args: Vec<String>) -> Result<StrandCamArgs> {
    // Build the derived command so that the program name shown in help/usage is
    // the runtime `app_name` (the derive default would be the crate name).
    //
    // Use `try_get_matches_from` rather than `get_matches_from` so that a parse
    // error returns a `clap::Error` instead of terminating the process. The
    // binary entry point ([cli_main]) restores the usual exit-on-error /
    // print-help behavior by calling `clap::Error::exit`; tests can recover the
    // error instead.
    let cli = {
        let command = CliArgs::command().name(app_name.to_string());
        let matches = command.try_get_matches_from(cli_args)?;
        CliArgs::from_arg_matches(&matches)?
    };

    // `camera_backend` and `list_cameras` are acted upon before full parsing by
    // `requested_camera_backend()` and `list_cameras_requested()`, which peek at
    // the raw process arguments. They appear in `CliArgs` only so the parser
    // accepts, validates, and documents them in `--help`.
    let _ = (&cli.camera_backend, cli.list_cameras);

    let secret = cli.strand_cam_cookie_secret.clone();

    let trusted_networks: Vec<String> = cli.trusted_network.clone();

    let mp4_filename_template = cli.mp4_filename_template.clone();
    let fmf_filename_template = cli.fmf_filename_template.clone();
    let ufmf_filename_template = cli.ufmf_filename_template.clone();

    let camera_name: Option<String> = cli.camera_name.clone();
    let camera_settings_filename = cli.camera_settings_filename.clone();

    #[cfg(feature = "flydratrax")]
    let flydratrax_calibration_source = {
        match (&cli.camera_xml_calibration, &cli.camera_pymvg_calibration) {
            (None, None) => crate::CalSource::PseudoCal,
            (Some(xml_fname), None) => crate::CalSource::XmlFile(PathBuf::from(xml_fname)),
            (None, Some(json_fname)) => crate::CalSource::PymvgJsonFile(PathBuf::from(json_fname)),
            (Some(_), Some(_)) => {
                eyre::bail!("Can only specify xml or pymvg calibration, not both.");
            }
        }
    };

    let csv_save_dir = shellexpand::full(&cli.csv_save_dir)
        .map_err(|e| eyre!("{}", e))?
        .into();

    let http_server_addr: Option<String> = cli.http_server_addr.clone();

    #[cfg(feature = "flydratrax")]
    let save_empty_data2d = matches!(cli.no_save_empty_data2d, 0);

    #[cfg(feature = "flydratrax")]
    let model_server_addr = cli.model_server_addr.parse().unwrap();

    let led_box_device_path = cli.led_box_device.clone();

    let standalone_or_braid = if let Some(braid_url) = cli.braid_url.clone() {
        // These values are not relevant or are set via
        // [braid_types::RemoteCameraInfoResponse], so they cannot be supplied on
        // the command line when running under braid.
        for (argname, is_set) in [
            ("pixel_format", cli.pixel_format.is_some()),
            (
                "strand_cam_cookie_secret",
                cli.strand_cam_cookie_secret.is_some(),
            ),
            (
                "camera_settings_filename",
                cli.camera_settings_filename.is_some(),
            ),
            ("http_server_addr", cli.http_server_addr.is_some()),
        ] {
            if is_set {
                eyre::bail!(
                    "'{argname}' cannot be set from the command line when calling \
                    strand-cam from braid.",
                );
            }
        }

        if cli.force_camera_sync_mode != 0 {
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
        let pixel_format = cli.pixel_format.clone();
        let force_camera_sync_mode = cli.force_camera_sync_mode != 0;
        let software_limit_framerate = braid_types::StartSoftwareFrameRateLimit::NoChange;

        let acquisition_duration_allowed_imprecision_msec =
            braid_types::DEFAULT_ACQUISITION_DURATION_ALLOWED_IMPRECISION_MSEC;

        let tracker_cfg_src = get_tracker_cfg()?;

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

    let no_browser = match cli.no_browser {
        0 => match cli.browser {
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
        v4l2loopback: cli.v4l2loopback.clone(),
        data_dir: cli.data_dir.clone(),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    //! Tests that pin the command-line argument parsing behavior of
    //! [parse_args]. They exist so that a later refactor (switching from the
    //! clap builder API to the derive API) can be verified to preserve the
    //! observable behavior of every argument.
    //!
    //! These run under the crate's default features
    //! (`flydra_feat_detect`, `bundle_files`), so the `flydratrax`- and
    //! `fiducial`-gated arguments are not exercised here.
    use super::*;

    /// Parse `args` (without the leading program name) and return the result.
    fn parse(args: &[&str]) -> Result<StrandCamArgs> {
        let cli_args: Vec<String> = std::iter::once("strand-cam")
            .chain(args.iter().copied())
            .map(String::from)
            .collect();
        parse_args("strand-cam", cli_args)
    }

    /// Parse `args`, asserting success and returning the standalone arguments.
    fn parse_standalone(args: &[&str]) -> StandaloneArgs {
        match parse(args).unwrap().standalone_or_braid {
            StandaloneOrBraid::Standalone(s) => s,
            StandaloneOrBraid::Braid(_) => panic!("expected standalone, got braid"),
        }
    }

    /// Parse `args`, asserting success and returning the braid arguments.
    fn parse_braid(args: &[&str]) -> BraidArgs {
        match parse(args).unwrap().standalone_or_braid {
            StandaloneOrBraid::Braid(b) => b,
            StandaloneOrBraid::Standalone(_) => panic!("expected braid, got standalone"),
        }
    }

    #[test]
    fn defaults_are_standalone() {
        let args = parse(&[]).unwrap();
        assert!(matches!(
            args.standalone_or_braid,
            StandaloneOrBraid::Standalone(_)
        ));
        // Standalone mode auto-opens the browser by default.
        assert!(!args.no_browser);
        assert_eq!(
            args.mp4_filename_template,
            "movie%Y%m%d_%H%M%S.%f_{CAMNAME}.mp4"
        );
        assert_eq!(
            args.fmf_filename_template,
            "movie%Y%m%d_%H%M%S.%f_{CAMNAME}.fmf"
        );
        assert_eq!(
            args.ufmf_filename_template,
            "movie%Y%m%d_%H%M%S.%f_{CAMNAME}.ufmf"
        );
        // `--csv-save-dir` defaults to `~/DATA`, shell-expanded.
        assert!(!args.csv_save_dir.contains('~'));
        assert!(args.csv_save_dir.ends_with("DATA"));
        assert!(args.led_box_device_path.is_none());
        assert!(args.data_dir.is_none());
    }

    #[test]
    fn camera_name_standalone() {
        let s = parse_standalone(&["--camera-name", "Basler-1234"]);
        assert_eq!(s.camera_name.as_deref(), Some("Basler-1234"));
    }

    #[test]
    fn browser_flag_forces_browser() {
        let args = parse(&["--browser"]).unwrap();
        assert!(!args.no_browser);
    }

    #[test]
    fn no_browser_flag_prevents_browser() {
        let args = parse(&["--no-browser"]).unwrap();
        assert!(args.no_browser);
    }

    #[test]
    fn browser_and_no_browser_conflict() {
        assert!(parse(&["--browser", "--no-browser"]).is_err());
    }

    #[test]
    fn filename_templates_override() {
        let args = parse(&[
            "--mp4_filename_template",
            "a_{CAMNAME}.mp4",
            "--fmf_filename_template",
            "b_{CAMNAME}.fmf",
            "--ufmf_filename_template",
            "c_{CAMNAME}.ufmf",
        ])
        .unwrap();
        assert_eq!(args.mp4_filename_template, "a_{CAMNAME}.mp4");
        assert_eq!(args.fmf_filename_template, "b_{CAMNAME}.fmf");
        assert_eq!(args.ufmf_filename_template, "c_{CAMNAME}.ufmf");
    }

    #[test]
    fn camera_backend_accepts_known_values() {
        for backend in ["pylon", "vimba", "webcam", "sim"] {
            assert!(
                parse(&["--camera-backend", backend]).is_ok(),
                "backend {backend} should parse"
            );
        }
    }

    #[test]
    fn camera_backend_rejects_unknown_value() {
        assert!(parse(&["--camera-backend", "nonsense"]).is_err());
    }

    #[test]
    fn pixel_format_standalone() {
        let s = parse_standalone(&["--pixel-format", "Mono8"]);
        assert_eq!(s.pixel_format.as_deref(), Some("Mono8"));
    }

    #[test]
    fn force_camera_sync_mode_standalone() {
        let s = parse_standalone(&["--force_camera_sync_mode"]);
        assert!(s.force_camera_sync_mode);
        // Absent by default.
        let s = parse_standalone(&[]);
        assert!(!s.force_camera_sync_mode);
    }

    #[test]
    fn http_server_addr_standalone() {
        let s = parse_standalone(&["--http-server-addr", "127.0.0.1:8080"]);
        assert_eq!(s.http_server_addr.as_deref(), Some("127.0.0.1:8080"));
    }

    #[test]
    fn camera_settings_filename_standalone() {
        let s = parse_standalone(&["--camera-settings-filename", "/etc/cam.pfs"]);
        assert_eq!(
            s.camera_settings_filename,
            Some(PathBuf::from("/etc/cam.pfs"))
        );
    }

    #[test]
    fn csv_save_dir_override() {
        let args = parse(&["--csv-save-dir", "/tmp/strand-data"]).unwrap();
        assert_eq!(args.csv_save_dir, "/tmp/strand-data");
    }

    #[test]
    fn led_box_device() {
        let args = parse(&["--led-box", "/dev/ttyUSB0"]).unwrap();
        assert_eq!(args.led_box_device_path.as_deref(), Some("/dev/ttyUSB0"));
    }

    #[test]
    fn cookie_secret_from_cli() {
        let args = parse(&["--strand-cam-cookie-secret", "abc123"]).unwrap();
        assert_eq!(args.secret.as_deref(), Some("abc123"));
    }

    #[test]
    fn trusted_networks_split_and_appended() {
        // Comma-delimited within one occurrence, plus repeated occurrences.
        let args = parse(&[
            "--trusted-network",
            "100.64.0.0/10,10.0.0.0/8",
            "--trusted-network",
            "192.168.0.0/16",
        ])
        .unwrap();
        assert_eq!(
            args.trusted_networks,
            vec![
                "100.64.0.0/10".to_string(),
                "10.0.0.0/8".to_string(),
                "192.168.0.0/16".to_string(),
            ]
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn data_dir_and_v4l2loopback_derived() {
        let args = parse(&[
            "--data-dir",
            "/var/strand",
            "--v4l2loopback",
            "/dev/video10",
        ])
        .unwrap();
        assert_eq!(args.data_dir, Some(PathBuf::from("/var/strand")));
        assert_eq!(args.v4l2loopback, Some(PathBuf::from("/dev/video10")));
    }

    #[test]
    fn braid_url_selects_braid_mode() {
        let b = parse_braid(&[
            "--braid-url",
            "http://127.0.0.1:1234/",
            "--camera-name",
            "Basler-1",
        ]);
        assert_eq!(b.braid_url, "http://127.0.0.1:1234/");
        assert_eq!(b.camera_name, "Basler-1");
    }

    #[test]
    fn braid_defaults_to_no_browser() {
        let args = parse(&[
            "--braid-url",
            "http://127.0.0.1:1234/",
            "--camera-name",
            "Basler-1",
        ])
        .unwrap();
        assert!(args.no_browser);
    }

    #[test]
    fn braid_requires_camera_name() {
        assert!(parse(&["--braid-url", "http://127.0.0.1:1234/"]).is_err());
    }

    #[test]
    fn braid_conflicts_with_pixel_format() {
        let err = parse(&[
            "--braid-url",
            "http://127.0.0.1:1234/",
            "--camera-name",
            "Basler-1",
            "--pixel-format",
            "Mono8",
        ])
        .unwrap_err();
        assert!(err.to_string().contains("pixel_format"));
    }

    #[test]
    fn braid_conflicts_with_http_server_addr() {
        assert!(
            parse(&[
                "--braid-url",
                "http://127.0.0.1:1234/",
                "--camera-name",
                "Basler-1",
                "--http-server-addr",
                "127.0.0.1:8080",
            ])
            .is_err()
        );
    }

    #[test]
    fn braid_conflicts_with_force_camera_sync_mode() {
        assert!(
            parse(&[
                "--braid-url",
                "http://127.0.0.1:1234/",
                "--camera-name",
                "Basler-1",
                "--force_camera_sync_mode",
            ])
            .is_err()
        );
    }

    #[test]
    fn unknown_argument_is_an_error() {
        assert!(parse(&["--this-does-not-exist"]).is_err());
    }

    #[test]
    fn help_omits_struct_docstring() {
        // The `CliArgs` doc comment documents the type for developers but must
        // not appear as a command description in `--help` (it would be noise).
        let mut short = CliArgs::command();
        let mut long = CliArgs::command();
        let short = short.render_help().to_string();
        let long = long.render_long_help().to_string();
        for help in [&short, &long] {
            assert!(
                !help.contains("Intermediate"),
                "help text leaked the struct docstring:\n{help}"
            );
            assert!(
                !help.contains("flat representation"),
                "help text leaked the struct docstring:\n{help}"
            );
        }
        // Sanity check that we are actually rendering the real help.
        assert!(short.contains("--no-browser"));
    }
}
