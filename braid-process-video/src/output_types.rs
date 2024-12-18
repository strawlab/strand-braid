use eyre::Result;
use ordered_float::NotNan;
use std::io::Write;

use crate::{output_braidz::BraidStorage, output_video::VideoStorage, PerCamRenderFrame};

pub(crate) enum OutputStorage<'lib> {
    Video(Box<VideoStorage<'lib>>),
    Debug(DebugStorage),
    Braid(BraidStorage),
}

impl<'lib> OutputStorage<'lib> {
    pub(crate) async fn render_frame(
        &mut self,
        out_fno: usize,
        synced_data: &crate::SyncedPictures,
        all_cam_render_data: &[PerCamRenderFrame<'_>],
    ) -> Result<()> {
        match self {
            OutputStorage::Debug(d) => {
                d.render_frame(out_fno, synced_data, all_cam_render_data)?;
            }
            OutputStorage::Braid(b) => {
                b.render_frame(out_fno, synced_data, all_cam_render_data)
                    .await?;
            }
            OutputStorage::Video(v) => {
                v.render_frame(out_fno, synced_data, all_cam_render_data)
                    .await?;
            }
        }
        Ok(())
    }

    pub(crate) fn path(&self) -> &std::path::Path {
        match self {
            OutputStorage::Debug(d) => &d.path,
            OutputStorage::Braid(b) => &b.output_braidz_path,
            OutputStorage::Video(v) => &v.path,
        }
    }
}

pub(crate) struct DebugStorage {
    pub(crate) fd: std::fs::File,
    pub(crate) path: std::path::PathBuf,
}

impl DebugStorage {
    pub(crate) fn render_frame(
        &mut self,
        out_fno: usize,
        synced_data: &crate::SyncedPictures,
        all_cam_render_data: &[PerCamRenderFrame<'_>],
    ) -> Result<()> {
        if let Some(braidz_info) = synced_data.braidz_info.as_ref() {
            if let Some(ts) = braidz_info.trigger_timestamp.as_ref() {
                let dt: chrono::DateTime<chrono::Utc> = ts.into();
                writeln!(
                    self.fd,
                    " - Output frame {}, Input frame {}, trigger timestamp {}",
                    out_fno, braidz_info.frame_num, dt
                )?;
            } else {
                writeln!(
                    self.fd,
                    " - Output frame {}, Input frame {}: Braidz present, but no trigger timestamp",
                    out_fno, braidz_info.frame_num,
                )?;
            }

            for kest_row in braidz_info.kalman_estimates.iter() {
                writeln!(self.fd, "  {kest_row:?}")?;
            }
        } else {
            writeln!(self.fd, " - Output frame {}: No braidz info", out_fno)?;
        }
        for cam_render_data in all_cam_render_data.iter() {
            let mut write_it = |pts: &[(NotNan<f64>, NotNan<f64>)], name| {
                if pts.is_empty() {
                    writeln!(
                        self.fd,
                        "   Output frame {}, camera {}: no detected {name} points",
                        out_fno, cam_render_data.p.best_name
                    )?;
                } else {
                    for xy in pts.iter() {
                        writeln!(
                            self.fd,
                            "   Output frame {}, camera {}: {name} points: {} {}",
                            out_fno, cam_render_data.p.best_name, xy.0, xy.1
                        )?;
                    }
                }
                Ok::<_, eyre::Error>(())
            };
            write_it(&cam_render_data.points, "feature")?;
            write_it(&cam_render_data.reprojected_points, "reprojected")?;
        }
        Ok(())
    }
}
