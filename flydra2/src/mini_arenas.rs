use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use nalgebra::Point2;
use serde::{Deserialize, Serialize};

use flydra_types::MiniArenaConfig;

use crate::{bundled_data::BundledAllCamsOneFrameUndistorted, MyFloat, Result};

/// Into a mini arena.
///
/// This is different from [flydra_types::MiniArenaLocator] because the mini
/// arena must exist.
///
/// Newtype wrapper around [u8].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MiniArenaIndex(u8);
impl MiniArenaIndex {
    pub(crate) fn new(val: u8) -> Self {
        MiniArenaIndex(val)
    }
    pub(crate) fn idx(&self) -> usize {
        self.0 as usize
    }
}

const NO_MINI_ARENA_MARKER: u8 = 255;

pub(crate) enum MiniArenaLocator {
    /// Location is not possible.
    OutOfBounds,
    /// Not in mini arena.
    NotInMiniArena,
    /// In mini arena with index.
    Index(MiniArenaIndex),
    /// No mini arenas are in use.
    OneArena,
}

/// Image of a mini arena for a calibrated camera.
pub(crate) struct MiniArenaImage {
    width: usize,
    // height is data.len() / width
    data: Vec<u8>,
}

impl std::fmt::Debug for MiniArenaImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MiniArenaImage")
            .field("width", &self.width)
            .field("data.len()", &self.data.len())
            .finish_non_exhaustive()
    }
}

impl MiniArenaImage {
    /// Get mini arena locator.
    pub(crate) fn get_mini_arena(&self, x: usize, y: usize) -> MiniArenaLocator {
        let idx = y * self.width + x;
        match self.data.get(idx).map(|x| *x) {
            None => MiniArenaLocator::OutOfBounds,
            Some(NO_MINI_ARENA_MARKER) => MiniArenaLocator::NotInMiniArena,
            Some(idx) => MiniArenaLocator::Index(MiniArenaIndex::new(idx)),
        }
    }
}

/// Build per-camera mini-arena images.
pub(crate) fn build_mini_arena_images(
    recon: Option<&flydra_mvg::FlydraMultiCameraSystem<MyFloat>>,
    mini_arena_config: &MiniArenaConfig,
    image_output_dir: Option<&Path>,
) -> Result<BTreeMap<String, MiniArenaImage>> {
    let mut mini_arena_images = BTreeMap::new();
    let recon = match recon {
        None => {
            return Ok(mini_arena_images);
        }
        Some(recon) => recon,
    };
    match mini_arena_config {
        MiniArenaConfig::NoMiniArena => {}
        MiniArenaConfig::XYGrid(xy_grid_cfg) => {
            for cam in recon.cameras() {
                let sz = cam.width() * cam.height();
                let mut mini_arena_image = vec![NO_MINI_ARENA_MARKER; sz];

                for row in 0..cam.height() {
                    for col in 0..cam.width() {
                        let pt = mvg::DistortedPixel {
                            coords: Point2::new(col as f64, row as f64),
                        };
                        let ray = cam.project_distorted_pixel_to_ray(&pt);
                        let coords_3d = crate::flat_2d::ray_to_flat_3d(&ray);
                        if let Some(coords_3d) = coords_3d {
                            if let Some(arena_idx) = xy_grid_cfg.get_arena_index(&coords_3d).idx() {
                                let coords_idx = row * cam.width() + col;
                                mini_arena_image[coords_idx] = arena_idx;
                            }
                        }
                    }
                }

                if let Some(dest_dir) = image_output_dir {
                    std::fs::create_dir_all(dest_dir)?;

                    // save debug image of mini arenas.
                    use machine_vision_formats::pixel_format::Mono8;
                    let frame = simple_frame::SimpleFrame::<Mono8>::new(
                        cam.width().try_into().unwrap(),
                        cam.height().try_into().unwrap(),
                        cam.width().try_into().unwrap(),
                        mini_arena_image.clone(),
                    )
                    .unwrap();
                    let png_buf =
                        convert_image::frame_to_image(&frame, convert_image::ImageOptions::Png)
                            .unwrap();

                    let dest_path =
                        PathBuf::from(dest_dir).join(format!("mini_arenas_{}.png", cam.name()));

                    std::fs::write(&dest_path, png_buf)?;
                    tracing::info!(
                        "saved mini arena image assignment image to {}",
                        dest_path.display()
                    );
                }

                mini_arena_images.insert(
                    cam.name().to_string(),
                    MiniArenaImage {
                        width: cam.width(),
                        data: mini_arena_image,
                    },
                );
            }
        }
    }
    Ok(mini_arena_images)
}

// ------ debug to CSV stuff ---------------

/// Debugging structure to save to CSV files
#[derive(Clone, Debug, Serialize, Deserialize)]
struct MiniArenaPointPerCamFlat {
    frame: u64,
    cam_name: String,
    mini_arena_idx: usize,
    undistorted_x: f64,
    undistorted_y: f64,
    distorted_idx: u8,
    distorted_x: f64,
    distorted_y: f64,
}

impl MiniArenaPointPerCamFlat {
    fn new(
        frame: u64,
        cam_name: String,
        mini_arena_idx: usize,
        orig: &crate::bundled_data::MiniArenaPointPerCam,
    ) -> Self {
        Self {
            frame,
            cam_name,
            mini_arena_idx,
            undistorted_x: orig.undistorted.x,
            undistorted_y: orig.undistorted.y,
            distorted_idx: orig.numbered_raw_udp_point.idx,
            distorted_x: orig.numbered_raw_udp_point.pt.x0_abs,
            distorted_y: orig.numbered_raw_udp_point.pt.y0_abs,
        }
    }
}

pub(crate) struct MiniArenaAssignmentDebug {
    wtr: csv::Writer<std::io::BufWriter<std::fs::File>>,
}

impl MiniArenaAssignmentDebug {
    pub(crate) fn new<P: AsRef<Path>>(path: P) -> crate::Result<Self> {
        let fd = std::fs::File::create(path)?;
        let bufwriter = std::io::BufWriter::new(fd);
        let wtr = csv::Writer::from_writer(bufwriter);
        Ok(Self { wtr })
    }

    pub(crate) fn write_frame(
        &mut self,
        undistorted: &BundledAllCamsOneFrameUndistorted,
    ) -> crate::Result<()> {
        let frame = undistorted.tdpt.frame.0;

        for (mini_arena_idx, mini_arena) in undistorted.per_mini_arena.iter().enumerate() {
            for (mini_arena_cam, mini_arena_data) in mini_arena.per_cam.iter() {
                for orig in mini_arena_data.iter() {
                    let row = MiniArenaPointPerCamFlat::new(
                        frame,
                        mini_arena_cam.as_str().to_string(),
                        mini_arena_idx,
                        orig,
                    );

                    self.wtr.serialize(row)?;
                }
            }
        }

        Ok(())
    }
}
