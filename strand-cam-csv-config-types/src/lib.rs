use serde::{Deserialize, Serialize};

// ---- strand-cam csv yaml configuration header -----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveCfgFview2_0_25 {
    pub name: String,
    pub version: String,
    pub git_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullCfgFview2_0_25 {
    pub app: SaveCfgFview2_0_25,
    pub created_at: chrono::DateTime<chrono::Local>,
    pub csv_rate_limit: Option<f32>,
    pub object_detection_cfg: flydra_feature_detector_types::ImPtDetectCfg,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraCfgFview2_0_26 {
    pub vendor: String,
    pub model: String,
    pub serial: String,
    pub width: u32,
    pub height: u32,
}

// TODO: also have flydratrax variant which saves flydra tracking params
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullCfgFview2_0_26 {
    pub app: SaveCfgFview2_0_25,
    pub camera: CameraCfgFview2_0_26,
    pub created_at: chrono::DateTime<chrono::Local>,
    pub csv_rate_limit: Option<f32>,
    pub object_detection_cfg: flydra_feature_detector_types::ImPtDetectCfg,
}
