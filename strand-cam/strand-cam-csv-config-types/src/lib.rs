//! support YAML frontmatter in .csv files saved by Strand Camera

use serde::{Deserialize, Serialize};
use std::io::BufRead;

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

pub fn read_csv_commented_header<R>(
    point_detection_csv_reader: &mut R,
) -> eyre::Result<FullCfgFview2_0_26>
where
    R: BufRead,
{
    enum ReadState {
        Initialized,
        FoundStartHeader,
        Reading(Vec<String>),
        Finished(eyre::Result<Vec<String>>),
        Marker,
    }
    impl ReadState {
        fn parse(&mut self, line1: &str) {
            let line = remove_trailing_newline(line1);
            let mut old = ReadState::Marker;
            std::mem::swap(self, &mut old);
            let next: ReadState = match old {
                ReadState::Initialized => {
                    if line.starts_with('#') {
                        if line == "# -- start of yaml config --" {
                            ReadState::FoundStartHeader
                        } else {
                            ReadState::Initialized
                        }
                    } else {
                        // *self = ReadState::Finished(Err(eyre::eyre!("no header")));
                        ReadState::Finished(Ok(Vec::new()))
                    }
                }
                ReadState::FoundStartHeader => {
                    if line.starts_with('#') {
                        if let Some(stripped) = line.strip_prefix("# ") {
                            ReadState::Reading(vec![stripped.to_string()])
                        } else {
                            ReadState::Finished(Err(eyre::eyre!("unexpected line prefix")))
                        }
                    } else {
                        ReadState::Finished(Err(eyre::eyre!("premature end of headers")))
                    }
                }
                ReadState::Reading(mut vec_lines) => {
                    if line.starts_with('#') {
                        if let Some(stripped) = line.strip_prefix("# ") {
                            if line == "# -- end of yaml config --" {
                                ReadState::Finished(Ok(vec_lines))
                            } else {
                                vec_lines.push(stripped.to_string());
                                ReadState::Reading(vec_lines)
                            }
                        } else {
                            ReadState::Finished(Err(eyre::eyre!("unexpected line prefix")))
                        }
                    } else {
                        ReadState::Finished(Err(eyre::eyre!("premature end of headers")))
                    }
                }
                ReadState::Finished(_) => {
                    ReadState::Finished(Err(eyre::eyre!("parsing after finish")))
                }
                ReadState::Marker => ReadState::Finished(Err(eyre::eyre!("parsing while parsing"))),
            };
            *self = next;
        }
        fn finish(self) -> std::result::Result<Vec<String>, eyre::Error> {
            if let ReadState::Finished(rv) = self {
                rv
            } else {
                Err(eyre::eyre!("premature end of header"))
            }
        }
    }

    let mut state = ReadState::Initialized;
    let mut this_line = String::new();
    loop {
        point_detection_csv_reader.read_line(&mut this_line)?;
        state.parse(&this_line);
        this_line.clear();
        if let ReadState::Finished(_) = &state {
            break;
        }
    }

    let header_lines = state.finish()?;
    let header = header_lines.join("\n");
    let yaml = serde_yaml::from_str(&header)?;
    Ok(StrandCamConfig::from_value(yaml)?.into_latest())
}

pub enum StrandCamConfig {
    FullCfgFview2_0_25(FullCfgFview2_0_25),
    FullCfgFview2_0_26(FullCfgFview2_0_26),
}

impl StrandCamConfig {
    fn from_value(cfg: serde_yaml::Value) -> eyre::Result<StrandCamConfig> {
        match serde_yaml::from_value(cfg.clone()) {
            Ok(cfg26) => Ok(StrandCamConfig::FullCfgFview2_0_26(cfg26)),
            Err(err26) => {
                if let Ok(cfg25) = serde_yaml::from_value(cfg) {
                    Ok(StrandCamConfig::FullCfgFview2_0_25(cfg25))
                } else {
                    // Return parse error for latest version
                    Err(err26.into())
                }
            }
        }
    }

    fn into_latest(self) -> FullCfgFview2_0_26 {
        match self {
            StrandCamConfig::FullCfgFview2_0_25(cfg25) => config25_upgrade(cfg25),
            StrandCamConfig::FullCfgFview2_0_26(cfg26) => cfg26,
        }
    }
}

fn config25_upgrade(orig: FullCfgFview2_0_25) -> FullCfgFview2_0_26 {
    FullCfgFview2_0_26 {
        app: orig.app,
        camera: CameraCfgFview2_0_26 {
            vendor: "default vendor".to_string(),
            model: "default model".to_string(),
            serial: "default serial".to_string(),
            width: 1280,
            height: 1024,
        },
        created_at: orig.created_at,
        csv_rate_limit: orig.csv_rate_limit,
        object_detection_cfg: orig.object_detection_cfg,
    }
}

fn remove_trailing_newline(line1: &str) -> &str {
    if let Some(stripped) = line1.strip_suffix('\n') {
        stripped
    } else {
        line1
    }
}
