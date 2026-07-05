// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::path::PathBuf;

use clap::{CommandFactory, FromArgMatches, Parser, ValueEnum};

use crate::{BraidArgs, StandaloneArgs, StandaloneOrBraid, StrandCamArgs, run_strand_cam_app};

use crate::APP_INFO;

use eyre::{Result, WrapErr, eyre};

/// Which camera vendor backend the merged Strand Camera binary should load.
///
/// The [`ValueEnum`] value names (`pylon`, `vimba`, `webcam`, `sim`) are the
/// strings accepted by `--camera-backend`, and match
/// [`braid_types::StartCameraBackend::camera_backend_arg`], which is how Braid
/// asks `strand-cam` for a particular backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum CameraBackend {
    /// Basler Pylon backend (`ci2-pylon`).
    #[default]
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

/// One-time process setup shared by every backend.
fn init_process() {
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
}

/// Build the `clap` command, using `app_name` as the displayed program name.
///
/// The derive default would use the crate name; the merged binary is invoked
/// under different names (e.g. `strand-cam-pylon`), so the runtime value is
/// substituted here.
fn command(app_name: &str) -> clap::Command {
    CliArgs::command().name(app_name.to_string())
}

/// Parse the process arguments for the binary entry point.
///
/// On `--help`, `--version`, or a usage error, `clap` prints the appropriate
/// message and exits with the conventional status code (the standard behavior
/// for a command-line program).
fn parse_cli(app_name: &str) -> CliArgs {
    let matches = command(app_name).get_matches();
    CliArgs::from_arg_matches(&matches).unwrap_or_else(|err| err.exit())
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
    init_process();

    let cli = parse_cli(app_name);

    match cli.camera_backend {
        CameraBackend::Pylon => {
            let module: &'static ci2_pylon::WrappedModule =
                Box::leak(Box::new(ci2_pylon::new_module()?));
            let guard = ci2_pylon::make_singleton_guard(&module)?;
            let mymod = ci2_async::into_threaded_async(module, &guard);
            cli_main(mymod, cli, app_name)?;
        }
        CameraBackend::Vimba => {
            let module: &'static ci2_vimba::WrappedModule =
                Box::leak(Box::new(ci2_vimba::new_module()?));
            let guard = ci2_vimba::make_singleton_guard(&module)?;
            let mymod = ci2_async::into_threaded_async(module, &guard);
            cli_main(mymod, cli, app_name)?;
        }
        CameraBackend::Webcam => {
            let module: &'static ci2_webcam::WrappedModule =
                Box::leak(Box::new(ci2_webcam::new_module()?));
            let guard = ci2_webcam::make_singleton_guard(&module)?;
            let mymod = ci2_async::into_threaded_async(module, &guard);
            cli_main(mymod, cli, app_name)?;
        }
        CameraBackend::Sim => {
            let module: &'static ci2_sim::WrappedModule =
                Box::leak(Box::new(ci2_sim::new_module()?));
            let guard = ci2_sim::make_singleton_guard(&module)?;
            let mymod = ci2_async::into_threaded_async(module, &guard);
            cli_main(mymod, cli, app_name)?;
        }
    }
    Ok(())
}

/// Run the Strand Camera application for an already-constructed camera module.
///
/// The command line is parsed by [cli_main_dispatch] (which needs the selected
/// backend before it can build `mymod`) and handed in as `cli`.
pub fn cli_main<M, C, G>(
    mymod: ci2_async::ThreadedAsyncCameraModule<M, C, G>,
    cli: CliArgs,
    app_name: &'static str,
) -> Result<ci2_async::ThreadedAsyncCameraModule<M, C, G>>
where
    M: ci2::CameraModule<CameraType = C, Guard = G> + 'static,
    C: 'static + ci2::Camera + Send,
    G: Send + 'static,
{
    // Enumerate cameras and exit, without launching the application or opening a
    // browser.
    if cli.list_cameras {
        return list_cameras(&mymod).map(|()| mymod);
    }

    let args = cli
        .into_strand_cam_args()
        .with_context(|| "interpreting command-line arguments".to_string())?;

    run_strand_cam_app(mymod, args, app_name)
}

fn get_tracker_cfg() -> Result<crate::ImPtDetectCfgSource> {
    let ai = (&APP_INFO, "object-detection".to_string());
    let tracker_cfg_src = crate::ImPtDetectCfgSource::ChangedSavedToDisk(ai);
    Ok(tracker_cfg_src)
}

