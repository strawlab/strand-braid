#![cfg_attr(
    feature = "backtrace",
    feature(error_generic_member_access, provide_any)
)]

use serde::{Deserialize, Serialize};

use flydra_types::{BraidCameraConfig, FakeSyncConfig, TriggerType, TriggerboxConfig};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("lookup error on variable: {source}")]
    ShellExpandLookupVarError {
        #[from]
        source: shellexpand::LookupError<std::env::VarError>,
        #[cfg(feature = "backtrace")]
        backtrace: std::backtrace::Backtrace,
    },
    #[error("IO error: {source}")]
    IoError {
        #[from]
        source: std::io::Error,
        #[cfg(feature = "backtrace")]
        backtrace: std::backtrace::Backtrace,
    },
    #[error("TOML deserialization error: {source}")]
    TomlDeError {
        #[from]
        source: toml::de::Error,
        #[cfg(feature = "backtrace")]
        backtrace: std::backtrace::Backtrace,
    },
}

type Result<T> = std::result::Result<T, Error>;

fn default_lowlatency_camdata_udp_addr() -> String {
    "127.0.0.1:0".to_string()
}

fn default_http_api_server_addr() -> String {
    "127.0.0.1:0".to_string()
}

fn default_model_server_addr() -> std::net::SocketAddr {
    flydra_types::DEFAULT_MODEL_SERVER_ADDR.parse().unwrap()
}

fn default_true() -> bool {
    true
}

/// Split `path` (which must be a file) into directory and filename component.
fn split_path<P: AsRef<std::path::Path>>(path: P) -> (std::path::PathBuf, std::path::PathBuf) {
    let path = path.as_ref();
    assert!(path.is_file());
    let mut components = path.components();
    let filename = components.next_back().unwrap().as_os_str().into();
    let dirname = components.as_path().into();
    (dirname, filename)
}

/// If `path` is relative, make it relative to `dirname`.
///
/// `path` must be utf-8 encoded and can start with a tilde, which is expanded
/// to the home directory.
fn fixup_relative_path(path: &mut std::path::PathBuf, dirname: &std::path::Path) -> Result<()> {
    let pathstr = path.as_os_str().to_str().unwrap();
    let expanded = shellexpand::full(&pathstr)?;
    *path = std::path::PathBuf::from(expanded.to_string());

    if path.is_relative() {
        *path = dirname.join(&path);
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MainbrainConfig {
    /// Filename of the camera calibration, optional.
    ///
    /// Can contain shell variables such as `~`, `$A`, or `${B}`.
    ///
    /// If the filename ends with .pymvg or .json, it will be treated as a pymvg
    /// calibration file. Else it will be treated considered in the flydra XML
    /// calibration format.
    pub cal_fname: Option<std::path::PathBuf>,
    /// Directory where data should be saved. Can contain shell variables.
    pub output_base_dirname: std::path::PathBuf,
    /// Parameters for Kalman filter and data association
    #[serde(default = "flydra_types::default_tracking_params_full_3d")]
    pub tracking_params: flydra_types::TrackingParams,
    // Raising the mainbrain thread priority is currently disabled.
    // /// Parameters to potentially raise the mainbrain thread priority.
    // sched_policy_priority: Option<(i32, i32)>,
    /// Address of UDP port to send low-latency detection data
    #[serde(default = "default_lowlatency_camdata_udp_addr")]
    pub lowlatency_camdata_udp_addr: String,
    /// Address of HTTP port for control API
    #[serde(default = "default_http_api_server_addr")]
    pub http_api_server_addr: String,
    /// Token required to use HTTP port for control API
    pub http_api_server_token: Option<String>,
    /// Address of HTTP port for model server emitting realtime tracking results
    #[serde(default = "default_model_server_addr")]
    pub model_server_addr: std::net::SocketAddr,
    /// Save rows to data2d_distorted where nothing detected (saves timestamps)
    #[serde(default = "default_true")]
    pub save_empty_data2d: bool,
    /// Secret to use for JWT auth on HTTP port for control API
    pub jwt_secret: Option<String>,
    /// For debugging: filename to store captured packet data.
    pub packet_capture_dump_fname: Option<std::path::PathBuf>,
    /// Threshold duration before logging error (msec).
    ///
    /// If the image acquisition timestamp precedes the computed trigger
    /// timestamp, clearly an error has happened. This error must lie in the
    /// computation of the trigger timestamp. This specifies the threshold error
    /// at which an error is logged. (The underlying source of such errors
    /// remains unknown.)
    pub acquisition_duration_allowed_imprecision_msec: Option<f64>,
}

impl std::default::Default for MainbrainConfig {
    fn default() -> Self {
        Self {
            cal_fname: Some(std::path::PathBuf::from("/path/to/cal.xml")),
            output_base_dirname: std::path::PathBuf::from("/path/to/savedir"),
            tracking_params: flydra_types::default_tracking_params_full_3d(),
            // Raising the mainbrain thread priority is currently disabled.
            // sched_policy_priority: None,
            lowlatency_camdata_udp_addr: default_lowlatency_camdata_udp_addr(),
            http_api_server_addr: default_http_api_server_addr(),
            http_api_server_token: None,
            model_server_addr: default_model_server_addr(),
            save_empty_data2d: true,
            jwt_secret: None,
            packet_capture_dump_fname: None,
            acquisition_duration_allowed_imprecision_msec:
                flydra_types::DEFAULT_ACQUISITION_DURATION_ALLOWED_IMPRECISION_MSEC,
        }
    }
}

/// The new configuration format.
///
/// Backwards compatibility is maintained by first attempting to deserialize
/// with this definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BraidConfig2 {
    pub mainbrain: MainbrainConfig,
    /// Triggerbox configuration.
    #[serde(default)]
    pub trigger: TriggerType,
    pub cameras: Vec<BraidCameraConfig>,
}

impl From<BraidConfig1> for BraidConfig2 {
    fn from(orig: BraidConfig1) -> BraidConfig2 {
        let trigger = match orig.trigger {
            None => TriggerType::FakeSync(FakeSyncConfig::default()),
            Some(x) => TriggerType::TriggerboxV1(x),
        };
        BraidConfig2 {
            mainbrain: orig.mainbrain,
            trigger,
            cameras: orig.cameras,
        }
    }
}

/// The old configuration format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BraidConfig1 {
    pub mainbrain: MainbrainConfig,
    pub trigger: Option<TriggerboxConfig>,
    pub cameras: Vec<BraidCameraConfig>,
}

