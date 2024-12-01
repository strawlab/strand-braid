use serde::{Deserialize, Serialize};

use flydra_types::{BraidCameraConfig, FakeSyncConfig, TriggerType, TriggerboxConfig};

/// The Braid configuration error type.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("lookup error on variable: {source}")]
    ShellExpandLookupVarError {
        #[from]
        source: shellexpand::LookupError<std::env::VarError>,

    },
    #[error("IO error: {source}")]
    IoError {
        #[from]
        source: std::io::Error,

    },
    #[error("TOML deserialization error: {source}")]
    TomlDeError {
        #[from]
        source: toml::de::Error,

    },
}

type Result<T> = std::result::Result<T, Error>;

/// The default value for [MainbrainConfig::http_api_server_addr].
pub const DEFAULT_HTTP_API_SERVER_ADDR: &str = "127.0.0.1:0";

fn default_http_api_server_addr() -> String {
    DEFAULT_HTTP_API_SERVER_ADDR.to_string()
}

/// The default value for [MainbrainConfig::output_base_dirname].
pub const DEFAULT_OUTPUT_BASE_DIRNAME: &str = "~/BRAID-DATA";

fn default_output_base_dirname() -> std::path::PathBuf {
    DEFAULT_OUTPUT_BASE_DIRNAME.into()
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

/// The sub-configuration of [BraidConfig] for `mainbrain` - the central
/// component of Braid that integrates information from multiple cameras and
/// performs tracking.
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
    /// Defaults to [DEFAULT_OUTPUT_BASE_DIRNAME].
    #[serde(default = "default_output_base_dirname")]
    pub output_base_dirname: std::path::PathBuf,
    /// Parameters for Kalman filter and data association
    #[serde(default = "flydra_types::default_tracking_params_full_3d")]
    pub tracking_params: flydra_types::TrackingParams,
    // Raising the mainbrain thread priority is currently disabled.
    // /// Parameters to potentially raise the mainbrain thread priority.
    // sched_policy_priority: Option<(i32, i32)>,
    /// Address of UDP port to send low-latency detection data
    pub lowlatency_camdata_udp_addr: Option<String>,
    #[serde(default)]
    pub lowlatency_camdata_udp_port: u16,
    /// Address of HTTP port for control API. This is specified in the format
    /// `IP:PORT` where:
    ///
    /// `IP` can be:
    ///  - a numerical IPv4 address:
    ///    - e.g. `1.1.1.1` uses the specific IP
    ///    - `127.0.0.1` for the loopback interface
    ///    - `0.0.0.0` to open the server on all available IPv4 interfaces
    ///  - a numerical IPv6 address:
    ///    - e.g. `[2001:db8:3333:4444:5555:6666:7777:8888]` uses the specific
    ///      IP
    ///    - `[::1]` for the loopback interface
    ///    - `[::]` to open the server on all available IPv6 interfaces
    ///  - a hostname which resolves to an IP address (depending on your DNS
    ///    configuration, resolves to either IPv4 or IPv6):
    ///    - `localhost` resolves to the IP address of the loopback interface
    ///    - e.g. `hostname` for a specific IP address
    ///
    /// `PORT` can be:
    ///  - `0` allows the operating system to choose an unassigned port
    ///    dynamically
    ///  - e.g. `1234` uses the specific port
    ///
    /// Set to `0.0.0.0:0` to be automatically assigned a public IP address with
    /// a dynamically assigned port.
    ///
    /// The default value is set to [DEFAULT_HTTP_API_SERVER_ADDR].
    #[serde(default = "default_http_api_server_addr")]
    pub http_api_server_addr: String,
    /// Address of HTTP port for model server emitting realtime tracking results
    #[serde(default = "default_model_server_addr")]
    pub model_server_addr: std::net::SocketAddr,
    /// Save rows to data2d_distorted where nothing detected (saves timestamps)
    #[serde(default = "default_true")]
    pub save_empty_data2d: bool,
    /// Secret to use for signing HTTP cookies (base64 encoded)
    pub secret_base64: Option<String>,
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
    /// The size of the buffer, in number of messages, used by the channel for
    /// sending data to disk.
    #[serde(default = "default_write_buffer_size_num_messages")]
    pub write_buffer_size_num_messages: usize,
}