/// The Strand Camera command line.
///
/// This is a flat, one-field-per-argument view of the command line, parsed by
/// `clap`. [`CliArgs::into_strand_cam_args`] turns it into the richer
/// [`StrandCamArgs`] consumed by the rest of the application, applying the
/// standalone-vs-Braid rules and filling in feature-gated fields.
///
/// The doc comment is intentionally kept out of `--help` (see the
/// `about`/`long_about` reset on the `command` attribute below): the program
/// needs no top-level description beyond its usage line.
#[derive(Parser, Debug)]
#[command(version, about = None, long_about = None)]
pub struct CliArgs {
    /// Force auto-opening of the browser.
    #[arg(long)]
    browser: bool,

    /// Prevent auto-opening of the browser.
    #[arg(long, conflicts_with = "browser")]
    no_browser: bool,

    /// Initial filename template for saved `.mp4` recordings.
    #[arg(long, default_value = crate::MP4_FILENAME_TEMPLATE_DEFAULT)]
    mp4_filename_template: String,

    /// Initial filename template for saved `.fmf` recordings.
    #[arg(long, default_value = crate::FMF_FILENAME_TEMPLATE_DEFAULT)]
    fmf_filename_template: String,

    /// Initial filename template for saved `.ufmf` recordings.
    #[arg(long, default_value = crate::UFMF_FILENAME_TEMPLATE_DEFAULT)]
    ufmf_filename_template: String,

    /// The name of the desired camera.
    #[arg(long)]
    camera_name: Option<String>,

    /// Which camera backend library to load. Only meaningful for the merged
    /// Strand Camera binary that supports multiple backends.
    #[arg(long, default_value_t, value_enum)]
    camera_backend: CameraBackend,

    /// List the cameras available for the selected backend and exit, without
    /// launching the application or opening a browser.
    #[arg(long)]
    list_cameras: bool,

    /// Path to a file with camera settings which will be loaded.
    #[arg(long)]
    camera_settings_filename: Option<PathBuf>,

    /// The socket address (`IP:PORT`) on which to serve the HTTP user
    /// interface.
    ///
    /// Both IPv4 (e.g. `192.168.1.10:3440`) and IPv6 (e.g.
    /// `[2001:db8::1]:3440`) addresses are accepted; an IPv6 address must be
    /// enclosed in square brackets. Giving a non-localhost IP address makes
    /// Strand Camera available remotely on the network. Remote clients must
    /// present an access token to connect. Using the unspecified IP address
    /// (`0.0.0.0:3440` for IPv4 or `[::]:3440` for IPv6) exposes the server on
    /// all network interfaces. Using port `0` (e.g. `127.0.0.1:0`) lets the
    /// operating system pick a free port.
    ///
    /// When not set, defaults to `127.0.0.1:3440`. This must not be set when
    /// running under Braid, which supplies the address via its per-camera
    /// configuration.
    #[arg(long)]
    http_server_addr: Option<String>,

    /// The directory in which to save CSV data files.
    #[arg(long, default_value = "~/DATA")]
    csv_save_dir: String,

    /// The desired pixel format. (incompatible with braid).
    #[arg(long)]
    pixel_format: Option<String>,

    /// The secret (base64 encoded) for signing HTTP cookies.
    #[arg(long, env = "STRAND_CAM_COOKIE_SECRET")]
    strand_cam_cookie_secret: Option<String>,

    /// A client network (CIDR, e.g. 100.64.0.0/10) trusted to have already
    /// authenticated the peer (e.g. Tailscale/WireGuard). Clients from it need
    /// no access token. May be given multiple times.
    #[arg(
        long = "trusted-network",
        value_name = "TRUSTED_NETWORK",
        env = "STRAND_CAM_TRUSTED_NETWORKS",
        value_delimiter = ','
    )]
    trusted_networks: Vec<String>,

    /// Force the camera to synchronize to an external trigger. (incompatible with braid).
    #[arg(long)]
    force_camera_sync_mode: bool,

    /// Braid HTTP URL address (e.g. 'http://host:port/').
    #[arg(long)]
    braid_url: Option<String>,

    /// The filename of the LED box device.
    #[arg(long = "led-box")]
    led_box_device: Option<String>,

    /// Filename of a flydra `.xml` camera calibration.
    #[cfg(feature = "flydratrax")]
    #[arg(long)]
    camera_xml_calibration: Option<String>,

    /// Filename of a pymvg `.json` camera calibration.
    #[cfg(feature = "flydratrax")]
    #[arg(long)]
    camera_pymvg_calibration: Option<String>,

    /// Do not save `data2d_distorted` rows when no detections are found.
    #[cfg(feature = "flydratrax")]
    #[arg(long)]
    no_save_empty_data2d: bool,

    /// The socket address of the model server.
    #[cfg(feature = "flydratrax")]
    #[arg(long, default_value = braid_types::DEFAULT_MODEL_SERVER_ADDR)]
    model_server_addr: std::net::SocketAddr,

    /// If set, output a copy of the video stream on this v4l2 device (e.g. `/dev/video0`).
    #[cfg(target_os = "linux")]
    #[arg(long)]
    v4l2loopback: Option<PathBuf>,

    /// If set, `.mp4` videos and log files are saved to this directory.
    #[arg(long)]
    data_dir: Option<PathBuf>,
}

