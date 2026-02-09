use eyre as anyhow;

// This is lightly modified from the version in braid-process-video.

pub(crate) struct PerCamRender {
    pub(crate) width: usize,
    pub(crate) height: usize,
}

pub(crate) struct PerCamRenderFrame<'a> {
    pub(crate) p: &'a PerCamRender,
    pub(crate) jpeg_buf: &'a [u8],
    pub(crate) reproj: &'a [crate::AprilTagReprojectedPoint<f64>],
}

/// Draw the camera render data to an SVG file, and also save a PNG
/// rasterization of it.
pub(crate) fn draw_cam_render_data<P: AsRef<std::path::Path>>(
    out_fname: P,
    cam_render_data: &PerCamRenderFrame<'_>,
) -> anyhow::Result<()> {
    let svg_width = cam_render_data.p.width;
    let svg_height = cam_render_data.p.height;
    let curx = 0;
    let composite_margin_pixels = 0;
    let feature_radius = 10;
    let detection_style = "fill: none; stroke: deepskyblue; stroke-width: 3;";
    let projection_style = "fill: none; stroke: magenta; stroke-width: 3;";

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
        // Write a filled white rectangle for background.
        w.single("rect", |d| {
            d.attr("x", 0)?;
            d.attr("y", 0)?;
            d.attr("width", svg_width)?;
            d.attr("height", svg_height)?;
            d.attr("style", "fill:white")
        })?;

        // Create a clipPath for the camera image size.
        w.elem("clipPath", |d| d.attr("id", "clip-path-cam"))?
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
            d.attr("clip-path", "url(#clip-path-cam)")
        })?
        .build(|w| {
            // Draw image from camera
            let jpeg_base64_buf = base64::encode(cam_render_data.jpeg_buf);
            let data_url = format!("data:image/jpeg;base64,{}", jpeg_base64_buf);
            w.single("image", |d| {
                d.attr("x", 0)?;
                d.attr("y", 0)?;
                d.attr("width", cam_render_data.p.width)?;
                d.attr("height", cam_render_data.p.height)?;
                d.attr("xlink:href", data_url)
            })?;

            for pt in cam_render_data.reproj.iter() {
                w.single("circle", |d| {
                    d.attr("cx", pt.detected_point[0])?;
                    d.attr("cy", pt.detected_point[1])?;
                    d.attr("r", feature_radius)?;
                    d.attr("style", detection_style)
                })?;
                w.single("circle", |d| {
                    d.attr("cx", pt.projected_point[0])?;
                    d.attr("cy", pt.projected_point[1])?;
                    d.attr("r", feature_radius)?;
                    d.attr("style", projection_style)
                })?;
                w.single("line", |d| {
                    d.attr("x1", pt.projected_point[0])?;
                    d.attr("y1", pt.projected_point[1])?;
                    d.attr("x2", pt.detected_point[0])?;
                    d.attr("y2", pt.detected_point[1])?;
                    d.attr("style", projection_style)
                })?;
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

    let usvg_opt = usvg::Options::default();

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

    // Write image to disk as PNG.
    pixmap.save_png(&out_fname)?;
    tracing::info!(
        "Saved april tag detection image to: {}",
        out_fname.as_ref().display()
    );
    Ok(())
}
