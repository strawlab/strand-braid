use serde::{Deserialize, Serialize};

use anyhow::Result;

use flydra_types::{FakeSyncConfig, TriggerType, TriggerboxConfig};
use image_tracker_types::ImPtDetectCfg;

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

fn default_3d_tracking_params() -> flydra_types::TrackingParams {
    flydra_types::TrackingParamsInner3D::default().into()
}

pub fn braid_start(name: &str) -> Result<()> {
    dotenv::dotenv().ok();

    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "braid=info,flydra2=info,flydra2_mainbrain=info,strand_cam=info,image_tracker=info,rt_image_viewer=info,flydra1_triggerbox=info,error");
    }

    env_tracing_logger::init();

    let version = format!("{} (git {})", env!("CARGO_PKG_VERSION"), env!("GIT_HASH"));
    log::info!("{} {}", name, version);
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MainbrainConfig {
    /// Filename of the camera calibration, optional. Can contain shell variables.
    pub cal_fname: Option<std::path::PathBuf>,
    /// Directory where data should be saved. Can contain shell variables.
    pub output_base_dirname: std::path::PathBuf,
    /// Parameters for Kalman filter and data association
    #[serde(default = "default_3d_tracking_params")]
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
}

impl std::default::Default for MainbrainConfig {
    fn default() -> Self {
        Self {
            cal_fname: Some(std::path::PathBuf::from("/path/to/cal.xml")),
            output_base_dirname: std::path::PathBuf::from("/path/to/savedir"),
            tracking_params: default_3d_tracking_params(),
            // Raising the mainbrain thread priority is currently disabled.
            // sched_policy_priority: None,
            lowlatency_camdata_udp_addr: default_lowlatency_camdata_udp_addr(),
            http_api_server_addr: default_http_api_server_addr(),
            http_api_server_token: None,
            model_server_addr: default_model_server_addr(),
            save_empty_data2d: true,
            jwt_secret: None,
        }
    }
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
        match self.mainbrain.cal_fname.as_mut() {
            Some(cal_fname) => {
                fixup_relative_path(cal_fname, &dirname)?;
            }
            None => {}
        }

        // fixup self.mainbrain.output_base_dirname
        fixup_relative_path(&mut self.mainbrain.output_base_dirname, &dirname)?;

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

fn return_false() -> bool {
    false
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BraidCameraConfig {
    /// The name of the camera (e.g. "Basler-22005677")
    pub name: String,
    /// The pixel format to use.
    pub pixel_format: Option<String>,
    /// Configuration for detecting points.
    #[serde(default = "im_pt_detect_config::default_absdiff")]
    pub point_detection_config: ImPtDetectCfg,
    /// Whether to raise the priority of the grab thread.
    #[serde(default = "return_false")]
    pub raise_grab_thread_priority: bool,
}

impl BraidCameraConfig {
    fn default_absdiff_config(name: String) -> Self {
        Self {
            name,
            pixel_format: None,
            point_detection_config: im_pt_detect_config::default_absdiff(),
            raise_grab_thread_priority: false,
        }
    }
}

pub fn parse_config_file(fname: &std::path::Path) -> Result<BraidConfig2> {
    use std::io::Read;

    let mut file = std::fs::File::open(&fname)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let mut cfg: BraidConfig2 = match toml::from_str(&contents) {
        Ok(cfg) => cfg,
        Err(err1) => {
            let cfg1: BraidConfig1 = match toml::from_str(&contents) {
                Ok(cfg1) => cfg1,
                Err(_err2) => {
                    return Err(err1.into());
                }
            };
            BraidConfig2::from(cfg1)
        }
    };
    // let mut cfg: BraidConfig = toml::from_str(&contents)?;
    cfg.fixup_relative_paths(&fname)?;
    Ok(cfg)
}