impl BraidConfig2 {
    /// For all paths which are relative, make them relative to the
    /// config file location.
    fn fixup_relative_paths(&mut self, orig_path: &std::path::Path) -> Result<()> {
        let (dirname, _orig_path) = split_path(orig_path);

        // fixup self.mainbrain.cal_fname
        if let Some(cal_fname) = self.mainbrain.cal_fname.as_mut() {
            fixup_relative_path(cal_fname, &dirname)?;
        }

        // fixup self.mainbrain.output_base_dirname
        fixup_relative_path(&mut self.mainbrain.output_base_dirname, &dirname)?;

        // fixup self.cameras.camera_settings_filename
        for camera_config in self.cameras.iter_mut() {
            if let Some(ref mut camera_settings_filename) =
                camera_config.camera_settings_filename.as_mut()
            {
                fixup_relative_path(camera_settings_filename, &dirname)?;
            }
        }

        Ok(())
    }
}

impl std::default::Default for BraidConfig2 {
    fn default() -> Self {
        Self {
            mainbrain: MainbrainConfig::default(),
            // This `trigger` field has a different default than
            // TriggerType::default() in order to show the user (who will query
            // this with the `braid default-config` command) how to configure
            // the trigger box.
            trigger: TriggerType::TriggerboxV1(TriggerboxConfig::default()),
            cameras: vec![
                BraidCameraConfig::default_absdiff_config("fake-camera-1".to_string()),
                BraidCameraConfig::default_absdiff_config("fake-camera-2".to_string()),
                BraidCameraConfig::default_absdiff_config("fake-camera-3".to_string()),
            ],
        }
    }
}

pub fn parse_config_file<P: AsRef<std::path::Path>>(fname: P) -> Result<BraidConfig2> {
    use std::io::Read;

    let mut file = std::fs::File::open(fname.as_ref())?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let mut cfg: BraidConfig2 = match toml::from_str(&contents) {
        Ok(cfg) => cfg,
        Err(err_cfg2) => {
            let cfg1: BraidConfig1 = match toml::from_str(&contents) {
                Ok(cfg1) => cfg1,
                Err(err_cfg1) => {
                    log::error!(
                        "parsing config file first as BraidConfig2 failed \
                    and then again as BraidConfig1 failed. The parse error for \
                    BraidConfig1 is: {}\n The original error when parsing \
                    BraidConfig2 will now be raised.",
                        err_cfg1
                    );
                    return Err(err_cfg2.into());
                }
            };
            BraidConfig2::from(cfg1)
        }
    };
    // let mut cfg: BraidConfig = toml::from_str(&contents)?;
    cfg.fixup_relative_paths(fname.as_ref())?;
    Ok(cfg)
}
