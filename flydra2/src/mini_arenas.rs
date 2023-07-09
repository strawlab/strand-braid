use std::collections::BTreeMap;

use nalgebra::Point2;

use flydra_types::MiniArenaConfig;

use crate::{MyFloat, Result};

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

/// Image of a mini arena for a calibrated camera.
#[derive(Debug)]
pub(crate) struct MiniArenaImage {
    width: usize,
    data: Vec<u8>,
}

impl MiniArenaImage {
    /// Get mini arena locator.
    ///
    /// Returns None if (x,y) location out of bounds or does not refer to a mini
    /// arena.
    pub(crate) fn get_mini_arena(&self, x: usize, y: usize) -> Option<MiniArenaIndex> {
        let idx = y * self.width + x;
        self.data.get(idx).and_then(|val| {
            if *val == NO_MINI_ARENA_MARKER {
                None
            } else {
                Some(MiniArenaIndex(*val))
            }
        })
    }
}

/// Build per-camera mini-arena images.
pub(crate) fn build_mini_arena_images(
    recon: Option<&flydra_mvg::FlydraMultiCameraSystem<MyFloat>>,
    mini_arena_config: &MiniArenaConfig,
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

                {
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
                    let fname = format!("mini_arenas_{}.png", cam.name());
                    std::fs::write(&fname, png_buf)?;
                    log::info!("saved mini arena image assignment image to {fname}");
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
