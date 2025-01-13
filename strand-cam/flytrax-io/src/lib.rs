use std::io::BufRead;

use eyre::{self as anyhow, Result};

use strand_cam_csv_config_types::FullCfgFview2_0_26;

pub fn read_csv_commented_header<R>(
    point_detection_csv_reader: &mut R,
) -> Result<FullCfgFview2_0_26>
where
    R: BufRead,
{
    enum ReadState {
        Initialized,
        FoundStartHeader,
        Reading(Vec<String>),
        Finished(std::result::Result<Vec<String>, anyhow::Error>),
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
                        // *self = ReadState::Finished(Err(anyhow::anyhow!("no header")));
                        ReadState::Finished(Ok(Vec::new()))
                    }
                }
                ReadState::FoundStartHeader => {
                    if line.starts_with('#') {
                        if let Some(stripped) = line.strip_prefix("# ") {
                            ReadState::Reading(vec![stripped.to_string()])
                        } else {
                            ReadState::Finished(Err(anyhow::anyhow!("unexpected line prefix")))
                        }
                    } else {
                        ReadState::Finished(Err(anyhow::anyhow!("premature end of headers")))
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
                            ReadState::Finished(Err(anyhow::anyhow!("unexpected line prefix")))
                        }
                    } else {
                        ReadState::Finished(Err(anyhow::anyhow!("premature end of headers")))
                    }
                }
                ReadState::Finished(_) => {
                    ReadState::Finished(Err(anyhow::anyhow!("parsing after finish")))
                }
                ReadState::Marker => {
                    ReadState::Finished(Err(anyhow::anyhow!("parsing while parsing")))
                }
            };
            *self = next;
        }
        fn finish(self) -> std::result::Result<Vec<String>, anyhow::Error> {
            if let ReadState::Finished(rv) = self {
                rv
            } else {
                Err(anyhow::anyhow!("premature end of header"))
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
    FullCfgFview2_0_25(strand_cam_csv_config_types::FullCfgFview2_0_25),
    FullCfgFview2_0_26(strand_cam_csv_config_types::FullCfgFview2_0_26),
}

impl StrandCamConfig {
    fn from_value(cfg: serde_yaml::Value) -> Result<StrandCamConfig> {
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

fn config25_upgrade(
    orig: strand_cam_csv_config_types::FullCfgFview2_0_25,
) -> strand_cam_csv_config_types::FullCfgFview2_0_26 {
    strand_cam_csv_config_types::FullCfgFview2_0_26 {
        app: orig.app,
        camera: strand_cam_csv_config_types::CameraCfgFview2_0_26 {
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
