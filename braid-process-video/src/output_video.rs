use chrono::{DateTime, Utc};
use color_eyre::{
    eyre::{self as anyhow},
    Result,
};
use std::io::Write;

use ci2_remote_control::{Mp4Codec, Mp4RecordingConfig};

use crate::{config::VideoOutputOptions, OutTimepointPerCamera, PerCamRenderFrame};

// fn default_mp4_config() -> Mp4RecordingConfig {
//     use ci2_remote_control::OpenH264Preset;
//     let preset = OpenH264Preset::AllFrames;
//     let codec = Mp4Codec::H264OpenH264(ci2_remote_control::OpenH264Options {
//         debug: false,
//         preset,
//     });
//     Mp4RecordingConfig {
//         codec,
//         max_framerate: Default::default(),
//         h264_metadata: None,
//     }
// }

fn default_mp4_config() -> Mp4RecordingConfig {
    // use ci2_remote_control::OpenH264Preset;
    // let preset = OpenH264Preset::AllFrames;
    // let codec = Mp4Codec::H264OpenH264(ci2_remote_control::OpenH264Options {
    //     debug: false,
    //     preset,
    // });
    Mp4RecordingConfig {
        codec: Mp4Codec::H264LessAvc,
        max_framerate: Default::default(),
        h264_metadata: None,
    }
}

pub(crate) struct VideoStorage<'lib> {
    pub(crate) path: std::path::PathBuf,
    pub(crate) mp4_writer: mp4_writer::Mp4Writer<'lib, std::fs::File>,
    /// timestamp of first frame
    pub(crate) first_timestamp: Option<DateTime<Utc>>,
    pub(crate) composite_margin_pixels: usize,
    pub(crate) feature_radius: String,
    pub(crate) feature_style: String,
    pub(crate) cam_text_style: String,
    pub(crate) video_options: VideoOutputOptions,
    pub(crate) cum_width: usize,
    pub(crate) cum_height: usize,
    pub(crate) usvg_opt: usvg::Options,
}

impl<'lib> VideoStorage<'lib> {
    pub(crate) fn new(
        v: &crate::config::VideoOutputConfig,
        output_filename: &std::path::Path,
        sources: &[crate::CameraSource],
    ) -> Result<Self> {
        // compute output width and height
        let cum_width: usize = sources.iter().map(|s| s.per_cam_render.width).sum();
        let cum_height: usize = sources
            .iter()
            .map(|s| s.per_cam_render.height)
            .max()
            .unwrap();

        if output_filename
            .extension()
            .and_then(|x| x.to_str())
            .map(|x| x.to_ascii_lowercase())
            != Some("mp4".to_string())
        {
            anyhow::bail!("expected extension mp4");
        }
        let fd = std::fs::File::create(output_filename)?;

        let mp4_cfg = default_mp4_config();

        let mp4_writer = mp4_writer::Mp4Writer::new(fd, mp4_cfg, None)?;
        let composite_margin_pixels = v
            .video_options
            .composite_margin_pixels
            .unwrap_or(crate::DEFAULT_COMPOSITE_MARGIN_PIXELS);

        let feature_radius = v
            .video_options
            .feature_radius
            .as_ref()
            .map(Clone::clone)
            .unwrap_or_else(|| crate::DEFAULT_FEATURE_RADIUS.to_string());
        let feature_style = v
            .video_options
            .feature_style
            .as_ref()
            .map(Clone::clone)
            .unwrap_or_else(|| crate::DEFAULT_FEATURE_STYLE.to_string());

        let cam_text_style = v
            .video_options
            .cam_text_style
            .as_ref()
            .map(Clone::clone)
            .unwrap_or_else(|| crate::DEFAULT_CAMERA_TEXT_STYLE.to_string());

        let mut usvg_opt = usvg::Options::default();
        // Get file's absolute directory.
        // usvg_opt.resources_dir = std::fs::canonicalize(&args[1]).ok().and_then(|p| p.parent().map(|p| p.to_path_buf()));
        usvg_opt.fontdb.load_system_fonts();

        Ok(Self {
            path: output_filename.to_path_buf(),
            mp4_writer,
            first_timestamp: None,
            composite_margin_pixels,
            feature_radius,
            feature_style,
            cam_text_style,
            video_options: v.video_options.clone(),
            cum_width,
            cum_height,
            usvg_opt,
        })
    }

