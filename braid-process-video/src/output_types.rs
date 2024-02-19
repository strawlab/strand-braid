use color_eyre::Result;
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
                d.render_frame(all_cam_render_data)?;
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
        all_cam_render_data: &[PerCamRenderFrame<'_>],
    ) -> Result<()> {
        for cam_render_data in all_cam_render_data.iter() {
            if cam_render_data.points.is_empty() {
                writeln!(
                    self.fd,
                    "   Output {}: no points",
                    cam_render_data.p.best_name
                )?;
            } else {
                for xy in cam_render_data.points.iter() {
                    writeln!(
                        self.fd,
                        "   Output {}: points: {} {}",
                        cam_render_data.p.best_name, xy.0, xy.1
                    )?;
                }
            }
        }
        Ok(())
    }
}
