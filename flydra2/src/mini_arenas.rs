use std::{collections::BTreeMap, path::Path};

use serde::{Deserialize, Serialize};

use braid_types::MiniArenaConfig;

use crate::{bundled_data::BundledAllCamsOneFrameUndistorted, MyFloat, Result};

/// Into a mini arena.
///
/// This is different from [braid_types::MiniArenaLocator] because the mini
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
        match self.data.get(idx) {
            None => MiniArenaLocator::OutOfBounds,
            Some(&NO_MINI_ARENA_MARKER) => MiniArenaLocator::NotInMiniArena,
            Some(idx) => MiniArenaLocator::Index(MiniArenaIndex::new(*idx)),
        }
    }
}

pub struct MiniArenaDebugConfig {
    /// Directory to save mini arena images.
    pub output_png_path: camino::Utf8PathBuf,
    /// Background image to use for mini arena images.
    pub background_image_jpeg_buf: Option<Vec<u8>>,
    pub april_detections: Option<Vec<braid_apriltag_types::AprilTagCoords2D>>,
}

impl std::fmt::Debug for MiniArenaDebugConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MiniArenaDebugConfig")
            .finish_non_exhaustive()
    }
}

/// Build per-camera mini-arena images.
pub(crate) fn build_mini_arena_images(
    recon: Option<&flydra_mvg::FlydraMultiCameraSystem<MyFloat>>,
    mini_arena_config: &MiniArenaConfig,
    mini_arena_debug_cfg: Option<&MiniArenaDebugConfig>,
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
                        let pt = braid_mvg::DistortedPixel {
                            coords: nalgebra::Point2::new(col as f64, row as f64),
                        };
                        let ray = cam.project_distorted_pixel_to_ray(&pt);
                        let coords_3d = crate::flat_2d::ray_to_flat_3d(&ray);
                        if let Some(coords_3d) = coords_3d {
                            let coords = [coords_3d.x, coords_3d.y, coords_3d.z];
                            if let Some(arena_idx) = xy_grid_cfg.get_arena_index(&coords).idx() {
                                let coords_idx = row * cam.width() + col;
                                mini_arena_image[coords_idx] = arena_idx;
                            }
                        }
                    }
                }

                if let Some(cfg) = &mini_arena_debug_cfg {
                    // save debug image of mini arenas.
                    use machine_vision_formats::pixel_format::Mono8;
                    let well_image = machine_vision_formats::image_ref::ImageRef::<Mono8>::new(
                        cam.width().try_into().unwrap(),
                        cam.height().try_into().unwrap(),
                        cam.width(),
                        &mini_arena_image,
                    )
                    .unwrap();
                    let well_jpeg_buf = convert_image::frame_to_encoded_buffer(
                        &well_image,
                        convert_image::EncoderOptions::Jpeg(100),
                    )
                    .unwrap();

                    annotate_mini_arena_image(&well_jpeg_buf, cfg, &cam, xy_grid_cfg).unwrap();
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

/// On top of background image, draw mini arena numbers.
fn annotate_mini_arena_image(
    well_jpeg_buf: &[u8],
    cfg: &MiniArenaDebugConfig,
    cam: &flydra_mvg::MultiCamera<f64>,
    xy_grid_cfg: &braid_types::XYGridConfig,
) -> eyre::Result<()> {
    let svg_width = cam.width();
    let svg_height = cam.height();
    // Draw SVG
    let mut wtr = tagger::new(tagger::upgrade_write(Vec::<u8>::new()));
    // let svg_width = self.cum_width + n_pics * 2 * composite_margin_pixels;
    // let svg_height = self.cum_height + 2 * composite_margin_pixels;
    wtr.elem("svg", |d| {
        d.attr("xmlns", "http://www.w3.org/2000/svg")?;
        d.attr("xmlns:xlink", "http://www.w3.org/1999/xlink")?;
        d.attr("viewBox", format_args!("0 0 {} {}", svg_width, svg_height))
    })?
    .build(|w| {

        // Draw background image
        if let Some(jpeg_buf) = &cfg.background_image_jpeg_buf {
            let jpeg_base64_buf = base64::encode(jpeg_buf);
            let data_url = format!("data:image/jpeg;base64,{}", jpeg_base64_buf);
            w.single("image", |d| {
                d.attr("x", 0)?;
                d.attr("y", 0)?;
                d.attr("width", svg_width)?;
                d.attr("height", svg_height)?;
                d.attr("xlink:href", data_url)
            })?;
        }

        // Draw well image
        {
            let jpeg_base64_buf = base64::encode(well_jpeg_buf);
            let data_url = format!("data:image/jpeg;base64,{}", jpeg_base64_buf);
            w.single("image", |d| {
                d.attr("x", 0)?;
                d.attr("y", 0)?;
                d.attr("width", svg_width)?;
                d.attr("height", svg_height)?;
                d.attr("opacity", "0.3")?;
                d.attr("xlink:href", data_url)
            })?;
        }

        // Draw april tag detections
        if let Some(detections) = &cfg.april_detections {
            for detection in detections.iter() {

                let id_str = if detection.vertical_flip {
                    format!("{} (v)", detection.id)
                } else {
                    format!("{}", detection.id)
                };
                w.elem("text", |d| {
                d.attr("x", format!("{}", detection.x))?;
                d.attr("y", format!("{}", detection.y))?;
                d.attr("text-anchor ", "middle")?;
                d.attr("dominant-baseline", "middle")?;
                d.attr(
                    "style",
                    "font-family: Arial, Helvetica, sans-serif; font-size: 30px; fill: red;",
                )?;
                Ok(())
            })?
            .build(|w| w.put_raw(id_str))?;
            }
        }

        // Draw mini arena numbers.
        for center in xy_grid_cfg.iter_centers() {
            let coord = [center.0, center.1, 0.0];
            // Get mini arena index.
            let idx = xy_grid_cfg.get_arena_index(&coord).idx().unwrap();

            // The arena center is in 3D world coordinates. Project it to pixel
            // coordinates.
            let pt = cam
                .project_3d_to_distorted_pixel(&braid_mvg::PointWorldFrame::from(coord))
                .coords;

            w.elem("text", |d| {
                d.attr("x", format!("{}", pt.x))?;
                d.attr("y", format!("{}", pt.y))?;
                d.attr("text-anchor ", "middle")?;
                d.attr("dominant-baseline", "middle")?;
                d.attr(
                    "style",
                    "font-family: Arial, Helvetica, sans-serif; font-size: 40px; fill: deepskyblue;",
                )?;
                Ok(())
            })?
            .build(|w| w.put_raw(format!("{}", idx)))?;
        }

        Ok(())
    })?;
    // Get the SVG file contents.
    let fmt_wtr = wtr.into_writer();
    let svg_buf = {
        fmt_wtr.error?;
        fmt_wtr.inner
    };

    let mut usvg_opt = usvg::Options::default();
    usvg_opt.fontdb_mut().load_system_fonts();

    // Now parse the SVG file.
    let rtree = usvg::Tree::from_data(&svg_buf, &usvg_opt)?;
    // Now render the SVG file to a pixmap.
    let pixmap_size = rtree.size().to_int_size();
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(pixmap_size.width(), pixmap_size.height()).unwrap();
    resvg::render(
        &rtree,
        resvg::tiny_skia::Transform::default(),
        &mut pixmap.as_mut(),
    );

    pixmap.save_png(&cfg.output_png_path)?;
    tracing::info!("Saved well image to {}", cfg.output_png_path);

    Ok(())
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

// TODO: this gets called from an async task but does blocking IO. It should be
// rewritten to use async IO.
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