    pub(crate) async fn render_frame(
        &mut self,
        out_fno: usize,
        synced_data: &crate::SyncedPictures,
        all_cam_render_data: &[PerCamRenderFrame<'_>],
    ) -> Result<()> {
        let synced_pics: &[OutTimepointPerCamera] = &synced_data.camera_pictures;
        let n_pics = synced_pics.len();

        let composite_margin_pixels = self.composite_margin_pixels;
        let feature_radius = &self.feature_radius;
        let feature_style = &self.feature_style;
        let cam_text_style = &self.cam_text_style;

        let ts = &synced_data.timestamp;

        // If there is no new data, we do not write a frame.

        let save_ts = if let Some(time_dilation_factor) = self.video_options.time_dilation_factor {
            if self.first_timestamp.is_none() {
                self.first_timestamp = Some(*ts);
            }

            let actual_time_delta =
                ts.signed_duration_since(*self.first_timestamp.as_ref().unwrap());
            let actual_time_delta_micros = actual_time_delta.num_microseconds().unwrap();
            let saved_time_delta =
                (actual_time_delta_micros as f64 * time_dilation_factor as f64).round() as i64;
            let saved_time_delta = chrono::Duration::microseconds(saved_time_delta);
            *ts + saved_time_delta
        } else {
            *ts
        };

        // Draw SVG
        let mut wtr = tagger::new(tagger::upgrade_write(Vec::<u8>::new()));
        let svg_width = self.cum_width + n_pics * 2 * composite_margin_pixels;
        let svg_height = self.cum_height + 2 * composite_margin_pixels;
        wtr.elem("svg", |d| {
            d.attr("xmlns", "http://www.w3.org/2000/svg")?;
            d.attr("xmlns:xlink", "http://www.w3.org/1999/xlink")?;
            d.attr("viewBox", format_args!("0 0 {} {}", svg_width, svg_height))
        })?
        .build(|w| {
            // Write a filled white rectangle for background.
            w.single("rect", |d| {
                d.attr("x", 0)?;
                d.attr("y", 0)?;
                d.attr("width", svg_width)?;
                d.attr("height", svg_height)?;
                d.attr("style", "fill:white")
            })?;

            // Create an SVG group.
            w.elem("g", |_| Ok(()))?.build(|w| {
                let mut curx = 0;
                for (cam_idx, cam_render_data) in all_cam_render_data.iter().enumerate() {
                    curx += composite_margin_pixels;

                    // Create a clipPath for the camera image size.
                    w.elem("clipPath", |d| {
                        d.attr("id", format!("clip-path-{}", cam_idx))
                    })?
                    .build(|w| {
                        w.single("rect", |d| {
                            d.attr("x", 0)?;
                            d.attr("y", 0)?;
                            d.attr("width", cam_render_data.p.width)?;
                            d.attr("height", cam_render_data.p.height)?;
                            // d.attr("style", "fill:green")?;
                            Ok(())
                        })?;
                        Ok(())
                    })?;

                    // Create a group using the clipPath above
                    w.elem("g", |d| {
                        d.attr(
                            "transform",
                            format!("translate({},{})", curx, composite_margin_pixels),
                        )?;
                        d.attr("clip-path", format!("url(#clip-path-{})", cam_idx))
                    })?
                    .build(|w| {
                        // Draw image from camera
                        if let Some(png_buf) = &cam_render_data.png_buf {
                            let png_base64_buf = base64::encode(&png_buf);
                            let data_url = format!("data:image/png;base64,{}", png_base64_buf);
                            w.single("image", |d| {
                                d.attr("x", 0)?;
                                d.attr("y", 0)?;
                                d.attr("width", cam_render_data.p.width)?;
                                d.attr("height", cam_render_data.p.height)?;
                                d.attr("xlink:href", data_url)
                            })?;
                        }

                        // Draw camera points
                        for xy in cam_render_data.points.iter() {
                            w.single("circle", |d| {
                                d.attr("cx", xy.0.as_ref())?;
                                d.attr("cy", xy.1.as_ref())?;
                                d.attr("r", feature_radius)?;
                                d.attr("style", feature_style)
                            })?;
                        }
                        Ok(())
                    })?;

                    // Create a group as above but without clipping
                    w.elem("g", |d| {
                        d.attr(
                            "transform",
                            format!("translate({},{})", curx, composite_margin_pixels),
                        )?;
                        Ok(())
                    })?
                    .build(|w| {
                        // Draw text annotation with camera names
                        {
                            let cam_text = format!(
                                "{} {}",
                                cam_render_data.p.best_name, cam_render_data.pts_chrono
                            );
                            w.elem("text", |d| {
                                d.attr("x", format!("{}", 10))?;
                                d.attr("y", format!("{}", 10))?;
                                d.attr("dy", "1em")?;
                                d.attr("style", cam_text_style)?;
                                Ok(())
                            })?
                            .build(|w| w.put_raw(cam_text))?;
                        }

                        Ok(())
                    })?;

                    curx += cam_render_data.p.width + composite_margin_pixels;
                }
                Ok(())
            })?;
            Ok(())
        })?;
        // Get the SVG file contents.
        let fmt_wtr = wtr.into_writer();
        let svg_buf = {
            fmt_wtr.error?;
            fmt_wtr.inner
        };

        // Now parse the SVG file.
        let rtree = usvg::Tree::from_data(&svg_buf, &self.usvg_opt.to_ref())?;
        // Now render the SVG file to a pixmap.
        let pixmap_size = rtree.svg_node().size.to_screen_size();
        let mut pixmap = tiny_skia::Pixmap::new(pixmap_size.width(), pixmap_size.height()).unwrap();
        tracing::debug!("rendering with resvg");
        resvg::render(&rtree, usvg::FitTo::Original, pixmap.as_mut()).unwrap();

        tracing::debug!("rasterizing");
        let rasterized = crate::tiny_skia_frame::Frame::new(pixmap)?;

        if false {
            // Write composited SVG to disk.
            let mut debug_svg_fd = std::fs::File::create(format!("frame{:05}.svg", out_fno))?;
            debug_svg_fd.write_all(&svg_buf)?;

            // Write rasterized image to disk as PNG.
            let png_buf =
                convert_image::frame_to_image(&rasterized, convert_image::ImageOptions::Png)?;
            std::fs::write(format!("frame{:05}.png", out_fno), png_buf)?;
        }

        // Save the pixmap into the MP4 file being saved.
        tracing::debug!("writing to MP4");
        self.mp4_writer.write(&rasterized, save_ts)?;

        Ok(())
    }
}
