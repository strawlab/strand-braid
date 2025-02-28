use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AprilConfig {
    pub created_at: chrono::DateTime<chrono::Local>,
    pub camera_name: String,
    pub camera_width_pixels: usize,
    pub camera_height_pixels: usize,
}

pub fn write_header<F>(mut fd: F, april_config: Option<&AprilConfig>) -> std::io::Result<()>
where
    F: std::io::Write,
{
    writeln!(
        fd,
        "# The homography matrix entries (h00,...) are described in the April Tags paper"
    )?;
    writeln!(
        fd,
        "# https://dx.doi.org/10.1109/ICRA.2011.5979561 . Entry h22 is not saved because"
    )?;
    writeln!(
        fd,
        "# it always has value 1. The center pixel of the detection is (h02,h12)."
    )?;
    if let Some(april_config) = april_config {
        let cfg_yaml = serde_yaml::to_string(&april_config).unwrap();
        writeln!(fd, "# -- start of yaml config --")?;
        for line in cfg_yaml.lines() {
            writeln!(fd, "# {}", line)?;
        }
        writeln!(fd, "# -- end of yaml config --")?;
    }

    Ok(())
}