impl std::default::Default for MainbrainConfig {
    fn default() -> Self {
        Self {
            cal_fname: None,
            output_base_dirname: default_output_base_dirname(),
            tracking_params: flydra_types::default_tracking_params_full_3d(),
            // Raising the mainbrain thread priority is currently disabled.
            // sched_policy_priority: None,
            lowlatency_camdata_udp_addr: None,
            lowlatency_camdata_udp_port: Default::default(),
            http_api_server_addr: default_http_api_server_addr(),
            model_server_addr: default_model_server_addr(),
            save_empty_data2d: true,
            secret_base64: None,
            packet_capture_dump_fname: None,
            acquisition_duration_allowed_imprecision_msec:
                flydra_types::DEFAULT_ACQUISITION_DURATION_ALLOWED_IMPRECISION_MSEC,
            write_buffer_size_num_messages: default_write_buffer_size_num_messages(),
        }
    }
}

pub const fn default_write_buffer_size_num_messages() -> usize {
    10000
}

/// The Braid configuration format used in [the Braid configuration `TOML`
/// file](https://strawlab.github.io/strand-braid/braid_configuration_and_launching.html).
///
/// This is the new configuration format. Backwards compatibility is maintained
/// by first attempting to deserialize with this definition.
///
/// See the types of each field for sub-configuration values.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BraidConfig {
    /// Mainbrain configuration
    #[serde(default = "MainbrainConfig::default")]
    pub mainbrain: MainbrainConfig,
    /// Triggerbox configuration.
    #[serde(default)]
    pub trigger: TriggerType,
    pub cameras: Vec<BraidCameraConfig>,
}

impl From<BraidConfig1> for BraidConfig {
    fn from(orig: BraidConfig1) -> BraidConfig {
        let trigger = match orig.trigger {
            None => TriggerType::FakeSync(FakeSyncConfig::default()),
            Some(x) => TriggerType::TriggerboxV1(x),
        };
        BraidConfig {
            mainbrain: orig.mainbrain,
            trigger,
            cameras: orig.cameras,
        }
    }
}

/// The old configuration format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[doc(hidden)]
pub struct BraidConfig1 {
    pub mainbrain: MainbrainConfig,
    pub trigger: Option<TriggerboxConfig>,
    pub cameras: Vec<BraidCameraConfig>,
}

impl BraidConfig {
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

impl std::default::Default for BraidConfig {
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

/// Parse a `.toml` file and return a [BraidConfig] structure.
pub fn parse_config_file<P: AsRef<std::path::Path>>(fname: P) -> Result<BraidConfig> {
    use std::io::Read;

    let mut file = std::fs::File::open(fname.as_ref())?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let mut cfg: BraidConfig = match toml::from_str(&contents) {
        Ok(cfg) => cfg,
        Err(err_cfg2) => {
            let cfg1: BraidConfig1 = match toml::from_str(&contents) {
                Ok(cfg1) => cfg1,
                Err(err_cfg1) => {
                    log::error!(
                        "parsing config file first as BraidConfig failed \
                    and then again as BraidConfig1 failed. The parse error for \
                    BraidConfig1 is: {}\n The original error when parsing \
                    BraidConfig will now be raised.",
                        err_cfg1
                    );
                    return Err(err_cfg2.into());
                }
            };
            BraidConfig::from(cfg1)
        }
    };
    // let mut cfg: BraidConfig = toml::from_str(&contents)?;
    cfg.fixup_relative_paths(fname.as_ref())?;
    Ok(cfg)
}