impl CliArgs {
    /// Translate the parsed command line into the application's [StrandCamArgs].
    ///
    /// This applies the rules that `clap` alone cannot express: the
    /// standalone-vs-Braid split selected by `--braid-url`, the arguments that
    /// are forbidden under Braid, and the per-mode default for auto-opening the
    /// browser.
    fn into_strand_cam_args(self) -> Result<StrandCamArgs> {
        let standalone_or_braid = if let Some(braid_url) = self.braid_url {
            // Under Braid these are either irrelevant or supplied via
            // [braid_types::RemoteCameraInfoResponse], so rejecting them keeps
            // the configuration unambiguous.
            for (flag, is_set) in [
                ("--pixel-format", self.pixel_format.is_some()),
                (
                    "--strand-cam-cookie-secret",
                    self.strand_cam_cookie_secret.is_some(),
                ),
                (
                    "--camera-settings-filename",
                    self.camera_settings_filename.is_some(),
                ),
                ("--http-server-addr", self.http_server_addr.is_some()),
                ("--force-camera-sync-mode", self.force_camera_sync_mode),
            ] {
                if is_set {
                    eyre::bail!(
                        "{flag} cannot be set on the command line when running under Braid"
                    );
                }
            }

            let camera_name = self
                .camera_name
                .ok_or_else(|| eyre!("--camera-name must be set when running under Braid"))?;

            StandaloneOrBraid::Braid(BraidArgs {
                braid_url,
                camera_name,
            })
        } else {
            let tracker_cfg_src = get_tracker_cfg()?;

            #[cfg(not(feature = "flydra_feat_detect"))]
            let _ = tracker_cfg_src; // This is unused without `flydra_feat_detect` feature.

            StandaloneOrBraid::Standalone(StandaloneArgs {
                camera_name: self.camera_name,
                pixel_format: self.pixel_format,
                force_camera_sync_mode: self.force_camera_sync_mode,
                software_limit_framerate: braid_types::StartSoftwareFrameRateLimit::NoChange,
                acquisition_duration_allowed_imprecision_msec:
                    braid_types::DEFAULT_ACQUISITION_DURATION_ALLOWED_IMPRECISION_MSEC,
                camera_settings_filename: self.camera_settings_filename,
                #[cfg(feature = "flydra_feat_detect")]
                tracker_cfg_src,
                http_server_addr: self.http_server_addr,
            })
        };

        // `--browser`/`--no-browser` override a per-mode default: standalone
        // opens the browser, Braid does not.
        let no_browser = if self.no_browser {
            true
        } else if self.browser {
            false
        } else {
            matches!(standalone_or_braid, StandaloneOrBraid::Braid(_))
        };

        let csv_save_dir = shellexpand::full(&self.csv_save_dir)
            .map_err(|e| eyre!("{e}"))?
            .into_owned();

        #[cfg(feature = "flydratrax")]
        let flydratrax_calibration_source =
            match (self.camera_xml_calibration, self.camera_pymvg_calibration) {
                (None, None) => crate::CalSource::PseudoCal,
                (Some(xml), None) => crate::CalSource::XmlFile(PathBuf::from(xml)),
                (None, Some(json)) => crate::CalSource::PymvgJsonFile(PathBuf::from(json)),
                (Some(_), Some(_)) => {
                    eyre::bail!("Can only specify xml or pymvg calibration, not both.");
                }
            };

        #[cfg(feature = "fiducial")]
        let apriltag_csv_filename_template =
            strand_cam_storetype::APRILTAG_CSV_TEMPLATE_DEFAULT.to_string();

        Ok(StrandCamArgs {
            standalone_or_braid,
            secret: self.strand_cam_cookie_secret,
            trusted_networks: self.trusted_networks,
            no_browser,
            mp4_filename_template: self.mp4_filename_template,
            fmf_filename_template: self.fmf_filename_template,
            ufmf_filename_template: self.ufmf_filename_template,
            csv_save_dir,
            led_box_device_path: self.led_box_device,
            #[cfg(feature = "flydratrax")]
            flydratrax_calibration_source,
            #[cfg(feature = "flydratrax")]
            save_empty_data2d: !self.no_save_empty_data2d,
            #[cfg(feature = "flydratrax")]
            model_server_addr: self.model_server_addr,
            #[cfg(feature = "fiducial")]
            apriltag_csv_filename_template,
            #[cfg(target_os = "linux")]
            v4l2loopback: self.v4l2loopback,
            data_dir: self.data_dir,
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    //! Tests pinning the command-line argument parsing behavior, covering both
    //! `clap` parsing (`CliArgs`) and the translation into [StrandCamArgs].
    //!
    //! These run under the crate's default features
    //! (`flydra_feat_detect`, `bundle_files`), so the `flydratrax`- and
    //! `fiducial`-gated arguments are not exercised here.
    use super::*;

    /// Parse `args` (without the leading program name) into [CliArgs].
    fn parse_cli_args(args: &[&str]) -> Result<CliArgs> {
        let argv: Vec<String> = std::iter::once("strand-cam")
            .chain(args.iter().copied())
            .map(String::from)
            .collect();
        let matches = command("strand-cam").try_get_matches_from(argv)?;
        Ok(CliArgs::from_arg_matches(&matches)?)
    }

    /// Parse `args` and translate them into [StrandCamArgs].
    fn parse(args: &[&str]) -> Result<StrandCamArgs> {
        parse_cli_args(args)?.into_strand_cam_args()
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
    fn camera_backend_defaults_to_pylon() {
        assert_eq!(
            parse_cli_args(&[]).unwrap().camera_backend,
            CameraBackend::Pylon
        );
    }

    #[test]
    fn camera_backend_parses_known_values() {
        for (text, expected) in [
            ("pylon", CameraBackend::Pylon),
            ("vimba", CameraBackend::Vimba),
            ("webcam", CameraBackend::Webcam),
            ("sim", CameraBackend::Sim),
        ] {
            assert_eq!(
                parse_cli_args(&["--camera-backend", text])
                    .unwrap()
                    .camera_backend,
                expected
            );
        }
    }

    #[test]
    fn camera_backend_rejects_unknown_value() {
        assert!(parse_cli_args(&["--camera-backend", "nonsense"]).is_err());
    }

    #[test]
    fn list_cameras_flag() {
        assert!(parse_cli_args(&["--list-cameras"]).unwrap().list_cameras);
        assert!(!parse_cli_args(&[]).unwrap().list_cameras);
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
            "--mp4-filename-template",
            "a_{CAMNAME}.mp4",
            "--fmf-filename-template",
            "b_{CAMNAME}.fmf",
            "--ufmf-filename-template",
            "c_{CAMNAME}.ufmf",
        ])
        .unwrap();
        assert_eq!(args.mp4_filename_template, "a_{CAMNAME}.mp4");
        assert_eq!(args.fmf_filename_template, "b_{CAMNAME}.fmf");
        assert_eq!(args.ufmf_filename_template, "c_{CAMNAME}.ufmf");
    }

    #[test]
    fn pixel_format_standalone() {
        let s = parse_standalone(&["--pixel-format", "Mono8"]);
        assert_eq!(s.pixel_format.as_deref(), Some("Mono8"));
    }

    #[test]
    fn force_camera_sync_mode_standalone() {
        let s = parse_standalone(&["--force-camera-sync-mode"]);
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
    fn data_dir_and_v4l2loopback() {
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
        assert!(err.to_string().contains("--pixel-format"));
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
                "--force-camera-sync-mode",
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
        let short = command("strand-cam").render_help().to_string();
        let long = command("strand-cam").render_long_help().to_string();
        for help in [&short, &long] {
            assert!(
                !help.contains("flat, one-field-per-argument"),
                "help text leaked the struct docstring:\n{help}"
            );
        }
        // Sanity check that we are actually rendering the real help.
        assert!(short.contains("--no-browser"));
    }
}
